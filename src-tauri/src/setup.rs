//! First-run helpers: find llama-server and model files on this PC, and check that a chosen file is what it
//! claims to be, so the setup wizard can guide people instead of asking them to paste paths.

use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SEARCH_BUDGET: Duration = Duration::from_secs(6);
const SKIP_DIRS: [&str; 8] = ["node_modules", "target", "windows", "$recycle.bin", ".git", "appdata", "program files", "programdata"];

#[derive(Serialize, Debug, PartialEq)]
pub struct PathCheck {
    pub ok: bool,
    pub message: String,
    pub size_mb: u64,
}

#[derive(Serialize, Debug)]
pub struct ModelFile {
    pub path: String,
    pub name: String,
    pub size_mb: u64,
    /// True for vision/audio projector files (`mmproj`), false for the main model.
    pub projector: bool,
}

fn bad(msg: impl Into<String>) -> PathCheck {
    PathCheck { ok: false, message: msg.into(), size_mb: 0 }
}

/// kind: "exe" (llama-server), "gguf" (model) or "mmproj".
pub fn check_path(path: &str, kind: &str) -> PathCheck {
    let p = path.trim().trim_matches('"');
    if p.is_empty() {
        return bad("");
    }
    let path = Path::new(p);
    let Ok(meta) = fs::metadata(path) else { return bad("I can't find that file.") };
    if !meta.is_file() {
        return bad("That is a folder; pick the file inside it.");
    }
    let size_mb = meta.len() / 1_048_576;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match kind {
        "exe" => {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if cfg!(windows) && ext != "exe" {
                return bad("This should be llama-server.exe.");
            }
            if !name.contains("llama") {
                return PathCheck { ok: true, message: "OK, but the name doesn't look like llama-server.".into(), size_mb };
            }
        }
        "gguf" | "mmproj" => {
            if ext != "gguf" {
                return bad("This should be a .gguf file.");
            }
            let mut magic = [0u8; 4];
            if fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_err() || &magic != b"GGUF" {
                return bad("This doesn't look like a valid GGUF file (is the download complete?).");
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if kind == "gguf" && name.contains("mmproj") {
                return bad("That is the vision/hearing file. Pick it in the next box; choose the main model here.");
            }
            if kind == "mmproj" && !name.contains("mmproj") {
                return PathCheck { ok: true, message: "OK, but the name doesn't contain 'mmproj'.".into(), size_mb };
            }
        }
        _ => {}
    }
    PathCheck { ok: true, message: format!("Found ({size_mb} MB)"), size_mb }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Depth-limited walk calling `visit` for every file; stops at the time budget.
fn walk(dir: &Path, depth: u32, started: Instant, visit: &mut dyn FnMut(&Path)) {
    if depth == 0 || started.elapsed() > SEARCH_BUDGET {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&p, depth - 1, started, visit);
            }
        } else if ft.is_file() {
            visit(&p);
        }
    }
}

pub fn detect_llama() -> Vec<String> {
    let exe = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let mut found: Vec<String> = vec![];
    let mut add = |p: PathBuf| {
        let s = p.to_string_lossy().to_string();
        if !found.contains(&s) {
            found.push(s);
        }
    };
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let c = d.join(exe);
            if c.is_file() {
                add(c);
            }
        }
    }
    let mut roots: Vec<PathBuf> = vec![];
    if let Some(h) = home() {
        roots.extend(["Downloads", "Documents", "Desktop", "llama.cpp", "AI", "Tools"].map(|d| h.join(d)));
        roots.push(h);
    }
    #[cfg(windows)]
    if let Ok(rd) = fs::read_dir("C:\\") {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().to_lowercase().contains("llama") {
                roots.push(e.path());
            }
        }
    }
    let started = Instant::now();
    for r in roots {
        walk(&r, 4, started, &mut |p| {
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(exe)) {
                add(p.to_path_buf());
            }
        });
    }
    found.truncate(8);
    found
}

pub fn scan_models() -> Vec<ModelFile> {
    let mut roots: Vec<(PathBuf, u32)> = vec![];
    if let Some(h) = home() {
        for (d, depth) in [
            ("Downloads", 4),
            ("Documents", 4),
            ("Desktop", 3),
            ("models", 4),
            (".lmstudio/models", 5),
            (".cache/llama.cpp", 3),
            (".cache/huggingface/hub", 6),
            ("AI", 5),
        ] {
            roots.push((h.join(d), depth));
        }
    }
    #[cfg(windows)]
    if let Ok(rd) = fs::read_dir("C:\\") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.contains("model") || n.contains("llama") {
                roots.push((e.path(), 4));
            }
        }
    }
    let started = Instant::now();
    let mut out: Vec<ModelFile> = vec![];
    for (r, depth) in roots {
        walk(&r, depth, started, &mut |p| {
            let is_gguf = p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
            if !is_gguf || out.len() >= 80 {
                return;
            }
            let path = p.to_string_lossy().to_string();
            if out.iter().any(|m| m.path == path) {
                return;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let size_mb = fs::metadata(p).map_or(0, |m| m.len() / 1_048_576);
            out.push(ModelFile { projector: name.to_lowercase().contains("mmproj"), path, name, size_mb });
        });
    }
    out.sort_by(|a, b| a.projector.cmp(&b.projector).then(a.name.cmp(&b.name)));
    out
}

