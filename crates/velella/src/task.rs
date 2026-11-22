use clap::Subcommand;
use std::path::Path;
use velella_core::store::Store;
use velella_core::task::TaskSpec;

/// Reference task covering every format feature; also written by `velella init`.
pub const SAMPLE_TASK: &str = "\
# velella task spec — committed to git, reviewable, replayable in CI.
name: upgrade-vite-6
agent: claude-code
base: main
worktree: auto
env: .velella/env/setup.sh
instruction: |
  Upgrade vite to v6 across the repo. Update all plugins, fix any
  breaking changes, keep the public API stable.
policy:
  permissions: ask
  budget:
    tokens: 5000000
    wall_clock: 2h
done:
  - run: pnpm test --run
  - run: pnpm build
  - diff_max_files: 30
verify:
  retries: 2
notify: [desktop, slack]
on_done: pr
";

#[derive(Subcommand)]
pub enum TaskCommand {
    /// List tasks and their normalized states
    List,
    /// Show one task's spec and state
    Show { name: String },
    /// Parse a task spec and queue it for the runner
    Run { path: std::path::PathBuf },
    /// Re-run verification for an escalated task (the human gate's exit)
    Verify { name: String },
    /// Commit, push and open a PR for an in_review task
    Pr { name: String },
    /// Remove worktrees of finished tasks (merged tasks also lose their
    /// branch; failed tasks keep it). --all includes in_review.
    Clean {
        #[arg(long)]
        all: bool,
    },
    /// Show a task's event log (the audit trail)
    Events { name: String },
}

pub fn run(command: TaskCommand) -> anyhow::Result<()> {
    match command {
        TaskCommand::List => list(),
        TaskCommand::Show { name } => show(&name),
        TaskCommand::Run { path } => queue(&path),
        TaskCommand::Verify { name } => {
            let resp =
                crate::client::call(serde_json::json!({ "cmd": "task.verify", "name": name }))?;
            println!("{}", resp["message"].as_str().unwrap_or("done"));
            Ok(())
        }
        TaskCommand::Pr { name } => {
            let resp = crate::client::call(serde_json::json!({ "cmd": "task.pr", "name": name }))?;
            println!("{}", resp["message"].as_str().unwrap_or("done"));
            Ok(())
        }
        TaskCommand::Clean { all } => {
            let resp = crate::client::call(serde_json::json!({ "cmd": "task.clean", "all": all }))?;
            for line in resp["report"].as_array().cloned().unwrap_or_default() {
                println!("{}", line.as_str().unwrap_or(""));
            }
            Ok(())
        }
        TaskCommand::Events { name } => {
            let resp = crate::client::call(serde_json::json!({ "cmd": "task.events", "name": name }))?;
            for e in resp["events"].as_array().cloned().unwrap_or_default() {
                println!(
                    "#{:<4} {:<24} {}",
                    e["seq"].as_i64().unwrap_or(0),
                    e["kind"].as_str().unwrap_or("?").trim_matches('"'),
                    e["payload"]
                );
            }
            Ok(())
        }
    }
}

fn open_store() -> anyhow::Result<Store> {
    let db = Path::new(".velella/state.db");
    if !db.exists() {
        anyhow::bail!("no .velella/state.db here — run `velella init` first");
    }
    Ok(Store::open(db)?)
}

fn list() -> anyhow::Result<()> {
    let tasks = open_store()?.list_tasks()?;
    if tasks.is_empty() {
        println!("no tasks yet — queue one with `velella task run <file.yaml>`");
        return Ok(());
    }
    println!("{:<28} {:<12} {:<14} ID", "NAME", "STATE", "AGENT");
    for t in tasks {
        println!("{:<28} {:<12} {:<14} {}", t.name, t.state, t.spec.agent, t.id);
    }
    Ok(())
}

fn show(name: &str) -> anyhow::Result<()> {
    let store = open_store()?;
    let Some(t) = store.get_task(name)? else {
        anyhow::bail!("task `{name}` not found");
    };
    println!("id:      {}", t.id);
    println!("state:   {}", t.state);
    println!("agent:   {}", t.spec.agent);
    println!("created: {}", t.created_at);
    println!("updated: {}", t.updated_at);
    println!("--- spec ---");
    print!("{}", t.spec.to_yaml()?);
    Ok(())
}

fn queue(path: &Path) -> anyhow::Result<()> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let spec = TaskSpec::from_yaml_str(&yaml)?;
    let id = open_store()?.queue_task(&spec)?;
    println!("queued `{}` as {}", spec.name, id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_task_parses() {
        let spec = TaskSpec::from_yaml_str(SAMPLE_TASK).unwrap();
        assert_eq!(spec.name, "upgrade-vite-6");
        assert_eq!(spec.done.len(), 3);
    }
}
