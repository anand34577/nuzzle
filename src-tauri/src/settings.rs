use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Credential-store ids the settings file currently refers to (so unused ones can be deleted on save).
static STORED: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// Tool names the model already has; a connector may not reuse them.
pub const BUILTIN_TOOLS: [&str; 23] = [
    "remember",
    "forget",
    "refresh_view",
    "move_pointer",
    "click",
    "scroll",
    "select_all",
    "drag_select",
    "type_text",
    "press_key",
    "open_url",
    "read_text_file",
    "list_directory",
    "write_text_file",
    "run_command",
    "clipboard_get",
    "clipboard_set",
    "web_fetch",
    "get_datetime",
    "system_info",
    "set_reminder",
    "list_reminders",
    "cancel_reminder",
];

/// Layout of the companion window, in CSS pixels at normal zoom. Only the character scales with `size_pct`; the
/// speech bubbles, controls and text stay the same size, so the window is sized around the character.
pub const PET_W: f64 = 200.0;
pub const PET_H: f64 = 240.0;
/// Gap under the character for the control bar.
pub const BAR_ROOM: f64 = 46.0;
/// Room above the character for the speech bubble.
pub const BUBBLE_ROOM: f64 = 114.0;
/// Narrowest the window gets (a speech bubble is 244 wide).
pub const MIN_WIN_W: f64 = 260.0;

/// Window size in logical pixels for a character scale `f` (1.0 = 100%).
pub fn window_size(f: f64) -> (f64, f64) {
    ((PET_W * f + 40.0).max(MIN_WIN_W), BUBBLE_ROOM + PET_H * f + BAR_ROOM)
}

/// The part of the window the character actually fills, as fractions (left, top, right, bottom) of its box.
pub const PET_BODY: (f64, f64, f64, f64) = (0.14, 0.05, 0.86, 0.97);

/// Rectangle (left, top, right, bottom) in physical pixels covered by the character when the window's top-left is
/// at `(x, y)`, for scale `f` and display scale factor `sf`.
pub fn pet_rect(x: f64, y: f64, f: f64, sf: f64) -> (f64, f64, f64, f64) {
    let (w, h) = window_size(f);
    let (bw, bh) = (PET_W * f, PET_H * f);
    let (left, top) = ((w - bw) / 2.0, h - BAR_ROOM - bh);
    (
        x + (left + bw * PET_BODY.0) * sf,
        y + (top + bh * PET_BODY.1) * sf,
        x + (left + bw * PET_BODY.2) * sf,
        y + (top + bh * PET_BODY.3) * sf,
    )
}

/// (id, default name, personality) of the built-in characters. Only one is active at a time.
pub const CHARACTERS: [(&str, &str, &str); 12] = [
    ("pip", "Pip", "curious and cheerful, like a little sprout who loves learning"),
    ("mochi", "Mochi", "a calm, affectionate cat: gentle, a little sassy, loves cosy moments"),
    ("bruno", "Bruno", "a warm, dependable bear: encouraging, steady and kind"),
    ("zap", "Zap", "an upbeat helper robot: energetic, precise and a bit nerdy"),
    ("nimbus", "Nimbus", "a gentle little cloud: dreamy, soothing and sweetly forgetful"),
    ("pebble", "Pebble", "a quiet, wise pebble: slow, dry-humoured and secretly very caring"),
    ("ember", "Ember", "a bold little fox: clever, playful and quick with a joke"),
    ("bun", "Bun", "a shy, kind bunny: soft-spoken, curious and easily delighted"),
    ("axo", "Axo", "a cheerful axolotl: endlessly optimistic and a little goofy"),
    ("wisp", "Wisp", "a calm, friendly spirit: poetic, gentle and quietly encouraging"),
    ("penny", "Penny", "a tidy little penguin: organised, practical and fond of a good plan"),
    ("cosmo", "Cosmo", "a curious space visitor: amazed by everyday things and full of big questions"),
];

pub fn persona(id: &str) -> &'static str {
    CHARACTERS.iter().find(|c| c.0 == id).map_or(CHARACTERS[0].2, |c| c.2)
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Webhook {
    pub name: String,
    pub description: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// A rectangle on the virtual desktop (physical pixels) that the companion may roam inside.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A user-configured MCP server: a local program (stdio) or a web endpoint (http).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct McpServerCfg {
    pub name: String,
    /// "stdio" or "http"
    pub kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub enabled: bool,
}

