//! Minimal HTTP API over the same dispatch core as the NDJSON protocol.
//!
//! Zero dependencies, HTTP/1.1 subset, one request per connection. This is
//! the phone-reachable door: an iOS Shortcut or Slack webhook hits
//! `POST /sessions/<id>/approve` with a bearer token and answers an agent's
//! permission dialog from anywhere. Security model: without `http_token`
//! the default bind is localhost-only; a token is required before exposing
//! beyond that.

use crate::serve::AppState;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;

/// Map (method, path, query, body) to a dispatch command. Pure — unit-tested.
/// Err = (http_status, message).
pub fn route(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: &serde_json::Value,
) -> Result<serde_json::Value, (u16, String)> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match (method, segs.as_slice()) {
        ("GET", []) => Ok(serde_json::json!({ "cmd": "status" })),
        ("GET", ["status"]) => Ok(serde_json::json!({ "cmd": "status" })),
        ("GET", ["tasks"]) => Ok(serde_json::json!({ "cmd": "task.list" })),
        ("GET", ["sessions"]) => Ok(serde_json::json!({ "cmd": "session.list" })),
        ("GET", ["sessions", id, "output"]) => {
            let mut cmd = serde_json::json!({ "cmd": "session.output", "id": id });
            if let Some(since) = query.get("since").and_then(|v| v.parse::<u64>().ok()) {
                cmd["since"] = serde_json::json!(since);
            }
            Ok(cmd)
        }
        ("POST", ["sessions", id, "send"]) => {
            let text = body["text"].as_str().unwrap_or_default();
            let enter = body["enter"].as_bool().unwrap_or(true);
            Ok(serde_json::json!({ "cmd": "session.send", "id": id, "text": text, "enter": enter }))
        }
        ("POST", ["sessions", id, "approve"]) => {
            // the phone door: answer the dialog the classifier flagged
            let answer = body["answer"].as_str().unwrap_or("1");
            Ok(serde_json::json!({ "cmd": "session.send", "id": id, "text": answer, "enter": true }))
        }
        ("POST", ["sessions", id, "kill"]) => {
            Ok(serde_json::json!({ "cmd": "session.kill", "id": id }))
        }
        ("POST", ["tasks", name, "verify"]) => {
            Ok(serde_json::json!({ "cmd": "task.verify", "name": name }))
        }
        ("POST", ["tasks", name, "pr"]) => {
            Ok(serde_json::json!({ "cmd": "task.pr", "name": name }))
        }
        _ => Err((404, format!("no route: {method} {path}"))),
    }
}

fn parse_query(qs: &str) -> HashMap<String, String> {
    qs.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn authorized(req_token: Option<&str>, headers: &HashMap<String, String>, query: &HashMap<String, String>) -> bool {
    let Some(expected) = req_token else { return true };
    if let Some(bearer) = headers.get("authorization") {
        if bearer.trim() == format!("Bearer {expected}") {
            return true;
        }
    }
    query.get("token").map(|t| t == expected).unwrap_or(false)
}

const INDEX: &str = "velella HTTP API - the browser dashboard lives at /\n\
  GET  /status\n\
  GET  /tasks\n\
  GET  /sessions\n\
  GET  /sessions/{id}/output?since=N\n\
  POST /sessions/{id}/send      {\"text\": \"...\", \"enter\": true}\n\
  POST /sessions/{id}/approve   {\"answer\": \"1\"}   <- the phone door\n\
  POST /sessions/{id}/kill\n\
  POST /tasks/{name}/verify\n\
  POST /tasks/{name}/pr\n";

/// The web dashboard: single file, no build step, served from the binary.
const DASHBOARD: &str = include_str!("../dashboard.html");

pub fn handle_conn(mut stream: std::net::TcpStream, state: &Arc<AppState>) -> std::io::Result<()> {
    let mut request_line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        // headers
        let mut headers: HashMap<String, String> = HashMap::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let line = line.trim_end().to_string();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        let content_length: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body_bytes = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body_bytes)?;
        }
        let body: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));

        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("").to_uppercase();
        let target = parts.next().unwrap_or("/");
        let (path, qs) = target.split_once('?').unwrap_or((target, ""));
        let query = parse_query(qs);

        if !authorized(state.config.http_token.as_deref(), &headers, &query) {
            respond(&mut stream, 401, &serde_json::json!({ "ok": false, "error": "unauthorized" }))?;
            return Ok(());
        }
        if path == "/" {
            respond_html(&mut stream, 200, DASHBOARD)?;
            return Ok(());
        }
        if path == "/help" {
            respond_plain(&mut stream, 200, INDEX)?;
            return Ok(());
        }
        match route(&method, path, &query, &body) {
            Ok(cmd) => {
                let resp = crate::serve::dispatch(cmd, state);
                let status = if resp["ok"].as_bool().unwrap_or(false) { 200 } else { 400 };
                respond(&mut stream, status, &resp)?;
            }
            Err((code, msg)) => {
                respond(&mut stream, code, &serde_json::json!({ "ok": false, "error": msg }))?;
            }
        }
    }
    Ok(())
}

