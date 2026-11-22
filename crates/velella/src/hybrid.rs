//! Remote Codex for a local authoritative workspace. Session hosts own lifetime.
pub(crate) mod tools;
mod transport;
#[cfg(windows)] mod secure_windows;
mod lifecycle;
mod turn;
mod ssh_snapshot;
mod mcp_policy;
mod app_turn;
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{fs,io::{BufRead,BufReader,Read,Write},path::{Path,PathBuf},process::{Child,Command,Stdio}};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config{
    pub session_id:String,pub workspace_id:String,pub project_root:PathBuf,pub directory:PathBuf,
    pub profile:Value,pub profile_fingerprint:String,
    #[serde(default,skip_serializing_if="Option::is_none")] pub collaboration_data:Option<PathBuf>,
    #[serde(default,skip_serializing_if="is_false")] pub collaboration_writes:bool,
}
fn is_false(value:&bool)->bool{!*value}
pub(crate) fn load(directory:&Path)->Result<Config>{
    let config:Config=serde_json::from_value(tools::document(&directory.join("hybrid.json"))?)?;
    anyhow::ensure!(config.directory==directory&&config.project_root.is_absolute(),"hybrid config path mismatch");uuid::Uuid::parse_str(&config.session_id)?;
    if let Some(data)=&config.collaboration_data {anyhow::ensure!(data.is_absolute()&&dunce::canonicalize(data)?==*data,"invalid collaboration data root");}
    anyhow::ensure!(!config.collaboration_writes||config.collaboration_data.is_some(),"collaboration write authority requires a bound session");
    anyhow::ensure!(config.profile_fingerprint==tools::digest(&serde_json::to_vec(&config.profile)?),"SSH profile fingerprint mismatch");Ok(config)
}
fn quote(s:&str)->String{format!("'{}'",s.replace('\'',"'\"'\"'"))}
fn remote_skill_policy(config:&Config,root:&str,target:Option<&ssh_snapshot::DirectTarget>)->Result<String>{
    let mut command=ssh_base(&config.profile,false,false,target.is_some())?;
    let script=format!("exec python3 {} policy",quote(&format!("{root}/provider.py")));
    ssh_destination(&mut command,&config.profile,target)?;command.arg(format!("bash -lc {}",quote(&script)));
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child=command.spawn().context("remote skill preflight could not start")?;
    let output=child.stdout.take().context("remote skill preflight output missing")?;
    let reader=std::thread::spawn(move||{let mut bytes=Vec::new();let result=output.take(256*1024+1).read_to_end(&mut bytes);(result,bytes)});
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    loop {if child.try_wait()?.is_some(){break}if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();anyhow::bail!("remote skill preflight timed out; no role started")};std::thread::sleep(std::time::Duration::from_millis(30));}
    let status=child.wait()?;anyhow::ensure!(status.success(),"remote skill preflight failed; no role started");
    let drain_deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);while !reader.is_finished()&&std::time::Instant::now()<drain_deadline{std::thread::sleep(std::time::Duration::from_millis(10));}
    anyhow::ensure!(reader.is_finished(),"remote skill preflight output did not close");let (result,bytes)=reader.join().map_err(|_|anyhow::anyhow!("remote skill preflight reader failed"))?;result?;
    anyhow::ensure!(bytes.len()<=256*1024,"remote skill catalog exceeds limit");let value:Value=serde_json::from_slice(&bytes)?;
    let rules=validate_remote_skill_policy(&value)?;
    let path=config.directory.join("hybrid-skill-policy.json");
    if path.exists(){anyhow::ensure!(tools::document(&path)?==value,"remote skill catalog changed; do not resume this role automatically");}
    else{crate::ipc::atomic_json(&path,&value)?;}
    Ok(rules)
}
fn validate_remote_skill_policy(value:&Value)->Result<String>{
    let catalog=value["catalog"].as_array().context("remote skill catalog missing")?;anyhow::ensure!(catalog.len()<=2048,"remote skill catalog exceeds limit");
    let mut names=std::collections::BTreeSet::new();let mut paths=std::collections::HashSet::new();
    for entry in catalog {let name=entry["name"].as_str().context("remote skill name missing")?;let path=entry["path"].as_str().context("remote skill path missing")?;
        anyhow::ensure!(!name.is_empty()&&name.len()<=1024&&!name.chars().any(char::is_control)&&path.starts_with('/')&&path.len()<=4096&&paths.insert(path)&&entry["enabled"]==false,"invalid or enabled remote skill");names.insert(name);}
    let rules=format!("skills.config=[{}]",names.into_iter().map(|name|format!("{{name={},enabled=false}}",serde_json::to_string(name).unwrap())).collect::<Vec<_>>().join(","));
    anyhow::ensure!(rules.len()<=24000&&value["rules"]==rules,"remote skill policy did not match catalog");
    Ok(rules)
}
/// Validate the locally persisted role and execution binding before recovery
/// or reuse. No credential is included in the remote role snapshot.
pub(crate) fn verify_role_binding(desc:&crate::host::Descriptor)->Result<()> {
    use crate::collaboration::{orchestration::v2::RoleDefinition,providers};
    anyhow::ensure!(desc.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker"),"not a hybrid worker");
    let role_path=desc.directory.join("hybrid-role.json");let meta=fs::symlink_metadata(&role_path)?;
    anyhow::ensure!(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=1024*1024,"invalid remote role snapshot");
    let role:RoleDefinition=serde_json::from_value(tools::document(&role_path)?)?;
    providers::validate_remote(&role)?;
    let remote=role.remote_codex.as_ref().context("remote role binding missing")?;
    anyhow::ensure!(remote.ssh_target_fingerprint.is_some(),"远程角色缺少已核对的 SSH 目标，请重新选择配置");
    anyhow::ensure!(desc.spawn.env.get("VELELLA_ROLE_CONFIG_FINGERPRINT")==Some(&providers::role_fingerprint(&role)?),"remote role snapshot changed");
    let config=load(&desc.directory)?;let data=config.collaboration_data.as_deref().context("remote collaboration data missing")?;
    anyhow::ensure!(config.session_id==desc.id&&config.collaboration_writes==role.writes(),"remote role authority changed");
    anyhow::ensure!(config.profile["id"]==remote.ssh_profile_id&&config.profile_fingerprint==remote.ssh_profile_fingerprint,"remote SSH profile binding changed");
    anyhow::ensure!(desc.spawn.cwd.as_deref().map(Path::new)==Some(config.project_root.as_path()),"remote role workspace changed");
    anyhow::ensure!(desc.endpoint==crate::ipc::endpoint(data,&desc.id)&&desc.spawn.env.get("VELELLA_SESSION_ENDPOINT")==Some(&desc.endpoint),"remote worker endpoint changed");
    anyhow::ensure!(desc.spawn.env.get("VELELLA_RUNTIME_EXE")==Some(&desc.spawn.program)&&Path::new(&desc.spawn.program).is_absolute(),"remote worker runtime changed");
    anyhow::ensure!(desc.spawn.args==["hybrid-worker",desc.directory.to_string_lossy().as_ref()]&&desc.spawn.env.get("VELELLA_HYBRID_MODEL")==Some(&providers::effective_model(&role)),"remote worker arguments changed");
    Ok(())
}
fn ssh_base(profile:&Value,tty:bool,forward:bool,fixed:bool)->Result<Command>{
    let host=profile["host"].as_str().context("SSH host missing")?;
    anyhow::ensure!(!host.is_empty()&&!host.starts_with('-')&&host.len()<=255&&host.bytes().all(|b|b.is_ascii_alphanumeric()||b"._:-[]".contains(&b)),"invalid SSH host");
    let mut c=crate::process::command("ssh");c.arg(if tty{"-tt"}else{"-T"});
    for opt in ["BatchMode=yes","StrictHostKeyChecking=yes","ForwardAgent=no","ForwardX11=no","PermitLocalCommand=no","RemoteCommand=none","ControlMaster=no","ControlPath=none","ConnectTimeout=10","ConnectionAttempts=1","ServerAliveInterval=15","ServerAliveCountMax=2","ExitOnForwardFailure=yes"]{c.args(["-o",opt]);}
    if !forward{c.args(["-o","ClearAllForwardings=yes"]);}
    c.args(["-p",&profile["port"].as_u64().unwrap_or(22).to_string()]);
    if !fixed {
        if let Some(user)=profile["user"].as_str().filter(|s|!s.is_empty()) {anyhow::ensure!(!user.starts_with('-')&&user.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b)),"invalid SSH user");c.args(["-l",user]);}
        if let Some(identity)=profile["identity"].as_str().filter(|s|!s.is_empty()){anyhow::ensure!(Path::new(identity).is_absolute()&&!identity.contains(['\n','\r','\0']),"invalid SSH key reference");c.args(["-i",identity]);}
    }
    Ok(c)
}
fn ssh_destination(command:&mut Command,profile:&Value,target:Option<&ssh_snapshot::DirectTarget>)->Result<()> {
    if let Some(target)=target{command.args(target.fixed_args());}
    else{command.arg("--").arg(profile["host"].as_str().context("SSH host missing")?);}
    Ok(())
}
fn ssh_query(command:Command)->Result<String>{ssh_query_input(command,None)}
fn ssh_query_input(mut command:Command,input:Option<&'static [u8]>)->Result<String>{
    command.stdout(Stdio::piped()).stderr(Stdio::null()).stdin(if input.is_some(){Stdio::piped()}else{Stdio::null()});
    let mut child=command.spawn().context("cannot inspect SSH effective configuration")?;
    let writer=if let Some(input)=input{let mut pipe=child.stdin.take().context("remote configuration input missing")?;Some(std::thread::spawn(move||pipe.write_all(input)))}else{None};
    let output=child.stdout.take().context("SSH configuration output missing")?;
    let reader=std::thread::spawn(move||{let mut bytes=Vec::new();let result=output.take(256*1024+1).read_to_end(&mut bytes);(result,bytes)});
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    let status=loop {if let Some(status)=child.try_wait()?{break status}if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();anyhow::bail!("SSH effective configuration query timed out")};std::thread::sleep(std::time::Duration::from_millis(20));};
    anyhow::ensure!(status.success(),"SSH effective configuration query failed");
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);while !reader.is_finished()&&std::time::Instant::now()<deadline{std::thread::sleep(std::time::Duration::from_millis(10));}
    anyhow::ensure!(reader.is_finished(),"SSH effective configuration output did not close");
    let (result,bytes)=reader.join().map_err(|_|anyhow::anyhow!("SSH configuration reader failed"))?;result?;
    if let Some(writer)=writer{anyhow::ensure!(writer.is_finished(),"remote configuration input did not close");writer.join().map_err(|_|anyhow::anyhow!("remote configuration writer failed"))??;}
    anyhow::ensure!(bytes.len()<=256*1024,"SSH effective configuration exceeds limit");Ok(String::from_utf8(bytes)?)
}
fn resolve_direct_target(profile:&Value)->Result<ssh_snapshot::DirectTarget>{
    let host=profile["host"].as_str().context("SSH host missing")?;
    let mut actual=ssh_base(profile,false,false,false)?;actual.args(["-G","--",host]);let actual=ssh_query(actual)?;
    let mut defaults=ssh_base(profile,false,false,false)?;defaults.args(["-G","-F",if cfg!(windows){"NUL"}else{"/dev/null"},"--",host]);let defaults=ssh_query(defaults)?;
    let target=ssh_snapshot::direct_target(&actual,&defaults)?;
    let mut rebuilt=ssh_base(profile,false,false,true)?;rebuilt.arg("-G");rebuilt.args(target.fixed_args());let rebuilt=ssh_query(rebuilt)?;
    let home=std::env::var_os(if cfg!(windows){"USERPROFILE"}else{"HOME"}).context("SSH home directory unavailable")?;
    ssh_snapshot::verify_reconstruction(&actual,&rebuilt,Path::new(&home))?;Ok(target)
}
fn bind_direct_target(directory:&Path,target:&ssh_snapshot::DirectTarget)->Result<()> {
    let path=directory.join("hybrid-ssh-target.json");let value=serde_json::to_value(target)?;
    if path.exists(){anyhow::ensure!(tools::document(&path)?==value,"SSH 实际目标已变化，请停止并重新核对远程会话");}
    else{crate::ipc::atomic_json(&path,&value)?;}
    Ok(())
}
fn direct_target_fingerprint(target:&ssh_snapshot::DirectTarget)->Result<String>{
    Ok(tools::digest(&serde_json::to_vec(&json!({"version":1,"target":target}))?))
}
fn verify_target_fingerprint(target:&ssh_snapshot::DirectTarget,expected:&str)->Result<()>{
    anyhow::ensure!(direct_target_fingerprint(target)?==expected,"SSH 实际目标已变化，请重新核对远程角色");Ok(())
}
pub(crate) fn preview_direct_target(profile:&Value)->Result<Value>{
    let target=resolve_direct_target(profile)?;
    Ok(json!({"hostname":target.hostname,"user":target.user,"ssh_target_fingerprint":direct_target_fingerprint(&target)?}))
}
fn probe_remote_provider(profile:&Value,target:&ssh_snapshot::DirectTarget)->Result<Value>{
    let mut command=ssh_base(profile,false,false,true)?;ssh_destination(&mut command,profile,Some(target))?;
    command.arg(format!("bash -lc {}",quote("python3 - probe")));
    let value:Value=serde_json::from_str(&ssh_query_input(command,Some(include_bytes!("hybrid/remote_provider.py")))?)?;
    let model=value["model"].as_str().context("remote model missing")?;
    let provider=value["provider"].as_str().context("remote provider missing")?;
    let fingerprint=value["provider_fingerprint"].as_str().context("remote provider fingerprint missing")?;
    let hash=fingerprint.strip_prefix("sha256:").unwrap_or("");
    anyhow::ensure!(model.starts_with("deepseek")&&model.len()<=200&&!model.chars().any(char::is_control)&&!provider.is_empty()&&provider.len()<=80&&hash.len()==64&&hash.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())&&value["credential_available"]==true&&value["wire_api"]=="responses","unsupported remote provider receipt");
    Ok(json!({"model":model,"provider":provider,"provider_fingerprint":fingerprint}))
}
pub(crate) fn preview_remote_binding(profile:&Value)->Result<Value>{
    let target=resolve_direct_target(profile)?;
    let provider=probe_remote_provider(profile,&target)?;
    Ok(json!({"target":{"hostname":target.hostname,"user":target.user,"ssh_target_fingerprint":direct_target_fingerprint(&target)?},"provider":provider}))
}
fn managed_native_binding(directory:&Path,remote:&mut Value)->Result<()> {
    let path=directory.join("hybrid-observed-turn.json");
    if path.exists(){
        let receipt=tools::document(&path)?;
        let id=receipt["native_id"].as_str().context("managed native identity missing")?;
        uuid::Uuid::parse_str(id)?;
        anyhow::ensure!(receipt["state"]=="complete"&&remote["native_id"].as_str().is_none_or(|remote|remote==id),"managed native identity changed");
        remote["native_id"]=json!(id);
    }
    Ok(())
}
// The remote helper writes ONLY generated session assets into a new private
// cache directory. It neither copies a project nor reads authentication files.
const BOOTSTRAP:&str=r#"
const fs=require('fs'),p=require('path'),os=require('os');let b='';
process.stdin.on('data',x=>{b+=x;if(b.length>262144)process.exit(2)});
process.stdin.on('end',()=>{
 const v=JSON.parse(b);if(!/^[0-9a-f-]{36}$/.test(v.session_id)||!/^b-[0-9a-f]{16}\.sock$/.test(v.socket_name))process.exit(2);
 const d=p.join(os.homedir(),'.cache','velella-hybrid',v.session_id);
 fs.mkdirSync(d,{recursive:true,mode:448});if(!fs.lstatSync(d).isDirectory()||fs.lstatSync(d).isSymbolicLink())process.exit(4);fs.chmodSync(d,448);
 const native=p.join(d,'native.json');let id=null;
 if(fs.existsSync(native)){const n=JSON.parse(fs.readFileSync(native,'utf8'));if(n.cwd!==d||!/^[0-9a-f-]{36}$/.test(n.session_id))process.exit(3);id=n.session_id}
 for(const [name,text]of Object.entries({'relay.cjs':v.relay,'identity.cjs':v.identity,'skill-preflight.cjs':v.skill_preflight,'provider.py':v.provider,'bridge.json':JSON.stringify({session_id:v.session_id,token:v.token,socket_name:v.socket_name})})){
  const f=p.join(d,name);if(fs.existsSync(f)&&!fs.lstatSync(f).isFile())process.exit(4);fs.writeFileSync(f,text,{mode:384});fs.chmodSync(f,384)
 }
 console.log(JSON.stringify({directory:d,native_id:id,os:process.platform,node:process.version,socket_name:v.socket_name}))
})"#;
const IDENTITY:&str=r#"const fs=require('fs'),p=require('path');let b='';process.stdin.on('data',x=>{b+=x;if(b.length>1048576)process.exit(2)});process.stdin.on('end',()=>{const v=JSON.parse(b);if(v.agent_id||v.cwd!==__dirname||!/^[0-9a-f-]{36}$/.test(v.session_id))process.exit(2);fs.writeFileSync(p.join(__dirname,'native.json'),JSON.stringify({session_id:v.session_id,cwd:v.cwd}),{mode:384})})"#;
pub(crate) fn run(directory:&Path,exec_prompt:Option<&Path>,probe:bool)->Result<()>{
    let config=load(directory)?;
    let _owner=fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(directory.join("hybrid-owner.lock"))?;
    fs2::FileExt::try_lock_exclusive(&_owner).context("hybrid session already owns a bridge")?;
    let _power=crate::power_continuity::Lease::acquire().context("hybrid CLI AC power protection")?;
    tools::uncertain(&config)?;
    let remote_role=if config.collaboration_data.is_some(){let role:crate::collaboration::orchestration::v2::RoleDefinition=serde_json::from_value(tools::document(&directory.join("hybrid-role.json"))?)?;crate::collaboration::providers::validate_remote(&role)?;Some(role)}else{None};
    let target=if config.collaboration_data.is_some(){
        let role=remote_role.as_ref().context("remote role missing")?;
        let expected=role.remote_codex.as_ref().and_then(|r|r.ssh_target_fingerprint.as_deref()).context("远程角色缺少已核对的 SSH 目标，请重新选择配置")?;
        let target=resolve_direct_target(&config.profile)?;
        verify_target_fingerprint(&target,expected)?;
        bind_direct_target(directory,&target)?;Some(target)
    }else{None};
    let token=format!("{}{}",uuid::Uuid::new_v4(),uuid::Uuid::new_v4());
    let server=transport::Server::start(config.clone(),token.clone())?;
    let mut setup=ssh_base(&config.profile,false,false,target.is_some())?;
    ssh_destination(&mut setup,&config.profile,target.as_ref())?;setup.arg(format!("bash -lc {}",quote(&format!("exec node -e {}",quote(BOOTSTRAP)))));
    setup.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child=setup.spawn()?;
    let socket_name=format!("b-{}.sock",&uuid::Uuid::new_v4().simple().to_string()[..16]);
    let assets=json!({"session_id":config.session_id,"socket_name":socket_name,"token":token,"relay":include_str!("hybrid/relay.cjs"),"identity":IDENTITY,"skill_preflight":include_str!("hybrid/skill_preflight.cjs"),"provider":include_str!("hybrid/remote_provider.py")});
    child.stdin.take().context("SSH bootstrap stdin")?.write_all(&serde_json::to_vec(&assets)?)?;
    // A bounded probe covers startup/network failures; no automatic retry.
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(25);
    while child.try_wait()?.is_none(){if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();anyhow::bail!("SSH relay bootstrap timed out; inspect remote state before retry")};std::thread::sleep(std::time::Duration::from_millis(30));}
    let output=child.wait_with_output()?;anyhow::ensure!(output.status.success(),"SSH relay bootstrap failed; no request replayed");
    anyhow::ensure!(output.stdout.len()<=16384,"remote bootstrap response too large");let mut remote:Value=serde_json::from_slice(&output.stdout)?;
    if remote_role.is_some(){managed_native_binding(directory,&mut remote)?;}
    anyhow::ensure!(remote["os"]=="linux","hybrid Codex currently requires Linux");
    let root=remote["directory"].as_str().context("remote session directory missing")?;
    anyhow::ensure!(root.starts_with('/')&&root.ends_with(&format!("/.cache/velella-hybrid/{}",config.session_id))&&!root.chars().any(char::is_control),"invalid remote scratch directory");
    anyhow::ensure!(root.len()+1+socket_name.len()<108,"remote home path is too long for a Unix forwarding socket");
    if !probe&&directory.join("hybrid-started.json").exists(){anyhow::ensure!(remote["native_id"].as_str().is_some(),"remote native identity not observed; refusing a replacement conversation");}
    crate::ipc::atomic_json(&directory.join("hybrid-remote.json"),&remote)?;
    if probe{return probe_relay(&config,server.port,root,&socket_name,target.as_ref())}
    let skill_rules=if config.collaboration_data.is_some(){Some(remote_skill_policy(&config,root,target.as_ref())?)}else{None};
    // Only this session's server is forwarded to Velella's own per-operation
    // approval ledger; this does not change the policy for other MCP servers.
    let mcp=format!("mcp_servers.velella_local={{command=\"node\",args=[{}],required=true,tool_timeout_sec=125,default_tools_approval_mode=\"approve\"}}",serde_json::to_string(&format!("{root}/relay.cjs"))?);
    let hook=format!("hooks.SessionStart=[{{hooks=[{{type=\"command\",command={},timeout=3}}]}}]",serde_json::to_string(&format!("node {}",quote(&format!("{root}/identity.cjs"))))?);
    let mut argv=vec!["codex".to_string(),"-c".into(),mcp,"-a".into(),"on-request".into(),"-s".into(),"workspace-write".into()];
    // Managed exec obtains native identity from authenticated structured output;
    // adding a native Hook would introduce an unrelated trust prompt.
    if remote_role.is_none(){argv.extend(["-c".into(),hook]);}
    if config.collaboration_data.is_some(){
        anyhow::ensure!(exec_prompt.is_some(),"managed remote roles require structured execution");
        let model=std::env::var("VELELLA_HYBRID_MODEL").context("managed remote role model missing")?;
        anyhow::ensure!(!model.trim().is_empty()&&model.len()<=200&&!model.chars().any(char::is_control),"invalid managed remote model");
        argv.extend(["-c".into(),"features.fast_mode=false".into(),"-c".into(),"features.apps=false".into(),"-c".into(),"features.multi_agent=false".into(),"-c".into(),skill_rules.context("remote skill policy missing")?,"-m".into(),model]);
    }
    if let Some(prompt)=exec_prompt{
        let bytes=fs::read(prompt)?;anyhow::ensure!(bytes.len()<=65536,"test prompt too large");let text=String::from_utf8(bytes)?;
        argv.extend(["exec".into(),"--json".into(),"--skip-git-repo-check".into()]);
        if config.collaboration_data.is_some(){argv.push("--ignore-user-config".into());}
        if let Some(id)=remote["native_id"].as_str(){argv.extend(["resume".into(),id.into()]);}
        argv.push(text);
    }else{
        if let Some(id)=remote["native_id"].as_str(){argv.extend(["resume".into(),id.into()]);}argv.push("--no-alt-screen".into());
    }
    let invocation=if let Some(role)=&remote_role {
        use base64::Engine;
        let expected=role.remote_codex.as_ref().and_then(|remote|remote.provider_fingerprint.as_deref()).context("remote provider binding missing")?;
        let encoded=base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&argv[1..])?);
        format!("python3 {} exec {} {}",quote(&format!("{root}/provider.py")),quote(expected),quote(&encoded))
    }else{argv.iter().map(|s|quote(s)).collect::<Vec<_>>().join(" ")};
    // Ubuntu can deny user namespaces to unconfined processes. If the existing
    // unconfined linux-sandbox AppArmor profile makes Codex's actual sandbox
    // work, use it for this process only; never relax an already confined shell.
    let command=format!("cd -- {} && if [ \"$(cat /proc/self/attr/current 2>/dev/null)\" = unconfined ] && command -v aa-exec >/dev/null 2>&1 && aa-exec -p linux-sandbox -- codex sandbox -- /bin/true >/dev/null 2>&1; then exec aa-exec -p linux-sandbox -- {}; else exec {}; fi{}",quote(root),invocation,invocation,if exec_prompt.is_some(){" < /dev/null"}else{""});
    let mut remote_process=ssh_base(&config.profile,exec_prompt.is_none(),true,target.is_some())?;
    remote_process.args(["-o","StreamLocalBindMask=0177","-R",&format!("{root}/{socket_name}:127.0.0.1:{}",server.port)]);
    if exec_prompt.is_some(){remote_process.arg("-n");remote_process.stdin(Stdio::null()).stdout(Stdio::piped());}
    ssh_destination(&mut remote_process,&config.profile,target.as_ref())?;remote_process.arg(format!("bash -lc {}",quote(&command)));
    eprintln!("远程 Codex：{}；本机项目：{}。本机写入和命令等待 Velella 审批。",config.profile["host"].as_str().unwrap(),config.project_root.display());
    // Persist before launching: loss of the local process must never turn an
    // unobserved remote execution into a confirmed shutdown or a safe retry.
    if config.collaboration_data.is_some(){crate::ipc::atomic_json(&directory.join("hybrid-turn-state.json"),&json!({"state":"uncertain","session_id":config.session_id}))?;}
    let mut child=spawn_marked(&mut remote_process,&directory.join("hybrid-started.json"),&json!({"started":true,"session_id":config.session_id,"remote_directory":root}))?;
    let observation=if exec_prompt.is_some(){
        let stream=child.stdout.take().context("remote event stream unavailable")?;
        match observe_exec(BufReader::new(stream),&mut std::io::stdout().lock(),remote["native_id"].as_str()){
            Ok(turn)=>Some(turn),Err(error)=>{
                let kill=child.kill().err();let wait=child.wait().err();drop(server);let cleanup=tools::uncertain(&config).err();
                anyhow::bail!("remote event stream incomplete; inspect audit before recovering: {error:#}; local SSH kill error: {kill:?}; local SSH wait error: {wait:?}; audit cleanup error: {cleanup:?}; remote process termination not confirmed");
            }
        }
    }else{None};
    let result=child.wait();drop(server);let cleanup=tools::uncertain(&config);
    let status=match result {Ok(status)=>status,Err(error)=>anyhow::bail!("local SSH wait failed; remote process termination not confirmed: {error}; audit cleanup error: {:?}",cleanup.err())};
    cleanup.context("remote process exited but audit cleanup failed; inspect pending operations")?;
    if let Some(turn)=observation {
        anyhow::ensure!(turn.finish(status.code())==turn::Outcome::Complete,"remote Codex turn failed or its outcome is uncertain; inspect audit before recovering");
        crate::ipc::atomic_json(&directory.join("hybrid-observed-turn.json"),&json!({"native_id":turn.native().context("completed turn has no native identity")?,"state":"complete"}))?;
        if config.collaboration_data.is_some(){crate::ipc::atomic_json(&directory.join("hybrid-turn-state.json"),&json!({"state":"complete","session_id":config.session_id,"native_id":turn.native()}))?;}
    }
    else {anyhow::ensure!(status.success(),"remote Codex exited or disconnected; inspect audit before recovering");}Ok(())
}
/// Call only after the local worker is confirmed dead. A complete receipt means
/// the structured turn completed and its SSH command exited successfully.
pub(crate) fn ensure_worker_shutdown_known(desc:&crate::host::Descriptor)->Result<()> {
    if desc.spawn.adapter.as_deref()!=Some("hybrid-orchestration-worker"){return Ok(())}
    let path=desc.directory.join("hybrid-turn-state.json");
    if !path.exists(){anyhow::ensure!(!desc.directory.join("hybrid-started.json").exists(),"远端执行结果未知；本机 SSH 已结束不能证明远端进程停止，禁止自动重试");return Ok(())}
    let receipt=tools::document(&path)?;
    anyhow::ensure!(receipt["state"]=="complete"&&receipt["session_id"]==desc.id&&receipt["native_id"].as_str().is_some_and(|id|uuid::Uuid::parse_str(id).is_ok()),"远端执行结果未知；本机 SSH 已结束不能证明远端进程停止，禁止自动重试");
    Ok(())
}
fn observe_exec(mut input:impl BufRead,output:&mut impl Write,expected:Option<&str>)->Result<turn::Turn>{
    let mut turn=turn::Turn::default();
    loop {
        let mut line=Vec::new();let size=(&mut input).take(2*1024*1024+1).read_until(b'\n',&mut line)?;
        if size==0 {break}
        output.write_all(&line)?;output.flush()?;
        anyhow::ensure!(size<=2*1024*1024&&line.last()==Some(&b'\n'),"invalid or oversized remote event frame");
        turn.event(&line,expected)?;
    }Ok(turn)
}
/// A session-owned task receiver. It reports local readiness separately from
/// remote native readiness and refuses another turn after any uncertain result.
pub(crate) fn worker(directory:&Path)->Result<()> {
    use base64::Engine;
    let config=load(directory)?;
    anyhow::ensure!(config.collaboration_data.is_some(),"hybrid worker requires a collaboration binding");
    let descriptor:crate::host::Descriptor=serde_json::from_value(tools::document(&directory.join("launch.json"))?)?;
    anyhow::ensure!(descriptor.directory==directory,"worker descriptor directory mismatch");verify_role_binding(&descriptor)?;
    let endpoint=std::env::var("VELELLA_SESSION_ENDPOINT").context("worker session endpoint missing")?;
    let notify=|event:&str,native:Option<&str>|->Result<()>{
        let reply=crate::ipc::request_with_timeout(&endpoint,&json!({"cmd":"hybrid_worker_event","event":event,"native_id":native}),std::time::Duration::from_secs(5))?;
        anyhow::ensure!(reply["ok"]==true,"hybrid worker event rejected");Ok(())
    };
    notify("ready",None)?;
    let stdin=std::io::stdin();let mut input=stdin.lock();
    loop {
        let mut line=Vec::new();let size=(&mut input).take(100000).read_until(b'\n',&mut line)?;
        if size==0 {break}
        anyhow::ensure!(size<100000&&line.last()==Some(&b'\n'),"invalid worker input frame");
        while line.last().is_some_and(|b|matches!(*b,b'\r'|b'\n')){line.pop();}
        let bytes=base64::engine::general_purpose::STANDARD.decode(&line).context("invalid worker input encoding")?;
        anyhow::ensure!(!bytes.is_empty()&&bytes.len()<=65536&&std::str::from_utf8(&bytes).is_ok(),"invalid worker prompt");
        notify("running",None)?;
        let prompt=directory.join(format!("hybrid-worker-prompt-{}.txt",uuid::Uuid::new_v4()));
        let result=(||->Result<String>{
            crate::ipc::atomic_write(&prompt,&bytes)?;run(directory,Some(&prompt),false)?;
            let result=tools::document(&directory.join("hybrid-observed-turn.json"))?;
            Ok(result["native_id"].as_str().context("completed turn identity missing")?.to_owned())
        })();
        let _=fs::remove_file(&prompt);
        match result {Ok(native)=>notify("completed",Some(&native))?,Err(error)=>{let _=notify("uncertain",None);return Err(error.context("remote turn interrupted; do not replay automatically"));}}
    }Ok(())
}
fn spawn_marked(command:&mut Command,path:&Path,value:&Value)->Result<Child>{
    let mut child=command.spawn()?;
    if let Err(error)=crate::ipc::atomic_json(path,value){let _=child.kill();let _=child.wait();return Err(error)}
    Ok(child)
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn managed_identity_uses_only_a_completed_observed_native_session(){
        let root=std::env::temp_dir().join(format!("hybrid-native-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let id=uuid::Uuid::new_v4().to_string();let path=root.join("hybrid-observed-turn.json");
        crate::ipc::atomic_json(&path,&json!({"state":"complete","native_id":id})).unwrap();
        let mut remote=json!({"native_id":null});managed_native_binding(&root,&mut remote).unwrap();assert_eq!(remote["native_id"],id);
        remote["native_id"]=json!(uuid::Uuid::new_v4().to_string());assert!(managed_native_binding(&root,&mut remote).is_err());
        crate::ipc::atomic_json(&path,&json!({"state":"uncertain","native_id":id})).unwrap();assert!(managed_native_binding(&root,&mut json!({})).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn managed_ssh_target_is_frozen_across_turns(){
        let root=std::env::temp_dir().join(format!("hybrid-target-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let target=ssh_snapshot::DirectTarget{hostname:"first.example".into(),user:"fixture".into(),identities_only:"yes".into(),identity_files:vec!["/fixture/key".into()]};
        bind_direct_target(&root,&target).unwrap();bind_direct_target(&root,&target).unwrap();
        let fingerprint=direct_target_fingerprint(&target).unwrap();
        verify_target_fingerprint(&target,&fingerprint).unwrap();
        let changed=ssh_snapshot::DirectTarget{hostname:"second.example".into(),..target};
        assert_ne!(fingerprint,direct_target_fingerprint(&changed).unwrap());
        assert!(verify_target_fingerprint(&changed,&fingerprint).is_err());
        assert!(bind_direct_target(&root,&changed).is_err());fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "read-only ssh -G check against the locally configured agent alias"]
    fn resolve_direct_agent_alias_without_connecting(){
        let profile=json!({"host":"agent","port":22,"user":"","identity":""});
        let target=resolve_direct_target(&profile).unwrap();
        let preview=preview_direct_target(&profile).unwrap();
        assert_eq!(preview["ssh_target_fingerprint"],direct_target_fingerprint(&target).unwrap());
        let mut command=ssh_base(&profile,false,true,true).unwrap();
        command.args(["-o","StreamLocalBindMask=0177"]);
        ssh_destination(&mut command,&profile,Some(&target)).unwrap();
        let args:Vec<_>=command.get_args().map(|v|v.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|v|v==["-F","NUL"]));
        assert_eq!(args.last(),Some(&target.hostname));
    }
    #[test]fn older_hybrid_config_does_not_enable_collaboration(){
        let id=uuid::Uuid::new_v4().to_string();
        let value=json!({"session_id":id,"workspace_id":"w","project_root":"C:/project","directory":"C:/session","profile":{"host":"agent"},"profile_fingerprint":"fixture"});
        let config:Config=serde_json::from_value(value).unwrap();assert!(config.collaboration_data.is_none());
        assert!(serde_json::to_value(config).unwrap().get("collaboration_data").is_none());
    }
    #[test]fn remote_skill_policy_rejects_enabled_duplicates_and_forged_rules(){
        let mut v=json!({"catalog":[{"name":"safe","path":"/home/u/skills/safe","enabled":false}],"rules":"skills.config=[{name=\"safe\",enabled=false}]"});
        assert!(validate_remote_skill_policy(&v).is_ok());v["catalog"][0]["enabled"]=json!(true);assert!(validate_remote_skill_policy(&v).is_err());
        v["catalog"][0]["enabled"]=json!(false);v["rules"]=json!("skills.config=[]");assert!(validate_remote_skill_policy(&v).is_err());
        v["rules"]=json!("skills.config=[{name=\"safe\",enabled=false}]");let duplicate=v["catalog"][0].clone();v["catalog"].as_array_mut().unwrap().push(duplicate);assert!(validate_remote_skill_policy(&v).is_err());
    }
    #[test]fn exec_stream_preserves_events_and_rejects_partial_frames(){
        let id=uuid::Uuid::new_v4().to_string();let lines=[json!({"type":"thread.started","thread_id":id}),json!({"type":"turn.started"}),json!({"type":"turn.completed"})].iter().map(|v|format!("{v}\n")).collect::<String>();
        let mut out=Vec::new();let turn=observe_exec(lines.as_bytes(),&mut out,Some(&id)).unwrap();assert_eq!(out,lines.as_bytes());assert_eq!(turn.finish(Some(0)),turn::Outcome::Complete);
        let partial=b"{\"type\":\"thread.started\"}";let mut out=Vec::new();assert!(observe_exec(partial.as_slice(),&mut out,None).is_err());assert_eq!(out,partial);
        assert!(observe_exec(vec![b'x';2*1024*1024+1].as_slice(),&mut Vec::new(),None).is_err());
    }
    #[test]fn remote_worker_binding_rejects_authority_or_profile_changes(){
        let root=std::env::temp_dir().join(format!("hybrid-role-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();let root=dunce::canonicalize(root).unwrap();
        let project=root.join("project");fs::create_dir(&project).unwrap();let session=root.join("session");fs::create_dir(&session).unwrap();let data=root.join("data");fs::create_dir(&data).unwrap();
        let id=uuid::Uuid::new_v4().to_string();let profile_id=uuid::Uuid::new_v4().to_string();let profile=json!({"id":profile_id,"host":"agent"});let fingerprint=tools::digest(&serde_json::to_vec(&profile).unwrap());
        let mut plan=crate::collaboration::orchestration::v2::seed("codex","fixture");plan["roles"][2]["remote_codex"]=json!({"ssh_profile_id":profile_id,"ssh_profile_fingerprint":fingerprint,"ssh_target_fingerprint":format!("sha256:{}","a".repeat(64)),"remote_model":"deepseek-flash","provider_fingerprint":format!("sha256:{}","b".repeat(64))});
        let role=crate::collaboration::orchestration::v2::validate(&plan).unwrap().roles[2].clone();
        let config=Config{session_id:id.clone(),workspace_id:"w".into(),project_root:project.clone(),directory:session.clone(),profile,profile_fingerprint:fingerprint,collaboration_data:Some(data.clone()),collaboration_writes:true};
        crate::ipc::atomic_json(&session.join("hybrid.json"),&serde_json::to_value(&config).unwrap()).unwrap();crate::ipc::atomic_json(&session.join("hybrid-role.json"),&serde_json::to_value(&role).unwrap()).unwrap();
        let endpoint=crate::ipc::endpoint(&data,&id);let runtime=std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let mut desc=crate::host::Descriptor{id,endpoint:endpoint.clone(),directory:session.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:runtime.clone(),args:vec!["hybrid-worker".into(),session.to_string_lossy().into_owned()],cwd:Some(project.to_string_lossy().into_owned()),env:std::collections::HashMap::from([("VELELLA_SESSION_ENDPOINT".into(),endpoint),("VELELLA_RUNTIME_EXE".into(),runtime),("VELELLA_HYBRID_MODEL".into(),crate::collaboration::providers::effective_model(&role)),("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),crate::collaboration::providers::role_fingerprint(&role).unwrap())]),adapter:Some("hybrid-orchestration-worker".into())}};
        verify_role_binding(&desc).unwrap();desc.spawn.env.insert("VELELLA_HYBRID_MODEL".into(),"other".into());assert!(verify_role_binding(&desc).is_err());
        desc.spawn.env.insert("VELELLA_HYBRID_MODEL".into(),crate::collaboration::providers::effective_model(&role));let mut altered=config.clone();altered.collaboration_writes=false;crate::ipc::atomic_json(&session.join("hybrid.json"),&serde_json::to_value(altered).unwrap()).unwrap();assert!(verify_role_binding(&desc).is_err());
        assert!(ensure_worker_shutdown_known(&desc).is_ok());
        crate::ipc::atomic_json(&session.join("hybrid-started.json"),&json!({"started":true})).unwrap();
        assert!(ensure_worker_shutdown_known(&desc).is_err());
        crate::ipc::atomic_json(&session.join("hybrid-turn-state.json"),&json!({"state":"uncertain","session_id":desc.id})).unwrap();
        assert!(ensure_worker_shutdown_known(&desc).is_err());
        crate::ipc::atomic_json(&session.join("hybrid-turn-state.json"),&json!({"state":"complete","session_id":desc.id,"native_id":uuid::Uuid::new_v4().to_string()})).unwrap();
        assert!(ensure_worker_shutdown_known(&desc).is_ok());
        crate::ipc::atomic_json(&session.join("hybrid-turn-state.json"),&json!({"state":"complete","session_id":"other","native_id":uuid::Uuid::new_v4().to_string()})).unwrap();
        assert!(ensure_worker_shutdown_known(&desc).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]fn failed_ssh_spawn_does_not_poison_native_recovery(){
        let root=std::env::temp_dir().join(format!("hybrid-spawn-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let marker=root.join("hybrid-started.json");let mut command=Command::new(root.join("nonexistent-ssh-program"));
        assert!(spawn_marked(&mut command,&marker,&json!({"started":true})).is_err());assert!(!marker.exists());fs::remove_dir(root).unwrap();
    }
}
fn probe_relay(config:&Config,port:u16,root:&str,socket_name:&str,target:Option<&ssh_snapshot::DirectTarget>)->Result<()>{
    let mut command=ssh_base(&config.profile,false,true,target.is_some())?;
    command.args(["-o","StreamLocalBindMask=0177","-R",&format!("{root}/{socket_name}:127.0.0.1:{port}")]);
    ssh_destination(&mut command,&config.profile,target)?;command.arg(format!("bash -lc {}",quote(&format!("exec node {}",quote(&format!("{root}/relay.cjs"))))));
    let mut child=command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    {let mut input=child.stdin.take().unwrap();for value in [json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"velella-probe","version":"1"}}}),json!({"jsonrpc":"2.0","method":"notifications/initialized"}),json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"local_read_file","arguments":{"path":"seed.txt"}}})]{writeln!(input,"{value}")?;}}
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(25);
    while child.try_wait()?.is_none(){if std::time::Instant::now()>=deadline{let _=child.kill();let _=child.wait();anyhow::bail!("MCP protocol probe timed out")};std::thread::sleep(std::time::Duration::from_millis(30));}
    let out=child.wait_with_output()?;let output=String::from_utf8(out.stdout)?;
    crate::ipc::atomic_json(&config.directory.join("hybrid-probe.json"),&json!({"exit":out.status.code(),"stdout":output,"stderr":String::from_utf8_lossy(&out.stderr)}))?;
    anyhow::ensure!(out.status.success(),"MCP relay process failed; inspect hybrid-probe.json");
    let responses=output.lines().map(serde_json::from_str::<Value>).collect::<std::result::Result<Vec<_>,_>>()?;
    anyhow::ensure!(responses.len()==3&&responses.iter().any(|r|r["id"]==3&&r["result"]["isError"]!=true),"MCP round trip incomplete; inspect hybrid-probe.json");
    println!("{}",json!({"ok":true,"responses":responses}));Ok(())
}
