pub mod sandbox;
pub mod restricted;
mod adapter;
mod board;
mod client;
mod cmd_session;
mod compat;
mod httpapi;
mod notify;
mod process;
mod runner;
mod serve;
mod sessions;
mod power_continuity;
mod task;
mod worktree;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "velella",
    version,
    about = "A colony of agents, sailing while you sleep."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold .velella/ (config.toml, tasks/, state.db) in this directory
    Init,
    /// Start the headless runtime: NDJSON API over TCP
    Serve {
        #[arg(long)]
        listen: Option<String>,
        #[arg(long, value_name = "PATH")]
        db: Option<std::path::PathBuf>,
    },
    /// Manage background agent tasks
    Task {
        #[command(subcommand)]
        command: task::TaskCommand,
    },
    /// Drive daemon-owned PTY sessions (agents keep running after you leave)
    Session {
        #[command(subcommand)]
        command: cmd_session::SessionCommand,
    },
    /// Collaborate through the desktop runtime's native IPC
    Collab(velella::collaboration::cli::Args),
    /// Manage the desktop runtime (projects, workspaces, tasks and terminals)
    Desktop(velella::desktop_cli::Args),
    /// Keyboard cockpit over the daemon (tasks, sessions, live output)
    Board,
    /// Compatibility probes: fixtures replay + live agent CLI checks
    Compat,
    /// Print version
    Version,
}

fn main() -> anyhow::Result<()> {
    // SessionManager can run in either binary; the restricted launcher is a
    // fresh process so restrictions never affect the daemon's other threads.
    match std::env::args().nth(1).as_deref() {
        Some("restricted-probe") => return restricted::run_probe(),
        Some("restricted-exec") => return restricted::run_encoded(
            &std::env::args().nth(2).ok_or_else(|| anyhow::anyhow!("restricted-exec requires a launch specification"))?),
        Some("restricted-tool") => return restricted::run_tool(
            &std::env::args().nth(2).ok_or_else(|| anyhow::anyhow!("restricted-tool requires a tool specification"))?),
        _ => {}
    }
    match Cli::parse().command {
        Command::Init => init(),
        Command::Serve { listen, db } => serve::run(listen, db),
        Command::Task { command } => task::run(command),
        Command::Session { command } => cmd_session::run(command),
        Command::Collab(args) => velella::collaboration::cli::run(args),
        Command::Desktop(args) => std::process::exit(velella::desktop_cli::run(args)),
        Command::Board => board::run(),
        Command::Compat => {
            let code = compat::run_live()?;
            std::process::exit(code);
        }
        Command::Version => {
            println!("velella {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn init() -> anyhow::Result<()> {
    let dir = std::path::Path::new(".velella");
    std::fs::create_dir_all(dir.join("tasks"))?;
    crate::worktree::ensure_runtime_ignored(std::path::Path::new("."));

    let cfg = dir.join("config.toml");
    if !cfg.exists() {
        std::fs::write(&cfg, velella_core::config::Config::default_doc())?;
    }

    let sample = dir.join("tasks").join("example.yaml");
    if !sample.exists() {
        std::fs::write(&sample, task::SAMPLE_TASK)?;
    }

    let db = dir.join("state.db");
    let _store = velella_core::store::Store::open(&db)?;
    println!("initialized {}", dir.display());
    Ok(())
}
