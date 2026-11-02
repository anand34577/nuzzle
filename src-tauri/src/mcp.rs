//! MCP (Model Context Protocol) client. The user lists servers in Settings; each one is started (stdio) or
//! contacted (http), its tools are discovered with `tools/list`, and they are offered to the model as
//! `mcp_<server>_<tool>` functions. Results are returned to the model as data, never as instructions.

use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::oneshot,
};

use crate::{log, settings::McpServerCfg, Core, LockExt};

const PROTOCOL: &str = "2025-06-18";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: u64 = 120;
const MAX_TOOLS_PER_SERVER: usize = 100;
const MAX_TOOLS_TOTAL: usize = 48;
const MAX_RESULT_CHARS: usize = 8000;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

// ------------------------------------------------------------------ transports

pub struct StdioClient {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    tail: Arc<Mutex<VecDeque<String>>>,
    _child: Child, // kill_on_drop
}

pub struct HttpClient {
    http: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
    session: Mutex<Option<String>>,
    next: AtomicU64,
}

pub enum Client {
    Stdio(StdioClient),
    Http(HttpClient),
}

async fn send_line(stdin: &tokio::sync::Mutex<ChildStdin>, v: &Value) -> Result<(), String> {
    let mut line = v.to_string();
    line.push('\n');
    let mut w = stdin.lock().await;
    w.write_all(line.as_bytes()).await.map_err(|e| format!("the MCP server isn't accepting input ({e})"))?;
    w.flush().await.map_err(|e| e.to_string())
}

fn command_for(cfg: &McpServerCfg) -> Command {
    // npx, uvx and friends are .cmd/.bat shims on Windows; run those through cmd.exe.
    #[cfg(windows)]
    let mut c = {
        let lower = cfg.command.to_lowercase();
        if lower.ends_with(".exe") {
            let mut c = Command::new(&cfg.command);
            c.args(&cfg.args);
            c
        } else {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&cfg.command).args(&cfg.args);
            c
        }
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = Command::new(&cfg.command);
        c.args(&cfg.args);
        c
    };
    c.envs(&cfg.env).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // no console window
    c
}

impl StdioClient {
    async fn spawn(cfg: &McpServerCfg, on_exit: impl FnOnce(String) + Send + 'static) -> Result<Self, String> {
        let mut child = command_for(cfg)
            .spawn()
            .map_err(|e| format!("I couldn't start '{}' ({e}). Is it installed and on your PATH?", cfg.command))?;
        #[cfg(windows)]
        if let Some(h) = child.raw_handle() {
            crate::job::adopt(h);
        }
        let stdin = Arc::new(tokio::sync::Mutex::new(child.stdin.take().ok_or("no stdin")?));
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();

        let (name, t2) = (cfg.name.clone(), tail.clone());
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                log::write("MCP", &format!("[{name}] {l}"));
                let mut t = t2.lk();
                if t.len() >= 5 {
                    t.pop_front();
                }
                t.push_back(l);
            }
        });

        let (p2, a2, s2, t3) = (pending.clone(), alive.clone(), stdin.clone(), tail.clone());
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v.get("method").is_some() {
                    // A request from the server (ping, roots, sampling...): answer pings, refuse the rest.
                    if let Some(id) = v.get("id") {
                        let reply = if v["method"] == "ping" {
                            json!({"jsonrpc": "2.0", "id": id, "result": {}})
                        } else {
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not supported"}})
                        };
                        let _ = send_line(&s2, &reply).await;
                    }
                    continue;
                }
                if let Some(id) = v["id"].as_u64() {
                    if let Some(tx) = p2.lk().remove(&id) {
                        let _ = tx.send(match v.get("error") {
                            Some(e) => Err(e["message"].as_str().unwrap_or("MCP error").to_string()),
                            None => Ok(v["result"].clone()),
                        });
                    }
                }
            }
            a2.store(false, Ordering::SeqCst);
            let tail_text = t3.lk().iter().cloned().collect::<Vec<_>>().join(" | ");
            let why = if tail_text.is_empty() { "The MCP server closed the connection.".to_string() } else { format!("The MCP server stopped: {tail_text}") };
            for (_, tx) in p2.lk().drain() {
                let _ = tx.send(Err(why.clone()));
            }
            on_exit(tail_text);
        });

        Ok(Self { stdin, pending, next: AtomicU64::new(1), alive, tail, _child: child })
    }

    async fn request(&self, method: &str, params: Value, secs: u64) -> Result<Value, String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(self.stopped());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lk().insert(id, tx);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = send_line(&self.stdin, &msg).await {
            self.pending.lk().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_secs(secs), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(self.stopped()),
            Err(_) => {
                self.pending.lk().remove(&id);
                Err(format!("The MCP server didn't answer within {secs} seconds."))
            }
        }
    }

    fn stopped(&self) -> String {
        let t = self.tail.lk().iter().cloned().collect::<Vec<_>>().join(" | ");
        if t.is_empty() { "The MCP server has stopped.".into() } else { format!("The MCP server has stopped: {t}") }
    }
}

