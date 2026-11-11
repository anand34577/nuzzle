//! A small local web API so other programs and agents (OpenClaw, PicoClaw, n8n, a shell script) can talk to the
//! companion. Off by default; every request needs the token from Settings. Listens on 127.0.0.1 unless the user
//! allows the local network.
//!
//!   GET  /health                  -> {"ok": true, "name": "Pip", "ready": true}
//!   POST /message {"text": "..."} -> {"reply": "..."}
//!   POST /v1/chat/completions     -> OpenAI-style chat (the last user message is answered), for tools that speak it
//!
//! Requests go through the same brain, memory, tools and approval cards as everything else.

use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{brain, log, Core, LockExt};

const MAX_BODY: usize = 64 * 1024;

/// Keeps the server running while it is switched on, restarting it when its settings change.
pub fn spawn(core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let (on, port, lan) = {
                let s = core.settings.lk();
                (s.agent_api_enabled && !s.agent_api_token.trim().is_empty(), s.agent_api_port, s.agent_api_lan)
            };
            if !on {
                core.remote_restart.notified().await;
                continue;
            }
            let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
            let listener = match TcpListener::bind((host, port)).await {
                Ok(l) => l,
                Err(e) => {
                    log::warn(&format!("agent api: can't listen on {host}:{port}: {e}"));
                    core.status(format!("The agent API couldn't use port {port} ({e}). Pick another in Settings -> Remote."));
                    core.remote_restart.notified().await;
                    continue;
                }
            };
            log::info(&format!("agent api: listening on {host}:{port}"));
            loop {
                tokio::select! {
                    _ = core.remote_restart.notified() => break,
                    acc = listener.accept() => {
                        if let Ok((stream, _)) = acc {
                            let c = core.clone();
                            tokio::spawn(async move { let _ = serve(c, stream).await; });
                        }
                    }
                }
            }
        }
    });
}

struct Request {
    method: String,
    path: String,
    auth: String,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, &'static str> {
    let mut buf = Vec::with_capacity(4096);
    let head_end = loop {
        let mut chunk = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk)).await.map_err(|_| "timeout")?.map_err(|_| "read")?;
        if n == 0 {
            return Err("closed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 * 1024 {
            return Err("headers too large");
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let (mut len, mut auth) = (0usize, String::new());
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().unwrap_or(0),
                "authorization" => auth = v.trim().to_string(),
                _ => {}
            }
        }
    }
    if len > MAX_BODY {
        return Err("body too large");
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let mut chunk = vec![0u8; len - body.len()];
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk)).await.map_err(|_| "timeout")?.map_err(|_| "read")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Ok(Request { method, path, auth, body })
}

async fn reply(stream: &mut TcpStream, status: &str, v: &Value) -> std::io::Result<()> {
    let body = v.to_string();
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

/// Constant-time comparison, so the token can't be guessed a character at a time from response timing.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The text to answer from a request body: `{"text": ...}`, or the last user message of an OpenAI-style chat.
fn prompt_of(path: &str, v: &Value) -> Option<String> {
    let text = if path.starts_with("/v1/chat/completions") {
        let last = v["messages"].as_array()?.iter().rev().find(|m| m["role"] == "user")?;
        match &last["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
            _ => return None,
        }
    } else {
        v["text"].as_str()?.to_string()
    };
    let t = text.trim();
    (!t.is_empty() && t.chars().count() <= 4000).then(|| t.to_string())
}

async fn serve(core: Arc<Core>, mut stream: TcpStream) -> std::io::Result<()> {
    let req = match read_request(&mut stream).await {
        Ok(r) => r,
        Err(e) => return reply(&mut stream, "400 Bad Request", &json!({"error": e})).await,
    };
    let (token, name) = {
        let s = core.settings.lk();
        (s.agent_api_token.trim().to_string(), s.name.clone())
    };
    let given = req.auth.strip_prefix("Bearer ").unwrap_or("").trim();
    if token.is_empty() || !same(given, &token) {
        return reply(&mut stream, "401 Unauthorized", &json!({"error": "missing or wrong token (Authorization: Bearer <token>)"})).await;
    }
    let ready = core.ready.load(std::sync::atomic::Ordering::SeqCst);
    match (req.method.as_str(), req.path.split('?').next().unwrap_or("")) {
        ("GET", "/health") => reply(&mut stream, "200 OK", &json!({"ok": true, "name": name, "ready": ready})).await,
        ("POST", path @ ("/message" | "/v1/chat/completions")) => {
            let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            let Some(text) = prompt_of(path, &v) else {
                return reply(&mut stream, "400 Bad Request", &json!({"error": "send {\"text\": \"...\"} (up to 4000 characters)"})).await;
            };
            // Quick commands ("/timer 10m tea") answer at once, without the model.
            let answer = match crate::quick::parse(&text) {
                Some(cmd) => Ok(crate::quick::run(&core, cmd).await),
                None => brain::respond_remote(core.clone(), text, brain::Origin::Api).await,
            };
            match answer {
                Ok(answer) if path == "/message" => reply(&mut stream, "200 OK", &json!({"reply": answer})).await,
                Ok(answer) => {
                    let out = json!({
                        "id": format!("nuzzle-{}", crate::sys::unix_now()), "object": "chat.completion", "created": crate::sys::unix_now(),
                        "model": "nuzzle", "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": answer}}],
                    });
                    reply(&mut stream, "200 OK", &out).await
                }
                Err(e) => reply(&mut stream, "503 Service Unavailable", &json!({"error": e})).await,
            }
        }
        _ => reply(&mut stream, "404 Not Found", &json!({"error": "use GET /health, POST /message or POST /v1/chat/completions"})).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_are_read_from_both_shapes() {
        assert_eq!(prompt_of("/message", &json!({"text": "  hi "})).as_deref(), Some("hi"));
        let chat = json!({"messages": [{"role": "system", "content": "x"}, {"role": "user", "content": "first"}, {"role": "assistant", "content": "a"},
            {"role": "user", "content": [{"type": "text", "text": "second"}]}]});
        assert_eq!(prompt_of("/v1/chat/completions", &chat).as_deref(), Some("second"));
        assert_eq!(prompt_of("/message", &json!({"text": ""})), None);
        assert_eq!(prompt_of("/message", &json!({})), None);
    }

    #[test]
    fn token_comparison() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd") && !same("abc", "ab") && !same("", "a"));
    }
}
