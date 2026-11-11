//! Runs the bundled speech server (`scripts/speech-server.py`: Hima-ASR for hearing, Paradee for speaking) for as
//! long as the app is open, when the user asks for that in Settings -> Advanced -> Ears and voice. It is started
//! with the user's Python, restarted when its settings change, stopped on quit, and its output goes to the log.

use serde_json::json;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};

use crate::{log, settings::Settings, Core, LockExt};

/// The server itself travels inside the app, so there is nothing extra to install or find.
const SCRIPT: &str = include_str!("../../scripts/speech-server.py");

/// The address the server listens on (this computer only), as entered for the speech engines.
pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// What is needed to run it, or `None` when the user hasn't asked for it.
fn wanted(s: &Settings) -> Option<(String, Vec<String>)> {
    if !s.speech_server_auto {
        return None;
    }
    let mut args = vec!["--port".to_string(), s.speech_port.to_string()];
    for (flag, dir) in [("--asr-dir", &s.speech_asr_dir), ("--tts-dir", &s.speech_tts_dir)] {
        if !dir.trim().is_empty() {
            args.extend([flag.to_string(), dir.trim().to_string()]);
        }
    }
    if args.len() == 2 {
        return None; // no model folder given: nothing to serve
    }
    if !s.tts_api_voice.trim().is_empty() {
        args.extend(["--voice".to_string(), s.tts_api_voice.trim().to_string()]);
    }
    let python = if s.speech_python.trim().is_empty() { "python".to_string() } else { s.speech_python.trim().to_string() };
    Some((python, args))
}

/// The settings that decide how the server runs: a change in any of them restarts it.
pub fn key(s: &Settings) -> String {
    format!("{:?}", wanted(s))
}

fn set(core: &Core, state: &str, detail: &str) {
    *core.speech.lk() = json!({"state": state, "detail": detail});
    core.emit("speech", json!(null));
}

/// Words for the two failures people will actually meet.
fn explain(last: &str) -> String {
    if last.contains("too old for the Hima model") {
        "Its onnxruntime is too old for the Hima model. In a terminal: python -m pip install -U onnxruntime".into()
    } else if last.contains("No module named") {
        format!("That Python is missing a package ({}). See Configuration in the wiki for the pip line.", last.trim())
    } else if last.is_empty() {
        "It stopped without saying why. The log has the details.".into()
    } else {
        last.chars().take(220).collect()
    }
}

fn script_path(core: &Core) -> std::io::Result<PathBuf> {
    let dir = core.models_dir.parent().map_or_else(|| core.models_dir.clone(), PathBuf::from);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("speech-server.py");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&path, SCRIPT)?;
    }
    Ok(path)
}

/// Copy the server's lines to the log; remember the last one and notice when it says it is listening.
fn follow(out: impl AsyncRead + Unpin + Send + 'static, core: Arc<Core>, last: Arc<std::sync::Mutex<String>>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(out).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            log::write("SPEECH", &l);
            if l.contains("listening on") {
                set(&core, "ready", "Running");
            }
            if !l.trim().is_empty() {
                *last.lk() = l;
            }
        }
    });
}

pub fn spawn(core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        let mut quick_failures = 0u32;
        loop {
            let s = core.settings.lk().clone();
            let Some((python, args)) = wanted(&s) else {
                set(&core, "off", "");
                core.speech_restart.notified().await;
                quick_failures = 0;
                continue;
            };
            let script = match script_path(&core) {
                Ok(p) => p,
                Err(e) => {
                    set(&core, "error", &format!("I couldn't write the speech server file ({e})."));
                    core.speech_restart.notified().await;
                    continue;
                }
            };
            let mut c = Command::new(&python);
            c.arg(&script).args(&args).env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8");
            c.kill_on_drop(true).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            crate::job::prepare(&mut c);
            #[cfg(windows)]
            c.creation_flags(0x0800_0000); // no console window
            set(&core, "starting", "Loading the speech models...");
            let mut child = match c.spawn() {
                Ok(ch) => ch,
                Err(e) => {
                    log::warn(&format!("speech server: couldn't start '{python}': {e}"));
                    set(&core, "error", &format!("I couldn't start Python ('{python}': {e}). Enter the full path to python in Settings."));
                    core.speech_restart.notified().await;
                    continue;
                }
            };
            crate::job::adopt(&child);
            let pid = child.id();
            if let Some(pid) = pid {
                core.child_pids.lk().push(pid);
                log::info(&format!("speech server started (pid {pid}, port {})", s.speech_port));
            }
            let last = Arc::new(std::sync::Mutex::new(String::new()));
            if let Some(o) = child.stdout.take() {
                follow(o, core.clone(), last.clone());
            }
            if let Some(e) = child.stderr.take() {
                follow(e, core.clone(), last.clone());
            }
            let started = Instant::now();
            let restart = tokio::select! {
                _ = child.wait() => false,
                _ = core.speech_restart.notified() => true,
            };
            let _ = child.kill().await;
            if let Some(pid) = pid {
                core.child_pids.lk().retain(|p| *p != pid);
            }
            if restart {
                quick_failures = 0;
                continue;
            }
            // It stopped by itself.
            tokio::time::sleep(Duration::from_millis(200)).await; // let the last lines arrive
            let why = explain(&last.lk());
            log::warn(&format!("speech server stopped: {why}"));
            quick_failures = if started.elapsed() > Duration::from_secs(60) { 1 } else { quick_failures + 1 };
            if quick_failures >= 2 {
                set(&core, "error", &why);
                core.status(format!("My speech server won't stay up. {why}"));
                core.speech_restart.notified().await; // wait for the settings to change
                quick_failures = 0;
            } else {
                set(&core, "starting", "It stopped; starting it again...");
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(3)) => {}, _ = core.speech_restart.notified() => {} }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_runs_when_asked_and_given_a_model() {
        let off = Settings { speech_asr_dir: "C:/models/hima".into(), ..Settings::default() };
        assert_eq!(wanted(&off), None);
        let no_models = Settings { speech_server_auto: true, ..Settings::default() };
        assert_eq!(wanted(&no_models), None);
        let on = Settings { speech_server_auto: true, speech_asr_dir: " C:/models/hima ".into(), tts_api_voice: "hf_alpha".into(), ..Settings::default() };
        let (python, args) = wanted(&on).unwrap();
        assert_eq!(python, "python");
        assert_eq!(args, ["--port", "8765", "--asr-dir", "C:/models/hima", "--voice", "hf_alpha"]);
        let both = Settings { speech_tts_dir: "D:/paradee".into(), speech_python: "C:/py/python.exe".into(), speech_port: 9000, ..on.clone() };
        let (python, args) = wanted(&both).unwrap();
        assert_eq!(python, "C:/py/python.exe");
        assert!(args.windows(2).any(|w| w == ["--tts-dir", "D:/paradee"]) && args[1] == "9000");
        assert_ne!(key(&on), key(&both), "a change restarts the server");
        assert_eq!(key(&on), key(&Settings { name: "Mochi".into(), ..on.clone() }), "unrelated settings don't");
        assert_eq!(url(8765), "http://127.0.0.1:8765/v1");
    }

    #[test]
    fn failures_are_explained() {
        assert!(explain("[speech] this onnxruntime is too old for the Hima model. Update it: ...").contains("pip install -U onnxruntime"));
        assert!(explain("ModuleNotFoundError: No module named 'paradee'").contains("missing a package"));
        assert!(explain("").contains("log"));
        assert!(SCRIPT.contains("/audio/transcriptions") && SCRIPT.contains("/audio/speech"));
    }
}
