#!/usr/bin/env python3
"""A small local speech server for Nuzzle: custom speech-to-text and text-to-speech behind OpenAI-style endpoints.

    POST /v1/audio/transcriptions   multipart form with "file" (WAV)      -> {"text": "..."}      Hima-ASR (ONNX)
    POST /v1/audio/feel             multipart form with "file" (WAV)      -> {"feeling": {...}}   Hima decision head
    POST /v1/audio/speech           {"input": "...", "voice": "af_heart"} -> 16-bit PCM, 24 kHz   Paradee (ONNX)
    GET  /health                                                          -> {"ok": true, "asr": ..., "tts": ...}

Everything runs on this computer (CPU, onnxruntime) and listens on 127.0.0.1 only. Point Nuzzle at it in
Settings -> Advanced -> Ears and voice: choose "Speech server" for either engine and enter http://127.0.0.1:8765/v1

Run it with the folders that hold the models:

    python scripts/speech-server.py --asr-dir "path/to/hima-asr" --tts-dir "path/to/Paradee/models"

  --asr-dir   Hima-ASR ONNX folder: tokenizer.json, embed.npy, keep_ids.json, start_of_text.npy, mel_filters.npy and
              onnx/encoder_model_q4.onnx (or _int8) + onnx/decoder_model_int8.onnx   (Prahaladha/hima-asr-ONNX)
              With hima_onnx.py and decide/ in it too (the whole repo), the reply also says how the speaker sounds:
              {"text": ..., "feeling": {"tone": {"is": "cheerful", "p": 0.7}, "emotion": ..., "urgency": ...}}
  --tts-dir   Paradee folder: paradee_int8.onnx and config.json                      (sahilmahendrakar/Paradee-8M-v1.3)
              Needs the paradee package: pip install git+https://github.com/sahilmahendrakar/paradee

Leave one out to serve only the other. Needs: pip install onnxruntime numpy tokenizers soundfile
"""
from __future__ import annotations

import argparse
import io
import json
import re
import sys
import threading
import time
import unicodedata
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np

MAX_BODY = 40 * 1024 * 1024  # a 30 s WAV is under 1 MB; this is only a guard
MAX_TEXT = 4000


