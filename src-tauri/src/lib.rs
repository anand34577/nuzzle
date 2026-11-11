// Byte-slice decoding below reads clearer with chunks_exact than with as_chunks.
#![allow(clippy::chunks_exact_to_as_chunks)]

mod agent_api;
mod audio;
mod avoid;
mod brain;
mod control;
mod download;
mod hello;
mod job;
mod log;
mod mcp;
mod memory;
mod quick;
mod reminders;
mod roam;
mod screen;
mod secrets;
mod settings;
mod setup;
mod speech;
mod sys;
mod telegram;
mod tools;
mod vad;
mod voice;

use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, RunEvent, State, WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::sync::{mpsc, oneshot, Notify};

use audio::{now_ms, Audio};
use screen::Geom;
use settings::Settings;

/// Lock a std mutex without panicking if another thread died while holding it.
pub trait LockExt<T> {
    fn lk(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lk(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Everything shared between the UI, the roaming loop and the brain.
pub struct Core {
    pub app: AppHandle,
    pub settings: Mutex<Settings>,
    settings_path: PathBuf,
    memory_path: PathBuf,
    pub models_dir: PathBuf,
    pub log_dir: PathBuf,
    pub memory: Mutex<Vec<memory::Entry>>,
    pub mcp: Arc<mcp::Hub>,
    pub reminders: Mutex<Vec<reminders::Reminder>>,
    reminders_path: PathBuf,
    pub downloading: AtomicBool,
    pub dl_cancel: AtomicBool,
    pub http: reqwest::Client,
    /// The user's microphone switch.
    pub mic: AtomicBool,
    /// What the audio thread obeys: the switch, and in push-to-talk mode also the key being held.
    pub gate: Arc<AtomicBool>,
    pub ptt_held: AtomicBool,
    /// Lets push-to-talk end an utterance the moment the key is released (an empty chunk).
    pub mic_tx: mpsc::Sender<Vec<u8>>,
    /// Global shortcuts currently registered from settings (chat, push-to-talk).
    shortcuts: Mutex<Vec<String>>,
    pub screen: AtomicBool,
    /// Master switch for anything that acts on the computer. Off until the user turns it on.
    pub hands: AtomicBool,
    /// Whether the companion wanders around the screen.
    pub roam: AtomicBool,
    /// True while the pointer/keyboard is being driven; the window is click-through then.
    pub acting: AtomicBool,
    /// Current on-screen size of the character in percent (mirrors settings.size_pct once applied).
    pub pet_pct: AtomicU32,
    /// Bumped on every stop; in-flight work compares against the value it started with.
    pub stop_gen: Arc<AtomicU64>,
    pub geom: Mutex<Option<Geom>>,
    pub audio: Audio,
    pub mic_rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
    pub history: tokio::sync::Mutex<Vec<Value>>,
    pub tts_tx: Mutex<Option<mpsc::UnboundedSender<(String, u64)>>>,
    pub ready: AtomicBool,
    pub child_pids: Mutex<Vec<u32>>,
    pub restart: Notify,
    pub act_lock: tokio::sync::Mutex<()>,
    pub last_activity: AtomicU64,
    /// Port of the running llama-server (random per launch) and the key it demands.
    pub port: AtomicU16,
    pub api_key: String,
    /// Waiting for the user to answer an approval card.
    pub approval: Mutex<Option<oneshot::Sender<bool>>>,
    pub approved_hooks: Mutex<HashSet<String>>,
    /// Clickable rectangles (CSS px, window-relative) reported by the page; everything else is click-through.
    pub hit: Mutex<Vec<[f64; 4]>>,
    pub roam_item: Mutex<Option<CheckMenuItem<Wry>>>,
    pub char_items: Mutex<Vec<(String, CheckMenuItem<Wry>)>>,
    /// Last model-server state: {"state": setup|loading|ready|error, "detail": text}.
    pub brain: Mutex<Value>,
    /// What was said this session (kept in memory only, never written to disk): {role, text, t}.
    pub transcript: Mutex<Vec<Value>>,
    /// Number of conversation turns in progress (hearing, thinking, replying). While any, I stay where I am.
    engaged: AtomicU32,
    /// The typing box is open.
    pub chatting: AtomicBool,
    /// The user is speaking to me right now (voice activity detected, utterance not finished).
    pub hearing: AtomicBool,
    /// Sentences queued for speech and not yet played.
    pub tts_pending: AtomicU32,
    /// Where the user last clicked outside me (physical px) and when: normally the box they are typing in.
    pub click_spot: Mutex<Option<(i32, i32, Instant)>>,
    /// The voice reference in use: (cache key, base64 WAV). See `voice.rs`.
    pub voice_ref: Mutex<Option<(String, String)>>,
    /// When the user last pressed on me (so I don't dodge away from someone who is petting or poking me).
    pub last_pet_touch: AtomicU64,
    /// "Come here": the roaming loop runs over to the pointer.
    pub summon: AtomicBool,
    /// Where the conversation turn in progress came from, when it isn't this computer (Telegram, agent API).
    pub remote: Mutex<Option<brain::Origin>>,
    /// Wakes the Telegram bot and the agent API when their settings change.
    pub remote_restart: Notify,
    /// The Telegram pairing code on show in Settings, and when it was made.
    pub pair_code: Mutex<Option<(String, Instant)>>,
    /// The bot's @username once Telegram accepted the token.
    pub telegram_bot: Mutex<Option<String>>,
    /// The approval card posted to Telegram (chat, message), so its buttons can be removed once answered.
    pub tg_approval: Mutex<Option<(i64, i64)>>,
    /// The bundled speech server: {"state": off|starting|ready|error, "detail": text}, and its wake-up call.
    pub speech: Mutex<Value>,
    pub speech_restart: Notify,
    /// How the user's voice sounded in the utterance being answered ("cheerful", "worried"), when the ears can tell.
    pub feeling: Mutex<Option<String>>,
}

/// Held while a conversation turn is in progress; the companion doesn't wander off until it is dropped.
pub struct Engaged(Arc<Core>);

impl Drop for Engaged {
    fn drop(&mut self) {
        self.0.engaged.fetch_sub(1, Ordering::SeqCst);
        self.0.touch(); // and then stays a few more seconds, so he doesn't dash off mid-goodbye
    }
}

impl Core {
    pub fn emit(&self, ev: &str, payload: Value) {
        let _ = self.app.emit(ev, payload);
    }

    pub fn status(&self, msg: impl Into<String>) {
        let msg = msg.into();
        if !msg.is_empty() {
            log::info(&format!("status: {msg}"));
        }
        self.emit("status", json!(msg));
    }

    pub fn set_brain(&self, state: &str, detail: &str) {
        let v = json!({"state": state, "detail": detail});
        *self.brain.lk() = v.clone();
        self.emit("brain", v);
    }

    /// Remember one line of the conversation (newest 200 kept) and tell the settings window.
    pub fn transcript_add(&self, role: &str, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        {
            let mut t = self.transcript.lk();
            t.push(json!({"role": role, "text": text, "t": sys::unix_now()}));
            let extra = t.len().saturating_sub(200);
            t.drain(..extra);
        }
        self.emit("transcript", json!(null));
    }

    pub fn engage(self: &Arc<Self>) -> Engaged {
        self.engaged.fetch_add(1, Ordering::SeqCst);
        Engaged(self.clone())
    }

    /// True while something is going on that I shouldn't wander away from: a conversation turn, speech, the typing
    /// box, an approval card, push-to-talk, or the pointer/keyboard being driven.
    pub fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::SeqCst) > 0
            || self.chatting.load(Ordering::SeqCst)
            || self.hearing.load(Ordering::SeqCst)
            || self.tts_pending.load(Ordering::SeqCst) > 0
            || self.ptt_held.load(Ordering::Relaxed)
            || self.acting.load(Ordering::Relaxed)
            || self.audio.is_playing()
            || self.approval.lk().is_some()
    }

    pub fn touch(&self) {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port.load(Ordering::Relaxed))
    }

