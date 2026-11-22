//! OMP 18.12: invocation-local native discovery and credential snapshot.
use super::*;
use rusqlite::{Connection, OpenFlags, params};

// Keep native MCP discovery. Its mcp.json reader is not a server allowlist by
// itself: loadCapability supplies the servers. Disable every non-native source.
const DISABLED: &[&str] = &["agent-plugins","agents-md","claude-md","custom-context-files","builtin-defaults","claude","claude-plugins","cline","agents","codex","cursor","gemini","opencode","github","mcp-json","omp-plugins","skillshare","ssh","vscode","windsurf"];

pub(super) fn auth_source() -> Result<PathBuf> {
    anyhow::ensure!(cfg!(windows), "OMP credential path resolution is currently verified on Windows only");
    anyhow::ensure!(std::env::var("OMP_AUTH_BROKER_URL").unwrap_or_default().trim().is_empty(), "OMP remote auth broker requires a separate invocation policy; local account was not changed");
    let home=PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).context("OMP user home is unavailable")?);
    let config=std::env::var("PI_CONFIG_DIR").ok().filter(|s|!s.is_empty()).unwrap_or_else(||".omp".into());
    anyhow::ensure!(!Path::new(&config).is_absolute() && !config.contains(':') && !Path::new(&config).components().any(|c|matches!(c,std::path::Component::ParentDir)), "OMP config root must be a relative directory under home");
    let profile=std::env::var("OMP_PROFILE").or_else(|_|std::env::var("PI_PROFILE")).unwrap_or_default();let profile=profile.trim();
    let agent=if profile.is_empty()||profile=="default" {
        std::env::var_os("PI_CODING_AGENT_DIR").filter(|s|!s.is_empty()).map(PathBuf::from).unwrap_or_else(||home.join(&config).join("agent"))
    } else {
        anyhow::ensure!(profile.len()<=64 && profile.as_bytes()[0].is_ascii_alphanumeric() && profile.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'.'|b'_'|b'-')) && !profile.ends_with('.'), "invalid OMP profile");
        home.join(&config).join("profiles").join(profile).join("agent")
    };
    Ok(agent.join("agent.db"))
}

const AUTH_SCHEMA: &str = "CREATE TABLE auth_credentials (id INTEGER PRIMARY KEY AUTOINCREMENT,provider TEXT NOT NULL,credential_type TEXT NOT NULL,data TEXT NOT NULL,disabled_cause TEXT DEFAULT NULL,identity_key TEXT DEFAULT NULL,created_at INTEGER NOT NULL DEFAULT (unixepoch()),updated_at INTEGER NOT NULL DEFAULT (unixepoch())); CREATE TABLE auth_credential_blocks (credential_id INTEGER NOT NULL,provider_key TEXT NOT NULL,block_scope TEXT NOT NULL DEFAULT '',blocked_until_ms INTEGER NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY (credential_id,provider_key,block_scope));";