impl Default for McpServerCfg {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: "stdio".into(),
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            url: String::new(),
            headers: HashMap::new(),
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub name: String,
    /// Size of the companion on screen, in percent of the base 220x300 window.
    pub size_pct: u32,
    /// Which built-in character is on screen (see `CHARACTERS`).
    pub character: String,
    /// How chatty and joky I am: "quiet", "normal" or "lively".
    pub playfulness: String,
    /// Keep out of the way of the pointer, the text caret and where the user last clicked.
    pub avoid_cursor: bool,

    /// Path to `llama-server` (Vulkan build). Empty = look it up on PATH.
    pub llama_server: String,
    /// Chat model (GGUF).
    pub model: String,
    /// Vision projector (GGUF). Empty = text-only brain, no screen understanding.
    pub mmproj: String,
    /// llama.cpp device names to use, e.g. ["Vulkan1"] or ["Vulkan0","Vulkan1"]. Empty = llama.cpp default.
    pub devices: Vec<String>,
    /// Layers to offload (99 = all). 0 = CPU only.
    pub gpu_layers: i32,
    pub ctx: u32,
    /// Extra command-line flags for llama-server, split on whitespace.
    pub llama_args: String,

    /// Voice and language passed to llama-server's `/tts` (LFM3: "female" / "male", "en").
    pub tts_voice: String,
    pub tts_lang: String,
    /// System prompt used when the model transcribes the user's speech.
    pub asr_prompt: String,
    /// Optional recording (.wav, 5-15 s of one person) the voice is cloned from. Empty = the character's own voice.
    pub voice_ref_path: String,
    /// "Try another voice" counter: changes the character's voice without changing the character.
    pub voice_variant: u32,
    /// Send my actual voice to the model (not only a transcript) so it can notice tone, mood and language.
    pub hear_tone: bool,
    /// Microphone and speaker by name; empty = follow the Windows default.
    pub mic_device: String,
    pub speaker_device: String,
    /// Playback volume in percent (100 = as synthesised).
    pub volume: u32,
    /// Speak replies aloud (off: words only appear in the bubble).
    pub speak_aloud: bool,
    /// "always": listen whenever the mic is on. "push": only while the push-to-talk shortcut is held.
    pub listen_mode: String,
    /// Global shortcuts (Tauri accelerator syntax, e.g. "CommandOrControl+Alt+C"); empty = none.
    pub chat_shortcut: String,
    pub ptt_shortcut: String,
    /// The mic and screen switches as the user last left them (restored at launch).
    pub mic_on: bool,
    pub screen_on: bool,

    /// Longest side of the screenshots shown to the model.
    pub max_image_width: u32,
    /// "norm1000": the model answers with 0-1000 coordinates (Qwen3-VL style).
    /// "pixels": it answers with pixel coordinates in the screenshot (Qwen2.5-VL style).
    pub coord_mode: String,
    /// Attach a screenshot to every message instead of only when the request looks screen-related.
    pub always_look: bool,
    /// Let the companion keep long-term notes about the user (and use them in conversation).
    pub memory_enabled: bool,
    /// "risky": ask before dangerous actions. "all": ask before every action. "full": never ask.
    pub approvals: String,
    /// The user has read and accepted the full-access warning (required for approvals == "full").
    pub full_access_ack: bool,
    /// Where the companion may wander; `None` = anywhere on the screen.
    pub roam_area: Option<Area>,
    /// User-configured MCP servers whose tools the companion can use.
    pub mcp_servers: Vec<McpServerCfg>,

