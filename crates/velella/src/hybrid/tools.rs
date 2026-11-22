//! Local tools for one explicitly selected project. Approval is not a sandbox.
use super::Config;
use anyhow::{Context,Result};
use base64::Engine;
use fs2::FileExt;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs,io::Read,path::{Component,Path,PathBuf},process::Stdio,time::{Duration,Instant}};
#[cfg(not(windows))] use std::io::Write;
#[path="platform_guard.rs"] mod platform_guard;

pub const LIMIT:usize=1024*1024;
pub fn digest(bytes:&[u8])->String{format!("sha256:{:x}",Sha256::digest(bytes))}
pub fn document(path:&Path)->Result<Value>{
    let f=fs::File::open(path)?;anyhow::ensure!(f.metadata()?.len()<=4*LIMIT as u64,"hybrid document too large");
    Ok(serde_json::from_reader(f)?)
}
fn audit<T>(config:&Config,f:impl FnOnce(&mut Vec<Value>)->Result<T>)->Result<T>{
    let lock=fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(config.directory.join("hybrid-audit.lock"))?;
    lock.lock_exclusive()?;
    let path=config.directory.join("hybrid-audit.json");
    let mut rows=if path.exists(){serde_json::from_value(document(&path)?)?}else{Vec::new()};
    let result=f(&mut rows)?;
    crate::ipc::atomic_json(&path,&rows)?;Ok(result)
}
pub fn status(config:&Config)->Result<Value>{
    audit(config,|rows|Ok(json!({"ok":true,"session_id":config.session_id,"workspace_id":config.workspace_id,"local_root":config.project_root,"remote_host":config.profile["host"],"operations":rows})))
}
pub fn decide(config:&Config,id:&str,stamp:&str,allow:bool)->Result<Value>{
    audit(config,|rows|{
        let row=rows.iter_mut().find(|r|r["id"]==id).context("operation not found")?;
        anyhow::ensure!(row["digest"]==stamp,"approval content changed; inspect again");
        anyhow::ensure!(row["state"]=="pending","operation is no longer awaiting approval");
        row["state"]=json!(if allow{"approved"}else{"rejected"});
        Ok(json!({"ok":true,"operation":row}))
    })
}
pub fn uncertain(config:&Config)->Result<()>{
    audit(config,|rows|{for row in rows{
        if matches!(row["state"].as_str(),Some("pending"|"approved")){row["state"]=json!("rejected");row["error"]=json!("connection ended before execution");}
        else if row["state"]=="executing"{row["state"]=json!("uncertain");row["error"]=json!("execution result unknown; never replay automatically");}
    }Ok(())})
}
fn field<'a>(v:&'a Value,key:&str)->Result<&'a str>{v[key].as_str().with_context(||format!("{key} must be a string"))}
pub fn scoped(config:&Config,relative:&str,new:bool)->Result<PathBuf>{
    anyhow::ensure!(relative.len()<=4096&&!relative.contains(['\0',':','\\']),"invalid local relative path");
    let root=dunce::canonicalize(&config.project_root)?;
    anyhow::ensure!(root==config.project_root,"authoritative project root changed; review before reconnecting");
    let rel=Path::new(relative);
    anyhow::ensure!(!rel.is_absolute(),"absolute local paths are not allowed");
    let mut current=root.clone();
    for part in rel.components(){match part{
        Component::Normal(name)=>{
            let name=name.to_string_lossy();
            anyhow::ensure!(!matches!(name.to_ascii_lowercase().as_str(),".git"|".velella")&&!name.ends_with(['.',' ']),"private metadata path denied");
            current.push(name.as_ref());
            if let Ok(meta)=fs::symlink_metadata(&current){
                anyhow::ensure!(!meta.file_type().is_symlink(),"symbolic links are not local tool targets");
                #[cfg(windows)] {use std::os::windows::fs::MetadataExt;anyhow::ensure!(meta.file_attributes()&0x400==0,"reparse points are not local tool targets");}
                let canonical=dunce::canonicalize(&current)?;anyhow::ensure!(canonical.starts_with(&root),"local path leaves project");
            }
        },Component::CurDir=>{},_=>anyhow::bail!("local path traversal denied"),}}
    if new{anyhow::ensure!(dunce::canonicalize(current.parent().context("file parent missing")?)?.starts_with(&root),"parent leaves project");}
    else{anyhow::ensure!(dunce::canonicalize(&current)?.starts_with(&root),"local path leaves project");}
    Ok(current)
}
pub fn catalog()->Value{json!([
 {"name":"local_write_evidence","description":"Create one new LOCAL .velella/evidence/<request_id>.log after approval. Available to managed read-only roles too; cannot overwrite source or existing evidence. Returns artifact path and SHA-256.","inputSchema":{"type":"object","properties":{"request_id":{"type":"string","description":"Stable UUID; never replay"},"content":{"type":"string"}},"required":["request_id","content"],"additionalProperties":false}},
 {"name":"local_read_file","description":"Read a file from the LOCAL authoritative project; shell tools outside this MCP run on Linux.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"encoding":{"enum":["utf8","base64"]}},"required":["path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}},
 {"name":"local_list_files","description":"List one directory in the LOCAL project.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}},
 {"name":"local_write_file","description":"Request an approved LOCAL write. Supply the read sha256 digest or absent; approval waits in Velella. Never repeat an uncertain operation.","inputSchema":{"type":"object","properties":{"request_id":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"},"encoding":{"enum":["utf8","base64"]},"expected_digest":{"type":"string"}},"required":["request_id","path","content","expected_digest"],"additionalProperties":false}},
 {"name":"local_run_command","description":"Request approval to run a program on the LOCAL computer, in the project. Native Codex shell runs REMOTELY. Approval is not an OS sandbox.","inputSchema":{"type":"object","properties":{"request_id":{"type":"string"},"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"}},"required":["request_id","program","args","cwd"],"additionalProperties":false}}
])}
pub fn call(config:&Config,name:&str,args:&Value)->Result<Value>{
    call_connected(config,name,args,None)
}
pub fn call_connected(config:&Config,name:&str,args:&Value,connected:Option<&std::sync::atomic::AtomicBool>)->Result<Value>{
    anyhow::ensure!(args.is_object(),"tool arguments must be an object");
    // The non-Windows path implementation still has path and conditional-write
    // races. Do not expose it to managed remote roles as a safe worktree bridge.
    anyhow::ensure!(platform_guard::local_tools_allowed(config.collaboration_data.is_some()),"remote orchestration local tools require the supported Windows file safety implementation");
    anyhow::ensure!(config.collaboration_data.is_none()||config.collaboration_writes||!matches!(name,"local_write_file"|"local_run_command"),"read-only collaboration role cannot modify local files or run local commands");
    if name=="local_read_file"{
        #[cfg(windows)]
        let bytes=super::secure_windows::read(config,field(args,"path")?)?;
        #[cfg(not(windows))]
        let bytes={
        let path=scoped(config,field(args,"path")?,false)?;let file=fs::File::open(&path)?;
        anyhow::ensure!(file.metadata()?.is_file()&&file.metadata()?.len()<=LIMIT as u64,"local file is not regular or exceeds 1 MiB");
        let mut bytes=Vec::new();file.take((LIMIT+1)as u64).read_to_end(&mut bytes)?;anyhow::ensure!(bytes.len()<=LIMIT,"local file grew beyond limit");
        bytes};
        let content=match args["encoding"].as_str().unwrap_or("utf8"){"utf8"=>String::from_utf8(bytes.clone()).context("binary file requires base64 encoding")?,"base64"=>base64::engine::general_purpose::STANDARD.encode(&bytes),_=>anyhow::bail!("unsupported encoding")};
        return Ok(json!({"host":"local","path":args["path"],"digest":digest(&bytes),"content":content}))
    }
    if name=="local_list_files"{
        #[cfg(windows)]
        let entries=super::secure_windows::list(config,field(args,"path")?)?;
        #[cfg(not(windows))]
        let entries={
        let path=scoped(config,field(args,"path")?,false)?;let mut entries=Vec::new();
        for entry in fs::read_dir(path)?.take(513){let e=entry?;let name=e.file_name().to_string_lossy().into_owned();if matches!(name.to_ascii_lowercase().as_str(),".git"|".velella"){continue}entries.push(json!({"name":name,"directory":e.file_type()?.is_dir(),"link":e.file_type()?.is_symlink()}));}
        anyhow::ensure!(entries.len()<=512,"directory listing exceeds 512 entries");entries};return Ok(json!({"host":"local","entries":entries}))
    }
    anyhow::ensure!(matches!(name,"local_write_file"|"local_run_command"|"local_write_evidence"),"unknown local tool");
    validate(config,name,args)?;
    let id=field(args,"request_id")?;anyhow::ensure!(!id.is_empty()&&id.len()<=128&&!id.chars().any(char::is_control),"invalid request_id");
    let stamp=digest(&serde_json::to_vec(&json!({"tool":name,"arguments":args}))?);
    audit(config,|rows|{
        anyhow::ensure!(rows.len()<256,"session approval ledger is full");
        anyhow::ensure!(!rows.iter().any(|r|r["id"]==id),"request_id already recorded; inspect its result, never replay");
        rows.push(json!({"id":id,"digest":stamp,"session_id":config.session_id,"workspace_id":config.workspace_id,"host":"local","tool":name,"arguments":args,"state":"pending","created_at":chrono::Utc::now().to_rfc3339()}));Ok(())
    })?;
    let deadline=Instant::now()+Duration::from_secs(90);
    loop{
        let state=audit(config,|rows|{
            let row=rows.iter_mut().find(|r|r["id"]==id).context("operation disappeared")?;
            if matches!(row["state"].as_str(),Some("pending"|"approved"))&&connected.is_some_and(|v|!v.load(std::sync::atomic::Ordering::SeqCst)){row["state"]=json!("rejected");row["error"]=json!("MCP connection ended before execution");}
            if row["state"]=="pending"&&Instant::now()>=deadline{row["state"]=json!("rejected");row["error"]=json!("approval expired");}
            if row["state"]=="approved"{row["state"]=json!("executing");return Ok("execute".to_string())}
            Ok(row["state"].as_str().unwrap_or("invalid").to_string())
        })?;
        match state.as_str(){"pending"=>std::thread::sleep(Duration::from_millis(150)),"execute"=>break,_=>anyhow::bail!("local operation {state}; not executed"),}
    }
    let result=validate(config,name,args).and_then(|_|match name{"local_write_file"=>write_file(config,args),"local_write_evidence"=>write_evidence(config,args),_=>run_command(config,args)});
    audit(config,|rows|{let row=rows.iter_mut().find(|r|r["id"]==id).context("operation disappeared")?;
        match &result{Ok(v)=>{row["state"]=json!(if v["uncertain"]==true{"uncertain"}else{"done"});row["result"]=v.clone();},Err(e)=>{row["state"]=json!("failed");row["error"]=json!(e.to_string());}}Ok(())})?;
    result
}
fn validate(config:&Config,name:&str,args:&Value)->Result<()>{
    if name=="local_write_evidence" {
        anyhow::ensure!(config.collaboration_data.is_some(),"evidence tool requires a managed session");
        uuid::Uuid::parse_str(field(args,"request_id")?)?;
        anyhow::ensure!(args.as_object().is_some_and(|o|o.len()==2)&&field(args,"content")?.len()<=64*1024,"invalid or oversized evidence snapshot");
        return Ok(())
    }
    if name=="local_write_file"{
        #[cfg(windows)] {
        let expected=field(args,"expected_digest")?;
        let actual=match super::secure_windows::read(config,field(args,"path")?) {Ok(b)=>digest(&b),Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|e|e.kind()==std::io::ErrorKind::NotFound)=>"absent".into(),Err(e)=>return Err(e)};
        anyhow::ensure!(expected==actual,"local file changed; read and request approval again"); decoded(args)?;
        }
        #[cfg(not(windows))] {
        let path=scoped(config,field(args,"path")?,true)?;
        anyhow::ensure!(path!=dunce::canonicalize(&config.project_root)?,"cannot overwrite project root");
        let expected=field(args,"expected_digest")?;
        let actual=if path.exists(){let file=fs::File::open(&path)?;anyhow::ensure!(file.metadata()?.is_file()&&file.metadata()?.len()<=LIMIT as u64,"existing file exceeds write limit");let mut bytes=Vec::new();file.take((LIMIT+1)as u64).read_to_end(&mut bytes)?;digest(&bytes)}else{"absent".into()};
        anyhow::ensure!(expected==actual,"local file changed; read and request approval again");
        decoded(args)?;
        }
    }else{
        anyhow::ensure!(scoped(config,field(args,"cwd")?,false)?.is_dir(),"local command cwd must be a directory");
        let program=field(args,"program")?;anyhow::ensure!(!program.is_empty()&&program.len()<=4096&&!program.chars().any(char::is_control),"invalid local program");
        let arguments=args["args"].as_array().context("args must be an array")?;
        anyhow::ensure!(arguments.len()<=64&&arguments.iter().all(|a|a.as_str().is_some_and(|s|s.len()<=8192&&!s.contains('\0'))),"invalid local command arguments");
    }Ok(())
}
fn write_evidence(config:&Config,args:&Value)->Result<Value>{
    #[cfg(windows)] {
        let bytes=field(args,"content")?.as_bytes();let id=uuid::Uuid::parse_str(field(args,"request_id")?)?;
        let path=super::secure_windows::write_evidence(config,&id,bytes)?;
        let stamp=digest(bytes);Ok(json!({"host":"local","path":path,"sha256":stamp.trim_start_matches("sha256:"),"digest":stamp,"bytes":bytes.len()}))
    }
    #[cfg(not(windows))] {let _=(config,args);anyhow::bail!("managed evidence requires Windows file safety")}
}
fn decoded(args:&Value)->Result<Vec<u8>>{
    let s=field(args,"content")?;anyhow::ensure!(s.len()<=2*LIMIT,"write content too large");
    let bytes=match args["encoding"].as_str().unwrap_or("utf8"){"utf8"=>s.as_bytes().to_vec(),"base64"=>base64::engine::general_purpose::STANDARD.decode(s)?,_=>anyhow::bail!("unsupported encoding")};
    anyhow::ensure!(bytes.len()<=LIMIT,"write exceeds 1 MiB");Ok(bytes)
}
fn write_file(config:&Config,args:&Value)->Result<Value>{
    #[cfg(windows)] {
        let bytes=decoded(args)?;
        super::secure_windows::write(config,field(args,"path")?,field(args,"expected_digest")?,&bytes)?;
        return Ok(json!({"host":"local","path":args["path"],"digest":digest(&bytes),"bytes":bytes.len()}));
    }
    #[cfg(not(windows))] {
    let path=scoped(config,field(args,"path")?,true)?;let bytes=decoded(args)?;
    let temp=path.with_file_name(format!(".velella-write-{}",uuid::Uuid::new_v4()));
    let result=(||->Result<()>{let mut f=fs::OpenOptions::new().create_new(true).write(true).open(&temp)?;f.write_all(&bytes)?;f.sync_all()?;
        validate(config,"local_write_file",args)?;
        if args["expected_digest"]=="absent"{fs::hard_link(&temp,&path)?;fs::remove_file(&temp)?;}else{fs::rename(&temp,&path)?;}Ok(())})();
    if result.is_err(){let _=fs::remove_file(&temp);}result?;
    Ok(json!({"host":"local","path":args["path"],"digest":digest(&bytes),"bytes":bytes.len()}))
    }
}
fn read_command_output(stream:impl Read,flag:&std::sync::atomic::AtomicBool)->Vec<u8>{
    let mut bytes=Vec::new();
    let result=stream.take((LIMIT/2+1)as u64).read_to_end(&mut bytes);
    if result.is_err()||bytes.len()>LIMIT/2{flag.store(true,std::sync::atomic::Ordering::SeqCst);}
    bytes.truncate(LIMIT/2);bytes
}
fn run_command(config:&Config,args:&Value)->Result<Value>{
    let mut command=crate::process::command(field(args,"program")?);
    command.args(args["args"].as_array().unwrap().iter().map(|a|a.as_str().unwrap())).current_dir(scoped(config,field(args,"cwd")?,false)?).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for(name,_)in std::env::vars_os(){if name.to_string_lossy().starts_with("VELELLA_"){command.env_remove(name);}}
    let mut child=command.spawn()?;let stdout=child.stdout.take().unwrap();let stderr=child.stderr.take().unwrap();
    let overflow=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    fn reader(stream:impl Read+Send+'static,flag:std::sync::Arc<std::sync::atomic::AtomicBool>)->std::thread::JoinHandle<Vec<u8>>{std::thread::spawn(move||read_command_output(stream,&flag))}
    let out=reader(stdout,overflow.clone());let err=reader(stderr,overflow.clone());let deadline=Instant::now()+Duration::from_secs(30);
    let mut uncertain=false;
    let exit=loop{match child.try_wait(){Ok(Some(status))=>{let code=status.code();uncertain|=code.is_none();break code},Ok(None)=>{},Err(_)=>{uncertain=true;let _=child.kill();let _=child.wait();break None}}
        if Instant::now()>=deadline||overflow.load(std::sync::atomic::Ordering::SeqCst){uncertain=true;let _=child.kill();let _=child.wait();break None}std::thread::sleep(Duration::from_millis(25));};
    // Descendants may retain pipes after the direct process exits. Never block
    // the tool on joining them; unfinished output is explicitly uncertain.
    let collect=|h:std::thread::JoinHandle<Vec<u8>>|if h.is_finished(){h.join().unwrap_or_default()}else{Vec::new()};
    std::thread::sleep(Duration::from_millis(20));
    uncertain|=!out.is_finished()||!err.is_finished()||overflow.load(std::sync::atomic::Ordering::SeqCst);
    Ok(json!({"host":"local","exit_code":exit,"stdout":String::from_utf8_lossy(&collect(out)),"stderr":String::from_utf8_lossy(&collect(err)),"uncertain":uncertain}))
}

#[cfg(test)]mod tests{
    use super::*;
    #[cfg(windows)]
    #[test]fn readonly_member_can_only_create_approved_new_evidence(){
        let mut c=fixture();c.project_root=dunce::canonicalize(&c.project_root).unwrap();c.collaboration_data=Some(c.directory.clone());
        assert!(call(&c,"local_write_file",&json!({})).is_err());assert!(call(&c,"local_run_command",&json!({})).is_err());
        let id=uuid::Uuid::new_v4().to_string();let copy=c.clone();let request=id.clone();
        let task=std::thread::spawn(move||call(&copy,"local_write_evidence",&json!({"request_id":request,"content":"checked source without modification"})));
        let deadline=Instant::now()+Duration::from_secs(5);
        let row=loop{let s=status(&c).unwrap();if let Some(row)=s["operations"].as_array().unwrap().first(){break row.clone()}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));};
        assert!(!c.project_root.join(".velella").exists(),"no evidence directory before approval");
        decide(&c,&id,row["digest"].as_str().unwrap(),true).unwrap();let result=task.join().unwrap().unwrap();
        assert_eq!(result["path"],format!(".velella/evidence/{id}.log"));assert_eq!(fs::read(c.project_root.join(result["path"].as_str().unwrap())).unwrap(),b"checked source without modification");
        assert!(call(&c,"local_write_evidence",&json!({"request_id":id,"content":"replay"})).is_err());
        assert!(call(&c,"local_write_evidence",&json!({"request_id":uuid::Uuid::new_v4().to_string(),"content":"x","path":"source.txt"})).is_err());
        fs::remove_dir_all(c.directory).unwrap();
    }
    #[cfg(not(windows))]
    #[test]fn remote_orchestration_local_tools_fail_closed_on_unsupported_host(){
        let mut c=fixture();c.collaboration_data=Some(c.directory.clone());c.collaboration_writes=true;
        fs::write(c.project_root.join("seed"),b"fixture").unwrap();
        for name in ["local_read_file","local_list_files","local_write_file","local_run_command"] {
            let error=call(&c,name,&json!({"path":"seed"})).unwrap_err().to_string();
            assert!(error.contains("supported Windows file safety implementation"),"{name}: {error}");
        }
        fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]fn command_output_read_error_marks_outcome_uncertain(){
        struct Broken;
        impl Read for Broken{fn read(&mut self,_:&mut [u8])->std::io::Result<usize>{Err(std::io::Error::other("fixture read failed"))}}
        let flag=std::sync::atomic::AtomicBool::new(false);
        assert!(read_command_output(Broken,&flag).is_empty());
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    }
    fn fixture()->Config{let directory=std::env::temp_dir().join(format!("velella-hybrid-tools-{}",uuid::Uuid::new_v4()));fs::create_dir(&directory).unwrap();let root=directory.join("project");fs::create_dir(&root).unwrap();Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root,directory,profile:json!({"host":"agent"}),profile_fingerprint:"fixture".into(),collaboration_data:None,collaboration_writes:false}}
    #[test]fn scope_and_private_metadata_are_rejected(){let c=fixture();for p in ["../secret","/etc/passwd","C:/Windows",".git/config",".velella/sessions","a/../../x","a\\b"]{assert!(scoped(&c,p,true).is_err(),"{p}");}fs::remove_dir_all(c.directory).unwrap();}
    #[test]fn directory_links_cannot_escape_the_selected_project(){
        let c=fixture();let outside=c.directory.join("outside");fs::create_dir(&outside).unwrap();fs::write(outside.join("secret.txt"),"OUTSIDE").unwrap();let link=c.project_root.join("link");
        #[cfg(windows)]{let result=crate::process::command("cmd.exe").args(["/d","/c","mklink","/J"]).arg(&link).arg(&outside).output().unwrap();assert!(result.status.success(),"cannot create isolated junction fixture");}
        #[cfg(unix)]{std::os::unix::fs::symlink(&outside,&link).unwrap();}
        assert!(call(&c,"local_read_file",&json!({"path":"link/secret.txt"})).is_err());assert!(scoped(&c,"link/new.txt",true).is_err());
        #[cfg(windows)]fs::remove_dir(&link).unwrap();
        #[cfg(unix)]fs::remove_file(&link).unwrap();
        assert_eq!(fs::read(outside.join("secret.txt")).unwrap(),b"OUTSIDE");fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]fn approved_local_write_and_command_run_once(){
        let c=fixture();
        let approve=|name:&'static str,args:Value|{
            let copy=c.clone();let id=args["request_id"].as_str().unwrap().to_owned();let task=std::thread::spawn(move||call(&copy,name,&args));let deadline=Instant::now()+Duration::from_secs(5);
            let row=loop{if let Some(r)=status(&c).unwrap()["operations"].as_array().unwrap().iter().find(|r|r["id"]==id){break r.clone()}assert!(Instant::now()<deadline,"pending operation did not appear");std::thread::sleep(Duration::from_millis(10));};
            decide(&c,&id,row["digest"].as_str().unwrap(),true).unwrap();task.join().unwrap().unwrap()
        };
        approve("local_write_file",json!({"request_id":"write-ok","path":"written.txt","content":"APPROVED","expected_digest":"absent"}));assert_eq!(fs::read(c.project_root.join("written.txt")).unwrap(),b"APPROVED");
        let args=if cfg!(windows){json!({"request_id":"command-once","program":"cmd.exe","args":["/d","/c","echo x>>counter.txt"],"cwd":"."})}else{json!({"request_id":"command-once","program":"/bin/sh","args":["-c","printf x >> counter.txt"],"cwd":"."})};
        approve("local_run_command",args.clone());assert!(call(&c,"local_run_command",&args).is_err());assert_eq!(fs::read_to_string(c.project_root.join("counter.txt")).unwrap().trim(),"x");fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]fn approved_write_checks_conflict_and_never_replays(){let c=fixture();let args=json!({"request_id":"write1","path":"a.txt","content":"new","expected_digest":"absent"});let copy=c.clone();let a=args.clone();let task=std::thread::spawn(move||call(&copy,"local_write_file",&a));
        let pending=loop{let s=status(&c).unwrap();if let Some(row)=s["operations"].as_array().unwrap().first(){break row.clone()}std::thread::sleep(Duration::from_millis(10));};
        fs::write(c.project_root.join("a.txt"),"external").unwrap();decide(&c,"write1",pending["digest"].as_str().unwrap(),true).unwrap();assert!(task.join().unwrap().is_err());assert_eq!(fs::read(c.project_root.join("a.txt")).unwrap(),b"external");assert!(call(&c,"local_write_file",&args).is_err());fs::remove_dir_all(c.directory).unwrap();}
    #[test]fn rejection_and_approval_are_bound_to_digest(){let c=fixture();let copy=c.clone();let task=std::thread::spawn(move||call(&copy,"local_write_file",&json!({"request_id":"deny1","path":"a","content":"x","expected_digest":"absent"})));let row=loop{let s=status(&c).unwrap();if let Some(r)=s["operations"].as_array().unwrap().first(){break r.clone()}std::thread::sleep(Duration::from_millis(10));};assert!(decide(&c,"deny1","wrong",true).is_err());decide(&c,"deny1",row["digest"].as_str().unwrap(),false).unwrap();assert!(task.join().unwrap().is_err());assert!(!c.project_root.join("a").exists());fs::remove_dir_all(c.directory).unwrap();}
    #[cfg(windows)]
    #[test]fn concurrent_approved_requests_do_not_overwrite_each_other(){
        let mut c=fixture();c.project_root=dunce::canonicalize(&c.project_root).unwrap();
        fs::write(c.project_root.join("race"),b"original").unwrap();let mut tasks=Vec::new();
        for (id,content) in [("first","one"),("second","two")]{let copy=c.clone();tasks.push(std::thread::spawn(move||call(&copy,"local_write_file",&json!({"request_id":id,"path":"race","content":content,"expected_digest":digest(b"original")}))));}
        let deadline=Instant::now()+Duration::from_secs(5);
        let rows=loop{let s=status(&c).unwrap();let rows=s["operations"].as_array().unwrap();if rows.len()==2{break rows.clone()}assert!(Instant::now()<deadline,"both operations must await approval");std::thread::sleep(Duration::from_millis(10));};
        for row in rows{decide(&c,row["id"].as_str().unwrap(),row["digest"].as_str().unwrap(),true).unwrap();}
        let results:Vec<_>=tasks.into_iter().map(|t|t.join().unwrap().is_ok()).collect();assert_eq!(results.iter().filter(|x|**x).count(),1,"only one approved write can use the original digest");
        let s=status(&c).unwrap();let states:Vec<_>=s["operations"].as_array().unwrap().iter().map(|r|r["state"].as_str().unwrap()).collect();assert!(states.contains(&"done")&&states.contains(&"failed"));
        let bytes=fs::read(c.project_root.join("race")).unwrap();assert!(bytes==b"one"||bytes==b"two");fs::remove_dir_all(c.directory).unwrap();
    }
}