impl HttpClient {
    fn new(cfg: &McpServerCfg) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .user_agent("Companion/0.1")
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, url: cfg.url.clone(), headers: cfg.headers.clone(), session: Mutex::new(None), next: AtomicU64::new(1) })
    }

    async fn post(&self, body: &Value, secs: u64) -> Result<reqwest::Response, String> {
        let mut req = self
            .http
            .post(&self.url)
            .timeout(Duration::from_secs(secs))
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL)
            .json(body);
        if let Some(s) = self.session.lk().clone() {
            req = req.header("Mcp-Session-Id", s);
        }
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        req.send().await.map_err(|e| format!("I couldn't reach the MCP server ({e})."))
    }

    async fn request(&self, method: &str, params: Value, secs: u64) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let resp = self.post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}), secs).await?;
        if let Some(s) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *self.session.lk() = Some(s.to_string());
        }
        if !resp.status().is_success() {
            return Err(format!("The MCP server answered HTTP {}.", resp.status().as_u16()));
        }
        let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
        let text = resp.text().await.map_err(|e| format!("The MCP response was cut off ({e})."))?;
        let msg = if ctype.contains("text/event-stream") { sse_message(&text, id) } else { serde_json::from_str::<Value>(&text).ok() };
        let msg = msg.ok_or("The MCP server sent a reply I couldn't read.")?;
        match msg.get("error") {
            Some(e) => Err(e["message"].as_str().unwrap_or("MCP error").to_string()),
            None => Ok(msg["result"].clone()),
        }
    }
}

/// The JSON-RPC message with `id` from a server-sent-events body.
fn sse_message(body: &str, id: u64) -> Option<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
        .find(|v| v["id"].as_u64() == Some(id) && v.get("method").is_none())
}

impl Client {
    async fn request(&self, method: &str, params: Value, secs: u64) -> Result<Value, String> {
        match self {
            Client::Stdio(c) => c.request(method, params, secs).await,
            Client::Http(c) => c.request(method, params, secs).await,
        }
    }

    async fn notify(&self, method: &str) {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        match self {
            Client::Stdio(c) => {
                let _ = send_line(&c.stdin, &msg).await;
            }
            Client::Http(c) => {
                let _ = c.post(&msg, 10).await;
            }
        }
    }

    async fn handshake(&self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({"protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": {"name": "companion", "version": "0.1"}}),
            20,
        )
        .await?;
        self.notify("notifications/initialized").await;
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<RawTool>, String> {
        let mut out = vec![];
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let params = cursor.as_ref().map_or(json!({}), |c| json!({"cursor": c}));
            let r = self.request("tools/list", params, 20).await?;
            for t in r["tools"].as_array().into_iter().flatten() {
                if let Some(name) = t["name"].as_str() {
                    out.push(RawTool {
                        name: name.to_string(),
                        description: t["description"].as_str().unwrap_or("").to_string(),
                        schema: t["inputSchema"].clone(),
                    });
                }
            }
            cursor = r["nextCursor"].as_str().map(str::to_string);
            if cursor.is_none() || out.len() >= MAX_TOOLS_PER_SERVER {
                break;
            }
        }
        out.truncate(MAX_TOOLS_PER_SERVER);
        Ok(out)
    }
}

// ------------------------------------------------------------------ hub

