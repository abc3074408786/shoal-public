//! Session-local observation only: no permission decisions or model context.
use crate::{host::Descriptor, ipc};
use anyhow::{Context, Result};
use fs2::FileExt;
use serde_json::{json, Value};
use std::{fs::{File, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, path::Path};

const EVENTS: &[&str] = &["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt"];
const LIMIT: u64 = 32 * 1024 * 1024;

pub fn attach(desc: &mut Descriptor, executable: &Path) -> Result<()> {
    if desc.spawn.sandbox.is_sandboxed() || !matches!(desc.spawn.program.as_str(), "claude" | "codex") || desc.spawn.adapter.as_deref()==Some("codex-chat") { return Ok(()) }
    let mut paths=vec![executable.parent().context("runtime directory")?.to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    desc.spawn.env.insert("PATH".into(),std::env::join_paths(paths)?.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_ACTIVITY_DIR".into(),desc.directory.to_string_lossy().into_owned());
    let entry=json!({"hooks":[{"type":"command","command":"velella-runtime activity-hook","timeout":3}]});
    if desc.spawn.program=="claude" {
        // Compose with the session's existing collaboration settings, if any.
        let index=desc.spawn.args.iter().position(|v|v=="--settings");
        let mut settings:Value=match index {Some(i)=>serde_json::from_slice(&std::fs::read(desc.spawn.args.get(i+1).context("settings path")?)?)?,None=>json!({"hooks":{}})};
        for event in EVENTS.iter().copied().filter(|e|*e!="Interrupt").chain(["PostToolUseFailure"]) {
            let hooks=settings["hooks"].as_object_mut().context("hook settings object")?;
            hooks.entry(event.to_string()).or_insert_with(||json!([])).as_array_mut().context("hook array")?.push(entry.clone());
        }
        let path=desc.directory.join("activity-settings.json");ipc::atomic_json(&path,&settings)?;
        if let Some(i)=index {desc.spawn.args[i+1]=path.to_string_lossy().into_owned()} else {desc.spawn.args.extend(["--settings".into(),path.to_string_lossy().into_owned()])}
    } else {
        // Invocation-local opt-in: recent Codex versions disable update_plan by
        // default. Preserve explicit caller choices and all permission policy.
        if !desc.spawn.args.iter().any(|arg|arg.starts_with("tools.update_plan")) {
            desc.spawn.args.extend(["-c".into(),"tools.update_plan.enabled=true".into()]);
        }
        // These lifecycle records never become visible work items. On Windows,
        // starting a shell for them made even a plain message wait before the model.
        for event in EVENTS.iter().filter(|event|!matches!(**event,"SessionStart"|"UserPromptSubmit")) {
            let key=format!("hooks.{event}=");
            // Preserve hooks already added to this same CLI layer by collaboration.
            let existing=desc.spawn.args.iter().position(|v|v.starts_with(&key));
            // Tool starts remain synchronous so their start is recorded before a
            // completion. Stop is observational and must not delay the reply.
            let hook=if *event=="Stop" {"{hooks=[{type=\"command\",command=\"velella-runtime activity-hook\",timeout=3,async=true}]}"}else{"{hooks=[{type=\"command\",command=\"velella-runtime activity-hook\",timeout=3}]}"};
            if let Some(i)=existing {
                let old=desc.spawn.args[i].strip_suffix(']').context("inline hook array")?;
                desc.spawn.args[i]=format!("{old},{hook}]");
            } else {desc.spawn.args.extend(["-c".into(),format!("{key}[{hook}]")])}
        }
    }
    Ok(())
}

fn clipped(value: &Value, limit: usize) -> String {
    let text=if let Some(s)=value.as_str(){s.to_owned()}else if value.is_null(){String::new()}else{serde_json::to_string_pretty(value).unwrap_or_default()};
    if text.chars().count()>limit {format!("{}\n[内容过长，已截断]",text.chars().take(limit).collect::<String>())} else {text}
}

pub fn append(directory:&Path, event:&Value)->Result<()> {
    let desc:Descriptor=serde_json::from_slice(&std::fs::read(directory.join("launch.json"))?)?;
    anyhow::ensure!(desc.spawn.env.get("VELELLA_ACTIVITY_DIR").is_some_and(|p|Path::new(p)==directory),"activity destination mismatch");
    let name=event["hook_event_name"].as_str().context("event name")?;
    if !EVENTS.contains(&name)&&name!="PostToolUseFailure" {return Ok(())}
    let cwd=event["cwd"].as_str().context("event working directory")?;
    anyhow::ensure!(dunce::canonicalize(cwd)?==dunce::canonicalize(desc.spawn.cwd.as_deref().context("session working directory")?)?,"activity working directory mismatch");
    if std::env::var("VELELLA_RESTORE_GENERATION").ok()==desc.spawn.env.get("VELELLA_RESTORE_GENERATION").cloned(){crate::terminal_restore::observe(&desc,event)?;}
    anyhow::ensure!(event["session_id"].as_str().is_some_and(|v|!v.is_empty()&&v.len()<256),"missing native session identity");
    let lock=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(directory.join("activity.lock"))?;
    // Never hold up the agent indefinitely for an observer.
    let mut locked=false;
    for _ in 0..50 {if lock.try_lock_exclusive().is_ok(){locked=true;break}std::thread::sleep(std::time::Duration::from_millis(10));}
    anyhow::ensure!(locked,"activity writer busy; one event could not be recorded");
    let mut file=OpenOptions::new().create(true).append(true).open(directory.join("activity.jsonl"))?;
    anyhow::ensure!(file.metadata()?.len()<LIMIT,"activity storage limit reached");
    let response=&event["tool_response"];
    let failed=name=="PostToolUseFailure" || response["isError"]==true || response["is_error"]==true || ["exit_code","exitCode"].iter().any(|k|response[k].as_i64().is_some_and(|c|c!=0));
    // Even JSON-escaped control characters must fit in one read page.
    let record=json!({"event":name,"time":chrono::Utc::now().to_rfc3339(),"native_id":clipped(&event["session_id"],256),"agent_id":clipped(&event["agent_id"],256),"turn_id":clipped(&event["turn_id"],256),"call_id":clipped(&event["tool_use_id"],256),"tool":clipped(&event["tool_name"],256),"input":clipped(&event["tool_input"],4000),"output":clipped(if name=="PostToolUseFailure"{&event["error"]}else{response},12000),"failed":failed});
    let mut bytes=serde_json::to_vec(&record)?;bytes.push(b'\n');file.write_all(&bytes)?;
    Ok(())
}

pub fn hook()->Result<()> {
    let directory=std::env::var_os("VELELLA_ACTIVITY_DIR").context("activity destination unavailable")?;
    let directory=Path::new(&directory);
    let result=(||{let mut data=Vec::new();std::io::stdin().take(1024*1024+1).read_to_end(&mut data)?;anyhow::ensure!(data.len()<=1024*1024,"activity event too large");append(directory,&serde_json::from_slice(&data)?)})();
    if let Err(error)=result {let _=ipc::atomic_json(&directory.join("activity-error.json"),&json!({"message":error.to_string()}));}
    // Observation must not approve, deny, or interrupt the native CLI.
    Ok(())
}

pub fn read(desc:&Descriptor,since:u64)->Result<Value> {
    let enabled=desc.spawn.env.contains_key("VELELLA_ACTIVITY_DIR");
    let error=std::fs::read(desc.directory.join("activity-error.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok());
    let mut file=match File::open(desc.directory.join("activity.jsonl")){Ok(f)=>f,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(json!({"ok":true,"enabled":enabled,"events":[],"next":0,"total":0,"error":error})),Err(e)=>return Err(e.into())};
    let total=file.metadata()?.len();let from=since.min(total);file.seek(SeekFrom::Start(from))?;
    let mut bytes=Vec::new();file.take(128*1024).read_to_end(&mut bytes)?;
    let complete=bytes.iter().rposition(|b|*b==b'\n').map(|p|p+1).unwrap_or(0);
    let mut cursor=from;let mut events=Vec::new();
    for line in bytes[..complete].split_inclusive(|b|*b==b'\n') {let mut event:Value=serde_json::from_slice(line).context("activity record is damaged")?;event["seq"]=json!(cursor);events.push(event);cursor+=line.len() as u64;}
    Ok(json!({"ok":true,"enabled":enabled,"events":events,"next":cursor,"total":total,"limited":total>=LIMIT,"error":error}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->Descriptor {
        let dir=std::env::temp_dir().join(format!("vl-activity-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&dir).unwrap();
        let mut d=Descriptor{id:"local-session".into(),endpoint:"unused".into(),directory:dir.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"claude".into(),args:vec![],cwd:Some(dir.to_string_lossy().into_owned()),env:Default::default(),adapter:Some("generic".into())}};
        attach(&mut d,&std::env::current_exe().unwrap()).unwrap();ipc::atomic_json(&dir.join("launch.json"),&d).unwrap();d
    }
    #[test]fn records_are_scoped_paged_and_do_not_capture_prompts(){
        let d=fixture();let event=json!({"hook_event_name":"PreToolUse","session_id":"native-one","cwd":d.spawn.cwd,"tool_use_id":"read-one","tool_name":"Read","tool_input":{"file_path":"a.txt"},"prompt":"PRIVATE_PROMPT","thinking":"PRIVATE_REASONING"});
        append(&d.directory,&event).unwrap();let page=read(&d,0).unwrap();assert_eq!(page["events"][0]["tool"],"Read");assert!(!page.to_string().contains("PRIVATE_"));assert!(read(&d,page["next"].as_u64().unwrap()).unwrap()["events"].as_array().unwrap().is_empty());
        let mut bad=event.clone();bad["cwd"]=json!(d.directory.parent());assert!(append(&d.directory,&bad).is_err());
        let mut file=OpenOptions::new().append(true).open(d.directory.join("activity.jsonl")).unwrap();file.write_all(b"{\"event\":").unwrap();assert_eq!(read(&d,page["next"].as_u64().unwrap()).unwrap()["next"],page["next"]);
        std::fs::remove_dir_all(&d.directory).unwrap();
    }
    #[test]fn failure_and_long_multibyte_results_are_bounded(){
        let d=fixture();append(&d.directory,&json!({"hook_event_name":"PostToolUse","session_id":"n","cwd":d.spawn.cwd,"tool_use_id":"c","tool_name":"Bash","tool_response":{"exitCode":1,"stdout":"你".repeat(30000)}})).unwrap();
        let p=read(&d,0).unwrap();assert_eq!(p["events"][0]["failed"],true);assert!(p["events"][0]["output"].as_str().unwrap().contains("已截断"));std::fs::remove_dir_all(&d.directory).unwrap();
    }
    #[test]fn session_hook_composition_preserves_existing_handlers(){
        let mut d=fixture();d.spawn.program="codex".into();d.spawn.args=vec!["-c".into(),"hooks.PreToolUse=[{hooks=[{type=\"command\",command=\"existing-handler\"}]}]".into()];attach(&mut d,&std::env::current_exe().unwrap()).unwrap();
        let v=d.spawn.args.iter().find(|s|s.starts_with("hooks.PreToolUse=")).unwrap();let parsed:toml::Value=toml::from_str(v).unwrap();assert_eq!(parsed["hooks"]["PreToolUse"].as_array().unwrap().len(),2);assert!(!d.spawn.args.iter().any(|s|s.contains("bypass")));std::fs::remove_dir_all(&d.directory).unwrap();
    }
    #[test]fn codex_text_turn_does_not_wait_for_activity_recorder(){
        let mut d=fixture();d.spawn.program="codex".into();d.spawn.args=vec!["-c".into(),"hooks.SessionStart=[{hooks=[{type=\"command\",command=\"existing-handler\"}]}]".into()];
        attach(&mut d,&std::env::current_exe().unwrap()).unwrap();
        for event in ["SessionStart","UserPromptSubmit"] {
            assert!(!d.spawn.args.iter().any(|s|s.starts_with(&format!("hooks.{event}="))&&s.contains("activity-hook")));
        }
        assert!(d.spawn.args.iter().any(|s|s.contains("existing-handler")));
        let stop=d.spawn.args.iter().find(|s|s.starts_with("hooks.Stop=")).unwrap();let parsed:toml::Value=toml::from_str(stop).unwrap();
        assert_eq!(parsed["hooks"]["Stop"][0]["hooks"][0]["async"].as_bool(),Some(true));
        for event in ["PreToolUse","PostToolUse","PermissionRequest","Interrupt","SessionEnd"] {
            assert!(d.spawn.args.iter().any(|s|s.starts_with(&format!("hooks.{event}="))&&s.contains("activity-hook")));
        }
        std::fs::remove_dir_all(&d.directory).unwrap();
    }
}