    /// User-defined connectors: each becomes a tool that POSTs its JSON args to `url`.
    pub webhooks: Vec<Webhook>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: "Pip".into(),
            character: "pip".into(),
            playfulness: "normal".into(),
            avoid_cursor: true,
            size_pct: 80,
            llama_server: String::new(),
            model: String::new(),
            mmproj: String::new(),
            devices: vec![],
            gpu_layers: 99,
            ctx: 16384,
            llama_args: "--reasoning off".into(),
            tts_voice: "female".into(),
            tts_lang: "en".into(),
            asr_prompt: "Perform ASR in English.".into(),
            voice_ref_path: String::new(),
            voice_variant: 0,
            hear_tone: true,
            mic_device: String::new(),
            speaker_device: String::new(),
            volume: 100,
            speak_aloud: true,
            listen_mode: "always".into(),
            chat_shortcut: "CommandOrControl+Alt+C".into(),
            ptt_shortcut: "CommandOrControl+Shift+Space".into(),
            mic_on: true,
            screen_on: true,
            max_image_width: 1024,
            coord_mode: "norm1000".into(),
            always_look: false,
            memory_enabled: true,
            approvals: "risky".into(),
            full_access_ack: false,
            roam_area: None,
            mcp_servers: vec![],
            webhooks: vec![],
        }
    }
}