# ----------------------------------------------------------------------------------------------- speech to text
class Hima:
    """Hima-ASR through onnxruntime: log-mel -> encoder -> greedy decoding. 16 kHz mono, clips of up to 30 seconds."""

    RATE = 16000
    N_FFT, HOP, MEL_BLOCK = 400, 160, 100
    LAYERS_KV = (18, 1, 0, 256)  # decoder key/value cache: layers, batch, past length, head width
    BOS, EOS = 2, 1
    LANGUAGES = {"as": "assamese", "bn": "bengali", "en": "english", "gu": "gujarati", "hi": "hindi", "kn": "kannada",
                 "ml": "malayalam", "mr": "marathi", "or": "odia", "pa": "punjabi", "ta": "tamil", "te": "telugu"}
    VARIANTS = (("q4", "encoder_model_q4.onnx"), ("int8", "encoder_model_int8.onnx"))

    def __init__(self, folder: str, threads: int = 4):
        import onnxruntime as ort
        from tokenizers import Tokenizer

        p = Path(folder)
        enc = next((p / "onnx" / name for _, name in self.VARIANTS if (p / "onnx" / name).exists()), None)
        dec = p / "onnx" / "decoder_model_int8.onnx"
        if enc is None or not dec.exists():
            raise FileNotFoundError(f"no Hima ONNX model in {p} (need onnx/encoder_model_q4.onnx and onnx/decoder_model_int8.onnx)")
        so = ort.SessionOptions()
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        so.intra_op_num_threads = threads
        self.enc = ort.InferenceSession(str(enc), so, providers=["CPUExecutionProvider"])
        self.dec = ort.InferenceSession(str(dec), so, providers=["CPUExecutionProvider"])
        self.embed = np.load(p / "embed.npy").astype(np.float32)
        self.keep = np.array(json.loads((p / "keep_ids.json").read_text(encoding="utf-8")))
        self.row = {int(t): i for i, t in enumerate(self.keep)}
        self.sot = np.load(p / "start_of_text.npy").astype(np.float32)
        self.filters = np.load(p / "mel_filters.npy")
        self.tok = Tokenizer.from_file(str(p / "tokenizer.json"))
        self.lock = threading.Lock()
        self.name = f"hima-asr ({enc.stem.rsplit('_', 1)[-1]})"

    def _log_mel(self, x: np.ndarray) -> tuple[np.ndarray, int]:
        if len(x) < 8000:
            x = np.pad(x, (0, 8000 - len(x)))
        xp = np.pad(x, (self.N_FFT // 2, self.N_FFT // 2), mode="reflect")
        n_frames = 1 + (len(xp) - self.N_FFT) // self.HOP
        idx = np.arange(self.N_FFT)[None, :] + self.HOP * np.arange(n_frames)[:, None]
        win = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(self.N_FFT) / self.N_FFT)).astype(np.float32)
        spec = np.fft.rfft(xp[idx] * win, axis=1)
        mag = (spec.real ** 2 + spec.imag ** 2).astype(np.float32)[:-1].T
        log = np.log10(np.maximum(self.filters.T @ mag, 1e-10))
        log = np.maximum(log, log.max() - 8.0)
        log = ((log + 4.0) / 4.0).astype(np.float32)
        n_valid = log.shape[1]
        return np.pad(log, ((0, 0), (0, (-n_valid) % self.MEL_BLOCK))), n_valid

    def _encoder_frames(self, n_mel: int) -> int:
        def post(n: int) -> int:
            for _ in range(3):
                n = (n - 1) // 2 + 1 if n > 0 else 0
            return n

        blocks = -(-n_mel // self.MEL_BLOCK)
        return sum(post(min(self.MEL_BLOCK, n_mel - i * self.MEL_BLOCK)) for i in range(blocks))

    @staticmethod
    def _stuck(p: list[int]) -> bool:
        """The decoder is repeating itself (the same piece or phrase over and over)."""
        for size in range(1, 41):
            times = 6 if size == 1 else 4 if size < 9 else 3
            if len(p) >= size * times and p[-size * times:] == p[-size:] * times:
                return True
        return False

    def transcribe(self, pcm: np.ndarray, language: str | None = None, style: str = "mixed") -> str:
        if len(pcm) < 1600 or float(np.sqrt(np.mean(pcm ** 2))) < 1e-4:
            return ""  # too short or digital silence: the model would invent words
        pcm = pcm[: self.RATE * 30]
        with self.lock:
            mel, n_valid = self._log_mel(pcm)
            n = self._encoder_frames(n_valid)
            sound = self.enc.run(["sound"], {"mel": mel[None], "n_frames": np.array(n, dtype=np.int64)})[0][:n]
            parts = [self.embed[[self.row[self.BOS]]], sound, self.sot[None]]
            if language in self.LANGUAGES:
                ids = self.tok.encode(f"<{self.LANGUAGES[language]} {style}>", add_special_tokens=False).ids
                parts.append(self.embed[[self.row[i] for i in ids]])
            x = np.concatenate(parts).astype(np.float32)
            pk = pv = np.zeros(self.LAYERS_KV, np.float32)
            pos, out = 0, []
            limit = min(512, int(1.5 * sound.shape[0]) + 8)
            while len(out) < limit:
                logits, pk, pv = self.dec.run(["logits", "present_k", "present_v"],
                                              {"x": x, "pos0": np.array(pos, dtype=np.int64), "past_k": pk, "past_v": pv})
                pos += x.shape[0]
                j = int(np.argmax(np.asarray(logits, dtype=np.float32).reshape(-1)))
                tok = int(self.keep[j])
                if tok == self.EOS:
                    break
                out.append(tok)
                if self._stuck(out):
                    break
                x = self.embed[[j]]
            text = self.tok.decode(out, skip_special_tokens=False)
        text = unicodedata.normalize("NFC", text)
        text = re.sub(r"<[^>]*>|\[[^\]]*\]", " ", text)  # language and style tags
        return " ".join(text.split())


class HimaWithFeelings:
    """Hima-ASR with its decision head, through the authors' own loader (hima_onnx.py + decide/ in the model folder).
    One listen gives the words and how they were said: tone, emotion and urgency."""

    FEEL = ["tone", "emotion", "urgency"]

    def __init__(self, folder: str, threads: int = 4):
        import importlib.util

        p = Path(folder)
        spec = importlib.util.spec_from_file_location("hima_onnx", str(p / "hima_onnx.py"))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        precision = next((n for n, f in Hima.VARIANTS if (p / "onnx" / f).exists()), "q4")
        self.m = mod.HimaONNX(str(p), precision=precision, threads=threads)
        self.lock = threading.Lock()
        self.name = f"hima-asr ({precision}) with the decision head"
        self.feel(np.random.default_rng(0).normal(0, 0.05, 16000).astype(np.float32))  # read the questions once, now

    @staticmethod
    def _top(decisions: dict) -> dict:
        """{"tone": {"cheerful": 0.7, ...}} -> {"tone": {"is": "cheerful", "p": 0.7}}"""
        return {name: {"is": max(probs, key=probs.get), "p": round(float(max(probs.values())), 3)} for name, probs in decisions.items()}

    @staticmethod
    def _silent(pcm: np.ndarray) -> bool:
        return len(pcm) < 1600 or float(np.sqrt(np.mean(pcm ** 2))) < 1e-4

    def run(self, pcm: np.ndarray, language: str | None = None, style: str = "mixed") -> dict:
        if self._silent(pcm):
            return {"text": "", "feeling": None}
        with self.lock:
            r = self.m.run(pcm[: 16000 * 30], self.FEEL, language if language in Hima.LANGUAGES else None, style, 16000)
        return {"text": r["text"], "feeling": self._top(r["decisions"])}

    def transcribe(self, pcm: np.ndarray, language: str | None = None, style: str = "mixed") -> str:
        return self.run(pcm, language, style)["text"]

    def feel(self, pcm: np.ndarray) -> dict | None:
        """How it sounds, without writing the words (quick enough to ask while someone is still talking)."""
        if self._silent(pcm):
            return None
        with self.lock:
            return self._top(self.m.decide(pcm[-16000 * 30:], self.FEEL, 16000))


def load_ears(folder: str, threads: int):
    p = Path(folder)
    if (p / "hima_onnx.py").exists() and (p / "decide" / "head.safetensors").exists() and (p / "decide" / "head_np.py").exists():
        return HimaWithFeelings(folder, threads)
    return Hima(folder, threads)


def read_audio(data: bytes, rate: int) -> np.ndarray:
    """Any WAV/FLAC/OGG soundfile can read -> mono float32 at `rate`."""
    import soundfile as sf

    pcm, sr = sf.read(io.BytesIO(data), dtype="float32", always_2d=True)
    pcm = pcm.mean(axis=1)
    if sr != rate and len(pcm):
        n = int(round(len(pcm) * rate / sr))
        pcm = np.interp(np.linspace(0, len(pcm) - 1, n), np.arange(len(pcm)), pcm).astype(np.float32)
    return pcm


# ----------------------------------------------------------------------------------------------- text to speech
class ParadeeVoice:
    """Paradee 8M: text -> 24 kHz speech. The voice sets the language (af_/am_/bf_/bm_ English, hf_/hm_ Hindi,
    ff_ French, em_ Spanish)."""

    def __init__(self, folder: str, threads: int = 2, default_voice: str = "af_heart"):
        from paradee import SAMPLE_RATE, Paradee

        p = Path(folder)
        model = next((p / n for n in ("paradee_int8.onnx", "paradee.onnx") if (p / n).exists()), None)
        if model is None or not (p / "config.json").exists():
            raise FileNotFoundError(f"no Paradee model in {p} (need paradee_int8.onnx and config.json)")
        self.tts = Paradee(model_path=str(model), config_path=str(p / "config.json"), threads=threads)
        self.rate = SAMPLE_RATE
        self.voices = list(json.loads((p / "config.json").read_text(encoding="utf-8")).get("voices", [])) or [default_voice]
        self.default_voice = default_voice if default_voice in self.voices else self.voices[0]
        self.lock = threading.Lock()
        self.name = f"paradee ({model.stem})"
        self.speak("Hello.", self.default_voice)  # load the dictionaries now, not on the first sentence

    def speak(self, text: str, voice: str | None = None, speed: float = 1.0) -> np.ndarray:
        voice = voice if voice in self.voices else self.default_voice
        with self.lock:
            return np.asarray(self.tts(text, speed=float(speed), voice=voice), dtype=np.float32).reshape(-1)


# ----------------------------------------------------------------------------------------- stand-ins for --dry-run
class FakeEars:
    name = "dry run"

    def transcribe(self, pcm, language=None, style="mixed"):
        return f"heard {len(pcm) / 16000:.1f} seconds of audio"


class FakeVoice:
    name, rate, voices, default_voice = "dry run", 24000, ["beep"], "beep"

    def speak(self, text, voice=None, speed=1.0):
        t = np.arange(int(self.rate * min(2.0, 0.05 * len(text) + 0.2))) / self.rate
        return (0.2 * np.sin(2 * np.pi * 440 * t)).astype(np.float32)


# -------------------------------------------------------------------------------------------------------- http
def form_parts(body: bytes, content_type: str) -> dict[str, bytes]:
    """Fields of a multipart/form-data body by name (the file's bytes under its field name)."""
    m = re.search(r'boundary="?([^";]+)"?', content_type)
    if not m:
        raise ValueError("not a multipart form")
    out = {}
    for part in body.split(b"--" + m.group(1).encode()):
        head, sep, data = part.partition(b"\r\n\r\n")
        name = re.search(rb'name="([^"]*)"', head)
        if sep and name:
            out[name.group(1).decode("utf-8", "replace")] = data[:-2] if data.endswith(b"\r\n") else data
    return out


class Handler(BaseHTTPRequestHandler):
    ears = None
    voice = None
    language = None
    style = "mixed"
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # one short line per request
        sys.stderr.write("[speech] " + (fmt % args) + "\n")

    def send(self, status: int, body: bytes, ctype: str = "application/json") -> None:
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def fail(self, status: int, message: str) -> None:
        self.send(status, json.dumps({"error": {"message": message}}).encode())

    def do_GET(self):
        if self.path.rstrip("/") in ("/health", "/v1/health", ""):
            return self.send(200, json.dumps({"ok": True, "asr": getattr(self.ears, "name", None), "tts": getattr(self.voice, "name", None),
                                              "voices": getattr(self.voice, "voices", [])}).encode())
        self.fail(404, "use POST /v1/audio/transcriptions or POST /v1/audio/speech")

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        if not 0 < length <= MAX_BODY:
            return self.fail(413, "empty or too large")
        body = self.rfile.read(length)
        path = self.path.split("?")[0].rstrip("/")
        started = time.perf_counter()
        try:
            if path.endswith("/audio/transcriptions"):
                if self.ears is None:
                    return self.fail(503, "speech-to-text is not loaded (start the server with --asr-dir)")
                parts = form_parts(body, self.headers.get("Content-Type", ""))
                if "file" not in parts:
                    return self.fail(400, 'send the audio as the form field "file"')
                language = parts.get("language", b"").decode().strip().lower() or self.language
                pcm = read_audio(parts["file"], 16000)
                if hasattr(self.ears, "run"):  # words and how they were said, from one listen
                    out = self.ears.run(pcm, language, self.style)
                else:
                    out = {"text": self.ears.transcribe(pcm, language, self.style)}
                self.log_message("heard %.1fs in %.0f ms: %s %s", len(pcm) / 16000, (time.perf_counter() - started) * 1000, out["text"][:60],
                                 json.dumps(out.get("feeling")) if out.get("feeling") else "")
                return self.send(200, json.dumps(out, ensure_ascii=False).encode("utf-8"))
            if path.endswith("/audio/feel"):
                if not hasattr(self.ears, "feel"):
                    return self.fail(501, "this speech-to-text model has no decision head (it needs hima_onnx.py and decide/ in the model folder)")
                parts = form_parts(body, self.headers.get("Content-Type", ""))
                if "file" not in parts:
                    return self.fail(400, 'send the audio as the form field "file"')
                feeling = self.ears.feel(read_audio(parts["file"], 16000))
                self.log_message("felt in %.0f ms: %s", (time.perf_counter() - started) * 1000, json.dumps(feeling))
                return self.send(200, json.dumps({"feeling": feeling}).encode())
            if path.endswith("/audio/speech"):
                if self.voice is None:
                    return self.fail(503, "text-to-speech is not loaded (start the server with --tts-dir)")
                req = json.loads(body.decode("utf-8"))
                text = str(req.get("input", "")).strip()
                if not text or len(text) > MAX_TEXT:
                    return self.fail(400, f'"input" must be 1 to {MAX_TEXT} characters')
                audio = self.voice.speak(text, req.get("voice"), float(req.get("speed") or 1.0))
                pcm16 = (np.clip(audio, -1.0, 1.0) * 32767.0).astype("<i2")
                self.log_message("spoke %.1fs in %.0f ms", len(audio) / self.voice.rate, (time.perf_counter() - started) * 1000)
                if str(req.get("response_format", "pcm")).lower() == "wav":
                    import soundfile as sf

                    buf = io.BytesIO()
                    sf.write(buf, pcm16, self.voice.rate, format="WAV", subtype="PCM_16")
                    return self.send(200, buf.getvalue(), "audio/wav")
                return self.send(200, pcm16.tobytes(), f"audio/pcm;rate={self.voice.rate};bits=16")
            self.fail(404, "use POST /v1/audio/transcriptions or POST /v1/audio/speech")
        except Exception as exc:  # noqa: BLE001 - a bad request must not take the server down
            self.log_message("failed: %r", exc)
            self.fail(500, f"{type(exc).__name__}: {exc}")


def main() -> None:
    ap = argparse.ArgumentParser(description="Local speech-to-text (Hima-ASR) and text-to-speech (Paradee) for Nuzzle.")
    ap.add_argument("--asr-dir", help="folder with the Hima-ASR ONNX model")
    ap.add_argument("--tts-dir", help="folder with paradee_int8.onnx and config.json")
    ap.add_argument("--host", default="127.0.0.1", help="address to listen on (default: this computer only)")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--language", default="", help="spoken language for speech-to-text, e.g. en or hi (default: detect)")
    ap.add_argument("--style", default="mixed", choices=["mixed", "native"], help="mixed: English words in Latin letters; native: native script")
    ap.add_argument("--voice", default="af_heart", help="default Paradee voice")
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--dry-run", action="store_true", help="no models: fake engines, to check the connection")
    a = ap.parse_args()

    if a.dry_run:
        Handler.ears, Handler.voice = FakeEars(), FakeVoice()
    else:
        if not a.asr_dir and not a.tts_dir:
            ap.error("give --asr-dir and/or --tts-dir (or --dry-run)")
        if a.asr_dir:
            t = time.perf_counter()
            try:
                Handler.ears = load_ears(a.asr_dir, a.threads)
            except Exception as exc:  # noqa: BLE001
                if "MatMulNBits" in str(exc):
                    sys.exit("[speech] this onnxruntime is too old for the Hima model. Update it:  python -m pip install -U onnxruntime")
                raise
            print(f"[speech] speech-to-text ready: {Handler.ears.name} in {time.perf_counter() - t:.1f}s", flush=True)
        if a.tts_dir:
            t = time.perf_counter()
            Handler.voice = ParadeeVoice(a.tts_dir, max(1, a.threads // 2), a.voice)
            print(f"[speech] text-to-speech ready: {Handler.voice.name}, voices {', '.join(Handler.voice.voices)} in {time.perf_counter() - t:.1f}s", flush=True)
    Handler.language, Handler.style = (a.language.strip().lower() or None), a.style
    server = ThreadingHTTPServer((a.host, a.port), Handler)
    print(f"[speech] listening on http://{a.host}:{a.port}/v1  (Ctrl+C stops it)", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
