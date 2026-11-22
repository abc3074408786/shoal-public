//! Invocation-local observers. They never change provider credentials or policy.
use super::*;
use std::{collections::HashSet,io::BufRead,path::PathBuf};

pub(super) fn env(desc:&Descriptor,key:&str)->Option<String>{desc.spawn.env.get(key).cloned().or_else(||std::env::var(key).ok()).filter(|s|!s.is_empty())}
fn home(desc:&Descriptor)->Result<PathBuf>{
    let keys=if cfg!(windows){["USERPROFILE","HOME"]}else{["HOME","USERPROFILE"]};
    keys.into_iter().find_map(|key|env(desc,key)).map(PathBuf::from).context("native account home is unavailable")
}
fn json(path:&Path)->Result<Value>{
    anyhow::ensure!(fs::metadata(path)?.len()<=1024*1024,"native metadata exceeds bounds");
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn same_cwd(desc:&Descriptor,value:&str)->bool{dunce::canonicalize(value).ok().zip(desc.spawn.cwd.as_deref().and_then(|s|dunce::canonicalize(s).ok())).is_some_and(|(a,b)|a==b)}
fn option<'a>(args:&'a [String],flag:&str)->Option<&'a str>{args.iter().enumerate().find_map(|(i,s)|if s==flag{args.get(i+1).map(String::as_str)}else{s.strip_prefix(&format!("{flag}="))})}
fn remember(desc:&mut Descriptor,key:&str){if let Some(value)=env(desc,key){desc.spawn.env.insert(key.into(),value);}}

