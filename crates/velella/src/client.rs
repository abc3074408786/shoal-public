use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// Default API address; same resolution as `velella serve`.
pub fn default_addr() -> anyhow::Result<String> {
    let path = Path::new(".velella/config.toml");
    if path.exists() {
        let cfg = velella_core::config::Config::parse(&std::fs::read_to_string(path)?)?;
        Ok(cfg.listen)
    } else {
        Ok(velella_core::config::Config::default().listen)
    }
}

/// One NDJSON request, one connection. Errors from the daemon become
/// `Err` on the client side.
pub fn call(req: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let addr = default_addr()?;
    let mut stream =
        TcpStream::connect(&addr).map_err(|_| anyhow::anyhow!("cannot reach daemon at {addr} — run `velella serve`"))?;
    stream.write_all(format!("{req}\n").as_bytes())?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    if line.is_empty() {
        anyhow::bail!("empty response from {addr}");
    }
    let resp: serde_json::Value = serde_json::from_str(line.trim())?;
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let msg = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown daemon error");
        anyhow::bail!("{msg}");
    }
    Ok(resp)
}

/// Live-follow a session's output by polling with absolute offsets.
/// Ctrl-C ends the client; the daemon keeps the session sailing.
pub fn attach(id: &str) -> anyhow::Result<()> {
    eprintln!("attaching to {id} — Ctrl-C to detach (session keeps running)");
    let mut since = 0u64;
    let mut stdout = std::io::stdout();
    loop {
        let resp = call(serde_json::json!({ "cmd": "session.output", "id": id, "since": since }))?;
        let from = resp["from"].as_u64().unwrap_or(0);
        let end = resp["end"].as_u64().unwrap_or(0);
        if from > since {
            eprintln!("\n[output truncated: {} bytes lost]", from - since);
        }
        if !resp["data"].as_str().unwrap_or("").is_empty() {
            write!(stdout, "{}", resp["data"].as_str().unwrap_or(""))?;
            stdout.flush()?;
        }
        since = end;
        let state = resp["state"].as_str().unwrap_or("running");
        if matches!(state, "done" | "failed" | "stopped") {
            let exit = resp["exit"].as_i64().unwrap_or(-1);
            eprintln!("\n[session {id}: {state}, exit {exit}]");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}
