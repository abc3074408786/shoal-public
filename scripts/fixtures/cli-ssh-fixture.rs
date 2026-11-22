//! Test-only SSH executable shim. Never installed or added to the user's PATH.
//! It connects the CLI's real framing client to an isolated local remote-stdio.
use std::{
    env,
    process::{Command, Stdio},
};
fn main() {
    if let Ok(log) = env::var("VELELLA_FIXTURE_SSH_LOG") {
        std::fs::write(log, env::args().skip(1).collect::<Vec<_>>().join("\n")).unwrap();
    }
    if let Ok(delay) = env::var("VELELLA_FIXTURE_SSH_DELAY_MS") {
        std::thread::sleep(std::time::Duration::from_millis(delay.parse().unwrap()));
    }
    let status = Command::new(env::var("VELELLA_FIXTURE_REMOTE_RUNTIME").unwrap())
        .arg("remote-stdio")
        .env(
            "VELELLA_DATA_DIR",
            env::var("VELELLA_FIXTURE_REMOTE_DATA").unwrap(),
        )
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
