//! Strict SSH + the runtime's framed protocol. Every call confirms one exact
//! inspected identity. No passwords, host-key acceptance, daemon startup or replay.
use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
};

#[derive(Clone)]
pub(super) struct Target {
    profile: Value,
    server_id: String,
    data_root: String,
}

pub(super) fn target(
    client: &Client,
    id: &str,
    server_id: uuid::Uuid,
    data_root: &str,
) -> Result<Target> {
    anyhow::ensure!(
        !data_root.is_empty()
            && data_root.len() <= 4096
            && !data_root.chars().any(char::is_control),
        "invalid remote data root"
    );
    let profiles = client.request(json!({"cmd":"ssh.list"}))?;
    let profile = find(array(&profiles, "profiles")?, id, "SSH profile")?;
    arguments(&profile)?;
    Ok(Target {
        profile,
        server_id: server_id.to_string(),
        data_root: data_root.into(),
    })
}

fn plain_token(text: &str, maximum: usize, allowed: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= maximum
        && !text.starts_with('-')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || allowed.contains(&b))
}

fn arguments(profile: &Value) -> Result<Vec<String>> {
    let host = profile["host"].as_str().context("SSH host missing")?;
    anyhow::ensure!(plain_token(host, 253, b"._-:[]"), "invalid SSH host");
    let port = profile["port"].as_u64().unwrap_or(22);
    anyhow::ensure!((1..=65535).contains(&port), "invalid SSH port");
    let mut args = vec!["-T".into()];
    for option in [
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "ForwardAgent=no",
        "ForwardX11=no",
        "ForwardX11Trusted=no",
        "ClearAllForwardings=yes",
        "PermitLocalCommand=no",
        "RemoteCommand=none",
        "RequestTTY=no",
        "ControlMaster=no",
        "ControlPath=none",
        "ConnectTimeout=10",
        "ConnectionAttempts=1",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=2",
    ] {
        args.extend(["-o".into(), option.into()]);
    }
    args.extend(["-p".into(), port.to_string()]);
    if let Some(user) = profile["user"].as_str().filter(|s| !s.is_empty()) {
        anyhow::ensure!(plain_token(user, 128, b"._-"), "invalid SSH user");
        args.extend(["-l".into(), user.into()]);
    }
    if let Some(identity) = profile["identity"].as_str().filter(|s| !s.is_empty()) {
        anyhow::ensure!(
            identity.len() <= 4096
                && !identity.chars().any(char::is_control)
                && std::path::Path::new(identity).is_absolute(),
            "SSH identity must be an absolute local key-file reference"
        );
        args.extend(["-i".into(), identity.into()]);
    }
    args.extend([
        "--".into(),
        host.into(),
        "velella-runtime remote-stdio".into(),
    ]);
    Ok(args)
}

fn frame(writer: &mut impl Write, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024,
        "remote request frame exceeds 2 MiB"
    );
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn reply(reader: &mut impl Read, id: u64) -> Result<Value> {
    let mut header = [0; 4];
    reader
        .read_exact(&mut header)
        .context("remote frame header was not received")?;
    let length = u32::from_be_bytes(header) as usize;
    anyhow::ensure!(
        (1..=8 * 1024 * 1024).contains(&length),
        "invalid remote response length"
    );
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    let value: Value = serde_json::from_slice(&bytes).context("invalid remote response JSON")?;
    anyhow::ensure!(
        value["id"] == id,
        "remote response ID differs from the request"
    );
    anyhow::ensure!(
        value["ok"] == true,
        "{}",
        value["error"]
            .as_str()
            .unwrap_or("remote request was rejected")
    );
    anyhow::ensure!(
        value["result"].is_object(),
        "remote response has no result object"
    );
    Ok(value["result"].clone())
}
fn identity(value: &Value) -> Result<()> {
    let root = value["data_root"].as_str().context("remote root missing")?;
    let id = value["server_id"]
        .as_str()
        .context("remote server identity missing")?;
    uuid::Uuid::parse_str(id)?;
    let absolute = match value["os"].as_str() {
        Some("linux") => root.starts_with('/'),
        Some("windows") => {
            root.starts_with("\\\\")
                || (root.len() >= 3
                    && root.as_bytes()[0].is_ascii_alphabetic()
                    && root.as_bytes()[1] == b':'
                    && matches!(root.as_bytes()[2], b'\\' | b'/'))
        }
        _ => false,
    };
    let capabilities = value["capabilities"]
        .as_array()
        .context("remote capabilities missing")?;
    anyhow::ensure!(
        absolute
            && root.len() <= 4096
            && !root.chars().any(char::is_control)
            && value["protocol"] == 1
            && value["runtime_version"]
                .as_str()
                .is_some_and(|s| !s.is_empty()
                    && s.len() <= 128
                    && !s.chars().any(char::is_control))
            && value["persistence"] == "prestarted_unverified"
            && capabilities.len() == 2
            && capabilities.contains(&json!("identity-v1"))
            && capabilities.contains(&json!("multiplex-v1")),
        "incompatible remote daemon identity"
    );
    Ok(())
}

