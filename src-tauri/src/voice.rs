//! The companion's voice. LFM3-Omni's `/tts` takes a short reference recording and speaks in that voice; without one
//! the server warns that "the model may behave randomly", so the voice drifts from sentence to sentence. So each
//! character gets a reference clip, generated once with a fixed seed (a stable voice that is theirs, and a different
//! one per character), or the user's own clip. It is cached on disk and sent with every sentence.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{log, settings::Settings, Core, LockExt};

/// Largest custom voice sample accepted (a 30 s 24 kHz mono 16-bit WAV is about 1.4 MB).
pub const MAX_SAMPLE_BYTES: u64 = 8 * 1024 * 1024;

/// What the generated reference clip says: warm, natural, a full sentence with a little melody in it.
const REFERENCE_LINE: &str = "Hello there! It is really lovely to meet you, and I am happy to help with anything you need today.";

/// A stable seed for a character's voice. Different characters sound different; the same one always sounds the same.
/// `variant` is bumped by "try another voice".
pub fn seed_for(character: &str, variant: u32) -> i64 {
    let mut h: u32 = 0x811c_9dc5; // FNV-1a
    for b in character.bytes().chain(variant.to_le_bytes()) {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    i64::from(h & 0x7fff_ffff)
}

/// Each character's own Paradee voice and speaking speed, in the language I speak. Paradee has eight voices, so
/// speed does the rest: Zap is quick, Pebble is slow, Wisp drifts. (voice id, speed)
pub fn paradee_for(character: &str, lang: &str) -> (&'static str, f64) {
    let (english, speed, female) = match character {
        "pip" => ("af_heart", 1.08, true),
        "mochi" => ("bf_isabella", 0.96, true),
        "bruno" => ("bm_fable", 0.9, false),
        "zap" => ("am_puck", 1.15, false),
        "nimbus" => ("af_heart", 0.9, true),
        "pebble" => ("bm_fable", 0.82, false),
        "ember" => ("am_puck", 1.05, false),
        "bun" => ("bf_isabella", 1.06, true),
        "axo" => ("af_heart", 1.16, true),
        "wisp" => ("bf_isabella", 0.86, true),
        "penny" => ("af_heart", 1.0, true),
        "cosmo" => ("am_puck", 0.95, false),
        _ => ("af_heart", 1.0, true),
    };
    let voice = match lang.to_lowercase().get(..2) {
        Some("hi") => if female { "hf_alpha" } else { "hm_omega" },
        Some("fr") => "ff_siwis",
        Some("es") => "em_alex",
        _ => english,
    };
    (voice, speed)
}

/// Cache key: when it changes the reference has to be made (or loaded) again.
pub fn key(s: &Settings) -> String {
    if s.voice_ref_path.trim().is_empty() {
        format!("auto|{}|{}|{}|{}", s.character, s.tts_voice, s.tts_lang, s.voice_variant)
    } else {
        format!("file|{}", s.voice_ref_path.trim())
    }
}

fn voices_dir(core: &Core) -> PathBuf {
    core.models_dir.parent().map_or_else(|| core.models_dir.join("voices"), |p| p.join("voices"))
}

fn auto_path(core: &Core, s: &Settings) -> PathBuf {
    let clean = |t: &str| t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    voices_dir(core).join(format!("{}-{}-{}-{}.wav", clean(&s.character), clean(&s.tts_voice), clean(&s.tts_lang), s.voice_variant))
}

/// A usable sample: a RIFF/WAVE file of sensible size.
pub fn check_sample(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path.trim());
    let meta = std::fs::metadata(p).map_err(|_| "I can't find that file.".to_string())?;
    if !meta.is_file() {
        return Err("That is a folder; pick a .wav file.".into());
    }
    if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        return Err("The voice sample must be a .wav file.".into());
    }
    if meta.len() > MAX_SAMPLE_BYTES {
        return Err("That recording is too long. Use a clean 5-15 second clip (under 8 MB).".into());
    }
    if meta.len() < 20_000 {
        return Err("That recording is too short. Use a clean 5-15 second clip of one person.".into());
    }
    let mut head = [0u8; 12];
    std::io::Read::read_exact(&mut std::fs::File::open(p).map_err(|e| e.to_string())?, &mut head).map_err(|e| e.to_string())?;
    if &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err("That doesn't look like a valid .wav file.".into());
    }
    Ok(())
}