    pub fn flags(&self) -> Value {
        let s = self.settings.lk();
        json!({
            "mic": self.mic.load(Ordering::Relaxed),
            "screen": self.screen.load(Ordering::Relaxed),
            "hands": self.hands.load(Ordering::Relaxed),
            "roam": self.roam.load(Ordering::Relaxed),
            "configured": s.configured(),
            "ready": self.ready.load(Ordering::Relaxed),
            "name": s.name,
            "character": s.character,
            "playfulness": s.playfulness,
            "size_pct": s.size_pct,
            "listen_mode": s.listen_mode,
            "ptt_held": self.ptt_held.load(Ordering::Relaxed),
            "ptt_shortcut": s.ptt_shortcut,
            "chat_shortcut": s.chat_shortcut,
            "speak_aloud": s.speak_aloud,
            "platform": std::env::consts::OS,
        })
    }

    /// Open the mic to the voice loop only when the switch is on and, in push-to-talk mode, the key is held.
    pub fn update_gate(&self) {
        let push = self.settings.lk().listen_mode == "push";
        let open = self.mic.load(Ordering::SeqCst) && (!push || self.ptt_held.load(Ordering::SeqCst));
        self.gate.store(open, Ordering::SeqCst);
    }

    /// Push-to-talk key went down or up. Pressing it while I speak interrupts me, so you can talk over me.
    pub fn push_to_talk(&self, down: bool) {
        if self.ptt_held.swap(down, Ordering::SeqCst) == down {
            return; // key repeat
        }
        if down {
            if self.audio.level_out() > 0.005 {
                self.stop_gen.fetch_add(1, Ordering::SeqCst);
                self.audio.flush();
            }
            self.audio.release_hold();
            self.touch();
        }
        self.update_gate();
        if !down {
            let _ = self.mic_tx.try_send(vec![]); // end the utterance now
        }
        self.push_flags();
    }

    /// Remember the mic / screen switches so the next launch starts the same way.
    fn remember_switch(&self, name: &str, on: bool) {
        let mut s = self.settings.lk();
        match name {
            "mic" => s.mic_on = on,
            "screen" => s.screen_on = on,
            _ => return,
        }
        if let Err(e) = s.save(&self.settings_path) {
            log::warn(&format!("saving the {name} switch failed: {e}"));
        }
    }

    pub fn push_flags(&self) {
        self.emit("flags", self.flags());
    }

    /// Answer a pending approval card (false when the user says no, stops, or it times out).
    pub fn answer_approval(&self, yes: bool) {
        if let Some(tx) = self.approval.lk().take() {
            let _ = tx.send(yes);
        }
    }

    /// (Re)start the MCP servers from the current settings.
    pub fn mcp_reload(self: &Arc<Self>) {
        let (core, cfgs) = (self.clone(), self.settings.lk().mcp_servers.clone());
        tauri::async_runtime::spawn(async move {
            let hub = core.mcp.clone();
            hub.reload(&core, cfgs).await;
        });
    }

    /// Add a reminder `in_secs` from now; `every` > 0 makes it repeat that many seconds apart.
    pub fn remind_add(&self, in_secs: u64, every: u64, text: &str) -> Result<reminders::Reminder, String> {
        let r = {
            let mut list = self.reminders.lk();
            let r = reminders::add_repeating(&mut list, sys::unix_now(), in_secs, every, text)?;
            if let Err(e) = reminders::save(&self.reminders_path, &list) {
                list.retain(|x| x.id != r.id);
                return Err(format!("I couldn't save that reminder ({e})."));
            }
            r
        };
        self.emit("reminders", json!(null));
        Ok(r)
    }

    pub fn remind_delete(&self, id: u64) -> Result<(), String> {
        {
            let mut list = self.reminders.lk();
            let before = list.len();
            list.retain(|r| r.id != id);
            if list.len() == before {
                return Err("That reminder is already gone.".into());
            }
            reminders::save(&self.reminders_path, &list).map_err(|e| format!("I couldn't save the change ({e})."))?;
        }
        self.emit("reminders", json!(null));
        Ok(())
    }

    /// Start a fresh conversation: the model forgets this session's chat (long-term memory stays).
    pub fn forget_chat(self: &Arc<Self>) {
        self.transcript.lk().clear();
        self.emit("transcript", json!(null));
        let c = self.clone();
        tauri::async_runtime::spawn(async move {
            c.history.lock().await.clear(); // waits for a turn in progress to finish
        });
    }

