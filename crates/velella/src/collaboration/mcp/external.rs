//! Bounded, full-duplex stdio relay for an explicitly selected tool catalog.
//! Resources, prompts, sampling and elicitation are outside this contract.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::Stdio,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 1024 * 1024;
const QUEUE_LIMIT: usize = 16;
const PENDING_LIMIT: usize = 128;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn descriptor(path: &Path, expected: &str) -> Result<Value> {
    let meta = std::fs::symlink_metadata(path).context("external MCP descriptor missing")?;
    ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= FRAME_LIMIT as u64, "invalid external MCP descriptor file");
    let bytes = std::fs::File::open(path)?.take(FRAME_LIMIT as u64 + 1).bytes().collect::<std::io::Result<Vec<_>>>()?;
    ensure!(bytes.len() <= FRAME_LIMIT && format!("{:x}", Sha256::digest(&bytes)) == expected, "external MCP descriptor changed; create a new session");
    let value: Value = serde_json::from_slice(&bytes)?;
    ensure!(value["command"].as_str().is_some_and(|s| !s.trim().is_empty() && s.len() <= 4096), "external MCP command invalid");
    ensure!(value["args"].as_array().is_some_and(|a| a.len() <= 64 && a.iter().all(|v| v.as_str().is_some_and(|s| s.len() <= 4096))), "external MCP args invalid");
    ensure!(value["tools"].as_array().is_some_and(|a| !a.is_empty() && a.len() <= 128 && a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128))), "external MCP tools invalid");
    if !value["source"].is_null() {
        let server:super::super::orchestration::v2::McpServer=serde_json::from_value(value.clone())?;
        super::super::mcp_catalog::verify(&server)?;
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side { Client, Server }
impl Side { fn other(self) -> Self { match self { Self::Client => Self::Server, Self::Server => Self::Client } } }
struct Pending { method: String, started: Instant }
struct Relay {
    allowed: HashSet<String>,
    client: HashMap<String, Pending>,
    server: HashMap<String, Pending>,
}
fn id_key(value: &Value) -> Result<String> {
    ensure!(value.is_string() || value.is_i64() || value.is_u64(), "MCP request ID must be a string or integer");
    Ok(value.to_string())
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
impl Relay {
    fn new(allowed: HashSet<String>) -> Self { Self { allowed, client: HashMap::new(), server: HashMap::new() } }
    fn pending(&mut self, side: Side) -> &mut HashMap<String, Pending> {
        match side { Side::Client => &mut self.client, Side::Server => &mut self.server }
    }
    fn check_deadlines(&self, now: Instant, timeout: Duration) -> Result<()> {
        ensure!(!self.client.values().chain(self.server.values()).any(|p| now.duration_since(p.started) >= timeout), "external MCP request timed out");
        Ok(())
    }
    fn message(&mut self, side: Side, mut value: Value) -> Result<Option<(Side, Value)>> {
        ensure!(value.is_object() && value["jsonrpc"] == "2.0", "invalid MCP JSON-RPC envelope");
        if let Some(method) = value.get("method").cloned() {
            let method = method.as_str().context("MCP method must be a string")?;
            ensure!(value.get("result").is_none() && value.get("error").is_none(), "MCP request contains a response");
            ensure!(value.get("params").is_none_or(Value::is_object), "MCP params must be an object");
            if let Some(id) = value.get("id").cloned() {
                let key = id_key(&id)?;
                ensure!(!self.pending(side).contains_key(&key), "duplicate in-flight MCP request ID");
                let permitted = match side {
                    Side::Client => matches!(method, "initialize" | "ping" | "tools/list" | "tools/call" | "logging/setLevel"),
                    Side::Server => matches!(method, "ping" | "roots/list"),
                };
                if !permitted { return Ok(Some((side, error(id, -32601, "method is outside this role's MCP tool contract")))); }
                if method == "tools/call" && !value["params"]["name"].as_str().is_some_and(|s| self.allowed.contains(s)) {
                    return Ok(Some((side, json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":"external MCP tool is not selected for this role"}],"isError":true}}))));
                }
                ensure!(self.client.len() + self.server.len() < PENDING_LIMIT, "too many in-flight MCP requests");
                if side == Side::Client && method == "initialize" {
                    let capabilities = value["params"]["capabilities"].as_object_mut().context("initialize capabilities missing")?;
                    capabilities.retain(|key, _| key == "roots");
                }
                self.pending(side).insert(key, Pending { method: method.into(), started: Instant::now() });
            } else {
                let permitted = match side {
                    Side::Client => matches!(method, "notifications/initialized" | "notifications/cancelled" | "notifications/roots/list_changed" | "notifications/progress"),
                    Side::Server => matches!(method, "notifications/cancelled" | "notifications/progress" | "notifications/message" | "notifications/tools/list_changed"),
                };
                if !permitted { return Ok(None); }
                if method == "notifications/cancelled" {
                    let key = id_key(&value["params"]["requestId"])?;
                    // Cancellation is advisory. Keep its deadline until the peer
                    // actually completes; repeated cancellation cannot hide a hang.
                    if !self.pending(side).contains_key(&key) { return Ok(None); }
                }
            }
        } else {
            let key = id_key(value.get("id").context("MCP response ID missing")?)?;
            ensure!(value.get("result").is_some() ^ value.get("error").is_some(), "MCP response requires exactly one result or error");
            if let Some(err) = value.get("error") { ensure!(err["code"].is_i64() && err["message"].is_string(), "invalid MCP error"); }
            let pending = self.pending(side.other()).remove(&key).context("unsolicited MCP response ID")?;
            if side == Side::Server && value.get("result").is_some() {
                if pending.method == "tools/list" {
                    let tools = value["result"]["tools"].as_array_mut().context("MCP tools/list response has no tools")?;
                    tools.retain(|tool| tool["name"].as_str().is_some_and(|s| self.allowed.contains(s)));
                } else if pending.method == "initialize" {
                    let capabilities = value["result"]["capabilities"].as_object_mut().context("MCP initialize response has no capabilities")?;
                    capabilities.retain(|key, _| matches!(key.as_str(), "tools" | "logging"));
                }
            }
        }
        Ok(Some((side.other(), value)))
    }
}

enum Event { Frame(Side, Vec<u8>), End(Side), Failed }
fn frame(input: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let count = input.take(FRAME_LIMIT as u64 + 1).read_until(b'\n', &mut bytes)?;
    ensure!(count <= FRAME_LIMIT, "MCP frame too large");
    Ok((count != 0).then_some(bytes))
}
fn reader(side: Side, input: impl Read + Send + 'static, tx: SyncSender<Event>) {
    std::thread::spawn(move || {
        let mut input = BufReader::new(input);
        loop {
            let event = match frame(&mut input) { Ok(Some(bytes)) => Event::Frame(side, bytes), Ok(None) => Event::End(side), Err(_) => Event::Failed };
            let terminal = !matches!(event, Event::Frame(..));
            if tx.send(event).is_err() || terminal { break; }
        }
    });
}
fn writer(mut output: impl Write + Send + 'static, tx: SyncSender<Event>) -> SyncSender<Value> {
    let (send, receive) = mpsc::sync_channel::<Value>(QUEUE_LIMIT);
    std::thread::spawn(move || {
        for value in receive {
            if writeln!(output, "{value}").and_then(|_| output.flush()).is_err() { let _ = tx.send(Event::Failed); break; }
        }
    });
    send
}
fn pump(events: Receiver<Event>, client: SyncSender<Value>, server: SyncSender<Value>, allowed: HashSet<String>, timeout: Duration) -> Result<()> {
    let mut relay = Relay::new(allowed);
    loop {
        relay.check_deadlines(Instant::now(), timeout)?;
        let event = match events.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        let (side, bytes) = match event {
            Event::Frame(side, bytes) => (side, bytes),
            Event::End(Side::Client) => return Ok(()),
            Event::End(Side::Server) => anyhow::bail!("external MCP closed stdout"),
            Event::Failed => anyhow::bail!("external MCP transport failed or exceeded frame limit"),
        };
        let action = match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => relay.message(side, value)?,
            Err(_) if side == Side::Client => Some((Side::Client, error(Value::Null, -32700, "Parse error"))),
            Err(_) => anyhow::bail!("external MCP returned invalid JSON"),
        };
        if let Some((side, value)) = action {
            match side { Side::Client => &client, Side::Server => &server }.try_send(value).context("external MCP output queue is blocked")?;
        }
    }
}

pub(crate) fn serve(path: &Path, expected: &str) -> Result<()> {
    let value = descriptor(path, expected)?;
    let allowed = value["tools"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    let mut command = crate::process::command(value["command"].as_str().unwrap());
    command.args(value["args"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()));
    // Selected external tools never inherit the control-plane credential routes.
    for (name, _) in std::env::vars_os() { if name.to_string_lossy().starts_with("VELELLA_") { command.env_remove(name); } }
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().context("cannot start selected external MCP")?;
    #[cfg(windows)]
    let group = match crate::process::ChildGroup::attach(child.id()) { Ok(group) => group, Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error).context("cannot own selected external MCP process tree"); } };
    let (events_tx, events_rx) = mpsc::sync_channel(QUEUE_LIMIT);
    reader(Side::Client, std::io::stdin(), events_tx.clone());
    reader(Side::Server, child.stdout.take().context("MCP stdout missing")?, events_tx.clone());
    let to_client = writer(std::io::stdout(), events_tx.clone());
    let to_server = writer(child.stdin.take().context("MCP stdin missing")?, events_tx);
    let result = pump(events_rx, to_client, to_server, allowed, REQUEST_TIMEOUT);
    #[cfg(windows)] { let _ = group.kill(); drop(group); }
    #[cfg(unix)] { unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); } }
    let _ = child.kill(); let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn relay() -> Relay { Relay::new(HashSet::from(["read".into()])) }
    #[test]
    fn external_mcp_matches_out_of_order_ids_and_duplex_requests() {
        let mut r = relay();
        r.message(Side::Client, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        r.message(Side::Client, json!({"jsonrpc":"2.0","id":"1","method":"tools/call","params":{"name":"read"}})).unwrap();
        let notify = json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"p","progress":1}});
        assert_eq!(r.message(Side::Server, notify.clone()).unwrap(), Some((Side::Client, notify)));
        r.message(Side::Server, json!({"jsonrpc":"2.0","id":1,"method":"roots/list"})).unwrap();
        assert!(r.message(Side::Client, json!({"jsonrpc":"2.0","id":1,"result":{"roots":[]}})).unwrap().is_some());
        let call = json!({"jsonrpc":"2.0","id":"1","result":{"tools":[{"name":"write"}]}});
        assert_eq!(r.message(Side::Server, call.clone()).unwrap(), Some((Side::Client, call)));
        let (_, list) = r.message(Side::Server, json!({"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"read"},{"name":"write"}]}})).unwrap().unwrap();
        assert_eq!(list["result"]["tools"], json!([{"name":"read"}]));
        assert!(r.client.is_empty() && r.server.is_empty());
    }
    #[test]
    fn external_mcp_rejects_bypass_and_unsolicited_or_duplicate_ids() {
        let mut r = relay();
        let (side, denied) = r.message(Side::Client, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write"}})).unwrap().unwrap();
        assert_eq!(side, Side::Client); assert_eq!(denied["result"]["isError"], true); assert!(r.client.is_empty());
        assert_eq!(r.message(Side::Client, json!({"jsonrpc":"2.0","id":3,"method":"resources/read"})).unwrap().unwrap().1["error"]["code"], -32601);
        assert_eq!(r.message(Side::Server, json!({"jsonrpc":"2.0","id":3,"method":"sampling/createMessage"})).unwrap().unwrap().1["error"]["code"], -32601);
        assert!(r.message(Side::Server, json!({"jsonrpc":"2.0","id":42,"result":{}})).is_err());
        let request = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
        r.message(Side::Client, request.clone()).unwrap(); assert!(r.message(Side::Client, request).is_err());
        assert!(r.message(Side::Client, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap().is_some());
        assert_eq!(r.client.len(), 1);
    }
    #[test]
    fn external_mcp_filters_capabilities_and_bounds_frames_and_pending() {
        let mut r = relay();
        let init = r.message(Side::Client, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"roots":{},"sampling":{},"elicitation":{}}}})).unwrap().unwrap().1;
        assert_eq!(init["params"]["capabilities"], json!({"roots":{}}));
        let result = r.message(Side::Server, json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{"tools":{},"resources":{},"prompts":{}}}})).unwrap().unwrap().1;
        assert_eq!(result["result"]["capabilities"], json!({"tools":{}}));
        assert!(frame(&mut &vec![b'x'; FRAME_LIMIT+1][..]).is_err());
        for id in 0..PENDING_LIMIT { r.message(Side::Client, json!({"jsonrpc":"2.0","id":id,"method":"ping"})).unwrap(); }
        assert!(r.message(Side::Client, json!({"jsonrpc":"2.0","id":999,"method":"ping"})).is_err());
        assert!(r.check_deadlines(Instant::now() + Duration::from_secs(121), REQUEST_TIMEOUT).is_err());
    }
    #[test]
    fn external_mcp_pump_times_out_while_peer_is_silent() {
        let (tx, rx) = mpsc::sync_channel(QUEUE_LIMIT); let (client, _) = mpsc::sync_channel(QUEUE_LIMIT); let (server, keep) = mpsc::sync_channel(QUEUE_LIMIT);
        tx.send(Event::Frame(Side::Client, br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec())).unwrap();
        let start = Instant::now();
        assert!(pump(rx, client, server, HashSet::new(), Duration::from_millis(30)).unwrap_err().to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(1)); drop(keep);
    }
}
