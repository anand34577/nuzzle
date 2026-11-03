//! One-click download of the recommended model (LFM3-Omni-2B, Q4_K_M) and its vision/hearing projector from
//! LiquidAI's official Hugging Face repo. Resumable, cancellable, size-checked. The URLs are fixed here, so the
//! UI cannot make the app download anything else.

use futures_util::StreamExt;
use serde_json::json;
use std::{
    path::Path,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

use crate::{log, setup, Core};

pub struct Item {
    pub key: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    /// SHA-256 of the file at the pinned revision (Hugging Face's LFS oid).
    pub sha256: &'static str,
}

/// The repo revision the URLs are pinned to, so an upstream re-upload can't change (or break) what we fetch.
pub const REVISION: &str = "3d29af2ad166f8299c2d820937e47dc0add20ffc";

pub const ITEMS: [Item; 2] = [
    Item {
        key: "model",
        name: "LFM3-Omni-2B-Q4_K_M.gguf",
        url: "https://huggingface.co/LiquidAI/LFM3-Omni-2B-GGUF/resolve/3d29af2ad166f8299c2d820937e47dc0add20ffc/LFM3-Omni-2B-Q4_K_M.gguf",
        size: 1_099_630_080,
        sha256: "9f8d30e9d4fd7a9349d586e077988229b3ca0a0e4ad730897563d1eccc9893e2",
    },
    Item {
        key: "mmproj",
        name: "mmproj-LFM3-Omni-2B-Q4_K_M.gguf",
        url: "https://huggingface.co/LiquidAI/LFM3-Omni-2B-GGUF/resolve/3d29af2ad166f8299c2d820937e47dc0add20ffc/mmproj-LFM3-Omni-2B-Q4_K_M.gguf",
        size: 291_586_720,
        sha256: "0a746c0794782a16d4456b230c2a0b5888a2d94a6677211edbed7d7a4177a39a",
    },
];

/// Hex SHA-256 of a file (blocking; run it on a blocking thread).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

const STALL: Duration = Duration::from_secs(30);
pub const CANCELLED: &str = "cancelled";

/// Download `url` to `dest` (through `dest.part`), resuming a partial file when the server allows it.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    if tokio::fs::metadata(dest).await.is_ok_and(|m| m.len() == expected) {
        progress(expected);
        return Ok(());
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("I couldn't create the models folder ({e})."))?;
    }
    let part = dest.with_extension("gguf.part");
    let mut have = tokio::fs::metadata(&part).await.map_or(0, |m| m.len());
    if have > expected {
        let _ = tokio::fs::remove_file(&part).await;
        have = 0;
    }

    let mut req = client.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.send().await.map_err(|e| format!("I couldn't reach the download server ({e}). Check your internet connection."))?;
    let status = resp.status();
    let mut file = match status.as_u16() {
        206 => tokio::fs::OpenOptions::new().append(true).open(&part).await,
        200 => {
            have = 0; // the server ignored the range request: start over
            tokio::fs::File::create(&part).await
        }
        416 => {
            let _ = tokio::fs::remove_file(&part).await;
            return Err("The partial download was stale; press Download again.".into());
        }
        code => return Err(format!("The download server answered {code}.")),
    }
    .map_err(|e| format!("I couldn't write the file ({e}). Is the disk full?"))?;

    let mut done = have;
    progress(done);
    let mut stream = resp.bytes_stream();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = file.flush().await;
            return Err(CANCELLED.into());
        }
        let next = tokio::time::timeout(STALL, stream.next())
            .await
            .map_err(|_| "The download stalled. Press Download again to resume.".to_string())?;
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| format!("The connection dropped ({e}). Press Download again to resume."))?;
        file.write_all(&chunk).await.map_err(|e| format!("I couldn't write the file ({e}). Is the disk full?"))?;
        done += chunk.len() as u64;
        progress(done);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);

    if done != expected {
        return Err("The download ended early. Press Download again to resume.".into());
    }
    tokio::fs::rename(&part, dest).await.map_err(|e| format!("I couldn't finish the file ({e})."))
}

