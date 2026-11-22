//! Reattach is handled by the viewer. A dead native terminal resumes only its
//! own observed provider identity; never the provider's directory-wide latest.
use crate::{host::{Descriptor,Status},ipc,workbench::Session};
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{fs,io::{Read,Seek,SeekFrom},path::Path};
mod providers;
#[cfg(test)]mod providers_tests;

const RECORD:&str="terminal-native-session.json";
const GENERATION:&str="VELELLA_RESTORE_GENERATION";
#[derive(Serialize,Deserialize)]
struct Identity{local_id:String,program:String,native_id:String,cwd:Option<String>,generation:Option<String>}
pub(crate) fn provider(desc:&Descriptor)->&str{
    let name=desc.spawn.program.rsplit(['/', '\\']).next().unwrap_or("");
    let name=name.strip_suffix(".exe").or_else(||name.strip_suffix(".cmd")).or_else(||name.strip_suffix(".ps1")).unwrap_or(name);
    match name{"codex"|"claude"|"gemini"|"opencode"|"omp"|"kimi"|"zcode-free"|"mcode"=>name,_=>match desc.spawn.env.get("VELELLA_PROVIDER").map(String::as_str){Some("gemini")=>"gemini",Some("opencode")=>"opencode",_=>""}}
}
pub(crate) fn supported(desc:&Descriptor)->bool{
    !desc.spawn.sandbox.is_sandboxed()&&!desc.is_legacy_chat()&&!provider(desc).is_empty()
        &&!desc.spawn.args.iter().any(|a|matches!(a.as_str(),"login"|"logout"|"exec"|"-p"|"--prompt"|"--print"|"--help"|"--version")||a.starts_with("--prompt=")||a.starts_with("--prompt-interactive="))
}
fn valid(program:&str,id:&str)->bool{
    if matches!(program,"codex"|"claude"|"omp"){uuid::Uuid::parse_str(id).is_ok()}
    else if program=="kimi"{id.strip_prefix("session_").is_some_and(|id|uuid::Uuid::parse_str(id).is_ok())}
    else if program=="opencode"{id.starts_with("ses_")&&id.len()>4&&id.len()<=256&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')}
    else if program=="gemini"{!id.is_empty()&&id.len()<=256&&!matches!(id,"latest"|"last")&&!id.bytes().all(|b|b.is_ascii_digit())&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b))}
    else if program=="zcode-free"{id.starts_with("sess_")&&id.len()>5&&id.len()<=256&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')}
    else if program=="mcode"{!id.is_empty()&&id.len()<=256&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b))}
    else{false}
}
fn status(desc:&Descriptor)->Option<Status>{fs::read(desc.directory.join("status.json")).ok().and_then(|b|serde_json::from_slice(&b).ok())}
fn observation(desc:&Descriptor)->Option<Identity>{
    let path=desc.directory.join(RECORD);if fs::metadata(&path).ok()?.len()>4096{return None}
    let record:Identity=serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (record.local_id==desc.id&&record.program==desc.spawn.program&&record.cwd==desc.spawn.cwd&&valid(provider(desc),&record.native_id)).then_some(record)
}
pub(crate) fn native_id(desc:&Descriptor,saved:Option<&str>)->Option<String>{
    if !supported(desc){return None}
    if desc.directory.join("terminal-native-ambiguous.json").exists(){return None}
    if let Some(record)=observation(desc){return Some(record.native_id)}
    let legacy_role=provider(desc)=="opencode"&&desc.spawn.env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT");
    if !legacy_role{if let Some(id)=status(desc).and_then(|s|s.native_id).filter(|id|valid(provider(desc),id)){return Some(id)}}
    // Upgrade old terminals from THEIR bounded activity journal, not a scan of
    // unrelated provider sessions. Ignore subagent identities and partial lines.
    if let Ok(mut file)=fs::File::open(desc.directory.join("activity.jsonl")){
        if let Ok(len)=file.metadata().map(|m|m.len()){
            let from=len.saturating_sub(128*1024);let _=file.seek(SeekFrom::Start(from));let mut bytes=Vec::new();
            if file.take(128*1024).read_to_end(&mut bytes).is_ok(){
                for line in bytes.split(|b|*b==b'\n').rev(){
                    if let Ok(v)=serde_json::from_slice::<Value>(line){
                        if v["agent_id"].as_str().is_some_and(|s|!s.is_empty()){continue}
                        if let Some(id)=v["native_id"].as_str().filter(|id|valid(provider(desc),id)){return Some(id.into())}
                    }
                }
            }
        }
    }
    saved.filter(|id|valid(provider(desc),id)).map(String::from)
}
pub(crate) fn needs_selection(session:&Session)->bool{
    session.task_id.is_none()&&session.kind=="terminal"&&matches!(provider(&session.descriptor),"zcode-free"|"mcode"|"gemini"|"opencode"|"omp"|"kimi")
        &&supported(&session.descriptor)
        &&native_id(&session.descriptor,session.native_id.as_deref()).is_none()&&!untouched(&session.descriptor)
        &&crate::host::confirmed_dead(&session.descriptor.directory)
}
pub(crate) fn zcode_database(desc:&Descriptor)->Result<std::path::PathBuf>{
    // The local free wrapper fixes its database regardless of inherited ZCode
    // variables. Resolve THAT saved launcher's wrapper; don't inspect a global
    // latest session or assume another account's database.
    let inherited=desc.spawn.env.get("PATH").cloned().unwrap_or_else(||std::env::var("PATH").unwrap_or_default());
    for directory in std::env::split_paths(&inherited).filter(|p|p.is_absolute()){
        let launcher=directory.join("zcode-free.cmd");
        if !launcher.is_file(){continue}
        anyhow::ensure!(fs::metadata(&launcher)?.len()<64*1024,"ZCode launcher is too large");
        let launcher=fs::read_to_string(launcher)?;
        for item in launcher.split('"').filter(|s|s.ends_with("zcode-free.cjs")){
            let script=Path::new(item);anyhow::ensure!(script.is_absolute()&&fs::metadata(script)?.len()<64*1024,"unsupported ZCode wrapper");
            let source=fs::read_to_string(script)?;
            anyhow::ensure!(source.contains("ZCODE_SESSION_DB_PATH: path.join(settingsDir, 'sessions.sqlite')"),"ZCode database binding could not be verified");
            return Ok(script.parent().context("ZCode wrapper directory")?.join("free-profile/.zcode/cli/sessions.sqlite"));
        }
        anyhow::bail!("ZCode wrapper does not expose a verifiable session database")
    }
    anyhow::bail!("saved ZCode launcher is unavailable")
}
pub(crate) fn candidates(desc:&Descriptor)->Result<Vec<Value>>{
    if matches!(provider(desc),"gemini"|"opencode"|"omp"|"kimi"){return providers::candidates(desc)}
    if provider(desc)=="mcode"{return minimax_candidates(desc)}
    anyhow::ensure!(provider(desc)=="zcode-free"&&supported(desc),"session selection is only available for the native ZCode wrapper");
    let cwd=desc.spawn.cwd.as_deref().context("ZCode working directory missing")?.replace('\\',"/");
    let database=zcode_database(desc)?;
    let connection=rusqlite::Connection::open_with_flags(database,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(1))?;
    let mut stmt=connection.prepare("SELECT id,title,time_created FROM session WHERE replace(directory,char(92),'/')=?1 COLLATE NOCASE ORDER BY time_created DESC LIMIT 100")?;
    let rows=stmt.query_map([cwd],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,i64>(2)?)))?;
    let mut result=Vec::new();for row in rows{let(id,title,created)=row?;if valid("zcode-free",&id){result.push(json!({"id":id,"title":title.unwrap_or_default().chars().take(256).collect::<String>(),"created_at":created}));}}
    Ok(result)
}
pub(crate) fn bind(desc:&Descriptor,id:&str)->Result<()>{
    anyhow::ensure!(candidates(desc)?.iter().any(|s|s["id"]==id),"selected native session does not belong to this terminal's directory");
    observe(desc,&json!({"session_id":id,"cwd":desc.spawn.cwd}))
}
fn untouched(desc:&Descriptor)->bool{
    !desc.spawn.env.contains_key(crate::host::INITIAL_PROMPT_ENV)
        &&status(desc).is_some_and(|s|s.pid>0&&s.input_sequence==Some(0)&&s.turns==0)
}
pub(crate) fn automatic_allowed(session:&Session)->bool{
    recovery_allowed(session,false)
}
pub(crate) fn open_allowed(session:&Session)->bool{
    // Reopening is a new user intent, unlike a poll of the stopped pane. Only
    // an identified native conversation may cross the explicit-stop boundary.
    native_id(&session.descriptor,session.native_id.as_deref()).is_some()&&recovery_allowed(session,true)
}
fn recovery_allowed(session:&Session,on_open:bool)->bool{
    let d=&session.descriptor;
    if session.task_id.is_some()||session.kind!="terminal"||!supported(d)||(!on_open&&d.directory.join("terminal-stop-requested.json").exists())||!crate::host::confirmed_dead(&d.directory){return false}
    if native_id(d,session.native_id.as_deref()).is_none()&&!untouched(d){return false}
    let claim=d.directory.join("terminal-auto-recovery.json");
    if claim.exists(){
        let Ok(bytes)=fs::read(claim)else{return false};let Ok(claim)=serde_json::from_slice::<Value>(&bytes)else{return false};
        if claim["attempts"].as_u64().unwrap_or(1)>=2{return false}
        let current=status(d);
        let changed=current.as_ref().is_some_and(|s|Some(u64::from(s.pid))!=claim["previous_host_pid"].as_u64());
        let observed=observation(d).is_some_and(|o|o.generation.is_some()&&o.generation==d.spawn.env.get(GENERATION).cloned());
        // A failed launch cannot be retried by every poll/reload. A later loss
        // after a confirmed native SessionStart is a distinct recovery event.
        if !changed||(!observed&&!(untouched(d)&&current.is_some_and(|s|s.exit.is_none()))){return false}
    }
    true
}
pub(crate) fn resume(desc:&mut Descriptor,saved:Option<&str>)->Result<Option<String>>{
    if !supported(desc){return Ok(None)}
    let id=native_id(desc,saved);
    anyhow::ensure!(id.is_some()||untouched(desc),"未取得此终端的原生会话标识；为避免打开错误对话，未创建替代会话");
    if let Some(id)=&id{
        let mut args=desc.spawn.args.clone();
        if provider(desc)=="codex"{
            if args.first().is_some_and(|a|a=="resume") {anyhow::ensure!(args.len()>=2,"saved resume command is incomplete");args.drain(..2);}
            args.splice(..0,["resume".into(),id.clone()]);
        }else if matches!(provider(desc),"opencode"|"kimi"){
            for flag in ["--session","-s","-S"]{remove_option(&mut args,flag)?;}
            if provider(desc)=="kimi"{for flag in ["--agent","--agent-file"]{remove_option(&mut args,flag)?;}}
            args.retain(|a|!matches!(a.as_str(),"--continue"|"-c"|"--fork"));
            args.splice(..0,["--session".into(),id.clone()]);
        }else if provider(desc)=="mcode"{
            anyhow::ensure!(candidates(desc)?.iter().any(|s|s["id"]==*id),"MiniMax session is missing or belongs to a different workspace");
            if let Some(at)=args.iter().position(|a|a=="--session"){anyhow::ensure!(at+1<args.len(),"saved MiniMax session command is incomplete");args.drain(at..at+2);}
            args.splice(..0,["--session".into(),id.clone()]);
        }else{
            for flag in ["--resume","-r"]{remove_option(&mut args,flag)?;}
            if provider(desc)=="gemini"{remove_option(&mut args,"--session-id")?;remove_option(&mut args,"--session-file")?;}
            if provider(desc)=="omp"{remove_option(&mut args,"--restore")?;args.retain(|a|!matches!(a.as_str(),"--continue"|"-c"));}
            args.splice(..0,["--resume".into(),id.clone()]);
        }
        desc.spawn.args=args;
        desc.spawn.env.insert("VELELLA_RESTORE_NATIVE_ID".into(),id.clone());
    }
    desc.spawn.env.insert(GENERATION.into(),uuid::Uuid::now_v7().to_string());
    Ok(id)
}
fn remove_option(args:&mut Vec<String>,flag:&str)->Result<()>{
    while let Some(at)=args.iter().position(|a|a==flag||a.starts_with(&format!("{flag}="))){
        if args[at]==flag{anyhow::ensure!(at+1<args.len()&&!args[at+1].starts_with('-'),"saved resume command is incomplete");args.drain(at..at+2);}else{args.remove(at);}
    }Ok(())
}
pub(crate) fn claim(desc:&Descriptor)->Result<()>{
    let path=desc.directory.join("terminal-auto-recovery.json");
    let attempts=fs::read(&path).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).and_then(|v|v["attempts"].as_u64()).unwrap_or(0)+1;
    ipc::atomic_json(&path,&json!({"attempted":true,"attempts":attempts,"previous_host_pid":status(desc).map(|s|s.pid),"generation":desc.spawn.env.get(GENERATION),"at":chrono::Utc::now().to_rfc3339()}))
}
pub(crate) fn observe(desc:&Descriptor,event:&Value)->Result<()>{
    if !supported(desc)||event["agent_id"].as_str().is_some_and(|s|!s.is_empty()){return Ok(())}
    let Some(id)=event["session_id"].as_str().filter(|id|valid(provider(desc),id))else{return Ok(())};
    let cwd=event["cwd"].as_str().context("native identity needs its working directory")?;
    anyhow::ensure!(dunce::canonicalize(cwd)?==dunce::canonicalize(desc.spawn.cwd.as_deref().context("saved working directory required")?)?,"native identity belongs to another working directory");
    ipc::atomic_json(&desc.directory.join(RECORD),&Identity{local_id:desc.id.clone(),program:desc.spawn.program.clone(),native_id:id.into(),cwd:desc.spawn.cwd.clone(),generation:desc.spawn.env.get(GENERATION).cloned()})?;
    match fs::remove_file(desc.directory.join("terminal-native-ambiguous.json")){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.into())}
    if matches!(event["hook_event_name"].as_str(),Some("UserPromptSubmit"|"Stop")){
        match fs::remove_file(desc.directory.join("terminal-auto-recovery.json")){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.into())}
    }
    Ok(())
}
pub(crate) fn observe_hook(desc:&Descriptor,event:&Value)->Result<()>{
    // Legacy OpenCode role plugins did not distinguish child events or report
    // a directory. They cannot establish a new durable terminal identity.
    if !hook_is_bound(desc,event){return Ok(())}
    let mut native=event.clone();if native["cwd"].is_null(){native["cwd"]=json!(desc.spawn.cwd);}
    observe(desc,&native)
}
pub(crate) fn hook_is_bound(desc:&Descriptor,event:&Value)->bool{
    if provider(desc)!="opencode"||!desc.spawn.env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT")||!event["cwd"].is_null(){return true}
    let expected=desc.spawn.env.get("VELELLA_RESTORE_NATIVE_ID").cloned().or_else(||observation(desc).map(|record|record.native_id));
    expected.as_deref().zip(event["session_id"].as_str()).is_some_and(|(expected,id)|expected==id)
}
pub(crate) fn attach(desc:&mut Descriptor,runtime:&Path)->Result<()>{
    if !supported(desc){return Ok(())}
    if provider(desc)=="mcode"{
        // The TUI has no invocation-local hook flag. Bind an explicit selection
        // from the original native data root instead of modifying global plugins.
        if !desc.spawn.env.contains_key("VELELLA_MINIMAX_DATA_DIR"){
            let data=std::env::var_os("MINIMAX_DATA_DIR").or_else(||std::env::var_os("MAVIS_DATA_DIR"))
                .map(std::path::PathBuf::from).unwrap_or_else(||std::path::PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).unwrap_or_default()).join(".minimax"));
            anyhow::ensure!(data.is_absolute(),"MiniMax data directory must be absolute");
            desc.spawn.env.insert("MINIMAX_DATA_DIR".into(),data.to_string_lossy().into_owned());
            desc.spawn.env.insert("VELELLA_MINIMAX_DATA_DIR".into(),data.to_string_lossy().into_owned());
        }
        return Ok(())
    }
    let parent=runtime.parent().context("restore runtime directory")?;
    let inherited=desc.spawn.env.get("PATH").cloned().unwrap_or_else(||std::env::var("PATH").unwrap_or_default());
    let mut paths=vec![parent.to_path_buf()];paths.extend(std::env::split_paths(&inherited).filter(|p|p!=parent));
    desc.spawn.env.insert("PATH".into(),std::env::join_paths(paths)?.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_RESTORE_DIR".into(),desc.directory.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_RESTORE_RUNTIME".into(),runtime.to_string_lossy().into_owned());
    // Claude's activity SessionStart already records the binding. Codex needs
    // one startup observer, while ordinary user prompts remain observer-free.
    if provider(desc)=="codex"&&!desc.spawn.env.contains_key(GENERATION){
        let key="hooks.SessionStart=";let hook="{hooks=[{type=\"command\",command=\"velella-runtime native-restore-hook\",timeout=3}]}";
        if let Some(at)=desc.spawn.args.iter().position(|a|a.starts_with(key)){
            if !desc.spawn.args[at].contains("velella-runtime native-restore-hook"){
                let old=desc.spawn.args[at].strip_suffix(']').context("native startup hook array")?;
                desc.spawn.args[at]=format!("{old},{hook}]");
            }
        }else{desc.spawn.args.extend(["-c".into(),format!("{key}[{hook}]")]);}
    }
    providers::attach(desc,runtime)
}
fn minimax_candidates(desc:&Descriptor)->Result<Vec<Value>>{
    let data=desc.spawn.env.get("VELELLA_MINIMAX_DATA_DIR").context("saved MiniMax data directory missing")?;
    let database=Path::new(data).join("v2/sqlite/runtime-state.sqlite");
    anyhow::ensure!(Path::new(data).is_absolute(),"invalid MiniMax data directory");
    let db=rusqlite::Connection::open_with_flags(database,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(1))?;
    let mut query=db.prepare("SELECT session_id,title,created_at_ms,workspace_dir FROM local_runtime_sessions WHERE parent_session_id IS NULL AND archived=0 ORDER BY created_at_ms DESC LIMIT 1000")?;
    let cwd=dunce::canonicalize(desc.spawn.cwd.as_deref().context("MiniMax workspace missing")?)?;
    let rows=query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<String>>(3)?)))?;
    let mut values=Vec::new();
    for row in rows{let(id,title,at,workspace)=row?;
        if valid("mcode",&id)&&workspace.and_then(|p|dunce::canonicalize(p).ok()).is_some_and(|p|p==cwd){values.push(json!({"id":id,"title":title.unwrap_or_default().chars().take(256).collect::<String>(),"created_at":at.unwrap_or_default()}));}
    }Ok(values)
}
pub(crate) fn hook(id:Option<&str>,event_name:Option<&str>)->Result<()>{
    // Observer failures never deny a native action or return permission output.
    let result=(||->Result<()>{
        let directory=std::env::var_os("VELELLA_RESTORE_DIR").context("restore destination missing")?;let directory=Path::new(&directory);
        let desc:Descriptor=serde_json::from_slice(&fs::read(directory.join("launch.json"))?)?;
        anyhow::ensure!(desc.directory==directory,"restore destination mismatch");
        anyhow::ensure!(std::env::var(GENERATION).ok()==desc.spawn.env.get(GENERATION).cloned(),"stale restore hook generation");
        let mut event=if let Some(id)=id{json!({"session_id":id,"cwd":std::env::current_dir()?,"hook_event_name":event_name})}else{
            let mut bytes=Vec::new();std::io::stdin().take(1024*1024).read_to_end(&mut bytes)?;serde_json::from_slice(&bytes)?};
        if let Some(name)=event_name{event["hook_event_name"]=json!(name);}
        observe(&desc,&event)
    })();
    if let Err(e)=result{eprintln!("native restore observer: {e}");}Ok(())
}
pub(crate) fn poll_native(desc:&Descriptor,owns:impl Fn(u32)->bool,input_sequence:u64)->Result<Option<String>>{providers::poll_kimi(desc,owns,input_sequence)}