pub(super) fn attach(desc:&mut Descriptor,runtime:&Path)->Result<()> {
    let provider=provider(desc).to_owned();
    // Native homes/config overrides are part of the saved invocation, not the
    // environment of whichever desktop happens to recover it later.
    for key in ["HOME","USERPROFILE","GEMINI_CLI_HOME","GEMINI_CLI_SYSTEM_DEFAULTS_PATH","SANDBOX","OPENCODE_DB","XDG_DATA_HOME","XDG_CONFIG_HOME","XDG_STATE_HOME","OPENCODE_CONFIG","OPENCODE_CONFIG_DIR","OPENCODE_CONFIG_CONTENT","OMP_PROFILE","PI_PROFILE","PI_CONFIG_DIR","PI_CODING_AGENT_DIR","PI_CODING_AGENT_SESSION_DIR","KIMI_CODE_HOME"]{remember(desc,key);}
    if provider=="kimi" {
        let root=env(desc,"KIMI_CODE_HOME").map(PathBuf::from).unwrap_or(home(desc)?.join(".kimi-code"));
        anyhow::ensure!(root.is_absolute(),"Kimi data root must be absolute");
        desc.spawn.env.insert("KIMI_CODE_HOME".into(),root.to_string_lossy().into_owned());
        return Ok(())
    }
    if desc.spawn.env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT"){return Ok(())}
    match provider.as_str(){
        "gemini"=>{
            let path=desc.directory.join("native-restore-gemini.json");
            if desc.spawn.env.get("GEMINI_CLI_SYSTEM_SETTINGS_PATH").map(PathBuf::from)==Some(path.clone()){return Ok(())}
            let source=env(desc,"GEMINI_CLI_SYSTEM_SETTINGS_PATH").map(PathBuf::from).unwrap_or_else(||PathBuf::from(if cfg!(windows){"C:/ProgramData/gemini-cli/settings.json"}else{"/etc/gemini-cli/settings.json"}));
            let mut settings=crate::collaboration::providers::restore_gemini_settings(&source)?;
            let hooks=settings.as_object_mut().context("Gemini settings object required")?.entry("hooks").or_insert_with(||json!({})).as_object_mut().context("Gemini hooks object required")?;
            for (event,name) in [("SessionStart","SessionStart"),("BeforeAgent","UserPromptSubmit"),("AfterAgent","Stop")]{
                let command=format!("\"{}\" native-restore-hook-event {name}",runtime.display());
                hooks.entry(event.to_string()).or_insert_with(||json!([])).as_array_mut().context("Gemini hook list required")?.push(json!({"hooks":[{"type":"command","command":command,"timeout":3000}]}));
            }
            ipc::atomic_json(&path,&settings)?;desc.spawn.env.insert("GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(),path.to_string_lossy().into_owned());
        },
        "opencode"=>{
            let path=desc.directory.join("native-restore-opencode.mjs");fs::write(&path,include_str!("opencode.mjs"))?;
            let url=reqwest::Url::from_file_path(path).map_err(|_|anyhow::anyhow!("absolute OpenCode observer path required"))?.to_string();
            let mut config:Value=serde_json::from_str(desc.spawn.env.get("OPENCODE_CONFIG_CONTENT").map(String::as_str).unwrap_or("{}"))?;
            let plugins=config.as_object_mut().context("OpenCode config object required")?.entry("plugin").or_insert_with(||json!([])).as_array_mut().context("OpenCode plugins list required")?;
            if !plugins.iter().any(|p|p.as_str()==Some(&url)){plugins.push(json!(url));}
            desc.spawn.env.insert("OPENCODE_CONFIG_CONTENT".into(),serde_json::to_string(&config)?);
        },
        "omp"=>{
            let path=desc.directory.join("native-restore-omp.mjs");fs::write(&path,include_str!("omp.mjs"))?;
            if !desc.spawn.args.iter().any(|arg|arg==path.to_string_lossy().as_ref()){desc.spawn.args.extend(["-e".into(),path.to_string_lossy().into_owned()]);}
        },_=>{}
    }Ok(())
}

pub(super) fn candidates(desc:&Descriptor)->Result<Vec<Value>>{
    match provider(desc){"kimi"=>{let mut rows=kimi_candidates(desc)?;rows.truncate(100);Ok(rows)},"opencode"=>opencode_candidates(desc),"omp"=>omp_candidates(desc),"gemini"=>gemini_candidates(desc),_=>Ok(vec![])}
}
fn kimi_candidates(desc:&Descriptor)->Result<Vec<Value>>{
    let root=env(desc,"KIMI_CODE_HOME").map(PathBuf::from).unwrap_or(home(desc)?.join(".kimi-code"));
    let path=root.join("session_index.jsonl");if !path.exists(){return Ok(vec![])}
    let file=fs::File::open(path)?;anyhow::ensure!(file.metadata()?.len()<=32*1024*1024,"Kimi metadata index exceeds observation bounds");
    let mut bytes=Vec::new();file.take(32*1024*1024).read_to_end(&mut bytes)?;
    let mut seen=HashSet::new();let mut rows=Vec::new();
    let lines=bytes.split(|b|*b==b'\n').collect::<Vec<_>>();
    for line in lines.into_iter().rev(){
        let Ok(value)=serde_json::from_slice::<Value>(line)else{continue};
        let Some(id)=value["sessionId"].as_str().filter(|id|valid("kimi",id))else{continue};
        if value["workDir"].as_str().is_some_and(|cwd|same_cwd(desc,cwd))&&seen.insert(id.to_string()){rows.push(json!({"id":id,"title":"Kimi","created_at":0}));}
    }Ok(rows)
}
fn opencode_candidates(desc:&Descriptor)->Result<Vec<Value>>{
    let root=env(desc,"XDG_DATA_HOME").map(PathBuf::from).unwrap_or(home(desc)?.join(".local/share"));
    let database=env(desc,"OPENCODE_DB").map(PathBuf::from).unwrap_or(root.join("opencode/opencode.db"));if !database.exists(){return Ok(vec![])}
    let db=rusqlite::Connection::open_with_flags(database,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;db.busy_timeout(std::time::Duration::from_secs(1))?;
    let mut query=db.prepare("SELECT id,title,time_created,directory FROM session WHERE parent_id IS NULL ORDER BY time_created DESC LIMIT 1000")?;
    let rows=query.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?)))?;
    let mut result=Vec::new();for row in rows{let(id,title,at,cwd)=row?;if valid("opencode",&id)&&same_cwd(desc,&cwd){result.push(json!({"id":id,"title":title.chars().take(256).collect::<String>(),"created_at":at}));if result.len()>=100{break}}}Ok(result)
}
fn files(root:&Path,depth:usize,budget:&mut usize,result:&mut Vec<PathBuf>)->Result<()>{
    if !root.is_dir(){return Ok(())}for entry in fs::read_dir(root)?{if *budget==0{break}*budget-=1;let entry=entry?;let kind=entry.file_type()?;if kind.is_file(){result.push(entry.path())}else if kind.is_dir()&&depth>0{files(&entry.path(),depth-1,budget,result)?;}}Ok(())
}
fn header(path:&Path)->Result<Value>{
    let mut line=String::new();std::io::BufReader::new(fs::File::open(path)?).take(64*1024).read_line(&mut line)?;Ok(serde_json::from_str(&line)?)
}
fn omp_root(desc:&Descriptor)->Result<PathBuf>{
    if let Some(root)=option(&desc.spawn.args,"--session-dir").map(String::from).or_else(||env(desc,"PI_CODING_AGENT_SESSION_DIR")){return Ok(PathBuf::from(root))}
    if let Some(root)=env(desc,"PI_CODING_AGENT_DIR"){return Ok(PathBuf::from(root).join("sessions"))}
    let root=home(desc)?.join(env(desc,"PI_CONFIG_DIR").unwrap_or_else(||".omp".into()));
    let profile=option(&desc.spawn.args,"--profile").map(String::from).or_else(||env(desc,"OMP_PROFILE")).or_else(||env(desc,"PI_PROFILE")).filter(|s|s!="default");
    Ok(if let Some(profile)=profile{anyhow::ensure!(profile.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)),"invalid OMP profile");root.join("profiles").join(profile)}else{root}.join("agent/sessions"))
}
fn omp_candidates(desc:&Descriptor)->Result<Vec<Value>>{
    let mut paths=vec![];files(&omp_root(desc)?,1,&mut 2000,&mut paths)?;let mut rows=vec![];
    for path in paths.into_iter().filter(|p|p.extension().is_some_and(|e|e=="jsonl")){
        let Ok(value)=header(&path)else{continue};let Some(id)=value["id"].as_str().filter(|id|valid("omp",id))else{continue};
        if value["type"]=="session"&&value["cwd"].as_str().is_some_and(|s|same_cwd(desc,s)){rows.push(json!({"id":id,"title":"OMP","created_at":0}));if rows.len()>=100{break}}
    }Ok(rows)
}
fn gemini_candidates(desc:&Descriptor)->Result<Vec<Value>>{
    // Gemini 0.67 Storage.getGlobalGeminiDir() appends .gemini to its
    // homedir() helper, which resolves GEMINI_CLI_HOME. The entrypoint's
    // memory-setting lookup uses a different path and is not the history root.
    let base=env(desc,"GEMINI_CLI_HOME").map(PathBuf::from).unwrap_or(home(desc)?);
    let root=if env(desc,"SANDBOX").as_deref()==Some("sandbox-exec"){base.join(".cache/.gemini")}else{base.join(".gemini")};
    let registry=root.join("projects.json");if !registry.exists(){return Ok(vec![])}let registry=json(&registry)?;
    let Some(projects)=registry["projects"].as_object()else{return Ok(vec![])};
    let Some(id)=projects.iter().find_map(|(cwd,id)|same_cwd(desc,cwd).then(||id.as_str()).flatten())else{return Ok(vec![])};
    anyhow::ensure!(id.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b)),"invalid Gemini project identifier");
    let mut paths=vec![];files(&root.join("tmp").join(id).join("chats"),0,&mut 1000,&mut paths)?;let mut rows=vec![];
    for path in paths{
        let value=if path.extension().is_some_and(|e|e=="jsonl"){header(&path)}else{json(&path)};let Ok(value)=value else{continue};
        if value["kind"]=="subagent"{continue}if let Some(id)=value["sessionId"].as_str().filter(|id|valid("gemini",id)){rows.push(json!({"id":id,"title":"Gemini CLI","created_at":0}));if rows.len()>=100{break}}
    }Ok(rows)
}