#[derive(Clone, Debug)]
pub struct RawTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Clone, Debug)]
pub struct ToolInfo {
    pub server: String,
    pub name: String,
    pub alias: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ServerStatus {
    pub name: String,
    /// off | starting | ready | error
    pub state: String,
    pub tools: Vec<String>,
    pub message: String,
}

#[derive(Default)]
pub struct Hub {
    clients: Mutex<HashMap<String, Arc<Client>>>,
    raw: Mutex<HashMap<String, Vec<RawTool>>>,
    tools: Mutex<Vec<ToolInfo>>,
    status: Mutex<Vec<ServerStatus>>,
    generation: AtomicU64,
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// Model-facing function names: `mcp_<server>_<tool>`, at most 64 characters, unique.
pub fn make_aliases(raw: &HashMap<String, Vec<RawTool>>) -> Vec<ToolInfo> {
    let mut servers: Vec<&String> = raw.keys().collect();
    servers.sort();
    let mut used = HashSet::new();
    let mut out = vec![];
    for s in servers {
        for t in &raw[s] {
            let mut alias: String = format!("mcp_{}_{}", sanitize(s), sanitize(&t.name)).chars().take(64).collect();
            let mut n = 2;
            while !used.insert(alias.clone()) {
                let suffix = format!("_{n}");
                alias = format!("{}{suffix}", alias.chars().take(64 - suffix.len()).collect::<String>());
                n += 1;
            }
            out.push(ToolInfo { server: s.clone(), name: t.name.clone(), alias, description: t.description.clone(), schema: t.schema.clone() });
        }
    }
    out.truncate(MAX_TOOLS_TOTAL);
    out
}

/// A tool-call result as plain text for the model.
pub fn format_result(r: &Value) -> Result<String, String> {
    let mut parts: Vec<String> = vec![];
    for c in r["content"].as_array().into_iter().flatten() {
        match c["type"].as_str() {
            Some("text") => parts.push(c["text"].as_str().unwrap_or("").to_string()),
            Some("image") => parts.push("[image omitted]".into()),
            Some("audio") => parts.push("[audio omitted]".into()),
            Some("resource") => parts.push(c["resource"]["text"].as_str().unwrap_or("[resource]").to_string()),
            Some("resource_link") => parts.push(c["uri"].as_str().unwrap_or("[link]").to_string()),
            _ => {}
        }
    }
    if parts.is_empty() && !r["structuredContent"].is_null() {
        parts.push(r["structuredContent"].to_string());
    }
    let mut text = parts.join("\n");
    if text.chars().count() > MAX_RESULT_CHARS {
        text = text.chars().take(MAX_RESULT_CHARS).collect::<String>() + "\n[truncated]";
    }
    if r["isError"].as_bool() == Some(true) {
        Err(if text.is_empty() { "The tool reported an error.".into() } else { text })
    } else {
        Ok(text)
    }
}

/// An object schema the model can use (MCP guarantees `inputSchema` is an object schema, but be defensive).
fn clean_schema(s: &Value) -> Value {
    let mut v = if s.is_object() { s.clone() } else { json!({}) };
    v["type"] = json!("object");
    if !v["properties"].is_object() {
        v["properties"] = json!({});
    }
    if v.to_string().len() > 6000 {
        return json!({"type": "object", "properties": {}, "additionalProperties": true});
    }
    v
}

impl Hub {
    pub fn statuses(&self) -> Vec<ServerStatus> {
        self.status.lk().clone()
    }

    pub fn tool_count(&self) -> usize {
        self.tools.lk().len()
    }

    /// Function specs for the model.
    pub fn specs(&self) -> Vec<Value> {
        self.tools
            .lk()
            .iter()
            .map(|t| {
                let desc: String = format!("[{}] {}", t.server, t.description).chars().take(500).collect();
                json!({"type": "function", "function": {"name": t.alias, "description": desc, "parameters": clean_schema(&t.schema)}})
            })
            .collect()
    }

    pub fn find(&self, alias: &str) -> Option<ToolInfo> {
        self.tools.lk().iter().find(|t| t.alias == alias).cloned()
    }