#[cfg(test)]
mod tests{
    use super::*;
    pub(super) fn fixture(program:&str)->Descriptor{
        let directory=std::env::temp_dir().join(format!("vl-restore-{}",uuid::Uuid::now_v7()));fs::create_dir_all(&directory).unwrap();
        Descriptor{id:"local".into(),endpoint:"unused".into(),spawn:crate::sessions::SpawnRequest{program:program.into(),args:vec!["--model".into(),"unchanged".into()],cwd:Some(directory.to_string_lossy().into_owned()),env:Default::default(),adapter:Some("generic".into()),sandbox:Default::default()},directory}
    }
    #[test]fn native_resume_keeps_identity_flags_and_environment(){
        for program in ["codex","claude","gemini","opencode","omp","kimi","zcode-free"]{
            let mut d=fixture(program);let id=match program{"zcode-free"=>"sess_abc123".into(),"opencode"=>"ses_abc123".into(),"kimi"=>format!("session_{}",uuid::Uuid::now_v7()),_=>uuid::Uuid::now_v7().to_string()};
            observe(&d,&json!({"session_id":id,"cwd":d.spawn.cwd})).unwrap();
            d.spawn.env.insert("CODEX_HOME".into(),"unchanged-home".into());let original=d.spawn.args.clone();
            assert_eq!(resume(&mut d,None).unwrap(),Some(id.clone()));assert_eq!(d.spawn.args[1],id);assert_eq!(&d.spawn.args[2..],original);assert_eq!(d.spawn.env["CODEX_HOME"],"unchanged-home");
            resume(&mut d,None).unwrap();assert_eq!(d.spawn.args.len(),original.len()+2);
            fs::remove_dir_all(d.directory).unwrap();
        }
    }
    #[test]fn unknown_identity_cannot_start_a_replacement_conversation(){let mut d=fixture("codex");assert!(resume(&mut d,None).is_err());fs::remove_dir_all(d.directory).unwrap();}
    #[test]fn minimax_resume_requires_explicit_identity_from_saved_workspace(){
        let mut d=fixture("mcode");let database=d.directory.join("native/v2/sqlite/runtime-state.sqlite");fs::create_dir_all(database.parent().unwrap()).unwrap();d.spawn.env.insert("VELELLA_MINIMAX_DATA_DIR".into(),d.directory.join("native").to_string_lossy().into_owned());
        let db=rusqlite::Connection::open(&database).unwrap();db.execute_batch("CREATE TABLE local_runtime_sessions(session_id TEXT,title TEXT,created_at_ms INTEGER,workspace_dir TEXT,parent_session_id TEXT,archived INTEGER);").unwrap();
        for(id,cwd,parent)in[("session_one",d.spawn.cwd.as_deref().unwrap(),None),("session_child",d.spawn.cwd.as_deref().unwrap(),Some("session_one")),("session_foreign","/elsewhere",None)]{db.execute("INSERT INTO local_runtime_sessions VALUES(?1,'test',1,?2,?3,0)",rusqlite::params![id,cwd,parent]).unwrap();}drop(db);
        let before=fs::read(&database).unwrap();assert!(native_id(&d,None).is_none());assert_eq!(candidates(&d).unwrap().len(),1);assert!(bind(&d,"session_child").is_err());assert!(bind(&d,"session_foreign").is_err());
        ipc::atomic_json(&d.directory.join("status.json"),&Status{pid:1,input_sequence:Some(2),..Status::default()}).unwrap();fs::write(d.directory.join("terminal-stop-requested.json"),b"{}").unwrap();
        let session=Session{id:d.id.clone(),workspace_id:"w".into(),title:"MiniMax Code".into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:d.clone()};
        assert!(needs_selection(&session),"explicitly stopping MiniMax must still allow selecting its original conversation");assert!(!automatic_allowed(&session),"explicit stop must not restart a process automatically");
        bind(&d,"session_one").unwrap();
        resume(&mut d,None).unwrap();assert_eq!(&d.spawn.args[..2],&["--session","session_one"]);resume(&mut d,None).unwrap();assert_eq!(d.spawn.args.iter().filter(|s|*s=="--session").count(),1);assert_eq!(fs::read(&database).unwrap(),before);fs::remove_dir_all(d.directory).unwrap();
    }
    #[test]fn recovering_an_existing_terminal_does_not_add_a_new_hook_approval(){
        let mut d=fixture("codex");let id=uuid::Uuid::now_v7().to_string();
        ipc::atomic_json(&d.directory.join("status.json"),&Status{pid:1,native_id:Some(id),..Status::default()}).unwrap();
        resume(&mut d,None).unwrap();let args=d.spawn.args.clone();attach(&mut d,&std::env::current_exe().unwrap()).unwrap();assert_eq!(d.spawn.args,args,"cold restore must keep the original hook contract");fs::remove_dir_all(d.directory).unwrap();
    }
    #[test]fn repeated_native_startup_failures_are_bounded_and_real_turns_reset_the_claim(){
        let mut d=fixture("codex");let id=uuid::Uuid::now_v7().to_string();
        ipc::atomic_json(&d.directory.join("status.json"),&Status{pid:10,native_id:Some(id.clone()),..Status::default()}).unwrap();
        let session=|d:&Descriptor|Session{id:d.id.clone(),workspace_id:"w".into(),title:"Codex".into(),kind:"terminal".into(),task_id:None,native_id:Some(id.clone()),descriptor:d.clone()};
        for pid in [11,12]{resume(&mut d,Some(&id)).unwrap();claim(&d).unwrap();observe(&d,&json!({"session_id":id,"cwd":d.spawn.cwd,"hook_event_name":"SessionStart"})).unwrap();ipc::atomic_json(&d.directory.join("status.json"),&Status{pid,native_id:Some(id.clone()),..Status::default()}).unwrap();}
        assert!(!automatic_allowed(&session(&d)),"two startup losses without a real turn must stop automatic retries");
        observe(&d,&json!({"session_id":id,"cwd":d.spawn.cwd,"hook_event_name":"Stop"})).unwrap();assert!(automatic_allowed(&session(&d)));fs::remove_dir_all(d.directory).unwrap();
    }
    #[test]fn identity_observer_rejects_other_workspaces_and_subagents(){
        let d=fixture("codex");let id=uuid::Uuid::now_v7().to_string();
        observe(&d,&json!({"session_id":id,"cwd":d.spawn.cwd,"agent_id":"child"})).unwrap();assert!(native_id(&d,None).is_none());
        assert!(observe(&d,&json!({"session_id":id,"cwd":d.directory.parent()})).is_err());
        fs::remove_dir_all(d.directory).unwrap();
    }
    #[test]fn legacy_zcode_binding_requires_an_explicit_choice_from_the_saved_directory(){
        let mut d=fixture("zcode-free");d.spawn.args.clear();let bin=d.directory.join("bin");fs::create_dir_all(&bin).unwrap();
        let wrapper=d.directory.join("zcode-free.cjs");fs::write(&wrapper,"ZCODE_SESSION_DB_PATH: path.join(settingsDir, 'sessions.sqlite')").unwrap();
        fs::write(bin.join("zcode-free.cmd"),format!("@echo off\n\"node.exe\" \"{}\" %*",wrapper.display())).unwrap();d.spawn.env.insert("PATH".into(),bin.to_string_lossy().into_owned());
        let database=d.directory.join("free-profile/.zcode/cli/sessions.sqlite");fs::create_dir_all(database.parent().unwrap()).unwrap();
        let connection=rusqlite::Connection::open(&database).unwrap();connection.execute_batch("CREATE TABLE session(id TEXT,title TEXT,directory TEXT,time_created INTEGER);").unwrap();
        for (id,cwd) in [("sess_first",d.spawn.cwd.as_deref().unwrap()),("sess_second",d.spawn.cwd.as_deref().unwrap()),("sess_foreign","/other")]{connection.execute("INSERT INTO session VALUES (?1,'fixture',?2,1)",rusqlite::params![id,cwd]).unwrap();}
        drop(connection);let before=fs::read(&database).unwrap();
        ipc::atomic_json(&d.directory.join("status.json"),&Status{pid:1234,input_sequence:Some(2),..Status::default()}).unwrap();
        let mut session=Session{id:d.id.clone(),workspace_id:"w".into(),title:"ZCode".into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:d.clone()};
        assert!(needs_selection(&session));assert_eq!(candidates(&d).unwrap().len(),2);assert!(native_id(&d,None).is_none(),"listing must not guess latest");
        assert!(bind(&d,"sess_foreign").is_err());bind(&d,"sess_first").unwrap();
        assert_eq!(native_id(&d,None).as_deref(),Some("sess_first"));assert!(!needs_selection(&session));assert!(automatic_allowed(&session));
        session.descriptor.spawn.args=vec!["--resume".into(),"sess_first".into()];
        assert_eq!(fs::read(&database).unwrap(),before,"selection only binds local identity, never rewrites provider history");
        fs::remove_dir_all(d.directory).unwrap();
    }
}