/// The reference clip for the current voice settings, base64-encoded for `/tts`, or `None` when there is none (the
/// voice then falls back to the plain male/female default).
pub async fn reference(core: &Arc<Core>, s: &Settings) -> Option<String> {
    if s.tts_engine != "model" || !s.runs_local_server() {
        return None; // another engine speaks: it has its own voices
    }
    let k = key(s);
    if let Some((ck, b64)) = core.voice_ref.lk().as_ref() {
        if *ck == k {
            return Some(b64.clone());
        }
    }
    let wav = if !s.voice_ref_path.trim().is_empty() {
        match check_sample(&s.voice_ref_path).and_then(|()| std::fs::read(s.voice_ref_path.trim()).map_err(|e| e.to_string())) {
            Ok(b) => b,
            Err(e) => {
                log::warn(&format!("custom voice sample unusable ({e}); using the built-in voice"));
                return None;
            }
        }
    } else {
        let path = auto_path(core, s);
        match std::fs::read(&path) {
            Ok(b) if b.len() > 20_000 => b,
            _ => {
                let made = generate(core, s).await?;
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = std::fs::write(&path, &made) {
                    log::warn(&format!("couldn't save the voice reference: {e}"));
                }
                made
            }
        }
    };
    let b64 = STANDARD.encode(&wav);
    *core.voice_ref.lk() = Some((k, b64.clone()));
    Some(b64)
}

/// Ask the model to speak the reference line once, with this character's seed.
async fn generate(core: &Arc<Core>, s: &Settings) -> Option<Vec<u8>> {
    log::info(&format!("making the voice for {} ({})", s.character, s.tts_voice));
    let r = core
        .http
        .post(format!("{}/tts", core.base_url()))
        .bearer_auth(&core.api_key)
        .timeout(Duration::from_secs(120))
        .json(&json!({"input": REFERENCE_LINE, "voice": s.tts_voice, "lang": s.tts_lang, "response_format": "wav",
            "seed": seed_for(&s.character, s.voice_variant)}))
        .send()
        .await
        .map_err(|e| log::warn(&format!("voice reference request failed: {e}")))
        .ok()?;
    if !r.status().is_success() {
        log::warn(&format!("voice reference request answered {}", r.status()));
        return None;
    }
    let bytes = r.bytes().await.ok()?;
    (bytes.len() > 20_000 && bytes.starts_with(b"RIFF")).then(|| bytes.to_vec())
}

/// Prepare the voice in the background, so the first thing I say doesn't wait for it.
pub fn warm(core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        let s = core.settings.lk().clone();
        if core.ready.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = reference(&core, &s).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_has_a_voice_of_their_own() {
        let all: std::collections::HashSet<String> =
            crate::settings::CHARACTERS.iter().map(|c| { let (v, s) = paradee_for(c.0, "en"); format!("{v}@{s}") }).collect();
        assert_eq!(all.len(), crate::settings::CHARACTERS.len(), "no two characters sound the same");
        assert_eq!(paradee_for("zap", "en-US"), ("am_puck", 1.15));
        assert_eq!(paradee_for("zap", "hi"), ("hm_omega", 1.15));
        assert_eq!(paradee_for("mochi", "HI-in").0, "hf_alpha");
        assert_eq!(paradee_for("bruno", "fr").0, "ff_siwis");
        assert_eq!(paradee_for("nobody", "es").0, "em_alex");
    }

    #[test]
    fn seeds_are_stable_and_differ_by_character_and_variant() {
        assert_eq!(seed_for("pip", 0), seed_for("pip", 0));
        assert_ne!(seed_for("pip", 0), seed_for("mochi", 0));
        assert_ne!(seed_for("pip", 0), seed_for("pip", 1));
        assert!(seed_for("zap", 7) >= 0);
    }

    #[test]
    fn the_cache_key_follows_what_shapes_the_voice() {
        let a = Settings::default();
        let mut b = a.clone();
        assert_eq!(key(&a), key(&b));
        b.tts_voice = "male".into();
        assert_ne!(key(&a), key(&b));
        let mut c = a.clone();
        c.character = "mochi".into();
        assert_ne!(key(&a), key(&c));
        let mut d = a.clone();
        d.voice_variant = 3;
        assert_ne!(key(&a), key(&d));
        let mut e = a.clone();
        e.voice_ref_path = "C:/me.wav".into();
        assert!(key(&e).starts_with("file|"));
        e.name = "Different".into(); // the display name doesn't change the voice
        assert_eq!(key(&e), key(&e.clone()));
    }

    #[test]
    fn voice_samples_are_checked() {
        let d = std::env::temp_dir().join(format!("nuzzle-voice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut good = b"RIFF\x00\x00\x00\x00WAVEfmt ".to_vec();
        good.resize(40_000, 0);
        let ok = d.join("me.wav");
        std::fs::write(&ok, &good).unwrap();
        assert!(check_sample(ok.to_str().unwrap()).is_ok());
        let short = d.join("short.wav");
        std::fs::write(&short, &good[..100]).unwrap();
        assert!(check_sample(short.to_str().unwrap()).unwrap_err().contains("too short"));
        let mp3 = d.join("me.mp3");
        std::fs::write(&mp3, &good).unwrap();
        assert!(check_sample(mp3.to_str().unwrap()).unwrap_err().contains(".wav"));
        let fake = d.join("fake.wav");
        std::fs::write(&fake, vec![1u8; 40_000]).unwrap();
        assert!(check_sample(fake.to_str().unwrap()).unwrap_err().contains("valid"));
        assert!(check_sample(d.to_str().unwrap()).is_err());
        assert!(check_sample("C:/nope/none.wav").is_err());
    }
}
