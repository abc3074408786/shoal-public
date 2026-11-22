//! Invocation-local provider configuration. Never writes account/global settings.
use anyhow::{Context,Result};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{io::Read,path::{Path,PathBuf},process::Stdio,time::{Duration,Instant}};
use super::orchestration::v2::{default_model,RoleDefinition,PROVIDERS};
mod native_policy;
mod skill_policy;
mod codex_skills;
mod opencode_policy;
mod omp_policy;
#[cfg(test)]
mod skill_center_tests;

pub fn program(provider:&str)->Result<&str>{match provider {"codex"=>Ok("codex"),"claude-code"=>Ok("claude"),"omp"=>Ok("omp"),"gemini"=>Ok("gemini"),"opencode"=>Ok("opencode"),"kimi"=>Ok("kimi"),_=>anyhow::bail!("unsupported provider")}}
pub fn skill_digest(path:&Path)->Result<String>{
    let (root,files)=skill_files(path)?;
    let manifest=files.iter().map(|(path,bytes)|json!({"path":path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/"),"sha256":format!("{:x}",Sha256::digest(bytes))})).collect::<Vec<_>>();
    Ok(format!("sha256:{:x}",Sha256::digest(serde_json::to_vec(&manifest)?)))
}
fn skill_files(path:&Path)->Result<(PathBuf,Vec<(PathBuf,Vec<u8>)>)>{
    let file=if path.is_dir(){path.join("SKILL.md")}else{path.to_owned()};
    anyhow::ensure!(file.file_name().is_some_and(|s|s.eq_ignore_ascii_case("SKILL.md")) && std::fs::metadata(&file)?.len()<=256*1024,"skill must reference a bounded SKILL.md");
    let root=file.parent().context("skill parent missing")?.to_owned();let mut files=Vec::new();let mut size=0;
    fn visit(root:&Path,path:&Path,files:&mut Vec<(PathBuf,Vec<u8>)>,size:&mut u64)->Result<()>{
        let meta=std::fs::symlink_metadata(path)?;
        anyhow::ensure!(!meta.file_type().is_symlink(),"skill snapshot cannot follow links");
        if meta.is_dir(){for entry in std::fs::read_dir(path)?{let entry=entry?;if entry.file_name()==".git"{continue}visit(root,&entry.path(),files,size)?;}}
        else{*size+=meta.len();anyhow::ensure!(meta.is_file() && *size<=32*1024*1024 && files.len()<2048,"skill resource snapshot exceeds bounds");anyhow::ensure!(dunce::canonicalize(path)?.starts_with(dunce::canonicalize(root)?),"skill resource leaves root");files.push((path.to_owned(),std::fs::read(path)?));}Ok(())
    }
    visit(&root,&root,&mut files,&mut size)?;files.sort_by(|a,b|a.0.cmp(&b.0));Ok((root,files))
}
fn freeze_skills(folder:&Path,role:&RoleDefinition)->Result<Vec<Value>>{
    let mut bindings=Vec::new();
    for (index,skill) in role.skills.iter().enumerate(){
        let target=folder.join("skills").join(format!("skill-{index}"));
        if !target.exists(){
            let (source,files)=skill_files(Path::new(&skill.path))?;
            for (path,bytes) in files{let output=target.join(path.strip_prefix(&source)?);std::fs::create_dir_all(output.parent().context("skill resource parent")?)?;std::fs::write(output,bytes)?;}
        }
        anyhow::ensure!(skill_digest(&target)?==skill.digest,"skill resources changed during capture");
        bindings.push(json!({"source":skill.path,"path":target.join("SKILL.md"),"digest":skill.digest}));
    }Ok(bindings)
}
pub fn role_fingerprint(role:&RoleDefinition)->Result<String>{
    Ok(format!("sha256:{:x}",Sha256::digest(serde_json::to_vec(&json!({"role":role,"model_policy":"codex-role-defaults-v1"}))?)))
}
fn located(program:&str)->Vec<String>{
    if cfg!(windows){
        let paths=crate::process::command("where.exe").arg(program).output().ok().filter(|o|o.status.success()).map(|o|String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|p|!p.is_empty()).map(str::to_owned).collect()).unwrap_or_default();
        if program=="opencode" {return opencode_executable(paths)}
        paths
    }
    else{std::env::var_os("PATH").into_iter().flat_map(|paths|std::env::split_paths(&paths).collect::<Vec<_>>()).map(|dir|dir.join(program)).filter(|path|path.is_file()).map(|path|path.to_string_lossy().into_owned()).collect()}
}
#[cfg(windows)]
fn opencode_executable(paths:Vec<String>)->Vec<String>{
    for path in &paths {
        if path.to_ascii_lowercase().ends_with("opencode.exe") && std::fs::symlink_metadata(path).is_ok_and(|meta|meta.is_file()&&!meta.file_type().is_symlink()) {return vec![path.clone()]}
        let Some(prefix)=Path::new(path).parent() else {continue};
        let candidate=prefix.join("node_modules/opencode-ai/bin/opencode.exe");
        if std::fs::symlink_metadata(&candidate).is_ok_and(|meta|meta.is_file()&&!meta.file_type().is_symlink()) {return vec![candidate.to_string_lossy().into_owned()]}
    }
    Vec::new()
}
#[cfg(not(windows))]
fn opencode_executable(paths:Vec<String>)->Vec<String>{paths}
fn bounded_read(mut reader:impl Read)->std::io::Result<Vec<u8>>{
    let mut kept=Vec::new();let mut buffer=[0_u8;1024];
    loop {let count=reader.read(&mut buffer)?;if count==0{break}let remaining=8192_usize.saturating_sub(kept.len());kept.extend_from_slice(&buffer[..count.min(remaining)]);}Ok(kept)
}
#[cfg(windows)]
fn observed_version(program:&str)->Value{
    let mut child=match crate::process::command(program).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn(){Ok(child)=>child,Err(error)=>return json!({"status":"not_observed","reason":format!("cannot start version probe: {error}")})};
    let group=match crate::process::ChildGroup::attach(child.id()) {Ok(group)=>group,Err(error)=>{let _=child.kill();let _=child.wait();return json!({"status":"not_observed","reason":format!("cannot own version probe process tree: {error}")})}};
    let stdout_pipe=child.stdout.take();let stderr_pipe=child.stderr.take();
    let stdout=std::thread::spawn(move||child_stdout(stdout_pipe));
    let stderr=std::thread::spawn(move||child_stdout(stderr_pipe));
    let deadline=Instant::now()+Duration::from_secs(2);let mut timed_out=false;let mut status=None;
    loop {match child.try_wait(){Ok(Some(value))=>{status=Some(value);break},Ok(None) if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(10)),Ok(None)=>{timed_out=true;let _=group.kill();let _=child.wait();break},Err(error)=>return json!({"status":"not_observed","reason":format!("version probe status failed: {error}")})}}
    drop(group);
    let mut output=stdout.join().ok().and_then(Result::ok).unwrap_or_default();output.extend(stderr.join().ok().and_then(Result::ok).unwrap_or_default());
    let output=String::from_utf8_lossy(&output).trim().to_owned();
    if timed_out{return json!({"status":"not_observed","reason":"version probe exceeded 2 seconds and its owned process group was stopped"})}
    match status {Some(value) if value.success() && !output.is_empty()=>json!({"status":"observed","value":output}),Some(value)=>json!({"status":"not_observed","reason":format!("version probe exited {value}")}),None=>json!({"status":"not_observed","reason":"version probe did not return a status"})}
}
#[cfg(windows)]
fn child_stdout(reader:Option<impl Read>)->std::io::Result<Vec<u8>>{match reader {Some(reader)=>bounded_read(reader),None=>Ok(vec![])}}
#[cfg(not(windows))]
fn observed_version(_program:&str)->Value{json!({"status":"not_observed","reason":"bounded native version probes are currently implemented for Windows only"})}
pub(crate) fn effective_model(role:&RoleDefinition)->String{if role.model.trim().is_empty(){if let Some(remote)=&role.remote_codex{remote.remote_model.clone().unwrap_or_default()}else{default_model(&role.provider,&role.kind).to_owned()}}else{role.model.clone()}}
fn model_receipt(model:&str)->Value{
    if model.trim().is_empty(){json!({"requested":Value::Null,"status":"inherits_cli_default_unverified","reason":"the CLI default is not a verified model support list"})}
    else{json!({"requested":model,"status":"requested_unverified","reason":"passing --model binds the request but does not prove account or service support"})}
}
#[derive(Clone,Debug)]
struct GeminiRecovery {bundle:PathBuf,version:String}
fn complete_gemini_package(path:&Path)->Result<Option<GeminiRecovery>>{
    let package=path.join("package.json");let bundle=path.join("bundle/gemini.js");
    let package_meta=match std::fs::symlink_metadata(&package){Ok(meta)=>meta,Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(error)=>return Err(error.into())};
    if !package_meta.is_file()||package_meta.file_type().is_symlink()||package_meta.len()>256*1024{return Ok(None)}
    let value:Value=serde_json::from_slice(&std::fs::read(&package)?)?;
    if value["name"]!="@google/gemini-cli"||value["bin"]["gemini"]!="bundle/gemini.js"{return Ok(None)}
    let bundle_meta=match std::fs::symlink_metadata(&bundle){Ok(meta)=>meta,Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(error)=>return Err(error.into())};
    if !bundle_meta.is_file()||bundle_meta.file_type().is_symlink()||bundle_meta.len()==0{return Ok(None)}
    Ok(Some(GeminiRecovery{bundle,version:value["version"].as_str().unwrap_or("unknown").to_owned()}))
}
fn gemini_recovery_bundle(root:&Path)->Result<Option<GeminiRecovery>>{
    if !root.is_dir(){return Ok(None)}let mut candidates=std::fs::read_dir(root)?.flatten().filter_map(|entry|{let name=entry.file_name();name.to_str().filter(|name|name.starts_with(".gemini-cli-")).map(|_|entry.path())}).collect::<Vec<_>>();candidates.sort();
    for candidate in candidates {let meta=std::fs::symlink_metadata(&candidate)?;if meta.is_dir()&&!meta.file_type().is_symlink(){if let Some(bundle)=complete_gemini_package(&candidate)?{return Ok(Some(bundle));}}}
    Ok(None)
}
fn gemini_recovery()->Result<Option<GeminiRecovery>>{
    for launcher in located("gemini") {let Some(global)=Path::new(&launcher).parent() else{continue};let packages=global.join("node_modules/@google");
        if complete_gemini_package(&packages.join("gemini-cli"))?.is_some(){return Ok(None)}
        if let Some(bundle)=gemini_recovery_bundle(&packages)?{return Ok(Some(bundle))}
    }Ok(None)
}
fn session_gemini_wrapper(folder:&Path,recovery:&GeminiRecovery)->Result<PathBuf>{
    let node=located("node").into_iter().map(PathBuf::from).find(|path|std::fs::symlink_metadata(path).is_ok_and(|meta|meta.is_file()&&!meta.file_type().is_symlink())).context("Gemini 临时恢复需要可用的 node 启动器")?;
    let wrapper=folder.join("gemini-recovery.cmd");std::fs::write(&wrapper,format!("@echo off\r\n\"{}\" \"{}\" %*\r\n",node.display(),recovery.bundle.display()))?;Ok(wrapper)
}
fn preflight(role:&RoleDefinition)->Result<Value>{
    let executable=program(&role.provider)?;let paths=located(executable);
    let launcher=if paths.is_empty(){json!({"status":"not_located","program":executable,"paths":[]})}else{json!({"status":"located","program":executable,"paths":paths})};
    // Gemini's launcher has a separately recorded broken-installation root cause.
    // Do not turn ordinary page loads or session setup into a third identical
    // --version/--help retry; surface the requested model as unverified instead.
    let recovery=if role.provider=="gemini" {gemini_recovery()?}else{None};
    let version=if role.provider=="gemini" {json!({"status":"not_observed","reason":"Gemini version probe intentionally withheld pending a launcher-integrity repair; this does not assert installation health"})} else {observed_version(executable)};
    let selected_model=effective_model(role);let mut model=model_receipt(&selected_model);if role.model.trim().is_empty()&&!selected_model.is_empty(){model["source"]=json!("role_default");}else if !selected_model.is_empty(){model["source"]=json!("user_selected");}
    let execution_mode=if role.provider=="codex"{json!({"requested":"standard","status":"configured_unverified","reason":"Codex session sets features.fast_mode=false; service mode has not been observed"})}else{json!({"requested":"standard","status":"not_provider_verified","reason":"No provider-specific Standard flag is assumed for this CLI"})};
    Ok(json!({"version":1,"provider":role.provider,"launcher":launcher,"cli_version":version,"model":model,"execution_mode":execution_mode,"gemini_session_recovery":recovery.as_ref().map(|value|json!({"status":"available","version":value.version,"bundle":value.bundle})).unwrap_or_else(||json!({"status":"not_available"})),"native_ready":"not_verified"}))
}
/// This receipt is produced before the native host is started and stored in
/// the service-owned descriptor, so an idempotent launch replay can return the
/// same observation without running another probe.
pub fn preflight_receipt(desc:&crate::host::Descriptor)->Value{desc.spawn.env.get("VELELLA_PREFLIGHT_RECEIPT").and_then(|text|serde_json::from_str(text).ok()).unwrap_or(Value::Null)}
pub fn skills()->Vec<Value>{
    let mut found=Vec::new();let home=std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(PathBuf::from);
    if let Some(home)=home {for folder in [home.join(".agents/skills"),home.join(".codex/skills")] {if let Ok(entries)=std::fs::read_dir(folder){for e in entries.flatten(){let p=e.path().join("SKILL.md");if let Ok(digest)=skill_digest(&p){found.push(json!({"name":e.file_name().to_string_lossy(),"path":p,"digest":digest}));}}}}}
    found
}
pub fn capabilities()->Value {
    let providers:Vec<Value>=PROVIDERS.iter().map(|id|{
        let exe=program(id).unwrap();let available=!located(exe).is_empty();
        json!({"id":id,"installed":available,"models":"explicit_or_cli_default","tools":tools(id),"native_validation":"not_verified","skills":true,"mcp":true})
    }).collect();json!({"ok":true,"providers":providers,"skills":skills(),"max_parallelism":5})
}
/// Project-scoped additions do not change saved role bindings or their snapshots.
pub fn scoped_capabilities(data:&Path,project:Option<&Path>)->Result<Value> {
    let mut value=capabilities();let mut rows=value["skills"].as_array().unwrap().clone();
    for row in &mut rows {row["scope"]=json!("global");}
    let config=data.join("extensions/state.json");
    if config.exists() {
        let settings:Value=serde_json::from_slice(&std::fs::read(config)?)?;
        let plugin=&settings["skill-center"];
        if plugin["enabled"]==true && plugin["installed"]!=false {
            let default=data.join("extensions/skill-center/data");let library=plugin["dataDir"].as_str().map(PathBuf::from).unwrap_or(default);
            append_managed_skills(&library,"global",&mut rows)?;
            if let Some(project)=project {append_managed_skills(&project.join(".velella/skills"),"project",&mut rows)?;}
        }
    }
    if let Some(project)=project {
        for directory in [".agents/skills",".codex/skills",".claude/skills",".gemini/skills",".opencode/skills",".kimi-code/skills"] {
            let folder=project.join(directory);
            if let Ok(entries)=std::fs::read_dir(folder){for e in entries.flatten().take(2048){let p=e.path().join("SKILL.md");if let Ok(digest)=skill_digest(&p){rows.push(json!({"name":e.file_name().to_string_lossy(),"path":p,"digest":digest,"scope":"project","source":"native"}));}}}
        }
    }
    let mut seen=std::collections::HashSet::new();rows.retain(|s|seen.insert(s["path"].as_str().unwrap_or("").to_ascii_lowercase()));
    rows.sort_by_key(|s|(s["scope"]!="project",s["name"].as_str().unwrap_or("").to_owned()));
    value["skills"]=json!(rows);Ok(value)
}
fn append_managed_skills(root:&Path,scope:&str,rows:&mut Vec<Value>)->Result<()> {
    let file=root.join("state.json");if !file.exists(){return Ok(())}
    anyhow::ensure!(!root.join("pending.json").exists(),"Skill Center has an interrupted operation; recover before selecting new role skills");
    anyhow::ensure!(std::fs::metadata(&file)?.len()<=8*1024*1024,"Skill Center catalog too large");
    let state:Value=serde_json::from_slice(&std::fs::read(file)?)?;
    for row in state["skills"].as_array().context("invalid Skill Center catalog")? {
        let name=row["name"].as_str().context("skill name missing")?;
        anyhow::ensure!(!name.is_empty()&&name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'-'|b'_')),"invalid managed skill name");
        let path=root.join("packages").join(name).join("SKILL.md");let digest=skill_digest(&path)?;
        anyhow::ensure!(row["digest"]==digest,"managed skill changed; inspect it in Skill Center");
        rows.push(json!({"name":name,"path":path,"digest":digest,"scope":scope,"source":"skill-center"}));
    }Ok(())
}
pub fn tools(provider:&str)->Vec<&'static str>{match provider {
    "omp"=>vec!["read","bash","edit","write","grep","glob","python","notebook","todo","web_search","ask"],
    "claude-code"=>vec!["Read","Glob","Grep","Bash","Edit","Write","WebFetch","WebSearch"],
    "kimi"=>vec!["Read","Glob","Grep","Bash","Edit","Write"],
    "opencode"=>vec!["read","glob","grep","bash","edit","write","webfetch","websearch","skill"],
    "gemini"=>vec!["read_file","list_directory","glob","grep_search","run_shell_command","replace","write_file","web_fetch","google_web_search","activate_skill"],
    _=>vec!["shell","web","image"],
}}
pub fn validate(role:&RoleDefinition)->Result<()> {
    super::orchestration::v2::validate_role(role)?;
    if role.remote_codex.is_some(){
        validate_remote(role)?;
        anyhow::ensure!(cfg!(windows),"远程 Codex 编排的本机文件安全目前仅支持 Windows，不能回退到本机执行");
        return Ok(())
    }
    for server in &role.mcp_servers {super::mcp_catalog::verify(server)?;}
    program(&role.provider)?;
    anyhow::ensure!(role.tools.iter().all(|t|tools(&role.provider).contains(&t.as_str())),"工具组合尚未适配该 CLI");
    Ok(())
}
/// Narrow first release: a remote worker can implement or explore using only
/// the session-owned bridge. Other role capabilities must not be silently lost.
pub(crate) fn validate_remote(role:&RoleDefinition)->Result<()> {
    super::orchestration::v2::validate_role(role)?;
    anyhow::ensure!(role.remote_codex.is_some()&&matches!(role.kind.as_str(),"implement"|"explore"),"远程 Codex 当前仅支持实现或探索角色");
    let remote=role.remote_codex.as_ref().unwrap();
    anyhow::ensure!(remote.ssh_target_fingerprint.is_some(),"远程 Codex 需要重新核对 SSH 实际目标");
    anyhow::ensure!(remote.remote_model.as_ref().is_some_and(|model|model==&effective_model(role))&&remote.provider_fingerprint.is_some(),"远程 Codex 需要重新核对 DeepSeek provider 和模型，不能使用本机角色默认模型");
    anyhow::ensure!(role.skills.is_empty()&&role.tools.is_empty()&&role.mcp_servers.is_empty(),"远程角色尚不支持自定义 Skills、工具或外部 MCP；请清空后再启动");
    Ok(())
}
fn json_file(path:&Path)->Result<Value>{if path.exists(){Ok(serde_json::from_slice(&std::fs::read(path)?)?)}else{Ok(json!({}))}}
fn append_hooks(config:&mut Value,hooks:Value)->Result<()>{
    if config["hooks"].is_null(){config["hooks"]=json!({});}
    let target=config["hooks"].as_object_mut().context("Gemini hooks must be an object")?;
    for (event,handlers) in hooks.as_object().context("hook definitions missing")?{
        target.entry(event).or_insert(json!([])).as_array_mut().context("Gemini hook event must be an array")?.extend(handlers.as_array().context("hook handlers missing")?.iter().cloned());
    }Ok(())
}
fn ensure_kimi_project_mcp_control(root:&Path)->Result<()> {
    native_policy::check_kimi_project(root)
}
/// Write one immutable descriptor per selected external server and return the
/// provider-native server map.  Providers execute only our proxy; the proxy
/// checks the frozen per-tool allowlist before it can reach the command.
fn selected_mcp(folder:&Path,runtime:&Path,role:&RoleDefinition)->Result<(Value,Vec<Value>)>{
    validate(role)?;
    let mut servers=serde_json::Map::new();let mut bindings=Vec::new();
    for server in &role.mcp_servers {if server.tools.is_empty(){continue}
        let descriptor=folder.join(format!("external-mcp-{}.json",server.id));
        let mut binding=json!({"id":server.id,"command":server.command,"args":server.args,"tools":server.tools});
        if let Some(source)=&server.source {binding["source"]=serde_json::to_value(source)?;binding["catalog"]=json!(server.catalog);}
        let bytes=serde_json::to_vec(&binding)?;
        let digest=format!("{:x}",Sha256::digest(&bytes));
        if descriptor.try_exists()? {super::mcp::external_descriptor(&descriptor,&digest)?;}
        else {
            use std::io::Write;
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&descriptor)?;
            file.write_all(&bytes)?;file.sync_all()?;
        }
        servers.insert(server.id.clone(),json!({"command":runtime,"args":["external-mcp",descriptor,digest]}));
        bindings.push(json!({"id":server.id,"tools":server.tools,"descriptor":descriptor,"sha256":digest}));
    }
    Ok((Value::Object(servers),bindings))
}
pub(crate) fn verify_mcp_bindings(desc:&crate::host::Descriptor)->Result<()> {
    if desc.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker") {return crate::hybrid::verify_role_binding(desc)}
    native_policy::verify(desc)?;
    skill_policy::verify(desc)?;
    if let Some(bindings)=desc.spawn.env.get("VELELLA_ROLE_FILE_BINDINGS") {
        for binding in serde_json::from_str::<Vec<Value>>(bindings)? {
            let path=Path::new(binding["path"].as_str().context("role configuration path missing")?);
            anyhow::ensure!(launch_file_digest(path)?==binding["sha256"].as_str().context("role configuration fingerprint missing")?,"role launch configuration changed; create a new session");
        }
    } else if desc.spawn.env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT") {
        anyhow::bail!("legacy role session has no launch configuration fingerprints; create a new session");
    }
    if let Some(bindings)=desc.spawn.env.get("VELELLA_MCP_BINDINGS") {
        for binding in serde_json::from_str::<Vec<Value>>(bindings)? {
            super::mcp::external_descriptor(Path::new(binding["descriptor"].as_str().context("MCP descriptor missing")?),binding["sha256"].as_str().context("MCP fingerprint missing")?)?;
        }
    }
    Ok(())
}
pub(crate) fn restore_gemini_settings(path:&Path)->Result<Value>{native_policy::gemini_settings(path)}
fn launch_file_digest(path:&Path)->Result<String>{
    let meta=std::fs::symlink_metadata(path)?;
    anyhow::ensure!(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=1024*1024,"role launch configuration is not a bounded ordinary file");
    let mut bytes=Vec::new();std::fs::File::open(path)?.take(1024*1024+1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=1024*1024,"role launch configuration exceeds bounds");
    Ok(format!("{:x}",Sha256::digest(&bytes)))
}
fn bind_launch_files(desc:&mut crate::host::Descriptor,folder:&Path)->Result<()> {
    // Explicit files only: mutable account credentials must not enter receipts.
    let mut bindings=Vec::new();
    for relative in ["instructions.md","mcp.json","claude-settings.json","gemini.json","gemini-recovery.cmd","lifecycle.mjs","opencode.json","agent.md","kimi-home/config.toml","kimi-home/mcp.json","omp-config.yml","omp-lifecycle.mjs","omp-home/.omp/agent/mcp.json"] {
        let path=folder.join(relative);if path.try_exists()? {bindings.push(json!({"path":path,"sha256":launch_file_digest(&path)?}));}
    }
    desc.spawn.env.insert("VELELLA_ROLE_FILE_BINDINGS".into(),serde_json::to_string(&bindings)?);
    Ok(())
}
fn kimi_allowlist(role:&RoleDefinition)->Vec<String>{let mut allow=role.tools.clone();if allow.is_empty(){allow.push("*".into());}allow.push("mcp__velella__*".into());for server in &role.mcp_servers {if !server.tools.is_empty(){allow.push(format!("mcp__{}__*",server.id));}}allow}
pub fn configure(desc:&mut crate::host::Descriptor,runtime:&Path,data:&Path,role:&RoleDefinition)->Result<Value> {
    validate(role)?;
    let mut preflight=preflight(role)?;
    anyhow::ensure!(preflight["launcher"]["status"]=="located","CLI 启动器未在 PATH 中定位：{}",role.provider);
    let folder=desc.directory.join("role");std::fs::create_dir_all(&folder)?;
    let skill_bindings=freeze_skills(&folder,role)?;
    desc.spawn.env.insert("VELELLA_SKILL_BINDINGS".into(),serde_json::to_string(&skill_bindings)?);
    let control=json!({"command":runtime,"args":["collaboration-mcp",data,desc.id]});
    let (external_mcp,mcp_bindings)=selected_mcp(&folder,runtime,role)?;
    desc.spawn.env.insert("VELELLA_MCP_BINDINGS".into(),serde_json::to_string(&mcp_bindings)?);
    let mut mcp_servers=external_mcp.as_object().cloned().unwrap_or_default();mcp_servers.insert("velella".into(),control.clone());let mcp_servers=Value::Object(mcp_servers);
    let instruction=skill_policy::instructions(role,&skill_bindings)?;
    std::fs::write(folder.join("instructions.md"),&instruction)?;
    desc.spawn.env.insert("VELELLA_ROLE_FILE".into(),folder.join("instructions.md").to_string_lossy().into_owned());
    if matches!(role.provider.as_str(),"codex"|"claude-code"|"gemini"|"opencode"|"omp") {
        desc.spawn.env.insert("VELELLA_SKILL_POLICY".into(),skill_policy::POLICY.into());
        desc.spawn.env.insert("VELELLA_ROLE_INSTRUCTION_SHA256".into(),launch_file_digest(&folder.join("instructions.md"))?);
    }
    desc.spawn.env.insert("VELELLA_PROVIDER".into(),role.provider.clone());
    desc.spawn.env.insert("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),role_fingerprint(role)?);
    desc.spawn.env.insert("VELELLA_RUNTIME_EXE".into(),runtime.to_string_lossy().into_owned());
    let selected_model=effective_model(role);if !selected_model.is_empty(){desc.spawn.args.extend(["--model".into(),selected_model]);}
    match role.provider.as_str(){
        "codex"=>{
            native_policy::configure_codex(desc,&mcp_servers)?;
            for (key,value) in [("features.multi_agent",json!(false)),("features.fast_mode",json!(false)),("developer_instructions",json!(instruction))]{desc.spawn.args.extend(["-c".into(),format!("{key}={}",to_toml(&value)?)]);}
            if !role.tools.is_empty(){for(key,name)in[("features.shell_tool","shell"),("tools.view_image","image")] {desc.spawn.args.extend(["-c".into(),format!("{key}={}",role.tools.iter().any(|t|t==name))]);}desc.spawn.args.extend(["-c".into(),format!("web_search=\"{}\"",if role.tools.iter().any(|t|t=="web"){"live"}else{"disabled"})]);}
            codex_skills::configure(desc)?;
        }
        "claude-code"=>{
            let prior=desc.directory.join("claude-collaboration-settings.json");
            let mut settings=json_file(&prior)?;
            settings["statusLine"]=json!({"type":"command","command":format!("\"{}\" context-usage",runtime.display())});
            let settings_path=folder.join("claude-settings.json");
            crate::ipc::atomic_json(&settings_path,&settings)?;
            if let Some(index)=desc.spawn.args.windows(2).position(|pair|pair[0]=="--settings" && Path::new(&pair[1])==prior) {
                desc.spawn.args[index+1]=settings_path.to_string_lossy().into_owned();
            }else{desc.spawn.args.extend(["--settings".into(),settings_path.to_string_lossy().into_owned()]);}
            let mcp=folder.join("mcp.json");crate::ipc::atomic_json(&mcp,&json!({"mcpServers":mcp_servers}))?;
            desc.spawn.args.extend(["--mcp-config".into(),mcp.to_string_lossy().into_owned(),"--strict-mcp-config".into(),"--append-system-prompt-file".into(),folder.join("instructions.md").to_string_lossy().into_owned(),"--disable-slash-commands".into(),"--disallowedTools".into(),"Agent,Task,Skill".into()]);
            if !role.tools.is_empty(){desc.spawn.args.extend(["--tools".into(),role.tools.join(",")]);}
        }
        "gemini"=>{
            if let Some(recovery)=gemini_recovery()? {let wrapper=session_gemini_wrapper(&folder,&recovery)?;desc.spawn.program=wrapper.to_string_lossy().into_owned();preflight["gemini_session_recovery"]["status"]=json!("active");preflight["gemini_session_recovery"]["effective_program"]=json!(wrapper);}
            let settings=folder.join("gemini.json");
            let base=std::env::var_os("GEMINI_CLI_SYSTEM_SETTINGS_PATH").map(PathBuf::from).unwrap_or_else(||PathBuf::from(if cfg!(windows){"C:/ProgramData/gemini-cli/settings.json"}else{"/etc/gemini-cli/settings.json"}));
            let mut config=native_policy::gemini_settings(&base)?;
            native_policy::configure_gemini(desc,&base,&mut config,&mcp_servers)?;
            skill_policy::configure_gemini(&mut config)?;
            let hook=runtime.to_string_lossy();
            let hooks=json!({"SessionStart":[{"hooks":[{"type":"command","command":format!("\"{hook}\" orchestration-hook SessionStart")}]}],"BeforeAgent":[{"hooks":[{"type":"command","command":format!("\"{hook}\" orchestration-hook UserPromptSubmit")}]}],"AfterAgent":[{"hooks":[{"type":"command","command":format!("\"{hook}\" orchestration-hook Stop")}]}]});
            append_hooks(&mut config,hooks)?;
            config["mcpServers"]=mcp_servers.clone();
            if !role.tools.is_empty(){config["tools"]["core"]=json!(role.tools.iter().filter(|tool|tool.as_str()!="activate_skill").collect::<Vec<_>>());}
            crate::ipc::atomic_json(&settings,&config)?;desc.spawn.env.insert("GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(),settings.to_string_lossy().into_owned());desc.spawn.adapter=Some("generic".into());
        }
        "opencode"=>{
            let launcher=located("opencode").into_iter().next().context("OpenCode native executable is unavailable")?;
            desc.spawn.program=launcher;
            opencode_policy::configure(desc,&folder,role,&instruction,&mcp_servers,&opencode_policy::auth_source())?;
        }
        "omp"=>{
            desc.spawn.program=located("omp").into_iter().next().context("OMP executable is unavailable")?;
            omp_policy::configure(desc,&folder,role,&mcp_servers,&omp_policy::auth_source()?)?;
        }
        "kimi"=>{
            ensure_kimi_project_mcp_control(Path::new(desc.spawn.cwd.as_deref().context("Kimi role has no workspace")?))?;
            let home=std::env::var_os("KIMI_CODE_HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).unwrap_or_default()).join(".kimi-code"));
            let private=folder.join("kimi-home");
            if private.exists(){
                let binding=json_file(&private.join("velella-binding.json"))?;
                anyhow::ensure!(binding["fingerprint"]==role_fingerprint(role)? && binding["state"]=="ready","private profile capture is incomplete or belongs to another role; retain it for inspection");
            }else{
            private_directory(&private)?;
            // A private invocation profile retains the account configuration; it never
            // writes back refreshed credentials or changes the user's default profile.
            copy_private(&home.join("credentials"),&private.join("credentials"))?;
            let mut config:toml::Value=if home.join("config.toml").is_file(){toml::from_str(&std::fs::read_to_string(home.join("config.toml"))?)?}else{toml::Value::Table(Default::default())};
            let table=config.as_table_mut().context("Kimi config must be a table")?;
            let hooks=table.entry("hooks").or_insert_with(||toml::Value::Array(vec![])).as_array_mut().context("Kimi hooks must be an array")?;
            for name in ["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","SessionEnd"]{
                let mapped=if name=="SessionEnd"{"Interrupt"}else{name};
                let value=json!({"event":name,"command":format!("\"{}\" orchestration-hook {mapped}",runtime.display()),"timeout":10});
                hooks.push(toml::Value::try_from(value)?);
            }
            std::fs::write(private.join("config.toml"),toml::to_string(&config)?)?;
            crate::ipc::atomic_json(&private.join("mcp.json"),&json!({"mcpServers":mcp_servers}))?;
            crate::ipc::atomic_json(&private.join("velella-binding.json"),&json!({"fingerprint":role_fingerprint(role)?,"state":"ready"}))?;
            }
            desc.spawn.env.insert("KIMI_CODE_HOME".into(),private.to_string_lossy().into_owned());
            let agent=folder.join("agent.md");let allow=kimi_allowlist(role);
            let front=serde_yaml::to_string(&json!({"name":"velella","description":role.name,"tools":allow,"disallowedTools":["Agent","AgentSwarm"],"subagents":[]}))?;
            std::fs::write(&agent,format!("---\n{front}---\n${{base_prompt}}\n{instruction}\n"))?;
            desc.spawn.args.extend(["--agent-file".into(),agent.to_string_lossy().into_owned()]);
            let skill_root=folder.join("skills");std::fs::create_dir_all(&skill_root)?;
            desc.spawn.args.extend(["--skills-dir".into(),skill_root.to_string_lossy().into_owned()]);
            desc.spawn.adapter=Some("generic".into());
        }
        _=>unreachable!(),
    }
    bind_launch_files(desc,&folder)?;
    desc.spawn.env.insert("VELELLA_PREFLIGHT_RECEIPT".into(),preflight.to_string());
    let mut effective=serde_json::to_value(role)?;effective["skill_bindings"]=json!(skill_bindings);effective["mcp_bindings"]=json!(mcp_bindings);effective["mcp_catalog_enforcement"]=json!("session_proxy_selected_tools; inherited_catalog_requires_native_validation");effective["skill_catalog_enforcement"]=json!(skill_policy::receipt(&role.provider));effective["preflight"]=preflight.clone();
    crate::ipc::atomic_json(&folder.join("effective.json"),&effective)?;Ok(preflight)
}
pub(crate) fn orchestration_hook_output(event:&str)->Result<Value>{
    skill_policy::hook_output(event,&std::env::vars().collect())
}
fn copy_private(source:&Path,target:&Path)->Result<()> {
    if !source.exists(){return Ok(())}
    let meta=std::fs::symlink_metadata(source)?;anyhow::ensure!(!meta.file_type().is_symlink(),"credential snapshot does not follow links");
    if meta.is_dir(){std::fs::create_dir_all(target)?;for item in std::fs::read_dir(source)?{let item=item?;copy_private(&item.path(),&target.join(item.file_name()))?;}}
    else if meta.is_file(){std::fs::copy(source,target)?;}Ok(())
}
fn private_directory(path:&Path)->Result<()> {
    std::fs::create_dir(path).context("private profile already exists or cannot be created; retain it for recovery")?;
    #[cfg(unix)]{
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]{
        // Pass the path through the environment, never through interpolated shell
        // code. Create an owner-only DACL before copying any account material.
        let output=crate::process::command("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",r#"$ErrorActionPreference='Stop'; $path=[Environment]::GetEnvironmentVariable('VELELLA_PRIVATE_PROFILE'); $identity=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; $acl=New-Object System.Security.AccessControl.DirectorySecurity; $acl.SetAccessRuleProtection($true,$false); $rule=New-Object System.Security.AccessControl.FileSystemAccessRule($identity,'FullControl','ContainerInherit,ObjectInherit','None','Allow'); $acl.AddAccessRule($rule); [System.IO.Directory]::SetAccessControl($path,$acl)"#]).env("VELELLA_PRIVATE_PROFILE",path).output()?;
        anyhow::ensure!(output.status.success(),"cannot protect the invocation profile; no credentials were copied");
    }
    Ok(())
}
fn to_toml(v:&Value)->Result<String>{
    Ok(match v {Value::Object(o)=>format!("{{{}}}",o.iter().map(|(k,v)|Ok(format!("{}={}",serde_json::to_string(k)?,to_toml(v)?))).collect::<Result<Vec<_>>>()?.join(",")),Value::Array(a)=>format!("[{}]",a.iter().map(to_toml).collect::<Result<Vec<_>>>()?.join(",")),_=>serde_json::to_string(v)?})
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn provider_overrides_are_valid_toml(){let s=format!("x={}",to_toml(&json!({"command":"C:\\Program Files\\runtime.exe","args":["collaboration-mcp","a"]})).unwrap());let _:toml::Value=toml::from_str(&s).unwrap();}
    #[test]fn provider_opencode_npm_shim_resolves_to_the_owned_native_executable(){
        let root=std::env::temp_dir().join(format!("velella-opencode-launcher-{}",uuid::Uuid::now_v7()));let prefix=root.join("bin");let executable=prefix.join("node_modules/opencode-ai/bin/opencode.exe");std::fs::create_dir_all(executable.parent().unwrap()).unwrap();std::fs::write(&executable,b"fixture").unwrap();
        let shim=prefix.join("opencode.cmd");std::fs::write(&shim,b"fixture").unwrap();assert_eq!(opencode_executable(vec![shim.to_string_lossy().into_owned()]),vec![executable.to_string_lossy().into_owned()]);std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]fn provider_opencode_generated_role_keeps_private_catalog_on_recovery(){
        let root=std::env::temp_dir().join(format!("velella-opencode-role-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();
        let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("opencode","goal")["roles"][0].clone()).unwrap();role.tools=vec!["read".into(),"skill".into()];
        let mut desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"opencode".into(),args:vec![],cwd:Some(root.to_string_lossy().into_owned()),env:Default::default(),adapter:None}};
        let folder=root.join("role");std::fs::create_dir_all(&folder).unwrap();
        let instruction=skill_policy::instructions(&role,&[]).unwrap();std::fs::write(folder.join("instructions.md"),&instruction).unwrap();
        desc.spawn.program=located("opencode").into_iter().next().unwrap();
        desc.spawn.env.insert("VELELLA_PROVIDER".into(),"opencode".into());desc.spawn.env.insert("VELELLA_SKILL_POLICY".into(),skill_policy::POLICY.into());desc.spawn.env.insert("VELELLA_SKILL_BINDINGS".into(),"[]".into());
        desc.spawn.env.insert("VELELLA_ROLE_FILE".into(),folder.join("instructions.md").to_string_lossy().into_owned());desc.spawn.env.insert("VELELLA_ROLE_INSTRUCTION_SHA256".into(),launch_file_digest(&folder.join("instructions.md")).unwrap());
        desc.spawn.env.insert("VELELLA_RUNTIME_EXE".into(),std::env::current_dir().unwrap().join("../../target/debug/velella-runtime.exe").to_string_lossy().into_owned());
        opencode_policy::configure(&mut desc,&folder,&role,&instruction,&json!({"velella":{"command":"fixture-never-execute","args":[]}}),&root.join("absent-auth.json")).unwrap();
        bind_launch_files(&mut desc,&folder).unwrap();
        let config:Value=serde_json::from_slice(&std::fs::read(root.join("role/opencode.json")).unwrap()).unwrap();
        assert_eq!(config["default_agent"],"velella");assert_eq!(config["agent"]["velella"]["permission"]["skill"]["*"],"deny");assert_eq!(config["mcp"].as_object().unwrap().keys().collect::<Vec<_>>(),vec!["velella"]);
        assert!(desc.spawn.program.to_ascii_lowercase().ends_with("opencode.exe"));assert_eq!(Path::new(&desc.spawn.env["OPENCODE_CONFIG"]),root.join("role/opencode.json"));assert_eq!(desc.spawn.env["OPENCODE_CONFIG_CONTENT"],"{}");
        verify_mcp_bindings(&desc).unwrap();
        if let Some(output)=std::env::var_os("VELELLA_OPENCODE_FIXTURE_OUT") {crate::ipc::atomic_json(Path::new(&output),&serde_json::to_value(&desc).unwrap()).unwrap();}else{std::fs::remove_dir_all(root).unwrap();}
    }
    #[test]#[ignore="requires a descriptor retained by the installed OpenCode lifecycle fixture"]
    fn provider_opencode_post_native_recovery(){
        let path=std::env::var_os("VELELLA_OPENCODE_FIXTURE_OUT").expect("fixture descriptor required");
        let desc:crate::host::Descriptor=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        verify_mcp_bindings(&desc).unwrap();
    }
    #[test]fn provider_external_mcp_uses_a_role_scoped_proxy_and_selected_tools(){
        let root=std::env::temp_dir().join(format!("velella-external-mcp-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();role.mcp_servers.push(super::super::orchestration::v2::McpServer{id:"docs".into(),command:"fixture-mcp".into(),args:vec!["--stdio".into()],catalog:vec!["read".into(),"write".into()],tools:vec!["read".into()],source:None});
        let runtime=Path::new("C:/runtime.exe");let (servers,bindings)=selected_mcp(&root,runtime,&role).unwrap();assert_eq!(servers["docs"]["command"],json!(runtime));assert_eq!(servers["docs"]["args"][0],"external-mcp");assert_eq!(bindings[0]["tools"],json!(["read"]));let descriptor=PathBuf::from(bindings[0]["descriptor"].as_str().unwrap());assert_eq!(json_file(&descriptor).unwrap()["tools"],json!(["read"]));std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_kimi_allowlist_keeps_the_selected_external_server(){
        let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("kimi","goal")["roles"][0].clone()).unwrap();role.tools=vec!["Read".into()];role.mcp_servers.push(super::super::orchestration::v2::McpServer{id:"docs".into(),command:"fixture".into(),args:vec![],catalog:vec!["search".into()],tools:vec!["search".into()],source:None});let allow=kimi_allowlist(&role);assert!(allow.contains(&"Read".into())&&allow.contains(&"mcp__velella__*".into())&&allow.contains(&"mcp__docs__*".into()));
    }
    #[test]fn provider_external_mcp_freezes_and_verifies_descriptor_on_replay(){
        let root=std::env::temp_dir().join(format!("velella-mcp-freeze-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();
        let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();
        role.mcp_servers.push(super::super::orchestration::v2::McpServer{id:"docs".into(),command:"fixture".into(),args:vec![],catalog:vec!["read".into(),"write".into()],tools:vec!["read".into()],source:None});
        let runtime=Path::new("runtime.exe");let (servers,bindings)=selected_mcp(&root,runtime,&role).unwrap();assert_eq!(selected_mcp(&root,runtime,&role).unwrap(),(servers.clone(),bindings.clone()));
        assert_eq!(servers["docs"]["args"][2],bindings[0]["sha256"]);
        let desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:None,env:std::collections::HashMap::from([("VELELLA_MCP_BINDINGS".into(),serde_json::to_string(&bindings).unwrap())]),adapter:None}};
        verify_mcp_bindings(&desc).unwrap();let path=Path::new(bindings[0]["descriptor"].as_str().unwrap());let mut altered=json_file(path).unwrap();altered["tools"]=json!(["write"]);crate::ipc::atomic_json(path,&altered).unwrap();
        assert!(verify_mcp_bindings(&desc).is_err());assert!(selected_mcp(&root,runtime,&role).is_err());assert_eq!(json_file(path).unwrap(),altered,"replay must not overwrite evidence of drift");std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_mcp_native_configuration_is_bound_on_recovery(){
        let root=std::env::temp_dir().join(format!("velella-mcp-config-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(root.join("kimi-home/credentials")).unwrap();
        let mut desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"claude".into(),args:vec![],cwd:None,env:std::collections::HashMap::from([("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),"fixture".into()),("VELELLA_SKILL_BINDINGS".into(),"[]".into())]),adapter:None}};
        assert!(verify_mcp_bindings(&desc).is_err(),"legacy unbound role recovery is not safe");
        let config=root.join("mcp.json");std::fs::write(&config,r#"{"mcpServers":{}}"#).unwrap();std::fs::write(root.join("kimi-home/credentials/token.json"),"fixture private token").unwrap();
        bind_launch_files(&mut desc,&root).unwrap();verify_mcp_bindings(&desc).unwrap();assert!(!desc.spawn.env["VELELLA_ROLE_FILE_BINDINGS"].contains("credentials"));
        std::fs::write(config,r#"{"mcpServers":{"unselected":{"command":"fixture"}}}"#).unwrap();assert!(verify_mcp_bindings(&desc).unwrap_err().to_string().contains("configuration changed"));std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_mcp_catalog_source_is_frozen_and_rechecked_before_launch_and_recovery(){
        let root=std::env::temp_dir().join(format!("velella-mcp-source-recovery-{}",uuid::Uuid::now_v7()));
        let hub=super::super::Hub::open(&root).unwrap();let path=root.join("catalog.json");
        std::fs::write(&path,r#"{"version":1,"entries":[{"id":"docs","command":"fixture","args":[],"catalog":["read"]}]}"#).unwrap();
        let preview=super::super::mcp_catalog::preview(&path).unwrap();
        let server=hub.confirm_mcp_source(&json!({"request_id":uuid::Uuid::now_v7().to_string(),"path":preview.path,"digest":preview.digest,"entry_id":"docs","tools":["read"],"confirmed":true})).unwrap()["server"].clone();
        let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();role.mcp_servers.push(serde_json::from_value(server).unwrap());
        let (_,bindings)=selected_mcp(&root,Path::new("runtime.exe"),&role).unwrap();
        let descriptor=Path::new(bindings[0]["descriptor"].as_str().unwrap());assert_eq!(json_file(descriptor).unwrap()["source"]["digest"],preview.digest);
        let desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:None,env:std::collections::HashMap::from([("VELELLA_MCP_BINDINGS".into(),serde_json::to_string(&bindings).unwrap())]),adapter:None}};
        verify_mcp_bindings(&desc).unwrap();
        let bytes=std::fs::read(&path).unwrap();std::fs::write(&path,[bytes.as_slice(),b" "].concat()).unwrap();
        assert!(selected_mcp(&root,Path::new("runtime.exe"),&role).is_err());assert!(verify_mcp_bindings(&desc).is_err());
        assert!(super::super::mcp::external_descriptor(descriptor,bindings[0]["sha256"].as_str().unwrap()).is_err());
        std::fs::write(&path,&bytes).unwrap();verify_mcp_bindings(&desc).unwrap();std::fs::remove_file(&path).unwrap();assert!(verify_mcp_bindings(&desc).is_err());
        drop(hub);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_selection_is_closed(){assert!(program("sh -c anything").is_err());assert_eq!(PROVIDERS,["codex","claude-code","omp","opencode"]);assert_eq!(program("omp").unwrap(),"omp");}
    #[test]fn provider_model_receipt_never_claims_unverified_support(){
        assert_eq!(model_receipt("")["status"],"inherits_cli_default_unverified");
        let requested=model_receipt("gpt-6-terra");assert_eq!(requested["requested"],"gpt-6-terra");assert_eq!(requested["status"],"requested_unverified");
    }
    #[test]fn provider_empty_codex_model_uses_the_role_default_without_claiming_support(){
        let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();role.model.clear();assert_eq!(effective_model(&role),"gpt-6-terra");role.kind="explore".into();assert_eq!(effective_model(&role),"gpt-6-luna");role.provider="kimi".into();assert!(effective_model(&role).is_empty());
    }
    #[test]fn provider_gemini_preflight_withholds_a_third_version_probe(){
        let role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("gemini","goal")["roles"][0].clone()).unwrap();
        let receipt=preflight(&role).unwrap();assert_eq!(receipt["cli_version"]["status"],"not_observed");assert!(receipt["cli_version"]["reason"].as_str().unwrap().contains("withheld"));
    }
    #[test]fn provider_preflight_receipt_survives_descriptor_replay(){
        let receipt=json!({"provider":"codex","cli_version":{"status":"observed","value":"fixture"}});
        let desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:std::env::temp_dir(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:None,env:std::collections::HashMap::from([("VELELLA_PREFLIGHT_RECEIPT".into(),receipt.to_string())]),adapter:None}};
        assert_eq!(preflight_receipt(&desc),receipt);
    }
    #[test]fn provider_gemini_recovery_bundle_requires_a_complete_staged_package(){
        let root=std::env::temp_dir().join(format!("velella-gemini-recovery-{}",uuid::Uuid::now_v7()));let staged=root.join(".gemini-cli-fixture");std::fs::create_dir_all(staged.join("bundle")).unwrap();std::fs::write(staged.join("package.json"),r#"{"name":"@google/gemini-cli","version":"0.47.0","bin":{"gemini":"bundle/gemini.js"}}"#).unwrap();std::fs::write(staged.join("bundle/gemini.js"),"fixture").unwrap();
        assert_eq!(gemini_recovery_bundle(&root).unwrap().as_ref().map(|bundle|bundle.bundle.clone()),Some(staged.join("bundle/gemini.js")));
        std::fs::remove_file(staged.join("bundle/gemini.js")).unwrap();assert!(gemini_recovery_bundle(&root).unwrap().is_none());std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_kimi_project_mcp_cannot_bypass_the_isolated_catalog(){
        let root=std::env::temp_dir().join(format!("velella-kimi-mcp-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(root.join(".kimi-code")).unwrap();std::fs::write(root.join(".mcp.json"),r#"{"mcpServers":{"velella":{"command":"other"}}}"#).unwrap();assert!(ensure_kimi_project_mcp_control(&root).is_err());std::fs::write(root.join(".mcp.json"),r#"{"mcpServers":{"safe":{"command":"other"}}}"#).unwrap();assert!(ensure_kimi_project_mcp_control(&root).is_err());std::fs::write(root.join(".mcp.json"),r#"{"mcpServers":{}}"#).unwrap();assert!(ensure_kimi_project_mcp_control(&root).is_ok());std::fs::write(root.join(".kimi-code/mcp.json"),r#"{"mcpServers":{"velella":{"command":"other"}}}"#).unwrap();assert!(ensure_kimi_project_mcp_control(&root).is_err());std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_skill_snapshot_freezes_supporting_resources(){
        let root=std::env::temp_dir().join(format!("velella-skills-{}",uuid::Uuid::now_v7()));let source=root.join("source");std::fs::create_dir_all(source.join("references")).unwrap();std::fs::write(source.join("SKILL.md"),"Read references/rules.txt").unwrap();std::fs::write(source.join("references/rules.txt"),"fixed rules").unwrap();
        let digest=skill_digest(&source).unwrap();let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("kimi","goal")["roles"][0].clone()).unwrap();role.skills.push(super::super::orchestration::v2::Skill{path:source.join("SKILL.md").to_string_lossy().into_owned(),digest:digest.clone()});
        let bindings=freeze_skills(&root.join("role"),&role).unwrap();let frozen=Path::new(bindings[0]["path"].as_str().unwrap());assert_eq!(skill_digest(frozen).unwrap(),digest);
        std::fs::write(source.join("references/rules.txt"),"changed rules").unwrap();assert_ne!(skill_digest(&source).unwrap(),digest);assert_eq!(skill_digest(frozen).unwrap(),digest);
        assert_eq!(freeze_skills(&root.join("role"),&role).unwrap(),bindings,"the same launch reuses the verified fixed snapshot");
        assert!(freeze_skills(&root.join("other-role"),&role).is_err(),"a new launch cannot silently capture changed resources");
        std::fs::write(frozen.parent().unwrap().join("references/rules.txt"),"tampered").unwrap();assert!(freeze_skills(&root.join("role"),&role).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn provider_gemini_hooks_preserve_existing_handlers_and_context(){
        let mut config=json!({"hooks":{"SessionStart":[{"existing":true}],"AfterAgent":[{"audit":true}]},"context":{"fileName":["CUSTOM.md"]}});
        append_hooks(&mut config,json!({"SessionStart":[{"velella":true}],"BeforeAgent":[{"velella":true}]})).unwrap();assert_eq!(config["hooks"]["SessionStart"].as_array().unwrap().len(),2);assert_eq!(config["hooks"]["AfterAgent"][0]["audit"],true);assert_eq!(config["context"]["fileName"],json!(["CUSTOM.md"]));
    }
    #[test]fn provider_private_profile_does_not_copy_into_an_existing_directory(){
        let root=std::env::temp_dir().join(format!("velella-private-{}",uuid::Uuid::now_v7()));private_directory(&root).unwrap();assert!(private_directory(&root).is_err());
        #[cfg(windows)]{
            let output=crate::process::command("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",r#"$ErrorActionPreference='Stop'; $acl=[System.IO.Directory]::GetAccessControl($env:VELELLA_PRIVATE_PROFILE); $sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; $rules=@($acl.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])); [Console]::WriteLine($acl.AreAccessRulesProtected -and $rules.Count -eq 1 -and $rules[0].IdentityReference.Value -eq $sid)"#]).env("VELELLA_PRIVATE_PROFILE",&root).output().unwrap();assert!(output.status.success(),"ACL verification failed: {}",String::from_utf8_lossy(&output.stderr));assert_eq!(String::from_utf8_lossy(&output.stdout).trim(),"True");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
