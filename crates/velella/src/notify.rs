//! Notifications: fire the user's `notify_cmd` on notable state changes.
//! velella stays dependency-free here — a toast, a Slack curl or a beep is
//! one config line away (VELELLA_KIND / SUBJECT / STATE in the environment).

use std::process::{Command, Stdio};
use velella_core::config::Config;

pub fn fire(cfg: &Config, kind: &str, subject: &str, state: &str) {
    let Some(cmd) = &cfg.notify_cmd else { return };
    if cmd.trim().is_empty() {
        return;
    }
    #[cfg(windows)]
    let child = Command::new("cmd")
        .args(["/C", cmd])
        .env("VELELLA_KIND", kind)
        .env("VELELLA_SUBJECT", subject)
        .env("VELELLA_STATE", state)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(not(windows))]
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .env("VELELLA_KIND", kind)
        .env("VELELLA_SUBJECT", subject)
        .env("VELELLA_STATE", state)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = child {
        eprintln!("[velella:notify] {kind}/{subject} -> {state}: notify_cmd failed: {e}");
    }
    // Fire-and-forget: the child is reaped by the OS on exit; a wedged
    // notifier must never stall the supervisor.
}
