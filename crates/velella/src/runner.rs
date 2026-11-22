//! The task runner: queued spec -> worktree -> agent session -> verified
//! work. One task at a time in this slice; the supervision primitives
//! (classifier, events, notify, gates) do the watching.

use crate::adapter;
use crate::notify;
use crate::sessions::{SessionManager, SpawnRequest};
use crate::worktree;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use velella_core::event::EventKind;
use velella_core::state::{SessionState, TaskState};
use velella_core::store::{Store, TaskRecord};
use velella_core::task::{parse_wall_clock, DoneCheck, OnDone};

/// Dependencies shared with the daemon; owned clones, short-lived locks —
/// the runner must never hold the store while an agent thinks.
#[derive(Clone)]
pub struct Deps {
    pub store: Arc<Mutex<Store>>,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub config: velella_core::config::Config,
}

pub fn spawn_thread(deps: Deps) {
    std::thread::spawn(move || {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if let Err(e) = tick(&deps, &root) {
                eprintln!("[velella:runner] {e}");
            }
        }
    });
}

fn tick(deps: &Deps, root: &Path) -> anyhow::Result<()> {
    let tasks = deps.store.lock().unwrap().list_tasks()?;
    let active = tasks
        .iter()
        .filter(|t| matches!(t.state, TaskState::Running | TaskState::Verifying))
        .count();
    if active >= deps.config.max_parallel {
        return Ok(());
    }
    let Some(task) = tasks.iter().find(|t| t.state == TaskState::Queued).cloned() else {
        return Ok(());
    };
    // claim synchronously so the next tick can't double-dispatch, then hand
    // the claimed task to its own thread — tasks sail in parallel
    if deps
        .store
        .lock()
        .unwrap()
        .set_task_state(&task.id, TaskState::Queued, TaskState::Running)
        .is_err()
    {
        return Ok(());
    }
    let deps = deps.clone();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = execute_running(&deps, &root, &task) {
            eprintln!("[velella:runner] task `{}` failed: {e}", task.name);
            {
                // archive from whatever state the task actually reached
                let store = deps.store.lock().unwrap();
                let current = store
                    .get_task(&task.name)
                    .ok()
                    .flatten()
                    .map(|t| t.state)
                    .unwrap_or(TaskState::Running);
                if current.can_transition_to(TaskState::Failed) {
                    let _ = store.set_task_state(&task.id, current, TaskState::Failed);
                }
                let _ = store.append_event(
                    EventKind::TaskFailed,
                    &task.id,
                    serde_json::json!({ "error": e.to_string() }),
                );
            }
            notify::fire(&deps.config, "task", &task.name, "failed");
        }
    });
    Ok(())
}