    /// Say a due reminder out loud, and bring the companion back on screen if it was hidden.
    fn announce(&self, text: &str) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.show();
        }
        self.touch();
        self.emit("reminder", json!(text));
        brain::speak(self, &format!("Reminder: {text}"));
    }

    /// Add a note to long-term memory and save it.
    pub fn mem_add(&self, text: &str, by: &str) -> Result<memory::Entry, String> {
        let entry = {
            let mut list = self.memory.lk();
            let e = memory::add(&mut list, text, by)?;
            if let Err(err) = memory::save(&self.memory_path, &list) {
                memory::delete(&mut list, e.id);
                log::warn(&format!("saving memory failed: {err}"));
                return Err(format!("I couldn't save that note ({err})."));
            }
            e
        };
        self.emit("memory", json!(null));
        Ok(entry)
    }

    pub fn mem_delete(&self, id: u64) -> Result<(), String> {
        {
            let mut list = self.memory.lk();
            if !memory::delete(&mut list, id) {
                return Err("That note is already gone.".into());
            }
            memory::save(&self.memory_path, &list).map_err(|e| format!("I couldn't save the change ({e})."))?;
        }
        self.emit("memory", json!(null));
        Ok(())
    }

    pub fn mem_clear(&self) -> Result<(), String> {
        {
            let mut list = self.memory.lk();
            list.clear();
            memory::save(&self.memory_path, &list).map_err(|e| format!("I couldn't save the change ({e})."))?;
        }
        self.emit("memory", json!(null));
        Ok(())
    }

    /// Instantly end whatever the companion is doing and give the user the controls back.
    pub fn stop_everything(&self) {
        self.stop_gen.fetch_add(1, Ordering::SeqCst);
        self.hands.store(false, Ordering::SeqCst);
        self.answer_approval(false);
        self.audio.flush();
        self.push_flags();
        self.status("Stopped. You have control.");
    }

    /// Resize the character to `settings.size_pct`, keeping its feet where they were. Only the character scales: the
    /// page stays at normal zoom so bubbles, controls and text keep their size, and the window grows around him.
    pub fn apply_size(&self) {
        let pct = self.settings.lk().size_pct.clamp(40, 200);
        let Some(w) = self.app.get_webview_window("main") else { return };
        let (lw, lh) = settings::window_size(f64::from(pct) / 100.0);
        let sf = w.scale_factor().unwrap_or(1.0);
        let (old, pos) = (w.outer_size().ok(), w.outer_position().ok());
        let _ = w.set_zoom(1.0);
        let _ = w.set_size(LogicalSize::new(lw, lh));
        if let (Some(o), Some(p)) = (old, pos) {
            let (nw, nh) = ((lw * sf) as i32, (lh * sf) as i32);
            let _ = w.set_position(PhysicalPosition::new(p.x + (o.width as i32 - nw) / 2, p.y + o.height as i32 - nh));
        }
        self.pet_pct.store(pct, Ordering::Relaxed);
        self.push_flags();
    }

    /// Save settings changed from the backend side (e.g. a Telegram chat was paired) and tell the settings window.
    pub fn save_settings_now(&self, s: Settings) -> Result<(), String> {
        s.save(&self.settings_path).map_err(|e| e.to_string())?;
        *self.settings.lk() = s;
        self.emit("settings-changed", json!(null));
        Ok(())
    }

    pub fn call_over(&self) {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.show();
        }
        self.summon.store(true, Ordering::SeqCst);
    }

    pub fn set_roam(&self, on: bool) {
        self.roam.store(on, Ordering::SeqCst);
        if let Some(item) = self.roam_item.lk().as_ref() {
            let _ = item.set_checked(on);
        }
        self.push_flags();
    }

    /// Use the downloaded model when none is configured yet, so a finished download "just works" without the
    /// user having to press Save. Returns true when the settings changed. `wake` restarts the brain; at startup it
    /// isn't running yet and picks the new settings up by itself (a stored wake-up would start it twice).
    pub fn adopt_downloaded(&self, wake: bool) -> bool {
        let [m, p] = [&download::ITEMS[0], &download::ITEMS[1]].map(|i| self.models_dir.join(i.name).to_string_lossy().to_string());
        let mut s = self.settings.lk().clone();
        if !s.model.trim().is_empty() || !setup::check_path(&m, "gguf").ok {
            return false;
        }
        s.model = m;
        if s.mmproj.trim().is_empty() && setup::check_path(&p, "mmproj").ok {
            s.mmproj = p;
        }
        if let Err(e) = s.save(&self.settings_path) {
            log::warn(&format!("saving the downloaded model choice failed: {e}"));
            return false;
        }
        log::info("using the downloaded model");
        *self.settings.lk() = s;
        self.push_flags();
        self.emit("settings-changed", json!(null));
        if wake {
            self.restart.notify_one();
        }
        true
    }

    pub fn window_center(&self) -> Option<(i32, i32)> {
        let w = self.app.get_webview_window("main")?;
        let p = w.outer_position().ok()?;
        let s = w.outer_size().ok()?;
        Some((p.x + s.width as i32 / 2, p.y + s.height as i32 / 2))
    }

    fn kill_children(&self) {
        for pid in self.child_pids.lk().drain(..) {
            job::kill_tree(pid);
        }
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(true) {
            let _ = w.hide();
        } else {
            let _ = w.show();
        }
    }
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit_to("settings", "settings-shown", ());
    }
}

/// Switch the on-screen character (one at a time). The name follows along while it is still a default name.
fn apply_character(core: &Core, id: &str) -> Result<(), String> {
    let Some(&(_, new_name, _)) = settings::CHARACTERS.iter().find(|c| c.0 == id) else {
        return Err("Unknown character.".into());
    };
    {
        let mut s = core.settings.lk();
        let was_default = settings::CHARACTERS.iter().any(|c| c.1 == s.name);
        s.character = id.to_string();
        if was_default {
            s.name = new_name.to_string();
        }
        s.save(&core.settings_path).map_err(|e| format!("I couldn't save your choice ({e})."))?;
    }
    for (cid, item) in core.char_items.lk().iter() {
        let _ = item.set_checked(cid == id);
    }
    core.push_flags();
    core.emit("character", json!(id));
    Ok(())
}

#[tauri::command]
fn list_memory(core: State<'_, Arc<Core>>) -> Vec<memory::Entry> {
    core.memory.lk().clone()
}

#[tauri::command]
fn add_memory(core: State<'_, Arc<Core>>, text: String) -> Result<memory::Entry, String> {
    core.mem_add(&text, "user")
}

#[tauri::command]
fn delete_memory(core: State<'_, Arc<Core>>, id: u64) -> Result<(), String> {
    core.mem_delete(id)
}

#[tauri::command]
fn clear_memory(core: State<'_, Arc<Core>>) -> Result<(), String> {
    core.mem_clear()
}

/// "Try another voice": the same character with a different voice. Applies at once; press "Hear my voice" to compare.
#[tauri::command]
fn voice_next(core: State<'_, Arc<Core>>) -> Result<u32, String> {
    let n = {
        let mut s = core.settings.lk();
        s.voice_variant = s.voice_variant.wrapping_add(1);
        s.save(&core.settings_path).map_err(|e| format!("I couldn't save that ({e})."))?;
        s.voice_variant
    };
    *core.voice_ref.lk() = None;
    voice::warm(core.inner().clone());
    Ok(n)
}

