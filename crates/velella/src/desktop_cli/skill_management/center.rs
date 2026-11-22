//! Skill Center storage. Package bytes, path checks and locks share native management.
use super::*;

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().with_context(||format!("missing {key}"))
}
fn read_json(path: &Path) -> Result<Value> {
    ancestors(path)?;
    let mut bytes=Vec::new(); fs::File::open(path)?.take(8*1024*1024+1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=8*1024*1024,"metadata exceeds 8 MiB");
    Ok(serde_json::from_slice(&bytes)?)
}
fn save(path: &Path, value: &Value) -> Result<()> {
    ancestors(path)?;
    let temp=path.with_extension(format!("{}.tmp",uuid::Uuid::now_v7()));
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?; f.sync_all()?;
    fs::rename(temp,path)?; Ok(())
}
fn empty() -> Value { json!({"version":1,"skills":[],"installations":[],"backups":[]}) }
fn ledger(root:&Path) -> Result<Value> {
    let file=root.join("state.json");
    if !file.exists() {return Ok(empty())}
    let value=read_json(&file)?;
    anyhow::ensure!(value["version"]==1 && ["skills","installations","backups"].iter().all(|k|value[*k].is_array()),"invalid Skill Center ledger");
    Ok(value)
}
fn root_for(library:&Path, r:&Value) -> Result<PathBuf> {
    match r["scope"].as_str().unwrap_or("global") {
        "global"=>Ok(library.to_owned()),
        "project"=>{let project=absolute(Path::new(field(r,"project")?))?;anyhow::ensure!(project.is_dir(),"project must be a directory");Ok(project.join(".velella/skills"))},
        _=>anyhow::bail!("scope must be global or project"),
    }
}
fn lock(root:&Path)->Result<fs::File> {
    ancestors(root)?; fs::create_dir_all(root)?;
    Store{base:root.into(),skills:root.join("packages"),backups:root.join("history")}.lock()
}
fn ready(root:&Path)->Result<()> {
    ancestors(root)?;
    anyhow::ensure!(!root.join("pending.json").exists(),"interrupted operation: inspect history and run recover before another write"); Ok(())
}
fn snapshot(base:&Path, p:&Package)->Result<PathBuf> {
    ancestors(base)?; fs::create_dir_all(base)?;
    let target=base.join(format!("velella-skill-stage-{}",uuid::Uuid::now_v7()));fs::create_dir(&target)?;
    for entry in &p.files {
        let path=target.join(&entry.relative);fs::create_dir_all(path.parent().unwrap())?;
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        file.write_all(&entry.bytes)?;file.sync_all()?;fs::set_permissions(path,entry.permissions.clone())?;
    }
    check_digest(&package(&target)?,&p.digest)?;Ok(target)
}
fn current(path:&Path)->Result<Value> {
    ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(_)=>Ok(json!(package(path)?.digest)),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(Value::Null),
        Err(e)=>Err(e.into()),
    }
}
fn replace_row(state:&mut Value,kind:&str,key:&str,id:&Value,row:Value) {
    let rows=state[kind].as_array_mut().unwrap();rows.retain(|v|v[key]!=*id);
    if !row.is_null(){rows.push(row)}
}
// The journal is durable before either rename. Recovery observes exact before/after
// digests; it never retries a provider launch or silently overwrites changed files.
fn recover(root:&Path)->Result<Value> {
    let file=root.join("pending.json");let tx=read_json(&file)?;
    let dest=Path::new(field(&tx,"destination")?);let backup=Path::new(field(&tx,"backup")?);
    ancestors(dest)?;ancestors(backup)?;
    let now=current(dest)?;
    if now!=tx["after"] {
        if now==tx["before"] && !now.is_null() {
            Store::vacant(backup)?;fs::create_dir_all(backup.parent().unwrap())?;fs::rename(dest,backup)?;
        } else {anyhow::ensure!(now.is_null() && current(backup)?==tx["before"],"recovery conflict: destination changed; retained journal and backup");}
        if let Some(stage)=tx["stage"].as_str() {
            let stage=Path::new(stage);anyhow::ensure!(current(stage)?==tx["after"],"staged package changed; journal retained");
            ancestors(dest)?;Store::vacant(dest)?;fs::create_dir_all(dest.parent().unwrap())?;fs::rename(stage,dest)?;
        }
    }
    anyhow::ensure!(current(dest)?==tx["after"],"publication digest mismatch; journal retained");
    save(&root.join("state.json"),&tx["state"])?;fs::remove_file(file)?;
    Ok(json!({"recovered":true,"destination":dest}))
}
fn transaction(root:&Path, mut state:Value, kind:&str, destination:&Path, before:Value, source:Option<&Path>, before_row:Value)->Result<Value> {
    anyhow::ensure!(current(destination)?==before,"destination changed; preview again");
    let base=destination.parent().and_then(Path::parent).context("invalid destination")?;
    ancestors(base)?;fs::create_dir_all(base)?;
    let _target_lock=if base!=root {Some(Store{base:base.into(),skills:destination.parent().unwrap().into(),backups:base.join("velella-skill-backups")}.lock()?)}else{None};
    let after=source.map(package).transpose()?.map(|p|p.digest);
    if before==json!(after) {
        // Equal bytes do not transfer ownership of a user's existing directory.
        if kind=="distribution"&&before_row.is_null(){return Ok(json!({"unchanged":true,"unmanaged_unchanged":true}));}
        save(&root.join("state.json"),&state)?;return Ok(json!({"unchanged":true}));
    }
    let stage=source.map(|path|snapshot(base,&package(path)?)).transpose()?;
    let id=uuid::Uuid::now_v7().to_string();let backup=base.join("velella-skill-backups").join(&id).join("package");
    let entry=json!({"id":id,"kind":kind,"destination":destination,"package":backup,"before":before,"after":after,"row":before_row,"restored":false,"created_at":chrono::Utc::now().to_rfc3339()});
    state["backups"].as_array_mut().unwrap().push(entry.clone());
    anyhow::ensure!(current(destination)?==before,"destination changed during staging; no publication");
    save(&root.join("pending.json"),&json!({"destination":destination,"backup":backup,"stage":stage,"before":before,"after":after,"state":state}))?;
    recover(root)?;Ok(json!({"backup":entry,"existing_sessions":"unchanged"}))
}
fn skill<'a>(state:&'a Value,name:&str)->Result<&'a Value> {
    check_name(name)?;state["skills"].as_array().unwrap().iter().find(|s|s["name"]==name).context("skill not found")
}
fn dependencies(body:&str)->Value {
    let front=body.strip_prefix("---\r\n").or_else(||body.strip_prefix("---\n")).and_then(|s|s.split("\n---").next());
    let parsed=front.and_then(|s|serde_yaml::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
    json!({"allowed_tools":parsed["allowed-tools"],"requires":parsed["requires"],"hooks":parsed["hooks"],"metadata":parsed["metadata"],"compatibility":parsed["compatibility"],"status":"unverified","note":"Declarations only; copying a skill does not provide tools, hooks or plugins."})
}
fn describe(p:&Package)->Result<Value> {
    let body=String::from_utf8(p.files.iter().find(|e|e.relative==Path::new("SKILL.md")).context("SKILL.md missing")?.bytes.clone()).context("SKILL.md must be UTF-8")?;
    Ok(json!({"path":p.root,"digest":p.digest,"bytes":p.bytes,"content":body,"dependencies":dependencies(&body),"files":p.files.iter().map(|e|json!({"path":e.relative,"bytes":e.bytes.len(),"sha256":format!("{:x}",Sha256::digest(&e.bytes))})).collect::<Vec<_>>()}))
}
fn dependency_check(p:&Package,agent:&str)->Result<Value> {
    let desc=describe(p)?;let requires=&desc["dependencies"]["requires"];let mut missing=Vec::new();
    if let Some(agents)=requires["agents"].as_array(){if !agents.iter().any(|a|a==agent){missing.push(format!("skill requires another Agent: {}",requires["agents"]));}}
    for key in ["commands","bins"] {
        if let Some(commands)=requires[key].as_array(){for command in commands {
            let command=command.as_str().context("required command must be a string")?;
            anyhow::ensure!(!command.starts_with('-')&&!command.is_empty()&&command.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'-'|b'_'|b'.')),"invalid required command name");
            #[cfg(windows)] let available=crate::process::command("where.exe").arg(command).output().is_ok_and(|o|o.status.success());
            #[cfg(not(windows))] let available=std::env::var_os("PATH").is_some_and(|v|std::env::split_paths(&v).any(|p|p.join(command).is_file()));
            if !available{missing.push(format!("missing command: {command}"));}
        }}
    }
    Ok(json!({"missing":missing,"status":if missing.is_empty(){"unverified"}else{"dependency_missing"},"declarations":desc["dependencies"]}))
}
fn git_bytes(repo:&Path,args:&[&str])->Result<Vec<u8>> {
    use std::{process::Stdio,time::{Duration,Instant}};
    let mut child=crate::process::command("git").arg("-c").arg("core.hooksPath=").arg("-c").arg("protocol.ext.allow=never").arg("-C").arg(repo).args(args).env("GIT_TERMINAL_PROMPT","0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    #[cfg(windows)]
    let group=match crate::process::ChildGroup::attach(child.id()){Ok(g)=>g,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e)}};
    fn drain(mut stream:impl Read)->std::io::Result<Vec<u8>> {let mut kept=Vec::new();let mut buf=[0;8192];loop{let n=stream.read(&mut buf)?;if n==0{break}let keep=n.min((33*1024*1024_usize).saturating_sub(kept.len()));kept.extend_from_slice(&buf[..keep]);}Ok(kept)}
    let stdout=child.stdout.take().unwrap();let stderr=child.stderr.take().unwrap();
    let out=std::thread::spawn(move||drain(stdout));let err=std::thread::spawn(move||drain(stderr));
    let until=Instant::now()+Duration::from_secs(45);
    let status=loop {if let Some(status)=child.try_wait()?{break status}if Instant::now()>=until{
        #[cfg(windows)] let _=group.kill();
        let _=child.kill();let _=child.wait();anyhow::bail!("Git request timed out; no automatic retry");
    }std::thread::sleep(Duration::from_millis(20));};
    let stdout=out.join().map_err(|_|anyhow::anyhow!("Git reader failed"))??;let stderr=err.join().map_err(|_|anyhow::anyhow!("Git reader failed"))??;
    anyhow::ensure!(status.success(),"git failed: {}",String::from_utf8_lossy(&stderr).chars().take(1000).collect::<String>());
    anyhow::ensure!(stdout.len()<33*1024*1024,"Git output exceeds package bounds");Ok(stdout)
}
fn git(repo:&Path,args:&[&str])->Result<String> {Ok(String::from_utf8(git_bytes(repo,args)?)?.trim().to_owned())}
fn export_git(repo:&Path,commit:&str,sub:&str,output:&Path)->Result<()> {
    // Read blobs directly. Checkout would run arbitrary repository smudge filters.
    let tree=git_bytes(repo,&["ls-tree","-r","-l","-z",commit,"--",if sub.is_empty(){"."}else{sub}])?;
    fs::create_dir(output)?;let mut count=0;let mut total=0_u64;
    for entry in tree.split(|b|*b==0).filter(|e|!e.is_empty()) {
        let entry=std::str::from_utf8(entry)?;let (meta,path)=entry.split_once('\t').context("invalid Git tree")?;let parts=meta.split_whitespace().collect::<Vec<_>>();
        anyhow::ensure!(parts.len()==4&&matches!(parts[0],"100644"|"100755")&&parts[1]=="blob","links and submodules are not supported in skills");
        count+=1;total+=parts[3].parse::<u64>()?;anyhow::ensure!(count<=2048&&total<=32*1024*1024,"Git skill exceeds package bounds");
        let relative=Path::new(path).strip_prefix(sub).context("Git path leaves selected subdirectory")?;
        anyhow::ensure!(relative.components().all(|c|matches!(c,std::path::Component::Normal(_)))&&!relative.as_os_str().is_empty(),"invalid Git resource path");
        let dest=output.join(relative);ancestors(&dest)?;fs::create_dir_all(dest.parent().unwrap())?;
        let bytes=git_bytes(repo,&["cat-file","blob",parts[2]])?;
        anyhow::ensure!(bytes.len() as u64==parts[3].parse::<u64>()?,"Git blob size changed");
        let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&dest)?;file.write_all(&bytes)?;
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&dest,fs::Permissions::from_mode(if parts[0]=="100755"{0o755}else{0o644}))?;}
    }Ok(())
}
fn preview(library:&Path,r:&Value)->Result<Value> {
    let id=uuid::Uuid::now_v7().to_string();let base=library.join("candidates").join(&id);ancestors(&base)?;fs::create_dir_all(&base)?;
    let source=&r["source"];let (path,provenance)=if source["kind"]=="git" {
        let url=field(source,"url")?;anyhow::ensure!(!url.starts_with('-')&&!url.contains(['\n','\r'])&&(url.starts_with("https://")||Path::new(url).is_absolute()),"use HTTPS or an absolute local Git repository");
        let reference=source["reference"].as_str().filter(|s|!s.is_empty()).unwrap_or("HEAD");
        anyhow::ensure!(!reference.starts_with('-')&&!reference.contains(['\n','\r']),"invalid Git reference");
        let sub=source["subdirectory"].as_str().unwrap_or("");
        anyhow::ensure!(Path::new(sub).components().all(|c|matches!(c,std::path::Component::Normal(_))),"subdirectory must stay inside repository");
        let repo=base.join("repository");fs::create_dir(&repo)?;git(&repo,&["init","--quiet"])?;
        git(&repo,&["fetch","--depth=1","--no-tags",url,reference])?;
        let commit=git(&repo,&["rev-parse","FETCH_HEAD^{commit}"])?;
        let exported=base.join("export");export_git(&repo,&commit,sub,&exported)?;
        (exported,json!({"kind":"git","url":url,"reference":reference,"subdirectory":sub,"commit":commit}))
    } else {let path=absolute(Path::new(field(source,"path")?))?;(path.clone(),json!({"kind":"local","path":path}))};
    let original=package(&path)?;let staged=snapshot(&base,&original)?;
    check_digest(&package(&path)?,&original.digest)?;
    let value=json!({"id":id,"path":staged,"digest":original.digest,"source":provenance});save(&base.join("candidate.json"),&value)?;
    let mut result=describe(&package(&staged)?)?;result["candidate"]=value;
    if let Some(name)=r["name"].as_str() {
        let root=root_for(library,r)?;let state=ledger(&root)?;
        if let Ok(previous)=skill(&state,name) {
            let old=package(&root.join("packages").join(name))?;
            check_digest(&old,field(previous,"digest")?)?;
            result["previous"]=describe(&old)?;
            let mut paths=std::collections::BTreeSet::new();for p in old.files.iter().chain(original.files.iter()){paths.insert(p.relative.clone());}
            result["changes"]=json!(paths.into_iter().filter_map(|path|{let a=old.files.iter().find(|e|e.relative==path);let b=original.files.iter().find(|e|e.relative==path);if a.map(|e|&e.bytes)==b.map(|e|&e.bytes){None}else{Some(json!({"path":path,"kind":if a.is_none(){"added"}else if b.is_none(){"removed"}else{"modified"}}))}}).collect::<Vec<_>>());
        }
    }
    Ok(result)
}
fn home()->Result<PathBuf> {Ok(PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).context("user home unavailable")?))}
fn route(r:&Value,name:&str,user:&Path)->Result<Option<PathBuf>> {
    let agent=field(r,"agent")?;
    anyhow::ensure!(["codex","claude-code","gemini","opencode","omp","kimi","zcode"].contains(&agent),"unsupported Agent");
    let project=r["scope"]=="project";
    let base=if project {absolute(Path::new(field(r,"project")?))?}else{absolute(user)?};
    let folder=match agent {"codex"=>if project{".agents/skills"}else{".codex/skills"},"claude-code"=>".claude/skills","gemini"=>".gemini/skills","kimi"=>".kimi-code/skills","opencode"=>if project{".opencode/skills"}else{".config/opencode/skills"},_=>return Ok(None)};
    if let Some(account)=r["account_home"].as_str().filter(|s|!s.is_empty()) {
        anyhow::ensure!(!project&&matches!(agent,"codex"|"claude-code"),"account directory applies only to global Codex/Claude distribution");
        return Ok(Some(absolute(Path::new(account))?.join("skills").join(name)))
    }
    let env_home=if project{None}else{match agent{"codex"=>std::env::var_os("CODEX_HOME"),"claude-code"=>std::env::var_os("CLAUDE_CONFIG_DIR"),"gemini"=>std::env::var_os("GEMINI_CLI_HOME"),"kimi"=>std::env::var_os("KIMI_CODE_HOME"),_=>None}};
    Ok(Some(if let Some(dir)=env_home {absolute(Path::new(&dir))?.join("skills").join(name)}else{base.join(folder).join(name)}))
}
fn distribution(root:&Path,state:&Value,r:&Value,user:&Path)->Result<Value> {
    let name=field(r,"name")?;let selected=skill(state,name)?;
    let alias=r["alias"].as_str().filter(|s|!s.is_empty()).unwrap_or(name);check_name(alias)?;
    let dest=route(r,alias,user)?;
    let mut value=json!({"agent":r["agent"],"name":name,"alias":alias,"source_digest":selected["digest"],"destination":dest,"availability":"unverified","effective_scope":r["scope"],"affected_agents":[r["agent"]],"source":root.join("packages").join(name)});
    let source=package(&root.join("packages").join(name))?;check_digest(&source,field(selected,"digest")?)?;
    value["dependencies"]=dependency_check(&source,field(r,"agent")?)?;
    if let Some(dest)=dest {
        let now=current(&dest)?;let path=json!(dest);
        let previous=state["installations"].as_array().unwrap().iter().find(|s|s["destination"]==path);
        value["current_digest"]=now.clone();value["managed"]=json!(previous.is_some());
        value["conflict"]=json!(!now.is_null()&&previous.is_none_or(|s|s["digest"]!=now));
        if dest.components().any(|c|c.as_os_str()==".agents") {value["affected_agents"]=json!(["codex","claude-code","gemini","opencode","omp","kimi"]);value["shared_directory"]=json!(true);}
        if r["scope"]=="global" {
            if let Some(project)=r["project"].as_str().filter(|s|!s.is_empty()) {
                let mut local=r.clone();local["scope"]=json!("project");local["project"]=json!(project);local.as_object_mut().unwrap().remove("account_home");
                if let Some(override_path)=route(&local,alias,user)? {value["project_override"]=json!({"path":override_path,"digest":current(&override_path)?});}
            }
        }
    }else{value["mode"]=json!("velella_context");}
    Ok(value)
}