fn session(
    mut input: impl Write,
    mut output: impl Read,
    root: Option<String>,
    target: Option<Target>,
    request: Option<Value>,
) -> Result<Value> {
    let mut hello = json!({"id":1,"method":"hello","protocol":1});
    if let Some(root) = root {
        hello["data_root"] = json!(root);
    }
    frame(&mut input, &hello)?;
    let hello = reply(&mut output, 1)?;
    identity(&hello)?;
    let Some(target) = target else {
        return Ok(hello);
    };
    anyhow::ensure!(hello["server_id"]==target.server_id&&hello["data_root"]==target.data_root,"remote server identity or data root changed; inspect and explicitly confirm the target again");
    frame(
        &mut input,
        &json!({"id":2,"method":"connect","protocol":1,"server_id":target.server_id,"data_root":target.data_root,"confirm":true}),
    )?;
    let connected = reply(&mut output, 2)?;
    anyhow::ensure!(
        connected == hello,
        "remote identity changed during confirmation"
    );
    frame(
        &mut input,
        &json!({"id":3,"method":"rpc","protocol":1,"server_id":target.server_id,"data_root":target.data_root,"request":request.context("remote RPC missing")?}),
    )?;
    let mut value = reply(&mut output, 3)?;
    value["remote_identity"] = hello.clone();
    if value["version"].is_null() {
        value["version"] = hello["runtime_version"].clone();
    }
    Ok(value)
}

fn transfer(
    profile: &Value,
    root: Option<String>,
    target: Option<Target>,
    request: Option<Value>,
    timeout: Duration,
) -> Result<Value> {
    let mut command = Command::new("ssh");
    command
        .args(arguments(profile)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().context("cannot start system OpenSSH")?;
    let input = child.stdin.take().context("SSH stdin unavailable")?;
    let output = child.stdout.take().context("SSH stdout unavailable")?;
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(session(input, output, root, target, request));
    });
    let result = receiver
        .recv_timeout(timeout)
        .context("SSH operation deadline exceeded; no request was retried");
    let _ = child.kill();
    let _ = child.wait();
    // On a deadline, inherited external proxy handles must not block this CLI.
    if result.is_ok() {
        let _ = worker.join();
    }
    result?
}

pub(super) fn inspect(profile: &Value, root: Option<&str>, timeout: Duration) -> Result<Value> {
    transfer(profile, root.map(str::to_owned), None, None, timeout)
}