fn snapshot_auth(source:&Path,target:&Path)->Result<()> {
    let mut dest=Connection::open(target)?;dest.execute_batch(AUTH_SCHEMA)?;
    if !source.try_exists()? {return Ok(())}
    for path in [source.to_owned(),PathBuf::from(format!("{}-wal",source.display())),PathBuf::from(format!("{}-shm",source.display()))] {
        if path.try_exists()? {let meta=std::fs::symlink_metadata(path)?;anyhow::ensure!(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=64*1024*1024,"OMP credential database must be a bounded ordinary file");}
    }
    let mut src=Connection::open_with_flags(source,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    src.busy_timeout(Duration::from_secs(2))?;src.execute_batch("PRAGMA query_only=ON;")?;
    let read=src.transaction()?;let write=dest.transaction()?;
    let mut statement=read.prepare("SELECT id,provider,credential_type,data,identity_key,created_at,updated_at FROM auth_credentials WHERE disabled_cause IS NULL")?;
    let mut rows=statement.query([])?;let mut count=0;let mut size=0;
    while let Some(row)=rows.next()? {
        let id:i64=row.get(0)?;let provider:String=row.get(1)?;let kind:String=row.get(2)?;let data:String=row.get(3)?;let identity:Option<String>=row.get(4)?;let created:i64=row.get(5)?;let updated:i64=row.get(6)?;
        count+=1;size+=data.len();anyhow::ensure!(count<=256&&data.len()<=1024*1024&&size<=8*1024*1024,"OMP credential snapshot exceeds bounds");
        // Validate JSON without exposing the contents in errors or receipts.
        anyhow::ensure!(serde_json::from_str::<Value>(&data).is_ok(),"OMP credential record contains invalid JSON");
        write.execute("INSERT INTO auth_credentials(id,provider,credential_type,data,identity_key,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![id,provider,kind,data,identity,created,updated])?;
    }
    let has_blocks:bool=read.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='auth_credential_blocks')",[],|r|r.get(0))?;
    if has_blocks {
        let mut stmt=read.prepare("SELECT b.credential_id,b.provider_key,b.block_scope,b.blocked_until_ms,b.updated_at FROM auth_credential_blocks b JOIN auth_credentials c ON c.id=b.credential_id WHERE c.disabled_cause IS NULL")?;
        let mut rows=stmt.query([])?;let mut count=0;
        while let Some(row)=rows.next()? {count+=1;anyhow::ensure!(count<=4096,"OMP account block snapshot exceeds bounds");write.execute("INSERT INTO auth_credential_blocks VALUES (?1,?2,?3,?4,?5)",params![row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,i64>(4)?])?;}
    }
    write.commit()?;Ok(())
}

fn check_project(cwd:&Path)->Result<()> {
    // OMP custom-tool factories and the project plugin registry have discovery
    // paths independent of MCP and --no-extensions. Never silently load them.
    for directory in cwd.ancestors() {
        for relative in [".omp/tools",".omp/plugins"] {
            anyhow::ensure!(!directory.join(relative).try_exists()?,"OMP role cannot isolate project executable discovery at {}; retain the project and use a clean role worktree",directory.join(relative).display());
        }
    }
    Ok(())
}

fn environment(folder:&Path)->Vec<(&'static str,String)> {
    let home=folder.join("omp-home");let agent=home.join(".omp/agent");
    vec![("HOME",home.to_string_lossy().into_owned()),("USERPROFILE",home.to_string_lossy().into_owned()),("OMP_PROFILE","default".into()),("PI_PROFILE","default".into()),("PI_CONFIG_DIR",".omp".into()),("PI_CODING_AGENT_DIR",agent.to_string_lossy().into_owned()),("PI_CONFIG_FILES",folder.join("omp-config.yml").to_string_lossy().into_owned()),("PI_CODING_AGENT_SESSION_DIR",agent.join("sessions").to_string_lossy().into_owned()),("OMP_AUTH_BROKER_URL","".into()),("OMP_AUTH_BROKER_TOKEN","".into()),("OMP_SKIP_SETUP","1".into())]
}

pub(super) fn configure(desc:&mut crate::host::Descriptor,folder:&Path,role:&RoleDefinition,servers:&Value,source:&Path)->Result<()> {
    check_project(Path::new(desc.spawn.cwd.as_deref().context("OMP role workspace missing")?))?;
    let home=folder.join("omp-home");let agent=home.join(".omp/agent");
    if home.try_exists()? {
        let binding=json_file(&home.join("velella-binding.json"))?;
        anyhow::ensure!(binding["fingerprint"]==role_fingerprint(role)?&&binding["state"]=="ready","OMP private profile capture is incomplete or belongs to another role");
    } else {
        private_directory(&home)?;std::fs::create_dir_all(&agent)?;
        snapshot_auth(source,&agent.join("agent.db")).context("cannot snapshot OMP credentials into private profile")?;
        crate::ipc::atomic_json(&home.join("velella-binding.json"),&json!({"fingerprint":role_fingerprint(role)?,"state":"ready"}))?;
    }
    crate::ipc::atomic_json(&agent.join("mcp.json"),&json!({"mcpServers":servers}))?;
    let overlay=json!({"disabledProviders":DISABLED,"enabledProviders":[],"mcp":{"enableProjectConfig":false},"extensions":[],"auth":{"broker":{"url":"","token":""}}});
    std::fs::write(folder.join("omp-config.yml"),serde_yaml::to_string(&overlay)?)?;
    std::fs::write(folder.join("omp-lifecycle.mjs"),include_str!("omp_lifecycle.mjs"))?;
    for (key,value) in environment(folder) {desc.spawn.env.insert(key.into(),value);}
    // Optional Python/notebook tools are absent until native setup installs
    // their dependencies. Do not make them prerequisites for a default role.
    let selected=if role.tools.is_empty(){tools("omp").iter().filter(|s|!matches!(**s,"python"|"notebook")).map(|s|s.to_string()).collect::<Vec<_>>()}else{role.tools.clone()};
    desc.spawn.env.insert("VELELLA_OMP_TOOLS".into(),serde_json::to_string(&selected)?);
    desc.spawn.args.extend(["--profile".into(),"default".into(),"--config".into(),folder.join("omp-config.yml").to_string_lossy().into_owned(),"--append-system-prompt".into(),folder.join("instructions.md").to_string_lossy().into_owned(),"--no-skills".into(),"--no-extensions".into(),"--no-rules".into(),"--no-lsp".into(),"--no-title".into(),"--no-prewalk".into(),"--service-tier".into(),"none".into(),"--approval-mode".into(),"always-ask".into(),"--tools".into(),selected.join(","),"--session-dir".into(),agent.join("sessions").to_string_lossy().into_owned(),"-e".into(),folder.join("omp-lifecycle.mjs").to_string_lossy().into_owned()]);
    desc.spawn.adapter=Some("generic".into());
    Ok(())
}

pub(super) fn verify(desc:&crate::host::Descriptor)->Result<()> {
    let folder=desc.directory.join("role");
    for (key,value) in environment(&folder) {anyhow::ensure!(desc.spawn.env.get(key)==Some(&value),"OMP invocation isolation changed: {key}");}
    let flags=["--no-skills","--no-extensions","--no-rules","--no-lsp","--no-title","--no-prewalk"];
    anyhow::ensure!(flags.iter().all(|flag|desc.spawn.args.iter().any(|arg|arg==flag)),"OMP role discovery flags changed");
    for (flag,value) in [("--profile","default".to_owned()),("--service-tier","none".into()),("--approval-mode","always-ask".into()),("--config",folder.join("omp-config.yml").to_string_lossy().into_owned()),("--append-system-prompt",folder.join("instructions.md").to_string_lossy().into_owned()),("-e",folder.join("omp-lifecycle.mjs").to_string_lossy().into_owned())] {anyhow::ensure!(desc.spawn.args.windows(2).filter(|pair|pair[0]==flag).all(|pair|pair[1]==value)&&desc.spawn.args.windows(2).any(|pair|pair[0]==flag),"OMP role argument changed: {flag}");}
    let selected:Vec<String>=serde_json::from_str(desc.spawn.env.get("VELELLA_OMP_TOOLS").context("OMP tools missing")?)?;
    anyhow::ensure!(!selected.is_empty()&&selected.iter().all(|tool|tools("omp").contains(&tool.as_str()))&&desc.spawn.args.windows(2).any(|pair|pair[0]=="--tools"&&pair[1]==selected.join(",")),"OMP tool selection changed");
    check_project(Path::new(desc.spawn.cwd.as_deref().context("OMP role workspace missing")?))?;
    let home=folder.join("omp-home");
    for relative in [".omp/agent/.mcp.json",".omp/agent/tools",".omp/plugins"] {anyhow::ensure!(!home.join(relative).try_exists()?,"OMP private profile acquired an unselected discovery source");}
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] #[ignore="requires installed OMP and retains a synthetic role for native execution"]
    fn omp_export_native_role() {
        let out=std::env::var_os("VELELLA_OMP_FIXTURE_OUT").expect("fixture output path required");
        let root=std::env::temp_dir().join(format!("velella-omp-native-{}",uuid::Uuid::now_v7()));let folder=root.join("role");std::fs::create_dir_all(&folder).unwrap();
        let mut role:RoleDefinition=serde_json::from_value(super::super::super::orchestration::v2::seed("omp","goal")["roles"][0].clone()).unwrap();role.tools=vec!["read".into()];
        let mut desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:located("omp").into_iter().next().unwrap(),args:vec![],cwd:Some(root.to_string_lossy().into_owned()),env:Default::default(),adapter:None}};
        let instruction=skill_policy::instructions(&role,&[]).unwrap();std::fs::write(folder.join("instructions.md"),instruction).unwrap();
        desc.spawn.env.insert("VELELLA_PROVIDER".into(),"omp".into());desc.spawn.env.insert("VELELLA_SKILL_POLICY".into(),skill_policy::POLICY.into());desc.spawn.env.insert("VELELLA_SKILL_BINDINGS".into(),"[]".into());
        desc.spawn.env.insert("VELELLA_ROLE_FILE".into(),folder.join("instructions.md").to_string_lossy().into_owned());desc.spawn.env.insert("VELELLA_ROLE_INSTRUCTION_SHA256".into(),launch_file_digest(&folder.join("instructions.md")).unwrap());
        let script=dunce::canonicalize(std::env::current_dir().unwrap().join("../../scripts/fixtures/omp-role-lifecycle.cjs")).unwrap();
        configure(&mut desc,&folder,&role,&json!({"velella":{"command":located("node").first().unwrap(),"args":[script,"--mcp",root]}}),&root.join("absent.db")).unwrap();
        bind_launch_files(&mut desc,&folder).unwrap();verify_mcp_bindings(&desc).unwrap();
        crate::ipc::atomic_json(Path::new(&out),&serde_json::to_value(desc).unwrap()).unwrap();
    }
    #[test] #[ignore="requires the native OMP fixture descriptor after installed CLI use"]
    fn omp_post_native_recovery() {
        let path=std::env::var_os("VELELLA_OMP_FIXTURE_OUT").expect("fixture path required");let desc:crate::host::Descriptor=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();verify_mcp_bindings(&desc).unwrap();
    }
    #[test] fn omp_auth_snapshot_reads_wal_and_preserves_blocks_without_copying_settings() {
        let root=std::env::temp_dir().join(format!("omp-auth-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();
        let source=root.join("source.db");let db=Connection::open(&source).unwrap();db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();db.execute_batch(AUTH_SCHEMA).unwrap();
        db.execute_batch("INSERT INTO auth_credentials(id,provider,credential_type,data) VALUES(1,'fixture','api_key','{\"key\":\"synthetic\"}'); INSERT INTO auth_credentials(id,provider,credential_type,data,disabled_cause) VALUES(2,'disabled','oauth','{}','disabled'); INSERT INTO auth_credential_blocks VALUES(1,'fixture','',9999999999999,1); CREATE TABLE settings(secret TEXT); INSERT INTO settings VALUES ('must not copy');").unwrap();
        let target=root.join("target.db");snapshot_auth(&source,&target).unwrap();let copied=Connection::open(&target).unwrap();
        assert_eq!(copied.query_row("SELECT COUNT(*) FROM auth_credentials",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(copied.query_row("SELECT COUNT(*) FROM auth_credential_blocks",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert!(copied.prepare("SELECT * FROM settings").is_err());assert_eq!(db.query_row("SELECT COUNT(*) FROM auth_credentials",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        drop(copied);drop(db);std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn omp_generated_role_is_bound_and_project_executables_are_rejected() {
        let root=std::env::temp_dir().join(format!("omp-role-{}",uuid::Uuid::now_v7()));let folder=root.join("role");std::fs::create_dir_all(&folder).unwrap();
        let role:RoleDefinition=serde_json::from_value(super::super::super::orchestration::v2::seed("omp","goal")["roles"][0].clone()).unwrap();
        let mut desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:"omp".into(),args:vec![],cwd:Some(root.to_string_lossy().into_owned()),env:Default::default(),adapter:None}};
        std::fs::write(folder.join("instructions.md"),"selected role context").unwrap();
        configure(&mut desc,&folder,&role,&json!({"velella":{"command":"fixture-never-execute","args":[]}}),&root.join("absent.db")).unwrap();verify(&desc).unwrap();
        assert_eq!(desc.spawn.env.get("OMP_SKIP_SETUP").map(String::as_str),Some("1"));
        let selected:Vec<String>=serde_json::from_str(&desc.spawn.env["VELELLA_OMP_TOOLS"]).unwrap();assert!(!selected.iter().any(|s|matches!(s.as_str(),"python"|"notebook")));
        let config:Value=serde_yaml::from_str(&std::fs::read_to_string(folder.join("omp-config.yml")).unwrap()).unwrap();assert_eq!(config["mcp"]["enableProjectConfig"],false);assert!(!config["disabledProviders"].as_array().unwrap().contains(&json!("native")));
        let mut changed=desc.clone();changed.spawn.env.insert("OMP_PROFILE".into(),"outside".into());assert!(verify(&changed).is_err());
        std::fs::create_dir_all(root.join(".omp/tools")).unwrap();assert!(verify(&desc).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
