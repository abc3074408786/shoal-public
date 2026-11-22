//! Inert native CLI stand-in for isolated PTY/submission integration only.
use std::{io::{BufRead,Write},process::{Command,Stdio}};
fn hook(name:&str,prompt:Option<&str>) {
    let runtime=std::env::var("VL_TEST_RUNTIME").unwrap();
    let mut child=Command::new(runtime).arg("hook").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    let text=prompt.map(|s|format!(",\"prompt\":\"{s}\"")).unwrap_or_default();
    write!(child.stdin.take().unwrap(),"{{\"hook_event_name\":\"{name}\",\"session_id\":\"00000000-0000-0000-0000-000000000007\",\"turn_id\":\"fixture-turn\"{text}}}").unwrap();
    assert!(child.wait().unwrap().success());
}
fn main() {
    let tracked=std::env::var_os("VELELLA_SESSION_ENDPOINT").is_some();
    if tracked {hook("SessionStart",None);}
    println!("Fixture ready; no model or network exists.");
    for line in std::io::stdin().lock().lines() {
        let line=line.unwrap().replace("\x1b[200~","").replace("\x1b[201~","");
        let prompt=line.trim();if prompt.is_empty(){continue;}
        // Only these fixed inputs are accepted, so JSON above needs no general
        // escaping and this fixture cannot become a shell or real CLI wrapper.
        assert!(matches!(prompt,"submission-fixture-prompt"|"submission-fixture-silent"|"submission-fixture-mismatch"));
        let mut log=std::fs::OpenOptions::new().create(true).append(true).open(std::env::var("VL_TEST_EVENTS").unwrap()).unwrap();
        writeln!(log,"received").unwrap();
        if !tracked {continue;}
        if prompt=="submission-fixture-silent" {continue;}
        hook("UserPromptSubmit",Some(if prompt=="submission-fixture-mismatch"{"another-prompt"}else{prompt}));
        hook("PreToolUse",None);hook("Stop",None);println!("Fixture completed.");
    }
}