fn respond(stream: &mut std::net::TcpStream, code: u16, body: &serde_json::Value) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn respond_plain(stream: &mut std::net::TcpStream, code: u16, text: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code} OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn respond_html(stream: &mut std::net::TcpStream, code: u16, html: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )?;
    stream.flush()
}

/// Bind and serve until the process dies. Called from `velella serve`.
pub fn serve_loop(state: Arc<AppState>, addr: &str) -> anyhow::Result<()> {
    let listener = std::net::TcpListener::bind(addr)?;
    let auth = if state.config.http_token.is_some() {
        "token required"
    } else {
        "localhost trust (set http_token before exposing)"
    };
    eprintln!("velella http api on {addr} ({auth})");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = handle_conn(stream, &state);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_map_to_dispatch_commands() {
        let q = HashMap::new();
        let empty = serde_json::json!({});

        assert_eq!(
            route("GET", "/status", &q, &empty).unwrap(),
            serde_json::json!({ "cmd": "status" })
        );
        assert_eq!(
            route("GET", "/sessions/ses_1/output", &q, &empty).unwrap(),
            serde_json::json!({ "cmd": "session.output", "id": "ses_1" })
        );
        let q2 = parse_query("since=42&token=x");
        assert_eq!(q2["since"], "42");
        assert_eq!(
            route("GET", "/sessions/ses_1/output", &q2, &empty).unwrap()["since"],
            serde_json::json!(42)
        );
        let body = serde_json::json!({ "answer": "2" });
        assert_eq!(
            route("POST", "/sessions/ses_1/approve", &q, &body).unwrap(),
            serde_json::json!({ "cmd": "session.send", "id": "ses_1", "text": "2", "enter": true })
        );
        assert_eq!(
            route("POST", "/tasks/t1/verify", &q, &empty).unwrap(),
            serde_json::json!({ "cmd": "task.verify", "name": "t1" })
        );
    }

    #[test]
    fn unknown_routes_404() {
        let q = HashMap::new();
        let err = route("DELETE", "/", &q, &serde_json::json!({})).unwrap_err();
        assert_eq!(err.0, 404);
    }

    #[test]
    fn token_auth_accepts_bearer_and_query() {
        let mut headers = HashMap::new();
        headers.insert("authorization".into(), "Bearer sec".into());
        let q = HashMap::new();
        assert!(authorized(Some("sec"), &headers, &q));
        let q2 = parse_query("token=sec");
        assert!(authorized(Some("sec"), &HashMap::new(), &q2));
        assert!(!authorized(Some("sec"), &HashMap::new(), &q));
        // no token configured: localhost trust
        assert!(authorized(None, &HashMap::new(), &q));
    }
}