pub(super) fn poll_kimi(desc:&Descriptor,owns:impl Fn(u32)->bool,input_sequence:u64)->Result<Option<String>>{
    if provider(desc)!="kimi"||!supported(desc){return Ok(None)}
    let root=env(desc,"KIMI_CODE_HOME").map(PathBuf::from).unwrap_or(home(desc)?.join(".kimi-code"));let locks=root.join("locks/sessions");
    if !locks.is_dir(){return Ok(None)}let mut found=Vec::new();
    for (index,entry) in fs::read_dir(locks)?.enumerate(){if index>=1000{return Ok(None)}let entry=entry?;if !entry.file_type()?.is_file(){continue}let path=entry.path();if path.extension().is_none_or(|e|e!="lock"){continue}
        let Some(id)=path.file_stem().and_then(|s|s.to_str()).filter(|id|valid("kimi",id))else{continue};
        let Ok(value)=json(&path)else{continue};if value["version"]==1&&value["pid"].as_u64().and_then(|p|u32::try_from(p).ok()).is_some_and(&owns){found.push(id.to_string())}
    }
    let metadata=fs::metadata(root.join("session_index.jsonl"))?;let stamp=json!({"bytes":metadata.len(),"modified":metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_nanos().to_string()});let cache=desc.directory.join("terminal-kimi-index.json");
    if found.len()==1&&!desc.directory.join("terminal-native-ambiguous.json").exists()&&json(&cache).ok().as_ref()==Some(&stamp){if let Some(record)=observation(desc){if record.native_id==found[0]&&record.generation==desc.spawn.env.get(GENERATION).cloned()&&(!desc.directory.join("terminal-auto-recovery.json").exists()||input_sequence==0){return Ok(Some(record.native_id))}}}
    let had_owned=!found.is_empty();let rows=kimi_candidates(desc)?;found.retain(|id|rows.iter().any(|row|row["id"]==*id));
    if found.len()>1{ipc::atomic_json(&desc.directory.join("terminal-native-ambiguous.json"),&json!({"ambiguous":true}))?;return Ok(None)}
    if found.len()!=1{if had_owned{ipc::atomic_json(&desc.directory.join("terminal-native-ambiguous.json"),&json!({"unverified":true}))?;}return Ok(None)}let id=found.pop().unwrap();
    if observation(desc).is_some_and(|record|record.native_id==id&&record.generation==desc.spawn.env.get(GENERATION).cloned())&&!desc.directory.join("terminal-native-ambiguous.json").exists()&&(!desc.directory.join("terminal-auto-recovery.json").exists()||input_sequence==0){return Ok(Some(id))}
    observe(desc,&json!({"session_id":id,"cwd":desc.spawn.cwd,"hook_event_name":if input_sequence>0{"UserPromptSubmit"}else{"SessionStart"}}))?;ipc::atomic_json(&cache,&stamp)?;Ok(Some(id))
}