/// Is this file usable as a voice sample? (`ok` plus a message for the settings window.)
#[tauri::command]
fn check_voice(path: String) -> Value {
    match voice::check_sample(&path) {
        Ok(()) => json!({"ok": true, "message": "This will be my voice. Only use recordings of people who agreed to it."}),
        Err(e) => json!({"ok": false, "message": e}),
    }
}

/// The typing box opened or closed (so I don't wander off while you type to me).
#[tauri::command]
fn set_chatting(core: State<'_, Arc<Core>>, on: bool) {
    core.chatting.store(on, Ordering::SeqCst);
    if on {
        core.touch();
    }
}

#[tauri::command]
fn list_transcript(core: State<'_, Arc<Core>>) -> Vec<Value> {
    core.transcript.lk().clone()
}

#[tauri::command]
fn clear_transcript(core: State<'_, Arc<Core>>) {
    core.inner().forget_chat(); // the model forgets it too, not only the list on screen
}

#[tauri::command]
fn download_status(core: State<'_, Arc<Core>>) -> Value {
    let items: Vec<Value> = download::ITEMS
        .iter()
        .map(|i| {
            let p = core.models_dir.join(i.name);
            let ready = std::fs::metadata(&p).is_ok_and(|m| m.len() == i.size);
            json!({"key": i.key, "name": i.name, "size": i.size, "ready": ready, "path": p.to_string_lossy()})
        })
        .collect();
    json!({"running": core.downloading.load(Ordering::SeqCst), "items": items})
}