/// Download every item, emitting `download` events for the settings window.
pub async fn run(core: Arc<Core>) {
    let total: u64 = ITEMS.iter().map(|i| i.size).sum();
    let client = match reqwest::Client::builder()
        .user_agent("Nuzzle/0.1")
        .connect_timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return core.emit("download", json!({"state": "error", "message": e.to_string()})),
    };

    log::info(&format!("downloading the model (revision {REVISION})"));
    let (mut base, mut paths) = (0u64, vec![]);
    for item in &ITEMS {
        let dest = core.models_dir.join(item.name);
        let mut last = Instant::now() - Duration::from_secs(1);
        let c = &core;
        let res = fetch(&client, item.url, &dest, item.size, &core.dl_cancel, |d| {
            if last.elapsed() > Duration::from_millis(200) {
                last = Instant::now();
                c.emit("download", json!({"state": "progress", "file": item.key, "done": base + d, "total": total}));
            }
        })
        .await;
        match res {
            Ok(()) => {
                base += item.size;
                paths.push(dest.to_string_lossy().to_string());
            }
            Err(e) if e == CANCELLED => {
                log::info("model download cancelled");
                return core.emit("download", json!({"state": "cancelled"}));
            }
            Err(e) => {
                log::warn(&format!("model download failed: {e}"));
                return core.emit("download", json!({"state": "error", "message": e}));
            }
        }
    }
    core.emit("download", json!({"state": "verifying"}));
    for ((p, kind), item) in paths.iter().zip(["gguf", "mmproj"]).zip(&ITEMS) {
        let r = setup::check_path(p, kind);
        let path = std::path::PathBuf::from(p);
        let sum = tokio::task::spawn_blocking(move || sha256_file(&path)).await.map_err(|e| e.to_string()).and_then(|r| r.map_err(|e| e.to_string()));
        let why = match (&r.ok, &sum) {
            (false, _) => Some(r.message.clone()),
            (_, Err(e)) => Some(format!("I couldn't read it back: {e}")),
            (_, Ok(s)) if s != item.sha256 => Some("its checksum doesn't match the official file".into()),
            _ => None,
        };
        if let Some(why) = why {
            let _ = tokio::fs::remove_file(p).await; // corrupt: make the next attempt start clean
            log::warn(&format!("downloaded file failed verification: {p}: {why}"));
            return core.emit("download", json!({"state": "error", "message": format!("A downloaded file didn't pass the check ({why}). Please try again.")}));
        }
    }
    log::info("model download finished");
    let adopted = core.adopt_downloaded(true);
    core.emit("download", json!({"state": "done", "model_path": paths[0], "mmproj_path": paths[1], "total": total, "adopted": adopted}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, thread};

    /// Minimal HTTP server: serves `body`, honouring `Range: bytes=N-`.
    fn serve(body: Vec<u8>, honour_range: bool) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let start = req
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                    .filter(|_| honour_range);
                let (code, slice) = match start {
                    Some(st) => ("206 Partial Content", &body[st..]),
                    None => ("200 OK", &body[..]),
                };
                let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len());
                let _ = s.write_all(slice);
            }
        });
        format!("http://{addr}/f.gguf")
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nuzzle-dl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn data() -> Vec<u8> {
        (0..60_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn downloads_a_file() {
        let body = data();
        let url = serve(body.clone(), true);
        let dest = tmp("full").join("m.gguf");
        let mut last = 0;
        fetch(&reqwest::Client::new(), &url, &dest, body.len() as u64, &AtomicBool::new(false), |d| last = d).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(last, body.len() as u64);
        assert!(!dest.with_extension("gguf.part").exists());
    }

    #[tokio::test]
    async fn resumes_a_partial_file() {
        let body = data();
        let url = serve(body.clone(), true);
        let dest = tmp("resume").join("m.gguf");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest.with_extension("gguf.part"), &body[..10_000]).unwrap();
        fetch(&reqwest::Client::new(), &url, &dest, body.len() as u64, &AtomicBool::new(false), |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn restarts_when_server_ignores_range() {
        let body = data();
        let url = serve(body.clone(), false);
        let dest = tmp("norange").join("m.gguf");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest.with_extension("gguf.part"), b"stale bytes").unwrap();
        fetch(&reqwest::Client::new(), &url, &dest, body.len() as u64, &AtomicBool::new(false), |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn short_download_is_rejected_and_cancel_works() {
        let body = data();
        let url = serve(body.clone(), true);
        let dest = tmp("short").join("m.gguf");
        let err = fetch(&reqwest::Client::new(), &url, &dest, body.len() as u64 + 5, &AtomicBool::new(false), |_| {}).await.unwrap_err();
        assert!(err.contains("ended early"), "{err}");
        assert!(!dest.exists());
        let err = fetch(&reqwest::Client::new(), &url, &tmp("cancel").join("m.gguf"), body.len() as u64, &AtomicBool::new(true), |_| {}).await.unwrap_err();
        assert_eq!(err, CANCELLED);
    }

    #[tokio::test]
    async fn unreachable_server_gives_friendly_error() {
        let dest = tmp("down").join("m.gguf");
        let err = fetch(&reqwest::Client::new(), "http://127.0.0.1:9/x.gguf", &dest, 10, &AtomicBool::new(false), |_| {}).await.unwrap_err();
        assert!(err.contains("internet connection"), "{err}");
    }

    #[test]
    fn urls_are_the_official_repo_at_a_pinned_revision() {
        for i in &ITEMS {
            assert!(i.url.starts_with(&format!("https://huggingface.co/LiquidAI/LFM3-Omni-2B-GGUF/resolve/{REVISION}/")));
            assert!(i.url.ends_with(i.name));
            assert_eq!(i.sha256.len(), 64);
        }
    }

    #[test]
    fn sha256_of_a_file() {
        let p = tmp("sha").join("x.bin");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