fn execute_running(deps: &Deps, root: &Path, task: &TaskRecord) -> anyhow::Result<()> {
    let spec = &task.spec;
    anyhow::ensure!(!spec.policy.execution.is_sandboxed(), "sandboxed tasks require the desktop runtime task executor");
    let mut launch_args = velella::native_launch::args(&spec.agent, &spec.launch)?;
    // Options require evidence from this adapter, before any checkout effect.
    // Keep legacy lookup timing and behavior when there are no options.
    let validated_adapter = if spec.launch.is_empty() {
        None
    } else {
        let (adapter, launch) = task_adapter(&spec.agent)?;
        launch_args = velella::native_launch::validated_args_for_program(
            &spec.agent, &spec.launch, &launch.program, false, None,
        )?;
        Some((adapter, launch))
    };
    // isolation first: nothing an agent does can touch the checkout
    let worktree = match worktree::create(root, &task.name, &spec.base) {
        Ok(w) => w,
        Err(e) => return Err(anyhow::anyhow!("worktree: {e}")),
    };
    deps.store
        .lock()
        .unwrap()
        .set_task_binding(&task.id, Some(&worktree.to_string_lossy()), None)?;

    // the adapter owns how its agent is launched
    let (adapter_spec, launch) = match validated_adapter {
        Some(adapter) => adapter,
        None => task_adapter(&spec.agent)?,
    };
    let session_id = {
        let req = SpawnRequest { sandbox: Default::default(),
            program: launch.program,
            args: launch.args.into_iter().chain(launch_args).collect(),
            cwd: Some(worktree.to_string_lossy().into_owned()),
            env: Default::default(),
            adapter: Some(spec.agent.clone()),
        };
        deps.sessions.lock().unwrap().spawn(&req)?
    };
    {
        let store = deps.store.lock().unwrap();
        store.set_task_binding(
            &task.id,
            Some(&worktree.to_string_lossy()),
            Some(&session_id.0),
        )?;
        let _ = store.append_event(
            EventKind::SessionSpawned,
            &task.id,
            serde_json::json!({ "session": session_id.0, "agent": spec.agent }),
        );
    }
    let Some(session) = deps.sessions.lock().unwrap().get(&session_id.0) else {
        anyhow::bail!("session vanished right after spawn");
    };

    // boot: answer known one-time dialogs (folder trust), wait for the
    // agent's real prompt — verified by the permission patterns being gone,
    // not just by the classifier's verdict. Anything unknown waits for a
    // human (notifications fire from the supervisor).
    let boot_deadline = Instant::now() + Duration::from_secs(180);
    let mut answered: std::collections::HashSet<usize> = Default::default();
    let mut agent_gone = false;
    loop {
        let snap = session.snapshot();
        if snap.state.is_terminal() {
            agent_gone = true; // one-shot agents: verify arbitrates
            break;
        }
        let tail = adapter::strip_ansi(&session.recent_text(8192));
        let mut acted = false;
        for (i, ba) in adapter_spec.boot.iter().enumerate() {
            if !answered.contains(&i) && adapter::normalize_contains(&tail, &ba.when) {
                session.send_text(&ba.send, true)?;
                answered.insert(i);
                acted = true;
                eprintln!("[velella:runner] boot answer {i} sent to {}", session.id);
            }
        }
        if acted {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        let dialog_still_up = adapter_spec
            .boot
            .iter()
            .any(|ba| adapter::normalize_contains(&tail, &ba.when));
        let settled = snap.idle_secs >= adapter_spec.boot_settle_secs;
        if snap.state == SessionState::WaitingInput && !dialog_still_up && settled {
            break;
        }
        if Instant::now() > boot_deadline {
            let _ = session.kill();
            return escalate(deps, task, TaskState::Running, "agent did not reach its prompt in 180s".into());
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    // hand over the instruction as the agent's first message. Plain
    // single-line text: claude's TUI runs in win32-input-mode (no bracketed
    // paste), where paste markers get eaten — plain keystrokes are the one
    // path proven end to end. Multi-line instructions are flattened; proper
    // win32-input encoding arrives with the structured-protocol work.
    if !agent_gone {
        if let Some(instruction) = &spec.instruction {
            let one_line = instruction
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !one_line.is_empty() {
                // text and Enter go as separate writes: a long burst ending
                // in \r gets its Enter swallowed (the TUI treats the burst
                // as paste), while a delayed lone \r always submits.
                session.send_text(&one_line, false)?;
                std::thread::sleep(Duration::from_millis(400));
                session.send_text("", true)?;
                // Arm: wait until the agent actually picks the work up
                // (state leaves waiting_input) before watching for the turn
                // to end — otherwise we'd verify a draft nobody read.
                let arm_deadline = Instant::now() + Duration::from_secs(90);
                let mut nudged = false;
                loop {
                    if session.snapshot().state != SessionState::WaitingInput {
                        break;
                    }
                    if !nudged && Instant::now() > arm_deadline {
                        // one insurance Enter before giving this turn up
                        session.send_text("", true)?;
                        nudged = true;
                        std::thread::sleep(Duration::from_secs(10));
                        continue;
                    }
                    if nudged && Instant::now() > arm_deadline + Duration::from_secs(10) {
                        break; // verify arbitrates whether anything happened
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }

    // turn loop: agent works -> settles at its prompt -> verify -> loop
    let budget = spec
        .policy
        .budget
        .wall_clock
        .as_deref()
        .and_then(parse_wall_clock)
        .unwrap_or(Duration::from_secs(3600));
    let deadline = Instant::now() + budget;
    let mut retries_left = i64::from(spec.verify.retries.min(1));

    'outer: loop {
        // wait for one turn to end: the agent settles at its prompt, or quits
        loop {
            let snap = session.snapshot();
            match snap.state {
                SessionState::WaitingInput => break,
                SessionState::Done | SessionState::Failed | SessionState::Stopped | SessionState::Fenced => {
                    agent_gone = true;
                    break;
                }
                // Running / WaitingPermission (human answering) / etc: keep waiting
                _ => {}
            }
            if Instant::now() > deadline {
                let _ = session.kill();
                return escalate(deps, task, TaskState::Running, "wall-clock budget exceeded".into());
            }
            std::thread::sleep(Duration::from_secs(2));
        }

        // Running → Verifying on the first cycle; retry cycles are already
        // in Verifying — the optimistic guard makes the no-op explicit.
        {
            let store = deps.store.lock().unwrap();
            if store
                .get_task(&task.name)?
                .map(|t| t.state)
                .unwrap_or(TaskState::Running)
                == TaskState::Running
            {
                store.set_task_state(&task.id, TaskState::Running, TaskState::Verifying)?;
            }
        }
        let _ = deps.store.lock().unwrap().append_event(
            EventKind::VerifyStarted,
            &task.id,
            serde_json::json!({ "checks": spec.done.len() }),
        );

        match verify_worktree(&worktree, &spec.base, &spec.done) {
            Ok(()) => {
                {
                    let store = deps.store.lock().unwrap();
                    store.set_task_state(&task.id, TaskState::Verifying, TaskState::InReview)?;
                    let _ = store.append_event(
                        EventKind::VerifyPassed,
                        &task.id,
                        serde_json::json!({ "checks": spec.done.len() }),
                    );
                }
                notify::fire(&deps.config, "task", &task.name, "in_review");
                deliver(deps, task, root, &worktree);
                return Ok(());
            }
            Err(report) => {
                {
                    let store = deps.store.lock().unwrap();
                    let _ = store.append_event(
                        EventKind::VerifyFailed,
                        &task.id,
                        serde_json::json!({ "report": report, "agent_gone": agent_gone }),
                    );
                }
                if !agent_gone && retries_left > 0 {
                    retries_left -= 1;
                    eprintln!("[velella:runner] `{}` verify failed, feeding back ({retries_left} retries left)", task.name);
                    let feedback = format!(
                        "The verification of your work failed: {}. Fix the issues, then finish your turn.",
                        report.replace('\n', " ")
                    );
                    session.send_text(&feedback, false)?;
                    std::thread::sleep(Duration::from_millis(400));
                    session.send_text("", true)?;
                    continue 'outer;
                }
                // can't or may not feed back: human gate
                let reason = if agent_gone {
                    "agent exited before verification passed".to_string()
                } else {
                    format!("verification failed after {} retries:\n\n{report}", spec.verify.retries)
                };
                return escalate(deps, task, TaskState::Verifying, reason);
            }
        }
    }
}

fn task_adapter(agent: &str) -> anyhow::Result<(adapter::AdapterSpec, adapter::LaunchSpec)> {
    let Some(spec) = adapter::lookup(agent) else {
        anyhow::bail!("unknown agent adapter `{agent}`");
    };
    let Some(launch) = spec.launch.clone() else {
        anyhow::bail!("adapter `{agent}` has no launch config");
    };
    Ok((spec, launch))
}

/// Delivery: `on_done: pr` commits the worktree, pushes the branch and
/// opens a PR (gh) or yields a compare link; `on_done: local_merge` merges
/// the verified branch into the main checkout. A delivery hiccup never
/// un-verifies the task — it lands in the event log instead.
fn deliver(deps: &Deps, task: &TaskRecord, root: &Path, worktree: &Path) {
    let spec = &task.spec;
    if spec.on_done == OnDone::Review {
        return;
    }
    // both deliveries need the agent's work committed first; agents often
    // leave it dirty, and a branch without commits makes merge a silent
    // no-op ("Already up to date")
    if let Err(e) = worktree::commit_all(worktree, &format!("velella: {}", task.name)) {
        let _ = deps.store.lock().unwrap().append_event(
            EventKind::TaskCompleted,
            &task.id,
            serde_json::json!({ "delivery_error": e.to_string() }),
        );
        return;
    }
    match spec.on_done {
        OnDone::Review => unreachable!("review does not deliver automatically"),
        OnDone::Pr => {
            let branch = format!("velella/{}", task.name);
            let title = format!("velella: {}", task.name);
            let body = format!(
                "Task `{}` (agent {}, base {})\n\nInstruction:\n{}\n\nVerified by velella against {} done-check(s).",
                task.name,
                spec.agent,
                spec.base,
                spec.instruction.as_deref().unwrap_or("-"),
                spec.done.len()
            );
            let outcome = (|| -> anyhow::Result<Option<String>> {
                if !worktree::push(worktree, &branch)? {
                    return Ok(None);
                }
                match worktree::create_pr(worktree, &branch, &spec.base, &title, &body)? {
                    Some(url) => Ok(Some(url)),
                    None => worktree::compare_url(worktree, &spec.base, &branch),
                }
            })();
            let store = deps.store.lock().unwrap();
            match outcome {
                Ok(Some(url)) => {
                    let _ = store.set_task_pr_url(&task.id, &url);
                    let _ = store.append_event(
                        EventKind::TaskCompleted,
                        &task.id,
                        serde_json::json!({ "result": "pr", "url": url }),
                    );
                }
                Ok(None) => {
                    let _ = store.append_event(
                        EventKind::TaskCompleted,
                        &task.id,
                        serde_json::json!({ "result": "committed locally (no remote)" }),
                    );
                }
                Err(e) => {
                    let _ = store.append_event(
                        EventKind::TaskCompleted,
                        &task.id,
                        serde_json::json!({ "pr_error": e.to_string() }),
                    );
                }
            }
        }
        OnDone::LocalMerge => {
            let branch = format!("velella/{}", task.name);
            let outcome = (|| -> anyhow::Result<String> {
                let head = worktree::head_branch(root)?;
                if head != spec.base {
                    anyhow::bail!(
                        "base `{}` is not checked out in the main checkout (HEAD is `{head}`) — merge and push by hand",
                        spec.base
                    );
                }
                worktree::merge(root, &branch, &format!("velella: {}", task.name))?;
                Ok(format!("merged into {}", spec.base))
            })();
            let store = deps.store.lock().unwrap();
            match outcome {
                Ok(msg) => {
                    // the merge IS the review: a successful local merge moves
                    // the task to its terminal state
                    let _ = store.set_task_state(&task.id, TaskState::InReview, TaskState::Merged);
                    let _ = store.append_event(
                        EventKind::TaskCompleted,
                        &task.id,
                        serde_json::json!({ "result": msg }),
                    );
                }
                Err(e) => {
                    let _ = store.append_event(
                        EventKind::TaskCompleted,
                        &task.id,
                        serde_json::json!({ "merge_error": e.to_string() }),
                    );
                }
            }
        }
    }
}

/// Delivery for a task that reached in_review without the runner's own
/// pass (manual re-verify, older tasks): `velella task pr <name>`.
pub fn pr_for(deps: &Deps, name: &str) -> anyhow::Result<String> {
    let task = deps
        .store
        .lock()
        .unwrap()
        .get_task(name)?
        .ok_or_else(|| anyhow::anyhow!("task `{name}` not found"))?;
    if task.state != TaskState::InReview {
        anyhow::bail!("task `{name}` is {} — delivery runs on in_review tasks", task.state);
    }
    let worktree = task
        .worktree
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("task `{name}` has no worktree"))?;
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    deliver(deps, &task, &root, &worktree);
    let url = deps.store.lock().unwrap().get_task(name)?.and_then(|t| t.pr_url);
    Ok(url.unwrap_or_else(|| "done — see the event log for the delivery outcome".into()))
}

/// Housekeeping: remove worktrees (and, for merged tasks, branches) of
/// finished tasks. Merged tasks lose their branch — the work is in history.
/// Failed tasks keep their branch — it may hold the only copy of the
/// attempt. Running/verifying/escalated tasks are never touched.
pub fn clean_tasks(deps: &Deps, all: bool) -> anyhow::Result<Vec<String>> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut report = Vec::new();
    let tasks = deps.store.lock().unwrap().list_tasks()?;
    for t in tasks {
        let cleanable = matches!(t.state, TaskState::Merged | TaskState::Failed)
            || (all && t.state == TaskState::InReview);
        if !cleanable {
            continue;
        }
        if let Some(wt) = &t.worktree {
            let path = PathBuf::from(wt);
            if path.exists() {
                let out = Command::new("git")
                    .current_dir(&root)
                    .args(["worktree", "remove", "--force"])
                    .arg(&path)
                    .output()?;
                if out.status.success() {
                    report.push(format!("{}: worktree removed", t.name));
                } else {
                    report.push(format!(
                        "{}: worktree remove failed: {}",
                        t.name,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
            }
            deps.store
                .lock()
                .unwrap()
                .set_task_binding(&t.id, None, None)?;
        }
        if t.state == TaskState::Merged {
            let branch = format!("velella/{}", t.name);
            let out = Command::new("git")
                .current_dir(&root)
                .args(["branch", "-D", &branch])
                .output()?;
            if out.status.success() {
                report.push(format!("{}: branch {branch} deleted", t.name));
            } else {
                report.push(format!(
                    "{}: branch delete failed: {}",
                    t.name,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        } else if t.state == TaskState::Failed {
            report.push(format!("{}: branch kept (failed attempt)", t.name));
        }
    }
    let _ = Command::new("git")
        .current_dir(&root)
        .args(["worktree", "prune"])
        .output();
    if report.is_empty() {
        report.push("nothing to clean".into());
    }
    Ok(report)
}

/// Re-run verification for a gated task — the human's way out of
/// `escalated` after fixing things by hand.
pub fn verify_gated(deps: &Deps, name: &str) -> anyhow::Result<String> {
    let task = deps
        .store
        .lock()
        .unwrap()
        .get_task(name)?
        .ok_or_else(|| anyhow::anyhow!("task `{name}` not found"))?;
    if task.state != TaskState::Escalated {
        anyhow::bail!("task `{name}` is {} — only escalated tasks can be re-verified", task.state);
    }
    let worktree = task
        .worktree
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("task `{name}` has no worktree"))?;
    match verify_worktree(&worktree, &task.spec.base, &task.spec.done) {
        Ok(()) => {
            {
                let store = deps.store.lock().unwrap();
                store.set_task_state(&task.id, TaskState::Escalated, TaskState::InReview)?;
                let _ = store.append_event(
                    EventKind::GateResolved,
                    &task.id,
                    serde_json::json!({ "resolved_by": "manual verify" }),
                );
            }
            notify::fire(&deps.config, "task", name, "in_review");
            Ok("verification passed — task moved to in_review".into())
        }
        Err(report) => Ok(format!("verification still failing:\n\n{report}")),
    }
}

/// Run the done-criteria in the worktree. Ok = the work is verifiably done.
fn verify_worktree(worktree: &Path, base: &str, checks: &[DoneCheck]) -> Result<(), String> {
    if checks.is_empty() {
        return Err("No verification criteria configured; work is unverified and requires review".into());
    }
    for check in checks {
        match check {
            DoneCheck::Run(cmd) => {
                #[cfg(windows)]
                let out = Command::new("cmd").args(["/C", cmd]).current_dir(worktree).output();
                #[cfg(not(windows))]
                let out = Command::new("sh").args(["-c", cmd]).current_dir(worktree).output();
                match out {
                    Ok(o) if o.status.success() => {}
                    Ok(o) => {
                        return Err(format!(
                            "`{cmd}` failed ({}): {}",
                            o.status,
                            tail(String::from_utf8_lossy(&o.stdout).as_bytes(), 600)
                        ))
                    }
                    Err(e) => return Err(format!("`{cmd}` could not run: {e}")),
                }
            }
            DoneCheck::DiffMaxFiles(max) => {
                let n = worktree::diff_file_count(worktree, base)
                    .map_err(|e| format!("diff guardrail failed: {e}"))?;
                if n > *max {
                    return Err(format!("diff touches {n} files (guardrail: {max})"));
                }
            }
        }
    }
    Ok(())
}

fn escalate(deps: &Deps, task: &TaskRecord, from: TaskState, reason: String) -> anyhow::Result<()> {
    {
        let store = deps.store.lock().unwrap();
        store.set_task_state(&task.id, from, TaskState::Escalated)?;
        let _ = store.append_event(
            EventKind::GateCreated,
            &task.id,
            serde_json::json!({ "reason": reason }),
        );
    }
    eprintln!("[velella:runner] task `{}` escalated: {reason}", task.name);
    notify::fire(&deps.config, "task", &task.name, "escalated");
    Ok(())
}

fn tail(bytes: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= max {
        return s.into_owned();
    }
    let cut = s
        .char_indices()
        .rev()
        .find(|(i, _)| *i <= s.len() - max)
        .map(|(i, _)| i)
        .unwrap_or(0);
    format!("…{}", &s[cut..])
}