#[tauri::command]
fn start_download(core: State<'_, Arc<Core>>) -> Result<(), String> {
    if core.downloading.swap(true, Ordering::SeqCst) {
        return Err("A download is already running.".into());
    }
    core.dl_cancel.store(false, Ordering::SeqCst);
    let c = core.inner().clone();
    tauri::async_runtime::spawn(async move {
        download::run(c.clone()).await;
        c.downloading.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn cancel_download(core: State<'_, Arc<Core>>) {
    core.dl_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn list_reminders(core: State<'_, Arc<Core>>) -> Vec<reminders::Reminder> {
    core.reminders.lk().clone()
}

#[tauri::command]
fn delete_reminder(core: State<'_, Arc<Core>>, id: u64) -> Result<(), String> {
    core.remind_delete(id)
}

#[tauri::command]
fn mcp_status(core: State<'_, Arc<Core>>) -> Value {
    json!({"servers": core.mcp.statuses(), "tool_count": core.mcp.tool_count()})
}

#[tauri::command]
fn mcp_reload(core: State<'_, Arc<Core>>) {
    core.inner().mcp_reload();
}

#[tauri::command]
fn send_text(core: State<'_, Arc<Core>>, text: String) -> Result<(), String> {
    let t = text.trim().to_string();
    if t.is_empty() {
        return Err("Type something first.".into());
    }
    if t.chars().count() > 2000 {
        return Err("That message is too long (2000 characters max).".into());
    }
    let c = core.inner().clone();
    // Slash commands (/timer, /note, /calc ...) are done at once and don't need the model.
    if let Some(cmd) = quick::parse(&t) {
        tauri::async_runtime::spawn(async move {
            let reply = quick::run(&c, cmd).await;
            c.touch();
            c.transcript_add("user", &t);
            c.transcript_add("assistant", &reply);
            c.emit("quick", json!(reply));
        });
        return Ok(());
    }
    if !core.ready.load(Ordering::SeqCst) {
        let why = if core.settings.lk().model.trim().is_empty() {
            "I have no brain yet: open Settings -> Setup to download one. Until then I can still do quick things; type /help."
        } else {
            "I'm still waking up. Give me a moment, or type /help for things I can do right now."
        };
        return Err(why.into());
    }
    tauri::async_runtime::spawn(brain::respond_text(c, t));
    Ok(())
}

#[tauri::command]
fn llama_info(core: State<'_, Arc<Core>>) -> Value {
    match setup::bundled_llama(&core.app) {
        Some((p, v)) => json!({"bundled": true, "path": p.to_string_lossy(), "version": v}),
        None => json!({"bundled": false}),
    }
}

/// View scale of a window (the settings window's "zoom").
#[tauri::command]
fn ui_zoom(app: AppHandle, label: String, pct: u32) -> Result<(), String> {
    if !(60..=200).contains(&pct) {
        return Err("Zoom must be between 60% and 200%.".into());
    }
    let w = app.get_webview_window(&label).ok_or("no such window")?;
    w.set_zoom(f64::from(pct) / 100.0).map_err(|e| e.to_string())
}

/// "Come here!": run over to the pointer (from the pet's menu, the tray, or asked by voice).
#[tauri::command]
fn summon(core: State<'_, Arc<Core>>) {
    core.call_over();
}

/// Telegram bot and agent API, for Settings -> Remote.
#[tauri::command]
fn remote_status(core: State<'_, Arc<Core>>) -> Value {
    let s = core.settings.lk().clone();
    json!({
        "telegram": telegram::status(&core),
        "api": {"enabled": s.agent_api_enabled, "port": s.agent_api_port, "lan": s.agent_api_lan,
            "url": format!("http://{}:{}", if s.agent_api_lan { "<this computer's address>" } else { "127.0.0.1" }, s.agent_api_port)},
    })
}

/// The bundled speech server's state, for Settings -> Advanced.
#[tauri::command]
fn speech_status(core: State<'_, Arc<Core>>) -> Value {
    core.speech.lk().clone()
}

#[tauri::command]
fn telegram_pair_code(core: State<'_, Arc<Core>>) -> String {
    telegram::new_pair_code(&core)
}

#[tauri::command]
fn telegram_unpair(core: State<'_, Arc<Core>>, id: i64) -> Result<(), String> {
    let mut s = core.settings.lk().clone();
    s.telegram_chats.retain(|c| c.id != id);
    core.save_settings_now(s)?;
    core.emit("remote", json!(null));
    Ok(())
}

/// A fresh random token for the agent API (the settings window shows it; saving makes it active).
#[tauri::command]
fn new_api_token() -> String {
    use rand::{distr::Alphanumeric, Rng};
    rand::rng().sample_iter(Alphanumeric).take(40).map(char::from).collect()
}

#[tauri::command]
fn focus_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Show the full-screen overlay where the user drags out the roaming area.
#[tauri::command]
fn pick_area(core: State<'_, Arc<Core>>) -> Result<(), String> {
    let app = &core.app;
    let main = app.get_webview_window("main").ok_or("The companion window isn't ready.")?;
    let mon = main
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| main.primary_monitor().ok().flatten())
        .ok_or("I couldn't find your screen.")?;
    let w = app.get_webview_window("area").ok_or("The area picker isn't available.")?;
    let _ = w.set_position(*mon.position());
    let _ = w.set_size(*mon.size());
    let _ = w.show();
    let _ = w.set_focus();
    let cur = core.settings.lk().roam_area;
    app.emit_to("area", "area-start", json!({"mx": mon.position().x, "my": mon.position().y, "scale": mon.scale_factor(), "current": cur}))
        .map_err(|e| e.to_string())
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("area") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn cancel_area(core: State<'_, Arc<Core>>) {
    hide_overlay(&core.app);
}

fn store_area(core: &Core, area: Option<settings::Area>) -> Result<(), String> {
    if let Some(a) = &area {
        settings::validate_area(a)?;
    }
    {
        let mut s = core.settings.lk();
        s.roam_area = area;
        s.save(&core.settings_path).map_err(|e| format!("I couldn't save the area ({e})."))?;
    }
    core.emit("area", json!(area));
    Ok(())
}

#[tauri::command]
fn save_area(core: State<'_, Arc<Core>>, x: i32, y: i32, w: u32, h: u32) -> Result<(), String> {
    store_area(&core, Some(settings::Area { x, y, w, h }))?;
    hide_overlay(&core.app);
    Ok(())
}

#[tauri::command]
fn clear_area(core: State<'_, Arc<Core>>) -> Result<(), String> {
    store_area(&core, None)
}

#[tauri::command]
fn set_size(core: State<'_, Arc<Core>>, pct: u32) -> Result<(), String> {
    if !(40..=200).contains(&pct) {
        return Err("Size must be between 40% and 200%.".into());
    }
    {
        let mut s = core.settings.lk();
        s.size_pct = pct;
        s.save(&core.settings_path).map_err(|e| format!("I couldn't save the size ({e})."))?;
    }
    core.apply_size();
    Ok(())
}

#[tauri::command]
fn set_character(core: State<'_, Arc<Core>>, id: String) -> Result<(), String> {
    apply_character(&core, &id)
}

#[tauri::command]
fn get_brain(core: State<'_, Arc<Core>>) -> Value {
    core.brain.lk().clone()
}

#[tauri::command]
fn check_path(path: String, kind: String) -> setup::PathCheck {
    setup::check_path(&path, &kind)
}

#[tauri::command]
async fn detect_llama() -> Vec<String> {
    tokio::task::spawn_blocking(setup::detect_llama).await.unwrap_or_default()
}

#[tauri::command]
async fn scan_models() -> Vec<setup::ModelFile> {
    tokio::task::spawn_blocking(setup::scan_models).await.unwrap_or_default()
}

/// Open the system page where the user grants a permission (macOS privacy panes; elsewhere nothing is needed).
#[tauri::command]
fn open_privacy(kind: String) -> Result<(), String> {
    let pane = match kind.as_str() {
        "microphone" => "Privacy_Microphone",
        "screen" => "Privacy_ScreenCapture",
        "accessibility" => "Privacy_Accessibility",
        _ => return Err("Unknown permission.".into()),
    };
    if !cfg!(target_os = "macos") {
        return Err("This system doesn't need that permission.".into());
    }
    open::that(format!("x-apple.systempreferences:com.apple.preference.security?{pane}")).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !setup::link_allowed(&url) {
        return Err("That link isn't allowed.".into());
    }
    open::that(&url).map_err(|e| e.to_string())
}

/// Custom title bar buttons for the frameless settings window.
#[tauri::command]
fn window_action(app: AppHandle, label: String, action: String) {
    let Some(w) = app.get_webview_window(&label) else { return };
    match action.as_str() {
        "minimize" => {
            let _ = w.minimize();
        }
        "maximize" => {
            let _ = if w.is_maximized().unwrap_or(false) { w.unmaximize() } else { w.maximize() };
        }
        "close" => {
            let _ = w.hide();
        }
        _ => {}
    }
}

#[tauri::command]
fn get_state(core: State<'_, Arc<Core>>) -> Value {
    core.flags()
}

#[tauri::command]
fn set_flag(core: State<'_, Arc<Core>>, name: String, value: bool) {
    match name.as_str() {
        "mic" => {
            core.mic.store(value, Ordering::SeqCst);
            core.update_gate();
            core.remember_switch("mic", value);
        }
        "screen" => {
            core.screen.store(value, Ordering::SeqCst);
            core.remember_switch("screen", value);
        }
        "hands" => {
            core.hands.store(value, Ordering::SeqCst);
            brain::prime(core.inner().clone()); // the tool list changed: get the new prompt ready now, not on the next request
            if !value {
                core.stop_gen.fetch_add(1, Ordering::SeqCst);
                core.answer_approval(false);
            }
        }
        "roam" => return core.set_roam(value),
        _ => {}
    }
    core.push_flags();
}

#[tauri::command]
fn get_settings(core: State<'_, Arc<Core>>) -> Settings {
    core.settings.lk().clone()
}

/// Saves the settings. Returns true when the model server is being restarted (only changes to the model or how
/// it runs need that, or a brain that isn't up yet); everything else applies straight away.
#[tauri::command]
fn save_settings(core: State<'_, Arc<Core>>, mut settings: Settings) -> Result<bool, String> {
    settings.validate()?;
    // The switches on the character are the source of truth; the settings window's copy may be older.
    settings.mic_on = core.mic.load(Ordering::SeqCst);
    settings.screen_on = core.screen.load(Ordering::SeqCst);
    settings.voice_variant = core.settings.lk().voice_variant; // changed by "try another voice", not by the form
    settings.hello_day = core.settings.lk().hello_day.clone(); // written by the morning hello
    settings.telegram_chats = core.settings.lk().telegram_chats.clone(); // paired from the phone, removed by telegram_unpair
    settings.save(&core.settings_path).map_err(|e| {
        log::warn(&format!("saving settings failed: {e}"));
        format!("I couldn't save your settings ({e}).")
    })?;
    let character = settings.character.clone();
    let speech_changed = speech::key(&core.settings.lk()) != speech::key(&settings);
    let (mcp_changed, server_changed, shortcuts_changed, voice_changed, remote_changed) = {
        let old = core.settings.lk();
        (
            old.mcp_servers != settings.mcp_servers,
            old.needs_restart(&settings),
            (&old.chat_shortcut, &old.ptt_shortcut, &old.listen_mode) != (&settings.chat_shortcut, &settings.ptt_shortcut, &settings.listen_mode),
            voice::key(&old) != voice::key(&settings),
            (old.telegram_enabled, &old.telegram_token, old.agent_api_enabled, old.agent_api_port, old.agent_api_lan, &old.agent_api_token)
                != (settings.telegram_enabled, &settings.telegram_token, settings.agent_api_enabled, settings.agent_api_port, settings.agent_api_lan, &settings.agent_api_token),
        )
    };
    let brain_up = matches!(core.brain.lk()["state"].as_str(), Some("ready" | "loading"));
    *core.settings.lk() = settings;
    core.push_flags();
    core.emit("character", json!(character));
    core.apply_size();
    if mcp_changed {
        core.inner().mcp_reload();
    }
    if speech_changed {
        core.speech_restart.notify_one();
    }
    if remote_changed {
        core.remote_restart.notify_waiters();
        core.emit("remote", json!(null));
    }
    core.audio.use_devices(core.settings.lk().audio_choice());
    core.audio.set_volume(core.settings.lk().volume);
    core.update_gate();
    if shortcuts_changed {
        if let Some(problem) = register_shortcuts(core.inner()) {
            core.status(problem); // saved anyway; the user can pick another combination
        }
    }
    let restart = server_changed || !brain_up;
    if voice_changed && !restart {
        voice::warm(core.inner().clone()); // a new voice is made in the background, not on the first sentence
    }
    if restart {
        core.restart.notify_one(); // reload the model server with the new settings
    }
    Ok(restart)
}

#[tauri::command]
async fn list_gpus(app: AppHandle, exe: String) -> Result<Vec<(String, String)>, String> {
    let user = settings::Settings { llama_server: exe, ..settings::Settings::default() };
    let bundled = setup::bundled_llama(&app).map(|b| b.0);
    brain::list_devices(&brain::llama_exe(&user, bundled.as_deref())).await
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn hide_main(app: AppHandle, core: State<'_, Arc<Core>>) {
    core.chatting.store(false, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn stop_all(core: State<'_, Arc<Core>>) {
    core.stop_everything();
}

#[tauri::command]
fn answer_approval(core: State<'_, Arc<Core>>, approve: bool) {
    core.answer_approval(approve);
}

#[tauri::command]
fn set_hit_regions(core: State<'_, Arc<Core>>, rects: Vec<[f64; 4]>) {
    *core.hit.lk() = rects;
}

#[tauri::command]
fn open_logs(core: State<'_, Arc<Core>>) -> Result<(), String> {
    open::that(&core.log_dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Microphones and speakers on this PC, the defaults, the user's choice and what is in use right now.
#[tauri::command]
async fn audio_devices(core: State<'_, Arc<Core>>) -> Result<Value, String> {
    let mut v = tokio::task::spawn_blocking(audio::devices).await.map_err(|e| e.to_string())?;
    let (mic, speaker) = core.audio.current();
    v["using_mic"] = json!(mic);
    v["using_speaker"] = json!(speaker);
    Ok(v)
}

/// Device, volume and "speak aloud" choices apply at once (no Save, no restart).
#[tauri::command]
fn set_audio(core: State<'_, Arc<Core>>, mic: String, speaker: String, volume: u32, speak_aloud: bool) -> Result<(), String> {
    if mic.len() > 300 || speaker.len() > 300 {
        return Err("That device name is too long.".into());
    }
    if volume > 150 {
        return Err("Volume must be between 0% and 150%.".into());
    }
    let choice = {
        let mut s = core.settings.lk();
        (s.mic_device, s.speaker_device, s.volume, s.speak_aloud) = (mic, speaker, volume, speak_aloud);
        s.save(&core.settings_path).map_err(|e| format!("I couldn't save that ({e})."))?;
        s.audio_choice()
    };
    core.audio.use_devices(choice);
    core.audio.set_volume(volume);
    core.push_flags();
    Ok(())
}

#[tauri::command]
fn test_speaker(core: State<'_, Arc<Core>>) {
    core.audio.chime();
}

#[tauri::command]
fn test_voice(core: State<'_, Arc<Core>>) -> Result<(), String> {
    if !core.ready.load(Ordering::SeqCst) {
        return Err("I'm still waking up, so I can't talk yet. The chime test works already.".into());
    }
    let name = core.settings.lk().name.clone();
    brain::say_anyway(&core, &format!("Hi! I'm {name}. This is how I sound."));
    Ok(())
}

/// (Re)register the chat and push-to-talk shortcuts from settings. Returns a message when some are taken.
fn register_shortcuts(core: &Arc<Core>) -> Option<String> {
    let gs = core.app.global_shortcut();
    for old in core.shortcuts.lk().drain(..) {
        let _ = gs.unregister(old.as_str());
    }
    let (chat, ptt, push) = {
        let s = core.settings.lk();
        (s.chat_shortcut.trim().to_string(), s.ptt_shortcut.trim().to_string(), s.listen_mode == "push")
    };
    let mut taken = vec![];
    if !chat.is_empty() {
        let c = core.clone();
        let res = gs.on_shortcut(chat.as_str(), move |_, _, ev| {
            if ev.state == ShortcutState::Pressed {
                if let Some(w) = c.app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
                c.emit("open-chat", json!(null));
            }
        });
        match res {
            Ok(()) => core.shortcuts.lk().push(chat.clone()),
            Err(e) => {
                log::warn(&format!("could not register {chat}: {e}"));
                taken.push(format!("{chat} (typing)"));
            }
        }
    }
    // Only grab the push-to-talk keys while that mode is on, so they stay free for other apps otherwise.
    if push && !ptt.is_empty() {
        let c = core.clone();
        match gs.on_shortcut(ptt.as_str(), move |_, _, ev| c.push_to_talk(ev.state == ShortcutState::Pressed)) {
            Ok(()) => core.shortcuts.lk().push(ptt.clone()),
            Err(e) => {
                log::warn(&format!("could not register {ptt}: {e}"));
                taken.push(format!("{ptt} (push-to-talk)"));
            }
        }
    }
    (!taken.is_empty()).then(|| format!("Another app is using {}. Pick a different shortcut in Settings -> Voice & sight.", taken.join(" and ")))
}

fn build_tray(app: &tauri::App, core: &Arc<Core>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "toggle", "Show / hide Nuzzle", true, None::<&str>)?;
    let come = MenuItem::with_id(app, "come", "Come here!", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let roam = CheckMenuItem::with_id(app, "roam", "Wander around the screen", true, true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop everything", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", "Open log folder", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Nuzzle", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let current = core.settings.lk().character.clone();
    let mut items = vec![];
    for (id, name, _) in settings::CHARACTERS {
        let it = CheckMenuItem::with_id(app, format!("char:{id}"), name, true, id == current, None::<&str>)?;
        items.push((id.to_string(), it));
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = items.iter().map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let characters = Submenu::with_items(app, "Character", true, &refs)?;
    *core.char_items.lk() = items;
    let menu = Menu::with_items(app, &[&show, &come, &settings, &characters, &roam, &stop, &sep, &logs, &quit])?;
    *core.roam_item.lk() = Some(roam.clone());

    let c = core.clone();
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Nuzzle")
        .menu(&menu)
        // Menu-bar icons open their menu on click on a Mac; elsewhere a left click shows or hides the companion.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            "toggle" => toggle_main(app),
            "come" => c.call_over(),
            "settings" => show_settings(app),
            "roam" => c.set_roam(roam.is_checked().unwrap_or(true)),
            "stop" => c.stop_everything(),
            "logs" => {
                let _ = open::that(&c.log_dir);
            }
            "quit" => app.exit(0),
            other => {
                if let Some(id) = other.strip_prefix("char:") {
                    if let Err(e) = apply_character(&c, id) {
                        c.status(e);
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, ev| {
            if cfg!(target_os = "macos") {
                return;
            }
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// The transparent window would otherwise swallow clicks meant for the apps beneath it, so it is
/// click-through everywhere except over the character, bubble and controls.
fn spawn_hit_tester(core: Arc<Core>) {
    std::thread::spawn(move || {
        let mut last: Option<bool> = None;
        let mut last_cursor = (f64::MAX, f64::MAX);
        let mut last_abs = (f64::MAX, f64::MAX);
        type Press = (Instant, (f64, f64), (i32, i32)); // when, cursor, window position
        let mut press: Option<Press> = None;
        let mut last_poke = Instant::now() - Duration::from_secs(5);
        let mut was_down = false;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let Some(win) = core.app.get_webview_window("main") else { continue };
            if !win.is_visible().unwrap_or(false) {
                last = None;
                continue;
            }
            let rects = core.hit.lk().clone();
            let (cur, pos, size, sf) = (core.app.cursor_position(), win.outer_position(), win.outer_size(), win.scale_factor());
            let mut ignore = core.acting.load(Ordering::Relaxed);
            if let (Ok(c), Ok(p), Ok(sz), Ok(sf)) = (cur, pos, size, sf) {
                let k = sf; // the page is never zoomed, so one CSS pixel is one scale-factor unit
                let (x, y) = ((c.x - p.x as f64) / k, (c.y - p.y as f64) / k);
                if !ignore && !rects.is_empty() {
                    ignore = !rects.iter().any(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]);
                }
                // Where the pointer is relative to the character's centre: the eyes follow it.
                // Screen position too (logical px), so being shaken while dragged and fast swipes can be told apart.
                let (cx, cy) = (x - f64::from(sz.width) / 2.0 / k, y - f64::from(sz.height) / 2.0 / k);
                if (cx - last_cursor.0).abs() > 2.0 || (cy - last_cursor.1).abs() > 2.0 || (c.x - last_abs.0).abs() > 2.0 || (c.y - last_abs.1).abs() > 2.0 {
                    last_cursor = (cx, cy);
                    last_abs = (c.x, c.y);
                    core.emit("cursor", json!({"x": cx, "y": cy, "sx": c.x / sf, "sy": c.y / sf}));
                }
                // A press and release on the character without moving it is a poke (double poke opens the chat).
                let over_pet = rects.first().is_some_and(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]);
                let down = avoid::left_button_down();
                if down && !was_down && over_pet && !core.acting.load(Ordering::Relaxed) {
                    press = Some((Instant::now(), (c.x, c.y), (p.x, p.y)));
                    core.last_pet_touch.store(now_ms(), Ordering::Relaxed);
                } else if down && !was_down && ignore && !core.acting.load(Ordering::Relaxed) {
                    // A click that went through to another app: that is where the user is working.
                    *core.click_spot.lk() = Some((c.x as i32, c.y as i32, Instant::now()));
                }
                if !down && was_down {
                    if let Some((t, (px, py), (wx, wy))) = press.take() {
                        let still = (c.x - px).abs() < 6.0 && (c.y - py).abs() < 6.0 && (p.x - wx).abs() < 3 && (p.y - wy).abs() < 3;
                        if still && t.elapsed() < Duration::from_millis(600) {
                            let double = last_poke.elapsed() < Duration::from_millis(450);
                            last_poke = Instant::now();
                            core.touch();
                            // Where it was touched (page px), so head, nose, belly and feet can react differently.
                            core.emit(if double { "dpoke" } else { "poke" }, json!({"x": x, "y": y}));
                        }
                    }
                }
                was_down = down;
            }
            if last != Some(ignore) {
                last = Some(ignore);
                let _ = win.set_ignore_cursor_events(ignore);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // Closing either window sends it to the tray; "Quit" in the tray menu really exits.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_flag,
            get_settings,
            save_settings,
            list_gpus,
            open_settings,
            hide_main,
            stop_all,
            answer_approval,
            set_hit_regions,
            open_logs,
            quit,
            set_character,
            set_size,
            list_reminders,
            delete_reminder,
            mcp_status,
            mcp_reload,
            send_text,
            llama_info,
            ui_zoom,
            focus_main,
            summon,
            remote_status,
            speech_status,
            telegram_pair_code,
            telegram_unpair,
            new_api_token,
            pick_area,
            cancel_area,
            save_area,
            clear_area,
            list_memory,
            add_memory,
            delete_memory,
            clear_memory,
            download_status,
            start_download,
            cancel_download,
            get_brain,
            check_path,
            detect_llama,
            scan_models,
            open_link,
            open_privacy,
            window_action,
            set_chatting,
            voice_next,
            check_voice,
            list_transcript,
            clear_transcript,
            audio_devices,
            set_audio,
            test_speaker,
            test_voice
        ])
        .setup(|app| {
            // A desktop companion, not a regular app: no Dock icon (the tray menu has everything).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let log_dir = app.path().app_log_dir()?;
            log::init(&log_dir);
            log::info("Nuzzle starting");

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let memory_path = app.path().app_config_dir()?.join("memory.json");
            let reminders_path = app.path().app_config_dir()?.join("reminders.json");
            let models_dir = app.path().app_local_data_dir()?.join("models");
            let (settings, load_warning) = Settings::load(&settings_path);

            // Mic and screen start the way the user last left them.
            let (mic_was_on, screen_was_on) = (settings.mic_on, settings.screen_on);
            let gate = Arc::new(AtomicBool::new(mic_was_on && settings.listen_mode != "push"));
            let (mic_tx, mic_rx) = mpsc::channel::<Vec<u8>>(64);
            let audio = audio::start(mic_tx.clone(), gate.clone(), settings.audio_choice(), settings.volume);

            let api_key: String = {
                use rand::{distr::Alphanumeric, Rng};
                rand::rng().sample_iter(Alphanumeric).take(32).map(char::from).collect()
            };

            let core = Arc::new(Core {
                app: app.handle().clone(),
                settings: Mutex::new(settings),
                settings_path,
                memory: Mutex::new(memory::load(&memory_path)),
                mcp: Arc::new(mcp::Hub::default()),
                reminders: Mutex::new(reminders::load(&reminders_path)),
                reminders_path,
                memory_path,
                models_dir,
                downloading: AtomicBool::new(false),
                dl_cancel: AtomicBool::new(false),
                log_dir,
                http: reqwest::Client::new(),
                mic: AtomicBool::new(mic_was_on),
                gate,
                ptt_held: AtomicBool::new(false),
                mic_tx,
                shortcuts: Mutex::new(vec![]),
                screen: AtomicBool::new(screen_was_on),
                hands: AtomicBool::new(false),
                roam: AtomicBool::new(true),
                acting: AtomicBool::new(false),
                pet_pct: AtomicU32::new(100),
                stop_gen: Arc::new(AtomicU64::new(0)),
                geom: Mutex::new(None),
                audio,
                mic_rx: tokio::sync::Mutex::new(mic_rx),
                history: tokio::sync::Mutex::new(vec![]),
                tts_tx: Mutex::new(None),
                ready: AtomicBool::new(false),
                child_pids: Mutex::new(vec![]),
                restart: Notify::new(),
                act_lock: tokio::sync::Mutex::new(()),
                last_activity: AtomicU64::new(0),
                port: AtomicU16::new(0),
                api_key,
                approval: Mutex::new(None),
                approved_hooks: Mutex::new(HashSet::new()),
                hit: Mutex::new(vec![]),
                roam_item: Mutex::new(None),
                char_items: Mutex::new(vec![]),
                brain: Mutex::new(json!({"state": "setup", "detail": ""})),
                transcript: Mutex::new(vec![]),
                engaged: AtomicU32::new(0),
                chatting: AtomicBool::new(false),
                hearing: AtomicBool::new(false),
                tts_pending: AtomicU32::new(0),
                click_spot: Mutex::new(None),
                voice_ref: Mutex::new(None),
                last_pet_touch: AtomicU64::new(0),
                summon: AtomicBool::new(false),
                remote: Mutex::new(None),
                remote_restart: Notify::new(),
                pair_code: Mutex::new(None),
                telegram_bot: Mutex::new(None),
                tg_approval: Mutex::new(None),
                speech: Mutex::new(json!({"state": "off", "detail": ""})),
                speech_restart: Notify::new(),
                feeling: Mutex::new(None),
            });
            app.manage(core.clone());
            build_tray(app, &core)?;
            if let Some(n) = load_warning {
                core.status(n);
            }

            // Emergency stop that works even when another app has focus.
            let c = core.clone();
            let reg = app.global_shortcut().on_shortcut(settings::STOP_SHORTCUT, move |_, _, ev| {
                if ev.state == ShortcutState::Pressed {
                    c.stop_everything();
                }
            });
            if let Err(e) = reg {
                let keys = if cfg!(target_os = "macos") { "Cmd+Option+Space" } else { "Ctrl+Alt+Space" };
                log::warn(&format!("could not register {keys}: {e}"));
                core.status(format!("{keys} is taken by another app. Use the stop button or the tray menu to stop me."));
            }

            // Quick typed chat and push-to-talk from anywhere (both configurable).
            if let Some(problem) = register_shortcuts(&core) {
                core.status(problem);
            }

            // Animation feed: audio levels for lip-sync / listening nods and the settings mic meter (slower while
            // nothing shows them; only sent when they change). Device problems and recoveries are said aloud.
            let c = core.clone();
            std::thread::spawn(move || {
                let mut last = (-1.0f32, -1.0f32);
                loop {
                    let shown = |l: &str| c.app.get_webview_window(l).is_some_and(|w| w.is_visible().unwrap_or(false));
                    let visible = shown("main") || shown("settings");
                    std::thread::sleep(Duration::from_millis(if visible { 50 } else { 250 }));
                    let (mi, mo) = (c.audio.level_in(), c.audio.level_out());
                    if mi > 0.03 || mo > 0.01 {
                        c.touch();
                    }
                    let changed = (mi - last.0).abs() > 0.002 || (mo - last.1).abs() > 0.002;
                    if visible && changed {
                        last = (mi, mo);
                        c.emit("levels", json!({ "mic": mi, "out": mo }));
                    }
                    for n in c.audio.take_notes() {
                        c.status(n);
                        c.emit("audio-devices", json!(null));
                    }
                }
            });

            core.apply_size();
            core.adopt_downloaded(false);
            core.mcp_reload();
            let c = core.clone();
            tauri::async_runtime::spawn(async move {
                // Reminders that came due while the app was closed wait until the character is on screen to say them.
                tokio::time::sleep(Duration::from_secs(8)).await;
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let now = sys::unix_now();
                    let due = {
                        let mut list = c.reminders.lk();
                        let due = reminders::take_due(&mut list, now);
                        if !due.is_empty() {
                            let _ = reminders::save(&c.reminders_path, &list);
                        }
                        due
                    };
                    if !due.is_empty() {
                        c.emit("reminders", json!(null));
                    }
                    for r in due {
                        // More than five minutes late means I wasn't running (or the computer slept) at the time.
                        let late = r.every == 0 && now.saturating_sub(r.due) > 300;
                        c.announce(&if late { format!("{} (this was due earlier, while I was away)", r.text) } else { r.text });
                        tokio::time::sleep(Duration::from_secs(4)).await;
                    }
                }
            });
            spawn_hit_tester(core.clone());
            roam::spawn(core.clone());
            telegram::spawn(core.clone());
            speech::spawn(core.clone());
            hello::spawn(core.clone());
            agent_api::spawn(core.clone());
            tauri::async_runtime::spawn(brain::run(core));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building companion");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(core) = app.try_state::<Arc<Core>>() {
                core.kill_children();
            }
        }
    });
}
