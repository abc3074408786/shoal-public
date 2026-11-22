//! Both binaries use this desktop IPC client; the legacy TCP service is unrelated.
use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Desktop data profile (defaults to VELELLA_DATA_DIR)
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    /// Credentials: explicit path, VELELLA_COLLAB_CREDENTIALS, then this session's last create/join
    #[arg(long, global = true)]
    pub credentials: Option<PathBuf>,
    /// Stable ID for every mutation. Reuse the SAME ID and content after an unknown response.
    #[arg(long, global = true)]
    pub request_id: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Clone, Subcommand)]
pub enum Command {
    /// Session-local MCP transport (identity is discovered from the session binding).
    Mcp,
    /// Propose a revision; only the user may approve it.
    RevisionPropose {
        #[arg(long)] file: PathBuf,
        #[arg(long)] revision: u64,
        #[arg(long)] reason: String,
    },
    /// Submit a draft plan for human confirmation (managed lead only).
    PlanSubmit {
        #[arg(long)] file: PathBuf,
        #[arg(long)] revision: u64,
    },
    /// Diagnose a first failure and propose the one evidence-based retry.
    RetryPropose {
        #[arg(long)] work: String,
        /// Stable human-assigned root cause key, shared across equivalent failures.
        #[arg(long)] cause_key: String,
        #[arg(long)] new_evidence: String,
    },
    /// Open a native coordinator terminal with local collaboration discovery.
    Start {
        #[arg(long)]
        workspace: String,
        #[arg(long, value_parser=["codex","claude-code","gemini","opencode","kimi"])]
        agent: String,
    },
    /// Read the actual source fingerprint for an assigned work item.
    Candidate {
        #[arg(long)]
        work: String,
    },
    Create {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Coordinator-controlled native terminal launch with invocation-local hooks and Skill.
    Launch {
        #[arg(long)]
        workspace: String,
        #[arg(long, value_parser=["codex","claude-code","gemini","opencode","kimi"])]
        agent: String,
    },
    /// Persist and, only when the recipient is safely idle, submit one native notification.
    Notify {
        #[arg(long)]
        to: String,
        #[arg(long)]
        text: String,
    },
    Invite {
        #[arg(long)]
        session: String,
        #[arg(long)]
        name: String,
    },
    Join {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        run: String,
        #[arg(long)]
        member: String,
        #[arg(long)]
        invitation: String,
        #[arg(long)]
        confirm: bool,
    },
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        text: String,
    },
    Receive {
        #[arg(long, default_value_t = 25000)]
        wait_ms: u64,
    },
    Ack {
        #[arg(long)]
        seq: i64,
    },
    Assign {
        #[arg(long)]
        assignee: String,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        instruction: String,
        #[arg(long)]
        write: bool,
        #[arg(long)]
        work: Option<String>,
        /// Stable logical work identity. Required for a new assignment.
        #[arg(long)]
        work_key: Option<String>,
        #[arg(long)]
        new_evidence: Option<String>,
    },
    Started {
        #[arg(long)]
        attempt: String,
    },
    Report {
        #[arg(long)]
        attempt: String,
        #[arg(long,value_parser=["completed","failed"])]
        outcome: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        candidate: Option<PathBuf>,
        /// Structured v2 checks, artifact fingerprints and remaining gaps.
        #[arg(long)]
        evidence: Option<PathBuf>,
        /// Required for a failed v2 report; stable root-cause key.
        #[arg(long)]
        cause_key: Option<String>,
    },
    ReviewRequest {
        #[arg(long)]
        work: String,
        #[arg(long)]
        reviewer: String,
    },
    ReviewReport {
        #[arg(long)]
        review: String,
        #[arg(long,value_parser=["passed","failed"])]
        verdict: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        candidate: Option<PathBuf>,
        #[arg(long)]
        evidence: Option<PathBuf>,
        /// Required for a failed v2 review; stable root-cause key.
        #[arg(long)]
        cause_key: Option<String>,
    },
    Accept {
        #[arg(long)]
        work: String,
        #[arg(long)]
        summary: String,
    },
    Claim {
        #[arg(long)]
        writer: String,
    },
    Release,
    Cancel {
        #[arg(long)]
        work: String,
        #[arg(long)]
        reason: String,
    },
    Leave,
    Status,
    Reconcile,
    End {
        #[arg(long,value_parser=["accepted","cancelled"])]
        outcome: String,
        #[arg(long)]
        summary: String,
    },
    /// Structured request from a UTF-8 JSON file; same identity and ID rules apply.
    Request {
        file: PathBuf,
    },
}
fn read(path: &std::path::Path) -> Result<Value> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 1024 * 1024,
        "request file exceeds 1 MiB"
    );
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}
fn candidate(path: Option<PathBuf>) -> Result<Value> {
    path.as_deref()
        .map(read)
        .transpose()
        .map(|v| v.map(|value| value.get("candidate").cloned().unwrap_or(value)).unwrap_or(Value::Null))
}
fn session(value: Option<String>) -> Result<String> {
    value
        .or_else(|| std::env::var("VELELLA_SESSION_ID").ok())
        .context("--session required for an existing terminal without VELELLA_SESSION_ID")
}