fn valid_tool_name(n: &str) -> bool {
    (1..=40).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn check_file(label: &str, p: &str) -> Result<(), String> {
    if !p.trim().is_empty() && !Path::new(p.trim()).is_file() {
        return Err(format!("{label}: I can't find a file at '{}'.", p.trim()));
    }
    Ok(())
}

fn valid_env_key(k: &str) -> bool {
    let mut c = k.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

pub fn validate_area(a: &Area) -> Result<(), String> {
    if !(200..=20_000).contains(&a.w) || !(150..=20_000).contains(&a.h) {
        return Err("The roaming area must be at least 200 x 150 pixels.".into());
    }
    Ok(())
}

/// The emergency stop; it is fixed so it can always be found.
pub const STOP_SHORTCUT: &str = "CommandOrControl+Alt+Space";

/// Same key combination? (Compares the parsed shortcuts, so "Ctrl+Alt+C" equals "control+alt+c".)
pub fn same_shortcut(a: &str, b: &str) -> bool {
    use tauri_plugin_global_shortcut::Shortcut;
    match (a.parse::<Shortcut>(), b.parse::<Shortcut>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

pub fn validate_shortcuts(chat: &str, ptt: &str) -> Result<(), String> {
    for (label, s) in [("Chat", chat), ("Push-to-talk", ptt)] {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        if s.len() > 60 || s.parse::<tauri_plugin_global_shortcut::Shortcut>().is_err() {
            return Err(format!("{label} shortcut '{s}' isn't a key combination I understand (try Ctrl+Alt+C)."));
        }
        if same_shortcut(s, STOP_SHORTCUT) {
            return Err(format!("{label} shortcut can't be Ctrl+Alt+Space: that one stops me."));
        }
    }
    if !chat.trim().is_empty() && same_shortcut(chat.trim(), ptt.trim()) {
        return Err("Chat and push-to-talk need different shortcuts.".into());
    }
    Ok(())
}

pub fn validate_mcp(servers: &[McpServerCfg]) -> Result<(), String> {
    if servers.len() > 12 {
        return Err("You can add up to 12 MCP servers.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for m in servers {
        let ok_name = (1..=24).contains(&m.name.len())
            && m.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !ok_name || !seen.insert(m.name.clone()) {
            return Err(format!("MCP server name '{}' must be unique: 1-24 lowercase letters, digits, - or _.", m.name));
        }
        match m.kind.as_str() {
            "stdio" => {
                if m.command.trim().is_empty() || m.command.len() > 300 {
                    return Err(format!("MCP server '{}' needs a command to run.", m.name));
                }
                if m.args.len() > 50 || m.args.iter().any(|a| a.len() > 1000) {
                    return Err(format!("MCP server '{}' has too many or too long arguments.", m.name));
                }
                if let Some(k) = m.env.keys().find(|k| !valid_env_key(k)) {
                    return Err(format!("MCP server '{}': '{k}' is not a valid environment variable name.", m.name));
                }
            }
            "http" => {
                if !(m.url.starts_with("http://") || m.url.starts_with("https://")) || m.url.len() > 500 {
                    return Err(format!("MCP server '{}' needs an http(s) address.", m.name));
                }
            }
            _ => return Err(format!("MCP server '{}' has an unknown type.", m.name)),
        }
    }
    Ok(())
}

impl Settings {
    /// Returns the settings plus a warning when the file existed but could not be read.
    pub fn load(path: &PathBuf) -> (Self, Option<String>) {
        let Ok(text) = fs::read_to_string(path) else { return (Self::default(), None) };
        match serde_json::from_str::<Self>(&text) {
            Ok(mut s) => {
                let (found, missing) = crate::secrets::reveal(&mut s, &crate::secrets::OsStore);
                *STORED.lock().unwrap_or_else(|e| e.into_inner()) = found;
                let warn = (!missing.is_empty()).then(|| {
                    "Some saved tokens for your connectors were not found in Windows Credential Manager; please enter them again.".to_string()
                });
                (s, warn)
            }
            Err(e) => {
                let bak = path.with_extension("json.bak");
                let _ = fs::copy(path, &bak);
                (
                    Self::default(),
                    Some(format!("Your settings file was unreadable ({e}); a copy is at {}.", bak.display())),
                )
            }
        }
    }

    /// Writes via a temp file so a crash can't leave half a file.
    pub fn save(&self, path: &PathBuf) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Tokens go to the credential store; the file only keeps references to them.
        let mut on_disk = self.clone();
        {
            let mut stored = STORED.lock().unwrap_or_else(|e| e.into_inner());
            *stored = crate::secrets::protect(&mut on_disk, &mut crate::secrets::OsStore, &stored);
        }
        let text = serde_json::to_string_pretty(&on_disk).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }

    pub fn audio_choice(&self) -> crate::audio::Choice {
        crate::audio::Choice { mic: self.mic_device.clone(), speaker: self.speaker_device.clone() }
    }

    /// True when going from `self` to `new` changes how the model server runs, so it must be restarted.
    pub fn needs_restart(&self, new: &Settings) -> bool {
        self.llama_server != new.llama_server
            || self.model != new.model
            || self.mmproj != new.mmproj
            || self.devices != new.devices
            || self.gpu_layers != new.gpu_layers
            || self.ctx != new.ctx
            || self.llama_args != new.llama_args
    }

    /// Human-readable reason the settings can't be used, if any.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 40 {
            return Err("Companion name must be 1-40 characters.".into());
        }
        if !(40..=200).contains(&self.size_pct) {
            return Err("Size must be between 40% and 200%.".into());
        }
        if !CHARACTERS.iter().any(|c| c.0 == self.character) {
            return Err("Unknown character.".into());
        }
        if !["quiet", "normal", "lively"].contains(&self.playfulness.as_str()) {
            return Err("Unknown playfulness level.".into());
        }
        check_file("llama-server", &self.llama_server)?;
        check_file("Model", &self.model)?;
        check_file("Vision file", &self.mmproj)?;
        if !(512..=1_048_576).contains(&self.ctx) {
            return Err("Context size must be between 512 and 1048576.".into());
        }
        if !(0..=999).contains(&self.gpu_layers) {
            return Err("GPU layers must be between 0 and 999.".into());
        }
        if !(256..=4096).contains(&self.max_image_width) {
            return Err("Screenshot width must be between 256 and 4096.".into());
        }
        if self.tts_voice.trim().is_empty() || self.tts_voice.len() > 32 {
            return Err("Choose a voice.".into());
        }
        let lang_ok = (2..=8).contains(&self.tts_lang.len())
            && self.tts_lang.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
        if !lang_ok {
            return Err("Speech language must be a code such as en or en-US.".into());
        }
        if self.asr_prompt.chars().count() > 400 {
            return Err("The listening instruction is too long (400 characters max).".into());
        }
        if !self.voice_ref_path.trim().is_empty() {
            crate::voice::check_sample(&self.voice_ref_path).map_err(|e| format!("Voice sample: {e}"))?;
        }
        if self.mic_device.len() > 300 || self.speaker_device.len() > 300 {
            return Err("That device name is too long.".into());
        }
        if self.volume > 150 {
            return Err("Volume must be between 0% and 150%.".into());
        }
        if !["always", "push"].contains(&self.listen_mode.as_str()) {
            return Err("Unknown listening mode.".into());
        }
        validate_shortcuts(&self.chat_shortcut, &self.ptt_shortcut)?;
        if self.listen_mode == "push" && self.ptt_shortcut.trim().is_empty() {
            return Err("Push-to-talk needs a shortcut.".into());
        }
        if !["norm1000", "pixels"].contains(&self.coord_mode.as_str()) {
            return Err("Unknown screen-position mode.".into());
        }
        if !["risky", "all", "full"].contains(&self.approvals.as_str()) {
            return Err("Unknown approval level.".into());
        }
        if self.approvals == "full" && !self.full_access_ack {
            return Err("Full access needs your confirmation first.".into());
        }
        if let Some(a) = &self.roam_area {
            validate_area(a)?;
        }
        validate_mcp(&self.mcp_servers)?;
        for d in &self.devices {
            let letters = d.chars().take_while(|c| c.is_ascii_alphabetic()).count();
            if letters == 0 || letters == d.len() || !d[letters..].chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("'{d}' is not a graphics device name like Vulkan0."));
            }
        }
        for a in self.llama_args.split_whitespace() {
            if ["--host", "--port", "--api-key", "-m", "--model", "--mmproj", "--device"].contains(&a) {
                return Err(format!("Extra flag '{a}' is managed by Companion; remove it."));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for w in &self.webhooks {
            if !valid_tool_name(&w.name)
                || w.name.starts_with("mcp_")
                || BUILTIN_TOOLS.contains(&w.name.as_str())
                || !seen.insert(w.name.clone())
            {
                return Err(format!("Connector name '{}' must be unique, letters/digits/_/- only.", w.name));
            }
            if !(w.url.starts_with("http://") || w.url.starts_with("https://")) {
                return Err(format!("Connector '{}' needs an http(s) address.", w.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("companion-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d.join("settings.json")
    }

    #[test]
    fn defaults_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn rejects_bad_values() {
        let bad = |f: &dyn Fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            assert!(s.validate().is_err());
        };
        bad(&|s| s.ctx = 10);
        bad(&|s| s.size_pct = 10);
        bad(&|s| s.size_pct = 500);
        bad(&|s| s.llama_args = "--port 9".into());
        bad(&|s| s.devices = vec!["Vulkan0".into(), "rm -rf".into()]);
        bad(&|s| {
            s.webhooks = vec![Webhook { name: "click".into(), url: "https://x".into(), ..Default::default() }]
        });
        bad(&|s| s.model = "C:/definitely/missing.gguf".into());
        bad(&|s| s.tts_lang = "en us!".into());
        bad(&|s| s.asr_prompt = "x".repeat(401));
        bad(&|s| s.tts_voice = " ".into());
    }

    #[test]
    fn window_fits_the_character_at_every_size() {
        let (w, h) = window_size(0.4);
        assert_eq!(w, MIN_WIN_W, "small character: window keeps room for the bubble");
        assert_eq!(h, BUBBLE_ROOM + PET_H * 0.4 + BAR_ROOM);
        let (w2, h2) = window_size(2.0);
        assert_eq!((w2, h2), (440.0, 640.0));
        // The character's box sits centred, above the bar room, inside the window.
        let (l, t, r, b) = pet_rect(0.0, 0.0, 2.0, 1.0);
        assert!(l > 0.0 && t > BUBBLE_ROOM - 1.0 && r < w2 && b < h2 - BAR_ROOM + 1.0);
        assert!(((l + r) / 2.0 - w2 / 2.0).abs() < 1e-6);
        // Display scaling grows the rectangle in physical pixels.
        let (l1, _, r1, _) = pet_rect(0.0, 0.0, 1.0, 1.0);
        let (l15, _, r15, _) = pet_rect(0.0, 0.0, 1.0, 1.5);
        assert!(((r15 - l15) - (r1 - l1) * 1.5).abs() < 1e-6);
    }

    #[test]
    fn shortcuts_and_audio_are_validated() {
        assert!(validate_shortcuts("CommandOrControl+Alt+C", "CommandOrControl+Shift+Space").is_ok());
        assert!(validate_shortcuts("", "").is_ok(), "both may be turned off");
        assert!(validate_shortcuts("Ctrl+Alt+Banana", "").is_err());
        assert!(validate_shortcuts("Control+Alt+Space", "").is_err(), "the stop shortcut is reserved");
        assert!(validate_shortcuts("Ctrl+Alt+C", "control+alt+c").is_err(), "must differ");
        let bad = |f: &dyn Fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            assert!(s.validate().is_err());
        };
        bad(&|s| s.volume = 151);
        bad(&|s| s.playfulness = "chaotic".into());
        bad(&|s| s.listen_mode = "sometimes".into());
        bad(&|s| {
            s.listen_mode = "push".into();
            s.ptt_shortcut = String::new();
        });
    }

    #[test]
    fn old_settings_files_get_the_new_defaults() {
        let s: Settings = serde_json::from_str(r#"{"name":"Mochi","character":"mochi"}"#).unwrap();
        assert_eq!((s.volume, s.speak_aloud, s.mic_on, s.listen_mode.as_str()), (100, true, true, "always"));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn only_server_changes_need_a_restart() {
        let a = Settings::default();
        let cosmetic = Settings { name: "Mochi".into(), tts_voice: "male".into(), memory_enabled: false, size_pct: 120, ..Settings::default() };
        assert!(!a.needs_restart(&cosmetic));
        assert!(a.needs_restart(&Settings { ctx: 4096, ..Settings::default() }));
        assert!(a.needs_restart(&Settings { devices: vec!["Vulkan1".into()], ..Settings::default() }));
        assert!(a.needs_restart(&Settings { llama_args: String::new(), ..Settings::default() }));
    }

    #[test]
    fn full_access_needs_acknowledgement() {
        let mut s = Settings { approvals: "full".into(), ..Settings::default() };
        assert!(s.validate().is_err());
        s.full_access_ack = true;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn roam_area_is_checked() {
        let mut s = Settings { roam_area: Some(Area { x: 0, y: 0, w: 50, h: 50 }), ..Settings::default() };
        assert!(s.validate().is_err());
        s.roam_area = Some(Area { x: -1920, y: 100, w: 1200, h: 300 });
        assert!(s.validate().is_ok());
    }

    #[test]
    fn mcp_servers_are_validated() {
        let ok = McpServerCfg { name: "files".into(), command: "npx".into(), args: vec!["-y".into(), "x".into()], ..Default::default() };
        assert!(validate_mcp(std::slice::from_ref(&ok)).is_ok());
        assert!(validate_mcp(&[ok.clone(), ok.clone()]).is_err(), "duplicate names");
        assert!(validate_mcp(&[McpServerCfg { name: "Bad Name".into(), ..ok.clone() }]).is_err());
        assert!(validate_mcp(&[McpServerCfg { command: " ".into(), ..ok.clone() }]).is_err());
        assert!(validate_mcp(&[McpServerCfg { env: [("1X".to_string(), "v".to_string())].into(), ..ok.clone() }]).is_err());
        let http = McpServerCfg { name: "web".into(), kind: "http".into(), url: "ftp://x".into(), ..Default::default() };
        assert!(validate_mcp(std::slice::from_ref(&http)).is_err());
        assert!(validate_mcp(&[McpServerCfg { url: "https://example.com/mcp".into(), ..http }]).is_ok());
        let many: Vec<McpServerCfg> = (0..13).map(|i| McpServerCfg { name: format!("s{i}"), ..ok.clone() }).collect();
        assert!(validate_mcp(&many).is_err());
    }

    #[test]
    fn connectors_cannot_shadow_mcp_or_builtin_tools() {
        for name in ["mcp_files", "run_command"] {
            let s = Settings {
                webhooks: vec![Webhook { name: name.into(), url: "https://x".into(), ..Default::default() }],
                ..Settings::default()
            };
            assert!(s.validate().is_err(), "{name}");
        }
    }

    #[test]
    fn unknown_character_is_rejected() {
        let s = Settings { character: "dragon".into(), ..Settings::default() };
        assert!(s.validate().is_err());
        assert!(persona("mochi").contains("cat"));
        assert_eq!(persona("nope"), CHARACTERS[0].2);
    }

    #[test]
    fn save_then_load_round_trips() {
        let p = tmp("rt");
        let s = Settings { name: "Mochi".into(), ..Settings::default() };
        s.save(&p).unwrap();
        let (l, warn) = Settings::load(&p);
        assert_eq!(l.name, "Mochi");
        assert!(warn.is_none());
    }

    #[test]
    fn corrupt_file_is_backed_up_not_lost() {
        let p = tmp("bad");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "{ not json").unwrap();
        let (s, warn) = Settings::load(&p);
        assert_eq!(s.name, "Pip");
        assert!(warn.is_some());
        assert!(p.with_extension("json.bak").exists());
    }
}