pub(super) fn execute(library:&Path,r:&Value)->Result<Value> {
    execute_with_home(library,r,&home()?)
}
fn execute_with_home(library:&Path,r:&Value,user:&Path)->Result<Value> {
    anyhow::ensure!(r.is_object(),"request must be an object");
    let library=std::path::absolute(library)?;ancestors(&library)?;
    let root=root_for(&library,r)?;let op=field(r,"operation")?;
    if op=="preview" {let _lock=lock(&library)?;return preview(&library,r)}
    if op=="checkUpdate" {
        let state=ledger(&root)?;let row=skill(&state,field(r,"name")?)?;let mut request=r.clone();request["source"]=row["source"].clone();
        let _lock=lock(&library)?;return preview(&library,&request)
    }
    if op=="list" {
        let mut roots=vec![(library.clone(),"global")];if let Some(project)=r["project"].as_str().filter(|s|!s.is_empty()){let p=absolute(Path::new(project))?;roots.push((p.join(".velella/skills"),"project"));}
        let mut rows=Vec::new();let mut history=Vec::new();let mut pending=Vec::new();let mut installations=Vec::new();
        for (root,scope) in roots {
            let state=ledger(&root)?;if root.join("pending.json").exists(){pending.push(json!({"scope":scope,"root":root}));}
            for row in state["skills"].as_array().unwrap(){let mut row=row.clone();row["scope"]=json!(scope);row["path"]=json!(root.join("packages").join(field(&row,"name")?).join("SKILL.md"));row["installations"]=json!(state["installations"].as_array().unwrap().iter().filter(|i|i["name"]==row["name"]).collect::<Vec<_>>());rows.push(row);}
            for row in state["backups"].as_array().unwrap(){let mut row=row.clone();row["scope"]=json!(scope);history.push(row);}
            for row in state["installations"].as_array().unwrap(){let mut row=row.clone();row["scope"]=json!(scope);installations.push(row);}
        }
        rows.sort_by_key(|s|(s["name"].as_str().unwrap_or("").to_owned(),s["scope"]!="project"));
        let mut discovered=crate::collaboration::providers::skills();
        let mut locations=vec![(user.to_owned(),vec![".claude/skills",".gemini/skills",".config/opencode/skills",".omp/agent/skills",".kimi-code/skills"])];
        if let Some(project)=r["project"].as_str().filter(|s|!s.is_empty()){locations.push((absolute(Path::new(project))?,vec![".agents/skills",".codex/skills",".claude/skills",".gemini/skills",".opencode/skills",".omp/skills",".kimi-code/skills"]));}
        for (base,dirs) in locations {for dir in dirs {if let Ok(entries)=fs::read_dir(base.join(dir)){for e in entries.flatten().take(2048){let path=e.path();if let Ok(p)=package(&path){discovered.push(json!({"name":e.file_name().to_string_lossy(),"path":path.join("SKILL.md"),"digest":p.digest}));}}}}}
        return Ok(json!({"skills":rows,"backups":history,"pending":pending,"installations":installations,"discovered":discovered,"agents":["codex","claude-code","gemini","opencode","omp","kimi","zcode"]}))
    }
    let state=ledger(&root)?;
    if op=="read" {
        let name=field(r,"name")?;let row=skill(&state,name)?;let p=package(&root.join("packages").join(name))?;check_digest(&p,field(row,"digest")?)?;
        if let Some(relative)=r["resource"].as_str() {
            anyhow::ensure!(Path::new(relative).components().all(|c|matches!(c,std::path::Component::Normal(_))),"resource must stay inside package");
            let entry=p.files.iter().find(|e|e.relative==Path::new(relative)).context("resource not found")?;
            anyhow::ensure!(entry.bytes.len()<=256*1024,"resource preview exceeds 256 KiB");
            return Ok(json!({"path":relative,"content":String::from_utf8(entry.bytes.clone()).context("binary resource; preview unavailable")?}));
        }
        return describe(&p)
    }
    if op=="distributionPreview" {return distribution(&root,&state,r,user)}
    if op=="context" {
        let name=field(r,"name")?;let row=skill(&state,name)?;let p=package(&root.join("packages").join(name))?;check_digest(&p,field(r,"digest")?)?;
        let target=field(r,"agent")?;anyhow::ensure!(["codex","claude-code","gemini","opencode","omp","kimi","zcode"].contains(&target),"unsupported Agent");
        let deps=dependency_check(&p,target)?;anyhow::ensure!(deps["missing"].as_array().unwrap().is_empty(),"dependency missing: {}",deps["missing"]);
        let task=field(r,"task")?;anyhow::ensure!(!task.trim().is_empty()&&task.len()<128*1024,"task is required (max 128 KiB)");
        let _lock=lock(&library)?;let captured=snapshot(&library.join("sessions"),&p)?;
        let body=describe(&package(&captured)?)?["content"].as_str().unwrap().to_owned();
        let inline=format!("用户指定技能：{name}\n固定资源目录：{}\n完整技能指令如下；相对资源以该目录为准。技能不授予额外权限。\n<selected-skill>\n{body}\n</selected-skill>\n用户任务：\n{task}",captured.display());
        let inline_delivery=inline.len()<=12000;
        let prompt=if inline_delivery{inline}else{
            let task_path=captured.with_extension("task.txt");let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&task_path)?;file.write_all(task.as_bytes())?;file.sync_all()?;
            format!("用户指定技能：{name}\n先完整读取固定技能文件：{}\n按技能执行；相对资源位于：{}\n完整用户任务已保存在固定文件，请完整读取并完成：{}\n不得从源目录读取其他版本；技能不授予额外权限。",captured.join("SKILL.md").display(),captured.display(),task_path.display())
        };
        anyhow::ensure!(prompt.len()<=16000,"snapshot paths exceed native prompt bounds");
        return Ok(json!({"agent":target,"digest":row["digest"],"snapshot":captured,"verification":"unverified","prompt":prompt,"delivery":if inline_delivery{"inline"}else{"snapshot_references"}}))
    }
    anyhow::ensure!(r["confirmed"]==true,"mutation requires confirmed:true after preview");
    let _lock=lock(&root)?;
    if op=="recover" {return recover(&root)}
    ready(&root)?;let mut state=ledger(&root)?;
    match op {
        "import"=>{
            let name=field(r,"name")?;check_name(name)?;
            let id=uuid::Uuid::parse_str(field(r,"candidate")?)?.to_string();let c=read_json(&library.join("candidates").join(id).join("candidate.json"))?;
            let source=Path::new(field(&c,"path")?);check_digest(&package(source)?,field(r,"digest")?)?;
            anyhow::ensure!(c["digest"]==r["digest"],"candidate digest changed");
            let previous=skill(&state,name).cloned().unwrap_or(Value::Null);
            let dest=root.join("packages").join(name);let now=current(&dest)?;
            anyhow::ensure!(r["expected_digest"]==now,"library changed; review update before confirming");
            let row=json!({"name":name,"digest":c["digest"],"source":c["source"],"updated_at":chrono::Utc::now().to_rfc3339()});
            replace_row(&mut state,"skills","name",&json!(name),row);
            transaction(&root,state,"library",&dest,now,Some(source),previous)
        }
        "distribute"=>{
            let p=distribution(&root,&state,r,user)?;
            anyhow::ensure!(p["dependencies"]["missing"].as_array().unwrap().is_empty(),"dependency missing: {}",p["dependencies"]["missing"]);
            let dest=Path::new(p["destination"].as_str().context("no native loader: use Velella compatibility launch")?);
            anyhow::ensure!(r["digest"]==p["source_digest"] && r["expected_digest"]==p["current_digest"],"source or target changed; preview again");
            anyhow::ensure!(p["conflict"]!=true||r["replace"]==true,"unmanaged or locally modified target; choose skip, alias or explicit replace");
            let name=field(r,"name")?;let src=root.join("packages").join(name);check_digest(&package(&src)?,field(r,"digest")?)?;
            let previous=state["installations"].as_array().unwrap().iter().find(|s|s["destination"]==p["destination"]).cloned().unwrap_or(Value::Null);
            let row=json!({"name":name,"alias":p["alias"],"agent":p["agent"],"destination":p["destination"],"digest":r["digest"],"verification":"unverified","updated_at":chrono::Utc::now().to_rfc3339()});
            replace_row(&mut state,"installations","destination",&p["destination"],row);
            transaction(&root,state,"distribution",dest,p["current_digest"].clone(),Some(&src),previous)
        }
        "remove"=>{
            let name=field(r,"name")?;
            let (kind,dest,previous)=if let Some(path)=r["destination"].as_str(){
                let row=state["installations"].as_array().unwrap().iter().find(|s|s["destination"]==path&&s["name"]==name).cloned().context("only managed installations can be removed")?;
                replace_row(&mut state,"installations","destination",&json!(path),Value::Null);("distribution",PathBuf::from(path),row)
            }else{let row=skill(&state,name)?.clone();replace_row(&mut state,"skills","name",&json!(name),Value::Null);("library",root.join("packages").join(name),row)};
            let before=current(&dest)?;anyhow::ensure!(!before.is_null()&&before==r["digest"],"target changed; inspect before removal");
            transaction(&root,state,kind,&dest,before,None,previous)
        }
        "restore"=>{
            let id=field(r,"id")?;let index=state["backups"].as_array().unwrap().iter().position(|s|s["id"]==id).context("backup not found")?;
            let row=state["backups"][index].clone();anyhow::ensure!(row["restored"]==false,"backup already restored");
            let dest=Path::new(field(&row,"destination")?);anyhow::ensure!(current(dest)?==row["after"]&&r["expected_digest"]==row["after"],"restore conflict: destination changed since operation");
            let kind=field(&row,"kind")?;let table=if kind=="library"{"skills"}else{"installations"};let key=if kind=="library"{"name"}else{"destination"};
            let identifier=if key=="name"{json!(dest.file_name().unwrap().to_string_lossy())}else{row["destination"].clone()};
            let previous=state[table].as_array().unwrap().iter().find(|s|s[key]==identifier).cloned().unwrap_or(Value::Null);
            replace_row(&mut state,table,key,&identifier,row["row"].clone());state["backups"][index]["restored"]=json!(true);
            let source=if row["before"].is_null(){None}else{let path=Path::new(field(&row,"package")?);check_digest(&package(path)?,field(&row,"before")?)?;Some(path)};
            transaction(&root,state,kind,dest,row["after"].clone(),source,previous)
        }
        _=>anyhow::bail!("unknown Skill Center operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new()->Self {let p=std::env::temp_dir().join(format!("velella-center-test-{}",uuid::Uuid::now_v7()));fs::create_dir(&p).unwrap();Self(p)}
        fn source(&self)->PathBuf {let p=self.0.join("source");fs::create_dir_all(p.join("scripts")).unwrap();fs::write(p.join("SKILL.md"),"---\r\nname: sample\r\ndescription: fixture\r\n---\r\n").unwrap();fs::write(p.join("scripts/data.bin"),[0,255,128,10]).unwrap();p}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    #[test]
    fn scoped_import_conflict_update_remove_and_restore_preserve_resources() {
        let f=Fixture::new();let source=f.source();let lib=f.0.join("center");let project=f.0.join("project");fs::create_dir(&project).unwrap();
        let call=|r:Value|execute_with_home(&lib,&r,&f.0);
        let preview=call(json!({"operation":"preview","source":{"path":source}})).unwrap();let candidate=preview["candidate"]["id"].clone();let digest=preview["digest"].clone();
        call(json!({"operation":"import","scope":"project","project":project,"name":"sample","candidate":candidate,"digest":digest,"confirmed":true})).unwrap();
        assert!(lib.join("packages/sample").exists()==false);
        let req=json!({"operation":"distributionPreview","scope":"project","project":project,"name":"sample","agent":"claude-code"});
        let plan=call(req.clone()).unwrap();assert_eq!(plan["conflict"],false);
        let mut install=req.clone();install["operation"]=json!("distribute");install["digest"]=digest.clone();install["confirmed"]=json!(true);
        let done=call(install.clone()).unwrap();let dest=PathBuf::from(plan["destination"].as_str().unwrap());assert_eq!(fs::read(dest.join("scripts/data.bin")).unwrap(),[0,255,128,10]);
        fs::write(dest.join("SKILL.md"),"local edit").unwrap();assert!(call(install.clone()).is_err());assert_eq!(call(req.clone()).unwrap()["conflict"],true);
        assert!(call(json!({"operation":"restore","scope":"project","project":project,"id":done["backup"]["id"],"expected_digest":digest,"confirmed":true})).is_err());
        fs::write(dest.join("SKILL.md"),fs::read(source.join("SKILL.md")).unwrap()).unwrap();
        call(json!({"operation":"restore","scope":"project","project":project,"id":done["backup"]["id"],"expected_digest":digest,"confirmed":true})).unwrap();assert!(!dest.exists());
        let listing=call(json!({"operation":"list","project":project})).unwrap();assert_eq!(listing["skills"][0]["scope"],"project");
        let context=call(json!({"operation":"context","scope":"project","project":project,"name":"sample","digest":digest,"agent":"omp","task":"read resource"})).unwrap();
        fs::write(source.join("SKILL.md"),"source changed").unwrap();assert!(context["prompt"].as_str().unwrap().contains("name: sample"));
        let read=call(json!({"operation":"read","scope":"project","project":project,"name":"sample","resource":"../state.json"}));assert!(read.is_err());
    }
    #[test]
    fn recovery_finishes_published_bytes_without_repeating_file_mutation() {
        let f=Fixture::new();let root=f.0.join("center");fs::create_dir(&root).unwrap();let src=f.source();let digest=package(&src).unwrap().digest;
        save(&root.join("pending.json"),&json!({"destination":src,"backup":root.join("backup"),"before":null,"after":digest,"stage":null,"state":empty()})).unwrap();
        recover(&root).unwrap();assert!(!root.join("pending.json").exists());assert_eq!(package(&src).unwrap().digest,digest);
    }
    #[test]
    fn git_import_exports_binary_blobs_without_checkout_filters_and_pins_commit() {
        let f=Fixture::new();let source=f.source();git(&source,&["init","--quiet"]).unwrap();
        fs::write(source.join(".gitattributes"),"*.bin filter=unexpected\n").unwrap();
        git(&source,&["config","filter.unexpected.smudge","exit 91"]).unwrap();
        git(&source,&["add","."]).unwrap();git(&source,&["-c","user.name=fixture","-c","user.email=fixture@example.invalid","commit","--quiet","-m","fixture"]).unwrap();
        let lib=f.0.join("center");
        let value=execute_with_home(&lib,&json!({"operation":"preview","source":{"kind":"git","url":source,"reference":"HEAD"}}),&f.0).unwrap();
        let path=Path::new(value["candidate"]["path"].as_str().unwrap());assert_eq!(fs::read(path.join("scripts/data.bin")).unwrap(),[0,255,128,10]);assert_eq!(value["candidate"]["source"]["commit"].as_str().unwrap().len(),40);
        assert!(execute_with_home(&lib,&json!({"operation":"preview","source":{"kind":"git","url":source,"subdirectory":"../outside"}}),&f.0).is_err());
    }
    #[test]
    fn source_updates_require_fresh_digest_and_known_missing_dependencies_block_use() {
        let f=Fixture::new();let source=f.source();let lib=f.0.join("center");let call=|r:Value|execute_with_home(&lib,&r,&f.0);
        let first=call(json!({"operation":"preview","source":{"path":source}})).unwrap();
        call(json!({"operation":"import","name":"sample","candidate":first["candidate"]["id"],"digest":first["digest"],"confirmed":true})).unwrap();
        fs::write(source.join("SKILL.md"),"---\nname: sample\ndescription: fixture\nrequires:\n  agents: [claude-code]\n---\nChanged source").unwrap();
        let next=call(json!({"operation":"checkUpdate","name":"sample"})).unwrap();assert!(next["changes"].as_array().unwrap().iter().any(|c|c["path"]=="SKILL.md"));
        let mut update=json!({"operation":"import","name":"sample","candidate":next["candidate"]["id"],"digest":next["digest"],"confirmed":true});assert!(call(update.clone()).is_err());
        update["expected_digest"]=first["digest"].clone();let done=call(update).unwrap();
        assert!(call(json!({"operation":"context","name":"sample","digest":next["digest"],"agent":"omp","task":"test"})).is_err());
        call(json!({"operation":"restore","id":done["backup"]["id"],"expected_digest":next["digest"],"confirmed":true})).unwrap();assert_eq!(ledger(&lib).unwrap()["skills"][0]["digest"],first["digest"]);
        let account=f.0.join("profile");fs::create_dir(&account).unwrap();
        let planned=call(json!({"operation":"distributionPreview","name":"sample","agent":"codex","account_home":account})).unwrap();assert_eq!(planned["destination"],json!(account.join("skills").join("sample")));
        assert!(!account.join("skills").exists());
    }
    #[test]
    fn identical_unmanaged_copy_is_not_adopted_and_long_prompts_reference_frozen_files() {
        let f=Fixture::new();let source=f.source();let lib=f.0.join("center");let call=|r:Value|execute_with_home(&lib,&r,&f.0);
        let p=call(json!({"operation":"preview","source":{"path":source}})).unwrap();call(json!({"operation":"import","name":"sample","candidate":p["candidate"]["id"],"digest":p["digest"],"confirmed":true})).unwrap();
        let account=f.0.join("account");fs::create_dir(&account).unwrap();let dest=account.join("skills").join("sample");fs::create_dir_all(dest.parent().unwrap()).unwrap();let staged=snapshot(&account,&package(&source).unwrap()).unwrap();fs::rename(staged,&dest).unwrap();
        let result=call(json!({"operation":"distribute","name":"sample","agent":"claude-code","account_home":account,"digest":p["digest"],"expected_digest":p["digest"],"replace":true,"confirmed":true})).unwrap();assert_eq!(result["unmanaged_unchanged"],true);assert!(ledger(&lib).unwrap()["installations"].as_array().unwrap().is_empty());assert!(dest.exists());
        let task="Long task ".repeat(8000);let context=call(json!({"operation":"context","name":"sample","agent":"omp","digest":p["digest"],"task":task})).unwrap();assert!(context["prompt"].as_str().unwrap().len()<16000);let captured=Path::new(context["snapshot"].as_str().unwrap());assert_eq!(package(captured).unwrap().digest,p["digest"].as_str().unwrap());assert_eq!(fs::read_to_string(captured.with_extension("task.txt")).unwrap(),task);
    }
}