fn session_binding(data: &std::path::Path, session: &str) -> Result<PathBuf> {
    let session = uuid::Uuid::parse_str(session).context("invalid Velella session ID")?;
    Ok(data.join("collaboration-sessions").join(format!("{session}.json")))
}

fn discover_credentials(data: &std::path::Path) -> Result<PathBuf> {
    let session = std::env::var("VELELLA_SESSION_ID")
        .context("--credentials required outside a Velella collaboration session")?;
    let binding = read(&session_binding(data, &session)?)
        .context("session has no readable collaboration binding; explicitly create or join first")?;
    anyhow::ensure!(binding["session_id"].as_str() == Some(session.as_str()), "collaboration session binding mismatch");
    let member = uuid::Uuid::parse_str(super::field(&binding, "member_id")?)
        .context("invalid collaboration member binding")?;
    Ok(data.join("collaboration-clients").join(format!("{member}.json")))
}

pub fn run(args: Args) -> Result<()> {
    if matches!(args.command, Command::Mcp) {
        let data = args.data_dir.unwrap_or_else(crate::ipc::data_dir);
        return super::mcp::serve(&data, &session(None)?);
    }
    let result = execute(args, std::time::Duration::from_secs(45))?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Shared IPC client for the public CLI and the compatible runtime entrypoint.
/// Returns a receipt with credential tokens removed; never prints or retries.
pub(crate) fn execute(args: Args, timeout: std::time::Duration) -> Result<Value> {
    let data = args.data_dir.unwrap_or_else(crate::ipc::data_dir);
    let mut req = match args.command {
        Command::Mcp=>anyhow::bail!("MCP uses a separate stdio stream; use velella-runtime collab mcp. For direct CLI operations use velella-cli collab --help"),
        Command::RevisionPropose{file,revision,reason}=>json!({"cmd":"collab.revision.propose","plan":read(&file)?,"revision":revision,"reason":reason}),
        Command::PlanSubmit {file,revision}=>json!({"cmd":"collab.plan.submit","plan":read(&file)?,"revision":revision}),
        Command::RetryPropose {work,cause_key,new_evidence}=>json!({"cmd":"collab.retry.propose","work_id":work,"cause_key":cause_key,"new_evidence":new_evidence}),
        Command::Start { workspace,agent }=>json!({"cmd":"collab.start","workspace_id":workspace,"launch":agent}),
        Command::Candidate { work }=>json!({"cmd":"collab.candidate","work_id":work}),
        Command::Create {
            session: s,
            goal,
            name,
        } => json!({"cmd":"collab.create","session_id":session(s)?,"goal":goal,"name":name}),
        Command::Launch { workspace, agent } => json!({"cmd":"collab.launch","workspace_id":workspace,"launch":agent}),
        Command::Notify { to, text } => json!({"cmd":"collab.notify","to":to,"text":text}),
        Command::Invite { session, name } => {
            json!({"cmd":"collab.invite","session_id":session,"name":name})
        }
        Command::Join {
            session: s,
            run,
            member,
            invitation,
            confirm,
        } => {
            json!({"cmd":"collab.join","session_id":session(s)?,"run_id":run,"member_id":member,"invitation":invitation,"confirmed":confirm})
        }
        Command::Send { to, text } => json!({"cmd":"collab.send","to":to,"text":text}),
        Command::Receive { wait_ms } => {
            json!({"cmd":"collab.receive","wait_ms":wait_ms.min(25000)})
        }
        Command::Ack { seq } => json!({"cmd":"collab.ack","seq":seq}),
        Command::Assign {
            assignee,
            workspace,
            title,
            instruction,
            write,
            work,
            work_key,
            new_evidence,
        } => {
            let mut path = dunce::canonicalize(workspace)?
                .to_string_lossy()
                .into_owned();
            if cfg!(windows) {
                path = path.replace('/', "\\").to_lowercase();
            }
            json!({"cmd":"collab.assign","assignee":assignee,"workspace_path":path,"title":title,"instruction":instruction,"write":write,"work_id":work,"work_key":work_key,"new_evidence":new_evidence})
        }
        Command::Started { attempt } => json!({"cmd":"collab.started","attempt_id":attempt}),
        Command::Report {
            attempt,
            outcome,
            summary,
            candidate: c,
            evidence,
            cause_key,
        } => {
            json!({"cmd":"collab.report","attempt_id":attempt,"outcome":outcome,"summary":summary,"candidate":candidate(c)?,"evidence":evidence.as_deref().map(read).transpose()?,"cause_key":cause_key})
        }
        Command::ReviewRequest { work, reviewer } => {
            json!({"cmd":"collab.review.request","work_id":work,"reviewer":reviewer})
        }
        Command::ReviewReport {
            review,
            verdict,
            summary,
            candidate: c,
            evidence,
            cause_key,
        } => {
            json!({"cmd":"collab.review.report","review_id":review,"verdict":verdict,"summary":summary,"candidate":candidate(c)?,"evidence":evidence.as_deref().map(read).transpose()?,"cause_key":cause_key})
        }
        Command::Accept { work, summary } => {
            json!({"cmd":"collab.accept","work_id":work,"summary":summary})
        }
        Command::Claim { writer } => json!({"cmd":"collab.claim","writer":writer}),
        Command::Release => json!({"cmd":"collab.release"}),
        Command::Cancel { work, reason } => {
            json!({"cmd":"collab.cancel","work_id":work,"reason":reason})
        }
        Command::Leave => json!({"cmd":"collab.leave"}),
        Command::Status => json!({"cmd":"collab.status"}),
        Command::Reconcile => json!({"cmd":"collab.reconcile"}),
        Command::End { outcome, summary } => {
            json!({"cmd":"collab.end","outcome":outcome,"summary":summary})
        }
        Command::Request { file } => read(&file)?,
    };
    let cmd = req["cmd"].as_str().context("cmd required")?.to_owned();
    anyhow::ensure!(
        cmd.starts_with("collab."),
        "collab request must use a collab command"
    );
    if !matches!(cmd.as_str(), "collab.start" | "collab.create" | "collab.join") {
        let explicit = args
            .credentials
            .or_else(|| std::env::var_os("VELELLA_COLLAB_CREDENTIALS").map(PathBuf::from));
        let path = match explicit { Some(path) => path, None => discover_credentials(&data)? };
        let credentials = read(&path)?;
        for key in ["run_id", "member_id", "token"] {
            req[key] = credentials[key].clone();
        }
        // The desktop adapter binds every ordinary native invocation to the
        // session that supplied its credentials.  This lets a handoff revoke
        // the old terminal even though the member credential itself survives.
        if let Ok(session)=std::env::var("VELELLA_SESSION_ID") {req["native_session_id"]=json!(session);}
    }
    if let Some(request_id) = args.request_id {
        req["request_id"] = json!(request_id);
    }
    if !matches!(
        cmd.as_str(),
        "collab.status" | "collab.receive" | "collab.reconcile" | "collab.candidate"
    ) {
        super::field(&req, "request_id")
            .context("mutations require --request-id; retain it for response reconciliation")?;
    }
    let mut result=crate::ipc::request_with_timeout(&crate::ipc::endpoint(&data,"daemon"),&req,timeout)
        .context("desktop response unknown; do not create a new request ID or resend terminal input; retry this exact command with the same request ID")?;
    anyhow::ensure!(
        result["ok"] == true,
        "{}",
        result["error"]
            .as_str()
            .unwrap_or("collaboration request failed")
    );
    if let Some(credentials) = result.get("credentials").cloned() {
        let member = super::field(&credentials, "member_id")?;
        uuid::Uuid::parse_str(member)?;
        let folder = data.join("collaboration-clients");
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{member}.json"));
        crate::ipc::atomic_json(&path, &credentials)?;
        // Publish only after a successful, explicit create/join and after the
        // credential file is durable. No tokens are stored in the binding.
        let session = super::field(&result["member"]["identity"], "session_id")?;
        let binding = session_binding(&data, session)?;
        std::fs::create_dir_all(binding.parent().context("session binding directory")?)?;
        crate::ipc::atomic_json(&binding, &json!({"session_id":session,"member_id":member}))
            .context("membership succeeded but discovery save failed; replay the same request ID")?;
        result.as_object_mut().unwrap().remove("credentials");
        result["credentials_file"] = json!(path);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use interprocess::local_socket::prelude::*;
    use std::process::{Command as Process, Output};

    // Run the public CLI parser/client in a child process so session environment
    // changes cannot race with other tests or touch the user's live profile.
    #[test]
    #[ignore = "subprocess entry point, invoked by discovery tests"]
    fn cli_child() {
        let Ok(input) = std::env::var("VELELLA_TEST_CLI_ARGS") else { return; };
        #[derive(Parser)]
        struct Client { #[command(flatten)] args: Args }
        let argv: Vec<String> = serde_json::from_str(&input).unwrap();
        run(Client::parse_from(argv).args).unwrap();
    }

    #[test]
    #[ignore = "subprocess entry point, invoked by public CLI tests"]
    fn public_cli_child() {
        let input = std::env::var("VELELLA_TEST_CLI_ARGS").unwrap();
        let argv: Vec<String> = serde_json::from_str(&input).unwrap();
        std::process::exit(crate::desktop_cli::main_from(argv.into_iter().map(Into::into)));
    }

    struct Fixture {
        root: PathBuf,
        peer: Option<std::thread::JoinHandle<()>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("velella-discovery-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&root).unwrap();
            let hub = crate::collaboration::Hub::open(&root).unwrap();
            let listener = crate::ipc::listen(&crate::ipc::endpoint(&root, "daemon")).unwrap();
            let peer = std::thread::spawn(move || loop {
                let mut stop = false;
                crate::ipc::handle(listener.accept().unwrap(), |wire| {
                    if wire["cmd"] == "fixture.shutdown" { stop = true; return Ok(json!({"ok":true})); }
                    let mut resolved = wire.clone();
                    if matches!(wire["cmd"].as_str(), Some("collab.create" | "collab.invite" | "collab.join")) {
                        // Fake only the desktop/host boundary; use the actual
                        // durable collaboration service for membership and auth.
                        resolved["identity"] = json!({"session_id":wire["session_id"],"host_id":wire["session_id"],"endpoint":"fixture","workspace_id":"w","workspace_path":"fixture"});
                    }
                    hub.execute_resolved(&wire, &resolved)
                });
                if stop { break; }
            });
            Self { root, peer: Some(peer) }
        }
        fn call(&self, session: &str, args: &[&str], credentials: Option<&str>) -> Output {
            let mut argv = vec!["collab"];
            argv.extend_from_slice(args);
            let mut child = Process::new(std::env::current_exe().unwrap());
            child.args(["--exact", "collaboration::cli::tests::cli_child", "--ignored", "--nocapture"])
                .env("VELELLA_TEST_CLI_ARGS", serde_json::to_string(&argv).unwrap())
                .env("VELELLA_DATA_DIR", &self.root)
                .env("VELELLA_SESSION_ID", session)
                .env_remove("VELELLA_COLLAB_CREDENTIALS");
            if let Some(path) = credentials { child.env("VELELLA_COLLAB_CREDENTIALS", path); }
            child.output().unwrap()
        }
        fn public_call(&self, session: &str, args: &[&str]) -> Value {
            let argv: Vec<_> = ["velella-cli", "--json", "--timeout-ms", "3000"].into_iter()
                .chain(args.iter().copied()).collect();
            let output = Process::new(std::env::current_exe().unwrap())
                .args(["--exact", "collaboration::cli::tests::public_cli_child", "--ignored", "--nocapture"])
                .env("VELELLA_TEST_CLI_ARGS", serde_json::to_string(&argv).unwrap())
                .env("VELELLA_DATA_DIR", &self.root)
                .env("VELELLA_SESSION_ID", session)
                .env_remove("VELELLA_COLLAB_CREDENTIALS")
                .output().unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            let value: Value = serde_json::from_str(&stdout[stdout.find('{').unwrap()..=stdout.rfind('}').unwrap()]).unwrap();
            assert_eq!(output.status.success(), value["ok"] == true, "{value}");
            value
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = crate::ipc::request(&crate::ipc::endpoint(&self.root, "daemon"), &json!({"cmd":"fixture.shutdown"}));
            if let Some(peer) = self.peer.take() { peer.join().unwrap(); }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn receipt(output: Output) -> Value {
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8(output.stdout).unwrap();
        serde_json::from_str(&stdout[stdout.find('{').unwrap()..=stdout.rfind('}').unwrap()]).unwrap()
    }

    #[test]
    fn public_cli_membership_mailbox_replay_and_local_only_guards() {
        let fixture = Fixture::new();
        let lead = uuid::Uuid::now_v7().to_string();
        let member = uuid::Uuid::now_v7().to_string();
        let request = uuid::Uuid::now_v7().to_string();
        let create = ["collab", "create", "--goal", "public CLI fixture", "--request-id", &request];
        let created = fixture.public_call(&lead, &create);
        assert_eq!(created["ok"], true, "{created}");
        assert!(created["result"].get("credentials").is_none());
        let path = created["result"]["credentials_file"].as_str().unwrap();
        let secret = read(std::path::Path::new(path)).unwrap()["token"].as_str().unwrap().to_owned();
        assert!(!created.to_string().contains(&secret));
        let replay = fixture.public_call(&lead, &create);
        assert_eq!(replay["result"]["run"]["id"], created["result"]["run"]["id"]);
        assert_eq!(fixture.public_call(&lead, &["collab", "status"])["result"]["run"]["goal"], "public CLI fixture");
        let invited = fixture.public_call(&lead, &["collab", "invite", "--session", &member, "--name", "member", "--request-id", &uuid::Uuid::now_v7().to_string()]);
        assert_eq!(invited["ok"], true, "{invited}");
        let join_id = uuid::Uuid::now_v7().to_string();
        let mut join = vec!["collab", "join", "--run", created["result"]["run"]["id"].as_str().unwrap(), "--member", invited["result"]["member"]["id"].as_str().unwrap(), "--invitation", invited["result"]["invitation"].as_str().unwrap(), "--request-id", &join_id];
        assert_eq!(fixture.public_call(&member, &join)["ok"], false);
        assert_eq!(fixture.public_call(&member, &["collab", "status"])["ok"], false);
        join.push("--confirm");
        assert_eq!(fixture.public_call(&member, &join)["ok"], true);
        let send_id = uuid::Uuid::now_v7().to_string();
        let to = invited["result"]["member"]["id"].as_str().unwrap();
        let send = ["collaboration", "send", "--to", to, "--text", "one public message", "--request-id", &send_id];
        assert_eq!(fixture.public_call(&lead, &send)["ok"], true);
        assert_eq!(fixture.public_call(&lead, &send)["ok"], true);
        let received = fixture.public_call(&member, &["collab", "receive", "--wait-ms", "0"]);
        let messages = received["result"]["messages"].as_array().unwrap();
        assert_eq!(messages.iter().filter(|m| m["body"].to_string().contains("one public message")).count(), 1);
        let seq = messages.last().unwrap()["seq"].to_string();
        assert_eq!(fixture.public_call(&member, &["collab", "ack", "--seq", &seq, "--request-id", &uuid::Uuid::now_v7().to_string()])["ok"], true);
        assert!(fixture.public_call(&member, &["collab", "receive", "--wait-ms", "0"])["result"]["messages"].as_array().unwrap().is_empty());
        assert_eq!(fixture.public_call(&uuid::Uuid::now_v7().to_string(), &["collab", "status"])["ok"], false);
        assert_eq!(fixture.public_call(&member, &["collab", "status", "--data-dir", fixture.root.join("other-profile").to_str().unwrap()])["ok"], false);
        let denied = fixture.public_call(&lead, &["--ssh-profile", "missing-profile", "--server-id", &uuid::Uuid::now_v7().to_string(), "--remote-data-dir", "/missing", "collab", "status", "--credentials", "missing.json"]);
        assert_eq!(denied["ok"], false);
        assert!(denied["error"]["message"].as_str().unwrap().contains("local-session-only"));
        assert_eq!(fixture.public_call(&lead, &["collab", "create", "--goal", "missing request ID"])["ok"], false);
        let bad = fixture.public_call(&lead, &["collab", "send"]);
        assert_eq!(bad["error"]["code"], "invalid_arguments");
    }

    #[test]
    fn created_session_discovers_credentials_without_arguments() {
        let fixture = Fixture::new();
        let session = uuid::Uuid::now_v7().to_string();
        let request = uuid::Uuid::now_v7().to_string();
        let created = receipt(fixture.call(&session, &["--request-id", &request, "create", "--goal", "discovery goal"], None));
        assert!(created.get("credentials").is_none());
        let status = receipt(fixture.call(&session, &["status"], None));
        assert_eq!(status["run"]["goal"], "discovery goal");
        assert!(!fixture.call(&uuid::Uuid::now_v7().to_string(), &["status"], None).status.success());
    }

    #[test]
    fn joined_session_discovers_only_after_confirmation_and_keeps_explicit_precedence() {
        let fixture = Fixture::new();
        let lead = uuid::Uuid::now_v7().to_string();
        let member = uuid::Uuid::now_v7().to_string();
        let other = uuid::Uuid::now_v7().to_string();
        let created = receipt(fixture.call(&lead, &["--request-id", &uuid::Uuid::now_v7().to_string(), "create", "--goal", "first run"], None));
        let invited = receipt(fixture.call(&lead, &["--request-id", &uuid::Uuid::now_v7().to_string(), "invite", "--session", &member, "--name", "member"], None));
        assert!(!fixture.call(&member, &["status"], None).status.success());
        let request = uuid::Uuid::now_v7().to_string();
        let mut join = vec!["--request-id", &request, "join", "--run", created["run"]["id"].as_str().unwrap(), "--member", invited["member"]["id"].as_str().unwrap(), "--invitation", invited["invitation"].as_str().unwrap()];
        assert!(!fixture.call(&member, &join, None).status.success());
        assert!(!fixture.call(&member, &["status"], None).status.success());
        join.push("--confirm");
        receipt(fixture.call(&member, &join, None));
        assert_eq!(receipt(fixture.call(&member, &["status"], None))["run"]["goal"], "first run");
        assert_eq!(receipt(fixture.call(&member, &["receive", "--wait-ms", "0"], None))["acked"], 0);
        // Repeat a successful join with its original ID: native session needn't restart.
        receipt(fixture.call(&member, &join, None));
        let second = receipt(fixture.call(&other, &["--request-id", &uuid::Uuid::now_v7().to_string(), "create", "--goal", "second run"], None));
        let second_path = second["credentials_file"].as_str().unwrap();
        let first_path = created["credentials_file"].as_str().unwrap();
        assert_eq!(receipt(fixture.call(&member, &["status"], Some(second_path)))["run"]["goal"], "second run");
        assert_eq!(receipt(fixture.call(&member, &["--credentials", first_path, "status"], Some(second_path)))["run"]["goal"], "first run");
        assert!(!fixture.call(&member, &["--credentials", "missing-credentials.json", "status"], None).status.success());
        assert!(!fixture.call("../outside", &["status"], None).status.success());
        // Profile override must not discover the original profile's membership.
        let empty = fixture.root.join("other-profile");
        assert!(!fixture.call(&member, &["--data-dir", empty.to_str().unwrap(), "status"], None).status.success());
    }
}
