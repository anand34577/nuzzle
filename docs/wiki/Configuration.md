Settings are saved as `settings.json` in `%APPDATA%\dev.companion.desktop`. Tokens for connectors and MCP servers are not in that file; they're in Windows Credential Manager. Everything below is edited in the Settings window.

## Setup

| Setting | What it does |
|---|---|
| Inference engine | Which `llama-server` to use. The default is the bundled one. You can point it at your own build. |
| Model | The LFM3-Omni files. Download them here, or choose files you already have. |
| Graphics cards | Which GPU(s) run the model. |

## Character, Chat, Memory

| Tab | What it does |
|---|---|
| Character | Choose a character, size, playfulness (quiet, normal, lively) and the roaming area. |
| Chat | Shows this session's conversation. It's kept in memory only and is gone when the app quits. |
| Memory | Facts the character remembers, and reminders. You can delete either. |

## Tools

Built-in tools, MCP servers and web connectors. See [Using Nuzzle](Usage#mcp-servers).

## Voice & sight

| Setting | What it does |
|---|---|
| Microphone, speaker | Choose devices. A level meter, chime and voice test are built in. If a headset is unplugged, Nuzzle switches device and tells you. |
| Volume, mute spoken replies | Mute keeps the text bubbles. |
| Language you speak | Helps it understand you. |
| Push to talk | Hold `Ctrl+Shift+Space` to speak. |
| My voice | Try other voices, or give it a recording of a voice you have permission to use. |

## Safety

| Mode | Behaviour |
|---|---|
| Ask for risky things (default) | Asks before writing files, running commands, reading your clipboard and pressing Enter. |
| Ask for everything | Asks before every action. |
| Full access | Never asks. A warning appears once before you can turn it on. |

## Appearance and Advanced

| Setting | What it does |
|---|---|
| Theme, density, font | Light, dark or system; compact or comfortable. |
| GPU layers | 99 puts the whole model on the GPU. 0 uses the processor only. |
| Memory size (tokens) | The context window. Larger values remember more of the conversation and use more GPU memory. |
| Extra llama-server options | Passed straight to `llama-server`. Leave empty unless you know you need something. |
| Open log folder | Opens the folder with the app's log files. |

## Other brains and speech engines

Settings -> Advanced has two cards for this. The defaults (everything in the local LFM3-Omni model) need no changes.

### Brain: a chat service

Choose *A chat service*, then enter the service address (it ends in `/v1`), the model name and, if needed, an API key.
Any OpenAI-compatible service works: OpenAI, OpenRouter, Groq, Together, Mistral, DeepSeek, or a server on this computer
such as Ollama (`http://localhost:11434/v1`) or LM Studio (`http://localhost:1234/v1`). The model has to support tool
calls. Turn off *This model can look at pictures* for a text-only model.

With a service on the internet, what you say and type, the memory notes and any screenshots are sent to that service.
Tools, files and approval cards stay on this computer. The key is stored in the system credential store.

If no local model is set up, hearing and speaking need their own engines (below); typing works either way.

### Ears and voice: your own speech servers

Choose *Speech server* for hearing, speaking or both, and enter the server address. Hearing uses
`POST <address>/audio/transcriptions` (a WAV file in a multipart form, with a `language` hint taken from the speech
language), speaking uses `POST <address>/audio/speech` and plays 16-bit PCM or WAV.

`scripts/speech-server.py` is such a server for two small models that run on the CPU:

```bash
pip install -U onnxruntime numpy tokenizers soundfile
pip install git+https://github.com/sahilmahendrakar/paradee
python scripts/speech-server.py --asr-dir "path/to/hima-asr" --tts-dir "path/to/Paradee/models"
```

- `--asr-dir`: the Hima-ASR ONNX folder (`onnx/encoder_model_q4.onnx`, `onnx/decoder_model_int8.onnx`, `tokenizer.json`,
  `embed.npy`, `keep_ids.json`, `start_of_text.npy`, `mel_filters.npy`). It needs a recent onnxruntime (1.19 is too old;
  1.31 works).
- `--tts-dir`: the folder with `paradee_int8.onnx` and `config.json`. Voices: `af_heart`, `am_puck`, `bf_isabella`,
  `bm_fable` (English), `hf_alpha`, `hm_omega` (Hindi), `ff_siwis` (French), `em_alex` (Spanish).
- `--language hi` fixes the spoken language when Nuzzle sends no hint; `--dry-run` starts it with fake engines to check
  the connection.

Then enter `http://127.0.0.1:8765/v1` for both servers in Settings and set the voice (for example `af_heart`). The
server has to be running before you talk; Nuzzle tells you once if it can't reach it.

### Letting Nuzzle run the speech server

Instead of starting `speech-server.py` yourself, turn on *Run the speech server for me* in the same card and fill in the
Hima-ASR folder, the Paradee folder (either may stay empty), and the Python to use (empty means `python` on PATH; it
needs the packages above). Saving points hearing and speaking at it. The pill next to the title shows *Starting...*,
*Running* or *Not running* with the reason, and the server's own lines are in the log under `SPEECH`.

With the voice field left empty, each character uses a Paradee voice and pace of their own.

### Feeling your voice

If the Hima folder is the whole `Prahaladha/hima-asr-ONNX` repository (it has `hima_onnx.py` and a `decide/` folder next
to `onnx/`), the server also answers how you sound: tone, emotion and urgency. While you talk, the character's
expression follows it, and the model is told (for example "the user's voice sounds worried") so it can answer in kind.
*Hear how you say things* in Voice & sight switches this off. A folder with only the ONNX files still transcribes.

## Morning hello

Settings -> Character -> *Morning hello*. Between 5 and 12, the first time you are at the computer, I say today's
reminders and, if you entered your town, the weather (from Open-Meteo). It happens once a day, only when there is
something to say, and never when playfulness is *quiet*.