/// The llama-server shipped inside the app (see scripts/fetch-llama.ps1), with its pinned version.
pub fn find_bundled(dirs: &[PathBuf]) -> Option<(PathBuf, String)> {
    let exe = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    dirs.iter().find(|d| d.join(exe).is_file()).map(|d| {
        let version = fs::read_to_string(d.join("VERSION")).unwrap_or_default().trim().to_string();
        (plain_path(d.join(exe)), version)
    })
}

/// Windows resource paths come back as `\\?\C:\...`; the plain form is friendlier to show and to spawn.
fn plain_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

pub fn bundled_llama(app: &tauri::AppHandle) -> Option<(PathBuf, String)> {
    use tauri::Manager;
    let mut dirs: Vec<PathBuf> = vec![];
    if let Ok(r) = app.path().resource_dir() {
        dirs.push(r.join("resources").join("llama"));
        dirs.push(r.join("llama"));
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("resources").join("llama"))) {
        dirs.push(d);
    }
    dirs.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/llama"))); // `tauri dev`
    find_bundled(&dirs)
}

/// Only these pages may be opened from the UI.
pub fn link_allowed(url: &str) -> bool {
    ["https://github.com/ggml-org/llama.cpp/releases", "https://huggingface.co/LiquidAI", "https://huggingface.co/models?library=gguf"]
        .contains(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nuzzle-setup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_and_missing_paths() {
        assert_eq!(check_path("  ", "gguf").message, "");
        assert!(!check_path("C:/nope/none.gguf", "gguf").ok);
    }

    #[test]
    fn gguf_magic_is_checked() {
        let d = tmp("gguf");
        let good = d.join("model-q4.gguf");
        fs::write(&good, b"GGUF\x03\0\0\0rest").unwrap();
        let fake = d.join("fake.gguf");
        fs::write(&fake, b"not a model").unwrap();
        let proj = d.join("mmproj-f16.gguf");
        fs::write(&proj, b"GGUF\x03\0\0\0").unwrap();
        assert!(check_path(good.to_str().unwrap(), "gguf").ok);
        assert!(!check_path(fake.to_str().unwrap(), "gguf").ok);
        assert!(!check_path(proj.to_str().unwrap(), "gguf").ok, "projector must not be accepted as the model");
        assert!(check_path(proj.to_str().unwrap(), "mmproj").ok);
        assert!(!check_path(d.to_str().unwrap(), "gguf").ok, "folders are rejected");
    }

    #[test]
    fn scan_walk_finds_nested_gguf() {
        let d = tmp("walk");
        let deep = d.join("a").join("b");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.gguf"), b"GGUF").unwrap();
        fs::write(deep.join("y.txt"), b"hi").unwrap();
        let mut seen = vec![];
        walk(&d, 4, Instant::now(), &mut |p| seen.push(p.to_path_buf()));
        assert_eq!(seen.len(), 2);
        let mut shallow = vec![];
        walk(&d, 1, Instant::now(), &mut |p| shallow.push(p.to_path_buf()));
        assert!(shallow.is_empty());
    }

    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(plain_path(PathBuf::from(r"\\?\C:\app\llama-server.exe")), PathBuf::from(r"C:\app\llama-server.exe"));
        assert_eq!(plain_path(PathBuf::from(r"C:\app\x.exe")), PathBuf::from(r"C:\app\x.exe"));
        let unc = PathBuf::from(r"\\?\UNC\server\share\x.exe");
        assert_eq!(plain_path(unc.clone()), unc);
    }

    #[test]
    fn bundled_llama_is_found_with_its_version() {
        let d = tmp("bundle");
        let exe = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
        assert!(find_bundled(std::slice::from_ref(&d)).is_none());
        fs::write(d.join(exe), b"x").unwrap();
        fs::write(d.join("VERSION"), "b12038\n").unwrap();
        let (p, v) = find_bundled(&[PathBuf::from("C:/nowhere"), d.clone()]).unwrap();
        assert_eq!(p, d.join(exe));
        assert_eq!(v, "b12038");
    }

    #[test]
    fn only_known_links_open() {
        assert!(link_allowed("https://github.com/ggml-org/llama.cpp/releases"));
        assert!(!link_allowed("https://evil.example/llama.cpp/releases"));
        assert!(!link_allowed("file:///C:/Windows/System32/cmd.exe"));
    }
}