fn allowed(command: &str) -> bool {
    matches!(
        command,
        "workbench.snapshot"
            | "project.add"
            | "project.update"
            | "agent.check"
            | "workbench.create"
            | "workbench.update"
            | "workbench.layout"
            | "workbench.layout.save"
            | "workbench.sessions"
            | "workbench.files"
            | "workbench.search"
            | "workbench.file.read"
            | "workbench.file.create"
            | "workbench.file.write"
            | "workbench.file.rename"
            | "workbench.file.delete"
            | "workbench.git"
            | "workbench.git.stage"
            | "workbench.git.unstage"
            | "workbench.git.commit"
            | "workbench.terminal.open"
            | "workbench.terminal.recover"
            | "workbench.terminal.stop"
            | "workbench.terminal.output"
            | "workbench.terminal.input"
            | "workbench.feedback.send"
            | "task.create"
            | "task.cancel"
            | "task.resume"
            | "task.verify"
            | "task.feedback"
            | "task.feedback.status"
            | "task.diff"
            | "task.events"
            | "terminal.output"
    )
}
pub(super) fn request(target: &Target, request: &Value, timeout: Duration) -> Result<Value> {
    anyhow::ensure!(request["launch"]!="hybrid-codex","hybrid Codex requires a local project");
    let mut request = request.clone();
    if request["cmd"] == "snapshot" {
        request["cmd"] = json!("workbench.snapshot");
    }
    let command = request["cmd"].as_str().unwrap_or("");
    if !allowed(command) {
        return Err(failure(
            "remote_not_supported",
            "This command is local-only; no local fallback or remote action was attempted.",
            json!({"operation":command,"server_id":target.server_id,"data_root":target.data_root}),
        ));
    }
    // Validate frame size before starting SSH. The remote boundary rejects
    // browser/orchestration/account configuration, just like the desktop path.
    let envelope = json!({"id":3,"method":"rpc","protocol":1,"server_id":target.server_id,"data_root":target.data_root,"request":request});
    anyhow::ensure!(
        serde_json::to_vec(&envelope)?.len() <= 2 * 1024 * 1024,
        "remote request exceeds the framed transport limit"
    );
    transfer(
        &target.profile,
        Some(target.data_root.clone()),
        Some(target.clone()),
        Some(request),
        timeout,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_arguments_are_noninteractive_and_cannot_inject_an_option() {
        let args =
            arguments(&json!({"host":"example.invalid","user":"fixture","port":22})).unwrap();
        for option in [
            "StrictHostKeyChecking=yes",
            "BatchMode=yes",
            "ForwardAgent=no",
            "ClearAllForwardings=yes",
            "PermitLocalCommand=no",
        ] {
            assert!(args.iter().any(|s| s == option));
        }
        assert_eq!(
            &args[args.len() - 3..],
            &["--", "example.invalid", "velella-runtime remote-stdio"]
        );
        for host in ["-oProxyCommand=evil", "host;evil", "host evil", "host\n"] {
            assert!(arguments(&json!({"host":host})).is_err());
        }
    }
    #[test]
    fn frame_rejects_wrong_ids_oversize_headers_and_changed_identity_before_rpc() {
        let mut output = Vec::new();
        let hello = json!({"protocol":1,"server_id":"019a12a0-0000-7000-8000-000000000001","data_root":"/fixture","os":"linux","runtime_version":"0.1.0","persistence":"prestarted_unverified","capabilities":["identity-v1","multiplex-v1"]});
        frame(&mut output, &json!({"id":1,"ok":true,"result":hello})).unwrap();
        let target = Target {
            profile: json!({}),
            server_id: "019a12a0-0000-7000-8000-000000000002".into(),
            data_root: "/fixture".into(),
        };
        let mut sent = Vec::new();
        assert!(session(
            &mut sent,
            std::io::Cursor::new(output),
            Some("/fixture".into()),
            Some(target),
            Some(json!({"cmd":"workbench.snapshot"}))
        )
        .is_err());
        let length = u32::from_be_bytes(sent[..4].try_into().unwrap()) as usize;
        assert_eq!(
            sent.len(),
            length + 4,
            "only hello was sent before identity rejection"
        );
        let mut huge = std::io::Cursor::new((9u32 * 1024 * 1024).to_be_bytes());
        assert!(reply(&mut huge, 1).is_err());
        let mut mismatched = Vec::new();
        frame(&mut mismatched, &json!({"id":4,"ok":true,"result":{}})).unwrap();
        assert!(reply(&mut std::io::Cursor::new(mismatched), 1).is_err());
    }
    #[test]
    fn remote_scope_does_not_inherit_local_management_authority() {
        for command in [
            "automation.save",
            "orchestration.create",
            "ssh.save",
            "runtime.identity",
            "browser.click",
            "service.stop",
        ] {
            assert!(!allowed(command));
        }
        assert!(allowed("workbench.terminal.input"));
        assert!(allowed("task.feedback"));
    }
}
