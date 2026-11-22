use crate::client;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Spawn a program in a daemon-owned PTY.
    /// Usage: velella session spawn -- <program> [args...]
    Spawn {
        /// Working directory for the command
        #[arg(long)]
        cwd: Option<String>,
        /// JSON execution policy; omitted means native (legacy behavior)
        #[arg(long)]
        sandbox_policy: Option<std::path::PathBuf>,
        /// Adapter used to classify this session's state (e.g. claude-code)
        #[arg(long)]
        adapter: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Probe the local isolation backend (does not change host configuration)
    SandboxCapabilities,
    /// List live sessions and their normalized states
    List,
    /// Follow a session's output live (Ctrl-C detaches, session keeps running)
    Attach { id: String },
    /// Send text (and Enter) to a session's stdin
    Send {
        id: String,
        text: String,
        #[arg(long)]
        no_enter: bool,
    },
    /// Show one session's state
    State { id: String },
    /// Orderly-stop a session
    Kill { id: String },
}

pub fn run(command: SessionCommand) -> anyhow::Result<()> {
    match command {
        SessionCommand::Spawn {
            cwd,
            adapter,
            sandbox_policy,
            command,
        } => {
            let (program, args) = command.split_first().ok_or_else(|| {
                anyhow::anyhow!("no command given — usage: velella session spawn -- <program> [args...]")
            })?;
            let sandbox: crate::sandbox::Policy = match sandbox_policy {
                Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
                None => Default::default(),
            };
            let resp = client::call(serde_json::json!({
                "cmd": "session.spawn",
                "program": program,
                "args": args,
                "cwd": cwd,
                "adapter": adapter,
                "sandbox": sandbox,
            }))?;
            println!(
                "spawned session {} ({})",
                resp["session_id"].as_str().unwrap_or("?"),
                resp["state"].as_str().unwrap_or("?")
            );
            println!(
                "attach with: velella session attach {}",
                resp["session_id"].as_str().unwrap_or("?")
            );
            Ok(())
        }
        SessionCommand::SandboxCapabilities => {
            println!("{}", serde_json::to_string_pretty(&crate::sandbox::capabilities())?);
            Ok(())
        }
        SessionCommand::List => {
            let resp = client::call(serde_json::json!({ "cmd": "session.list" }))?;
            let sessions = resp["sessions"].as_array().cloned().unwrap_or_default();
            if sessions.is_empty() {
                println!("no sessions — spawn one with `velella session spawn -- <cmd>`");
                return Ok(());
            }
            println!(
                "{:<22} {:<19} {:<10} {:<8} PROGRAM",
                "ID", "STATE", "IDLE_S", "EXIT"
            );
            for s in sessions {
                println!(
                    "{:<22} {:<19} {:<10} {:<8} {}",
                    s["id"].as_str().unwrap_or("?"),
                    s["state"].as_str().unwrap_or("?"),
                    s["idle_secs"].as_u64().unwrap_or(0),
                    s["exit"]
                        .as_i64()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "-".into()),
                    s["program"].as_str().unwrap_or("?"),
                );
            }
            Ok(())
        }
        SessionCommand::Attach { id } => client::attach(&id),
        SessionCommand::Send {
            id,
            text,
            no_enter,
        } => {
            client::call(serde_json::json!({
                "cmd": "session.send", "id": id, "text": text, "enter": !no_enter,
            }))?;
            println!("sent to {id}");
            Ok(())
        }
        SessionCommand::State { id } => {
            let resp = client::call(serde_json::json!({ "cmd": "session.output", "id": id }))?;
            println!("state:     {}", resp["state"].as_str().unwrap_or("?"));
            println!("exit:      {}", resp["exit"].as_i64().map(|e| e.to_string()).unwrap_or_else(|| "-".into()));
            println!("idle_secs: {}", resp["idle_secs"].as_u64().unwrap_or(0));
            println!("bytes:     {}", resp["end"].as_u64().unwrap_or(0));
            Ok(())
        }
        SessionCommand::Kill { id } => {
            client::call(serde_json::json!({ "cmd": "session.kill", "id": id }))?;
            println!("kill requested for {id}");
            Ok(())
        }
    }
}