    fn set_status(&self, name: &str, state: &str, message: &str) {
        let mut st = self.status.lk();
        let tools: Vec<String> = self.raw.lk().get(name).map(|v| v.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
        let entry = ServerStatus { name: name.to_string(), state: state.into(), tools, message: message.into() };
        match st.iter_mut().find(|s| s.name == name) {
            Some(e) => *e = entry,
            None => st.push(entry),
        }
    }

    fn rebuild(&self) {
        *self.tools.lk() = make_aliases(&self.raw.lk());
    }

    /// Stop everything and start the enabled servers from `cfgs`. Returns once all have settled.
    pub async fn reload(self: &Arc<Self>, core: &Arc<Core>, cfgs: Vec<McpServerCfg>) {
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.clients.lk().clear(); // dropping a client kills its process
        self.raw.lk().clear();
        self.tools.lk().clear();
        self.status.lk().clear();
        for c in &cfgs {
            self.set_status(&c.name, if c.enabled { "starting" } else { "off" }, "");
        }
        core.emit("mcp", json!(null));

        let mut tasks = vec![];
        for cfg in cfgs.into_iter().filter(|c| c.enabled) {
            let (hub, core) = (self.clone(), core.clone());
            tasks.push(tokio::spawn(async move { hub.start_one(&core, cfg, gen).await }));
        }
        for t in tasks {
            let _ = t.await;
        }
    }

    async fn start_one(self: &Arc<Self>, core: &Arc<Core>, cfg: McpServerCfg, gen: u64) {
        let result = tokio::time::timeout(START_TIMEOUT, async {
            let client = if cfg.kind == "http" {
                Client::Http(HttpClient::new(&cfg)?)
            } else {
                let (hub, core2, name) = (self.clone(), core.clone(), cfg.name.clone());
                Client::Stdio(
                    StdioClient::spawn(&cfg, move |tail| {
                        if hub.generation.load(Ordering::SeqCst) == gen {
                            let msg = if tail.is_empty() { "The server stopped.".to_string() } else { format!("The server stopped: {tail}") };
                            hub.set_status(&name, "error", &msg);
                            core2.emit("mcp", json!(null));
                        }
                    })
                    .await?,
                )
            };
            client.handshake().await?;
            let tools = client.list_tools().await?;
            Ok::<_, String>((client, tools))
        })
        .await
        .unwrap_or_else(|_| Err("The server took too long to start.".into()));

        if self.generation.load(Ordering::SeqCst) != gen {
            return; // settings changed again while this was starting
        }
        match result {
            Ok((client, tools)) => {
                log::info(&format!("MCP '{}' ready with {} tools", cfg.name, tools.len()));
                self.raw.lk().insert(cfg.name.clone(), tools);
                self.clients.lk().insert(cfg.name.clone(), Arc::new(client));
                self.rebuild();
                self.set_status(&cfg.name, "ready", "");
            }
            Err(e) => {
                log::warn(&format!("MCP '{}' failed: {e}", cfg.name));
                self.set_status(&cfg.name, "error", &e);
            }
        }
        core.emit("mcp", json!(null));
    }

    pub async fn call(&self, alias: &str, args: Value) -> Result<String, String> {
        let t = self.find(alias).ok_or("That tool isn't available any more.")?;
        let client = self.clients.lk().get(&t.server).cloned().ok_or("That MCP server isn't running.")?;
        let args = if args.is_object() { args } else { json!({}) };
        let r = client.request("tools/call", json!({"name": t.name, "arguments": args}), CALL_TIMEOUT).await?;
        format_result(&r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(names: &[&str]) -> Vec<RawTool> {
        names.iter().map(|n| RawTool { name: n.to_string(), description: "d".into(), schema: json!({}) }).collect()
    }

    #[test]
    fn aliases_are_valid_and_unique() {
        let mut m = HashMap::new();
        m.insert("files".to_string(), raw(&["read file", "read_file", "read.file"]));
        m.insert("web".to_string(), raw(&["fetch"]));
        let t = make_aliases(&m);
        assert_eq!(t.len(), 4);
        let set: HashSet<&String> = t.iter().map(|x| &x.alias).collect();
        assert_eq!(set.len(), 4, "aliases must be unique: {t:?}");
        for x in &t {
            assert!(x.alias.starts_with("mcp_"));
            assert!(x.alias.len() <= 64);
            assert!(x.alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{}", x.alias);
        }
        let long = HashMap::from([("s".to_string(), raw(&[&"x".repeat(200)]))]);
        assert_eq!(make_aliases(&long)[0].alias.len(), 64);
    }

    #[test]
    fn results_are_flattened() {
        let ok = json!({"content": [{"type": "text", "text": "hello"}, {"type": "image", "data": "..."}, {"type": "text", "text": "world"}]});
        assert_eq!(format_result(&ok).unwrap(), "hello\n[image omitted]\nworld");
        let err = json!({"content": [{"type": "text", "text": "boom"}], "isError": true});
        assert_eq!(format_result(&err).unwrap_err(), "boom");
        let structured = json!({"content": [], "structuredContent": {"a": 1}});
        assert_eq!(format_result(&structured).unwrap(), "{\"a\":1}");
        let big = json!({"content": [{"type": "text", "text": "x".repeat(20_000)}]});
        assert!(format_result(&big).unwrap().len() < 8100);
    }

    #[test]
    fn schemas_are_made_safe() {
        assert_eq!(clean_schema(&json!(null))["type"], "object");
        assert!(clean_schema(&json!({"type": "object"}))["properties"].is_object());
        let huge = json!({"type": "object", "properties": {"x": {"description": "y".repeat(7000)}}});
        assert_eq!(clean_schema(&huge)["additionalProperties"], true);
    }

    #[test]
    fn sse_bodies_are_parsed() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(sse_message(body, 7).unwrap()["result"]["ok"], true);
        assert!(sse_message(body, 8).is_none());
    }

    #[test]
    fn tool_list_is_capped() {
        let mut m = HashMap::new();
        for s in 0..6 {
            m.insert(format!("s{s}"), raw(&(0..20).map(|i| format!("t{i}")).map(|s| Box::leak(s.into_boxed_str()) as &str).collect::<Vec<_>>()));
        }
        assert_eq!(make_aliases(&m).len(), MAX_TOOLS_TOTAL);
    }

    /// End-to-end against a tiny stdio server written in Python (skipped when Python isn't installed).
    #[tokio::test]
    async fn talks_to_a_stdio_server() {
        let py = if cfg!(windows) { "python" } else { "python3" };
        if std::process::Command::new(py).arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("companion-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("server.py");
        std::fs::write(
            &script,
            r#"
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if 'id' not in m: continue
    meth = m['method']
    if meth == 'initialize':
        r = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'fake', 'version': '1'}}
    elif meth == 'tools/list':
        r = {'tools': [{'name': 'echo', 'description': 'Echo text', 'inputSchema': {'type': 'object', 'properties': {'text': {'type': 'string'}}}}]}
    elif meth == 'tools/call':
        r = {'content': [{'type': 'text', 'text': 'echo: ' + m['params']['arguments'].get('text', '')}]}
    else:
        print(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'error': {'code': -32601, 'message': 'nope'}}), flush=True); continue
    print(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}), flush=True)
"#,
        )
        .unwrap();
        let cfg = McpServerCfg {
            name: "fake".into(),
            command: py.into(),
            args: vec![script.to_string_lossy().to_string()],
            ..Default::default()
        };
        let client = Client::Stdio(StdioClient::spawn(&cfg, |_| {}).await.unwrap());
        client.handshake().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let r = client.request("tools/call", json!({"name": "echo", "arguments": {"text": "hi"}}), 10).await.unwrap();
        assert_eq!(format_result(&r).unwrap(), "echo: hi");
        let err = client.request("bogus/method", json!({}), 10).await.unwrap_err();
        assert_eq!(err, "nope");
    }

    #[tokio::test]
    async fn missing_program_gives_friendly_error() {
        let cfg = McpServerCfg { name: "x".into(), command: "definitely-not-a-real-program-xyz.exe".into(), ..Default::default() };
        // On Windows the cmd.exe shim starts fine and then exits; the handshake must fail rather than hang.
        let res = tokio::time::timeout(Duration::from_secs(15), async {
            let c = Client::Stdio(StdioClient::spawn(&cfg, |_| {}).await?);
            c.handshake().await
        })
        .await
        .expect("must not hang");
        assert!(res.is_err());
    }
}
