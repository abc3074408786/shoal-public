//! Offline edits to Codex's native stdio MCP configuration, never connections.
use super::*;
use fs2::FileExt;
use serde::Deserialize;
use sha2::{Digest,Sha256};
use std::{fs,io::{Read,Write},path::Path};
use toml_edit::{DocumentMut,InlineTable,Item};
const MAX: usize=1024*1024;

#[derive(Subcommand)]
pub(super) enum CodexMcpCommand {
    /// Report path, full-file digest and redacted server names; no values
    Inspect,
    /// Preview an additive stdio server JSON against the current config
    Preview { name: String, #[arg(long)] file: PathBuf },
    Install {
        name: String,
        #[arg(long)] file: PathBuf,
        #[arg(long)] config_digest: String,
        #[arg(long)] spec_digest: String,
        #[arg(long,required=true)] confirm: bool,
    },
    Remove {
        name: String,
        #[arg(long)] config_digest: String,
        #[arg(long,required=true)] confirm: bool,
    },
    Backups,
    Restore {
        id: uuid::Uuid,
        #[arg(long)] config_digest: String,
        #[arg(long,required=true)] confirm: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    command: String,
    #[serde(default)] args: Vec<String>,
    #[serde(default)] env_vars: Vec<String>,
    #[serde(default="enabled")] enabled: bool,
}
fn enabled()->bool {true}
fn digest(bytes: &Option<Vec<u8>>) -> String {
    bytes.as_ref().map(|v|format!("sha256:{:x}",Sha256::digest(v))).unwrap_or_else(||"absent".into())
}
fn plain(path:&Path)->Result<()> {
    let meta=fs::symlink_metadata(path)?;
    #[cfg(windows)] {use std::os::windows::fs::MetadataExt;
        anyhow::ensure!(meta.file_attributes()&0x400==0,"native MCP path must not contain reparse points");}
    anyhow::ensure!(!meta.file_type().is_symlink(),"native MCP path must not contain links"); Ok(())
}
fn paths(path:&Path)->Result<()> {
    anyhow::ensure!(path.is_absolute(),"native MCP paths must be absolute");
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {Ok(_)=>plain(part)?,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),Err(e)=>return Err(e.into())}
    } Ok(())
}
fn read(path:&Path,max:usize)->Result<Option<Vec<u8>>> {
    paths(path)?;
    let meta=match fs::symlink_metadata(path) {Ok(m)=>m,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(e.into())};
    anyhow::ensure!(meta.is_file()&&meta.len()<=max as u64,"native MCP file must be a bounded ordinary file");
    let mut bytes=Vec::new();fs::File::open(path)?.take((max+1) as u64).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=max,"native MCP file exceeds bounds");Ok(Some(bytes))
}
fn private(path:&Path,directory:bool)->Result<()> {
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(path,fs::Permissions::from_mode(if directory {0o700}else{0o600}))?;}
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{Foundation::LocalFree,Security::{SetFileSecurityW,DACL_SECURITY_INFORMATION,PROTECTED_DACL_SECURITY_INFORMATION,Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW}};
        let sddl=widestring::U16CString::from_str(if directory {"D:P(A;OICI;FA;;;OW)"}else{"D:P(A;;FA;;;OW)"})?;
        let mut descriptor=std::ptr::null_mut();
        anyhow::ensure!(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,std::ptr::null_mut())}!=0,"cannot prepare private MCP file ACL");
        let full=fs::canonicalize(path)?;
        let name:Vec<u16>=full.as_os_str().encode_wide().chain(Some(0)).collect();
        let success=unsafe{SetFileSecurityW(name.as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,descriptor)};
        unsafe{LocalFree(descriptor);}
        anyhow::ensure!(success!=0,"cannot protect native MCP backup permissions");
    } Ok(())
}
fn write_new(path:&Path,bytes:&[u8])->Result<()> {
    paths(path)?;
    let mut options=fs::OpenOptions::new();options.create_new(true).write(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(path)?;private(path,false)?;file.write_all(bytes)?;file.sync_all()?;Ok(())
}
fn document(before:&Option<Vec<u8>>)->Result<DocumentMut> {
    let bytes=before.as_deref().unwrap_or_default();
    let text=std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)).map_err(|_|anyhow::anyhow!("native config must be UTF-8 TOML; content omitted"))?;
    text.parse().map_err(|_|anyhow::anyhow!("native config is not supported valid TOML; content omitted"))
}
fn name(name:&str)->Result<()> {
    anyhow::ensure!(!name.is_empty()&&name.len()<=64&&name.as_bytes()[0].is_ascii_alphabetic()
        &&name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'),"MCP name must start with a letter and use only letters, numbers, - or _ (max 64)");Ok(())
}
fn spec(path:&Path)->Result<(Spec,String)> {
    let path=std::path::absolute(path)?;
    let bytes=read(&path,65536)?.context("server JSON file is missing")?;
    let data:Spec=serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).map_err(|_|anyhow::anyhow!("server JSON must contain command, optional args/env_vars/enabled only; values omitted"))?;
    anyhow::ensure!(!data.command.trim().is_empty()&&data.command.len()<=1024&&!data.command.chars().any(char::is_control),"server command is empty or invalid; value omitted");
    anyhow::ensure!(data.args.len()<=64&&data.args.iter().all(|a|a.len()<=8192&&!a.contains('\0')),"server argument bounds exceeded; values omitted");
    anyhow::ensure!(data.env_vars.len()<=64&&data.env_vars.iter().all(|v|!v.is_empty()&&v.len()<=128&&(v.as_bytes()[0].is_ascii_alphabetic()||v.starts_with('_'))&&v.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')),"invalid inherited environment variable names");
    let stamp=digest(&Some(bytes));Ok((data,stamp))
}
fn edit(before:&Option<Vec<u8>>,entry_name:&str,server:Option<&Spec>)->Result<Vec<u8>> {
    name(entry_name)?;let mut doc=document(before)?;
    let table=doc.entry("mcp_servers").or_insert(Item::Table(toml_edit::Table::new())).as_table_like_mut().context("mcp_servers is not a table; config unchanged")?;
    if let Some(server)=server {
        anyhow::ensure!(!table.contains_key(entry_name),"MCP server name already exists; inspect and remove explicitly before replacement");
        let mut entry=InlineTable::new();entry.insert("command",server.command.clone().into());
        let args=server.args.iter().map(|a|toml_edit::Value::from(a.clone())).collect::<toml_edit::Array>();
        let env=server.env_vars.iter().map(|a|toml_edit::Value::from(a.clone())).collect::<toml_edit::Array>();
        entry.insert("args",args.into());entry.insert("env_vars",env.into());entry.insert("enabled",server.enabled.into());
        table.insert(entry_name,Item::Value(entry.into()));
    } else {anyhow::ensure!(table.remove(entry_name).is_some(),"MCP server name is missing; config unchanged");}
    let mut text=doc.to_string();
    if before.as_ref().is_some_and(|v|v.contains(&b'\n')&&v.iter().enumerate().all(|(i,b)|*b!=b'\n'||(i>0&&v[i-1]==b'\r'))) {
        text=text.replace("\r\n","\n").replace('\n',"\r\n");
    }
    let mut bytes=text.into_bytes();
    if before.as_ref().is_some_and(|v|v.starts_with(b"\xef\xbb\xbf")) {bytes.splice(..0,b"\xef\xbb\xbf".iter().copied());}
    anyhow::ensure!(bytes.len()<=MAX,"resulting native config exceeds 1 MiB");Ok(bytes)
}

struct Store { config:PathBuf, backups:PathBuf }
impl Store {
    fn at(home:PathBuf)->Result<Self> {
        paths(&home)?;Ok(Self{config:home.join("config.toml"),backups:home.join("velella-mcp-backups")})
    }
    fn current(&self)->Result<Option<Vec<u8>>> {read(&self.config,MAX)}
    fn lock(&self)->Result<fs::File> {
        paths(&self.backups)?;fs::create_dir_all(&self.backups)?;private(&self.backups,true)?;
        let path=self.backups.join(".lock");paths(&path)?;
        let lock=fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path)?;
        private(&path,false)?;lock.try_lock_exclusive().context("another Velella native MCP operation is in progress; no retry")?;Ok(lock)
    }
    fn backup(&self,id:uuid::Uuid)->Result<PathBuf> {let path=self.backups.join(id.to_string());paths(&path)?;Ok(path)}
    fn receipt(&self,id:uuid::Uuid)->Result<Value> {
        let bytes=read(&self.backup(id)?.join("receipt.json"),16384)?.context("backup receipt missing")?;
        let value:Value=serde_json::from_slice(&bytes).map_err(|_|anyhow::anyhow!("backup receipt is invalid; content omitted"))?;
        let fields=["version","id","config_path","operation","name","before_digest","after_digest","created_at"];
        anyhow::ensure!(value.as_object().is_some_and(|o|o.len()==fields.len()&&o.keys().all(|key|fields.contains(&key.as_str()))),"backup receipt fields are invalid; content omitted");
        anyhow::ensure!(value["version"]==1&&value["id"]==id.to_string()&&value["config_path"]==json!(self.config),"backup identity or native config path mismatch");
        name(value["name"].as_str().context("backup name missing")?)?;
        anyhow::ensure!(matches!(value["operation"].as_str(),Some("install"|"remove"|"restore")),"invalid backup operation");
        for field in ["before_digest","after_digest"] {
            let stamp=value[field].as_str().context("backup digest missing")?;
            anyhow::ensure!(stamp=="absent"||stamp.strip_prefix("sha256:").is_some_and(|hex|hex.len()==64&&hex.bytes().all(|b|b.is_ascii_hexdigit())),"invalid backup digest");
        }
        anyhow::ensure!(value["created_at"].as_str().is_some_and(|s|chrono::DateTime::parse_from_rfc3339(s).is_ok()),"invalid backup timestamp");Ok(value)
    }
    fn change(&self,before:&Option<Vec<u8>>,after:&Option<Vec<u8>>,operation:&str,entry_name:&str)->Result<Value> {
        let id=uuid::Uuid::now_v7();let folder=self.backup(id)?;fs::create_dir(&folder)?;private(&folder,true)?;
        let value=json!({"version":1,"id":id,"config_path":self.config,"operation":operation,"name":entry_name,
            "before_digest":digest(before),"after_digest":digest(after),"created_at":chrono::Utc::now().to_rfc3339()});
        if let Some(bytes)=before {write_new(&folder.join("before.bin"),bytes)?;}
        if let Some(bytes)=after {write_new(&folder.join("next.tmp"),bytes)?;}
        write_new(&folder.join("receipt.json"),&serde_json::to_vec_pretty(&value)?)?;
        let publish=(||->Result<()> {
            anyhow::ensure!(digest(&self.current()?)==digest(before),"native config changed before publication; no automatic retry");
            if after.is_some() {fs::rename(folder.join("next.tmp"),&self.config)?;}
            else {fs::rename(&self.config,folder.join("removed.bin"))?;}
            anyhow::ensure!(digest(&self.current()?)==digest(after),"native config changed after publication");
            write_new(&folder.join("applied.json"),b"{\"applied\":true}")?;Ok(())
        })();
        publish.map_err(|error|failure("native_mcp_publication_unconfirmed",format!("{error:#}. Inspect config and backup; nothing was retried."),json!({"backup_id":id,"backup_path":folder})))?;
        Ok(json!({"applied":true,"config_path":self.config,"config_digest":digest(after),"backup":value,
            "backup_path":folder,"server_started":false,"existing_sessions":"unchanged"}))
    }
}

pub(super) fn execute(command:&CodexMcpCommand)->Result<Value> {
    let home=if let Some(home)=std::env::var_os("CODEX_HOME") {
        anyhow::ensure!(!home.is_empty(),"CODEX_HOME is empty");PathBuf::from(home)
    } else {PathBuf::from(std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).context("user home unavailable")?).join(".codex")};
    execute_at(&Store::at(home)?,command)
}
fn execute_at(store:&Store,command:&CodexMcpCommand)->Result<Value> {
    match command {
        CodexMcpCommand::Inspect => {
            let current=store.current()?;let doc=document(&current)?;
            let servers=if let Some(value)=doc.get("mcp_servers") {
                value.as_table_like().context("mcp_servers is not a table; content omitted")?.iter().map(|(name,value)|json!({"name":name,
                    "transport":if value.get("command").and_then(Item::as_str).is_some(){"stdio"}else if value.get("url").and_then(Item::as_str).is_some(){"http"}else{"other"},
                    "enabled":value.get("enabled").and_then(Item::as_bool).unwrap_or(true)})).collect::<Vec<_>>()
            } else {Vec::new()};
            Ok(json!({"provider":"codex","config_path":store.config,"config_digest":digest(&current),"servers":servers,"values_redacted":true}))
        }
        CodexMcpCommand::Preview {name,file} => {
            let before=store.current()?;let(server,spec_digest)=spec(file)?;let after=Some(edit(&before,name,Some(&server))?);
            Ok(json!({"provider":"codex","name":name,"transport":"stdio","config_path":store.config,
                "config_digest":digest(&before),"spec_digest":spec_digest,"proposed_digest":digest(&after),
                "enabled":server.enabled,"arguments_count":server.args.len(),"inherited_env_names":server.env_vars,"values_redacted":true,"server_started":false}))
        }
        CodexMcpCommand::Install {name,file,config_digest,spec_digest,..} => {
            let _lock=store.lock()?;let before=store.current()?;
            anyhow::ensure!(digest(&before)==*config_digest,"native config digest is stale; inspect/preview again");
            let(server,stamp)=spec(file)?;anyhow::ensure!(stamp==*spec_digest,"server JSON digest is stale; preview again");
            let after=Some(edit(&before,name,Some(&server))?);store.change(&before,&after,"install",name)
        }
        CodexMcpCommand::Remove {name,config_digest,..} => {
            let _lock=store.lock()?;let before=store.current()?;
            anyhow::ensure!(digest(&before)==*config_digest,"native config digest is stale; inspect again");
            let after=Some(edit(&before,name,None)?);store.change(&before,&after,"remove",name)
        }
        CodexMcpCommand::Backups => {
            paths(&store.backups)?;let mut rows=Vec::new();
            if store.backups.exists() {for entry in fs::read_dir(&store.backups)?.take(1002) {
                let entry=entry?;if entry.file_name()==".lock" {continue;}
                anyhow::ensure!(rows.len()<1000,"native MCP backup listing exceeds 1000 entries");
                let id=uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).context("unexpected native MCP backup folder")?;
                let mut value=store.receipt(id)?;let applied=store.backup(id)?.join("applied.json");paths(&applied)?;
                value["publication_marker_present"]=json!(applied.is_file());rows.push(value);
            }}
            rows.sort_by_key(|row|row["id"].as_str().unwrap_or_default().to_owned());
            Ok(json!({"backups":rows,"values_redacted":true,"backup_contents_verified":false}))
        }
        CodexMcpCommand::Restore {id,config_digest,..} => {
            let _lock=store.lock()?;let value=store.receipt(*id)?;
            let before=store.current()?;anyhow::ensure!(digest(&before)==*config_digest&&value["after_digest"]==*config_digest,"native config no longer matches this backup's published state; restoration would overwrite newer changes");
            let after=read(&store.backup(*id)?.join("before.bin"),MAX)?;
            anyhow::ensure!(value["before_digest"]==digest(&after),"native MCP backup bytes do not match receipt");
            document(&after)?;store.change(&before,&after,"restore",value["name"].as_str().context("backup name missing")?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture { root:PathBuf,store:Store,file:PathBuf }
    impl Fixture {
        fn new()->Self {
            let root=std::env::temp_dir().join(format!("velella-native-mcp-test-{}",uuid::Uuid::now_v7()));
            fs::create_dir(&root).unwrap();let home=root.join("codex");fs::create_dir(&home).unwrap();
            let file=root.join("server.json");fs::write(&file,br#"{"command":"velella-fixture-never-execute","args":["fixture"],"env_vars":["FIXTURE_KEY"]}"#).unwrap();
            Self{store:Store::at(home).unwrap(),root,file}
        }
        fn preview(&self)->Value {execute_at(&self.store,&CodexMcpCommand::Preview{name:"sample".into(),file:self.file.clone()}).unwrap()}
        fn install(&self,preview:&Value)->Result<Value> {
            execute_at(&self.store,&CodexMcpCommand::Install{name:"sample".into(),file:self.file.clone(),
                config_digest:preview["config_digest"].as_str().unwrap().into(),spec_digest:preview["spec_digest"].as_str().unwrap().into(),confirm:true})
        }
        fn restore(&self,receipt:&Value)->Result<Value> {
            execute_at(&self.store,&CodexMcpCommand::Restore{id:uuid::Uuid::parse_str(receipt["backup"]["id"].as_str().unwrap()).unwrap(),
                config_digest:receipt["config_digest"].as_str().unwrap().into(),confirm:true})
        }
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}

    #[test]
    fn native_mcp_lifecycle_preserves_comments_unknown_fields_and_exact_rollback_bytes() {
        let fixture=Fixture::new();let original=b"# preserved comment\r\nmodel='fixture'\r\n[mcp_servers.keep]\r\ncommand='fixture-keep'\r\nargs=['private-value-do-not-print']\r\n";
        fs::write(&fixture.store.config,original).unwrap();
        let preview=fixture.preview();let installed=fixture.install(&preview).unwrap();
        assert_eq!(preview["proposed_digest"],installed["config_digest"]);
        let current=String::from_utf8(fixture.store.current().unwrap().unwrap()).unwrap();
        assert!(current.contains("# preserved comment\r\n"));assert!(current.contains("private-value-do-not-print"));
        let inspected=execute_at(&fixture.store,&CodexMcpCommand::Inspect).unwrap();
        assert_eq!(inspected["servers"].as_array().unwrap().len(),2);
        assert!(!inspected.to_string().contains("private-value-do-not-print"));
        let removed=execute_at(&fixture.store,&CodexMcpCommand::Remove{name:"sample".into(),config_digest:installed["config_digest"].as_str().unwrap().into(),confirm:true}).unwrap();
        fixture.restore(&removed).unwrap();fixture.restore(&installed).unwrap();
        assert_eq!(fixture.store.current().unwrap().unwrap(),original);
    }

    #[test]
    fn restoring_initially_absent_config_removes_it_and_retains_a_recovery_receipt() {
        let fixture=Fixture::new();let preview=fixture.preview();assert_eq!(preview["config_digest"],"absent");
        assert!(!fixture.store.backups.exists());
        let installed=fixture.install(&preview).unwrap();let restored=fixture.restore(&installed).unwrap();
        assert_eq!(restored["config_digest"],"absent");assert!(!fixture.store.config.exists());
        fixture.restore(&restored).unwrap();assert!(fixture.store.config.is_file());
        let rows=execute_at(&fixture.store,&CodexMcpCommand::Backups).unwrap();
        assert_eq!(rows["backups"].as_array().unwrap().len(),3);
        assert_eq!(rows["backup_contents_verified"],false);
    }

    #[test]
    fn stale_review_and_newer_config_cannot_be_overwritten() {
        let fixture=Fixture::new();let preview=fixture.preview();
        fs::write(&fixture.store.config,b"model='externally-edited'\n").unwrap();assert!(fixture.install(&preview).is_err());
        let preview=fixture.preview();fs::write(&fixture.file,br#"{"command":"changed-fixture"}"#).unwrap();assert!(fixture.install(&preview).is_err());
        let installed=fixture.install(&fixture.preview()).unwrap();
        let mut bytes=fixture.store.current().unwrap().unwrap();bytes.extend_from_slice(b"\n# later edit\n");fs::write(&fixture.store.config,&bytes).unwrap();
        assert!(fixture.restore(&installed).is_err());assert_eq!(fixture.store.current().unwrap().unwrap(),bytes);
    }

    #[test]
    fn tampered_backups_and_in_progress_lock_reject_without_editing_config() {
        let fixture=Fixture::new();fs::write(&fixture.store.config,b"# original\n").unwrap();let preview=fixture.preview();
        let lock=fixture.store.lock().unwrap();assert!(fixture.install(&preview).is_err());drop(lock);
        let installed=fixture.install(&preview).unwrap();
        fs::write(Path::new(installed["backup_path"].as_str().unwrap()).join("before.bin"),b"tampered").unwrap();
        assert!(fixture.restore(&installed).is_err());
        assert_eq!(digest(&fixture.store.current().unwrap()),installed["config_digest"].as_str().unwrap());
    }

    #[test]
    fn malformed_secret_config_and_unsupported_spec_never_leak_values() {
        let fixture=Fixture::new();fs::write(&fixture.store.config,b"[api_key=SECRET_NEVER_PRINT\n").unwrap();
        let error=execute_at(&fixture.store,&CodexMcpCommand::Inspect).unwrap_err();assert!(!format!("{error:#}").contains("SECRET_NEVER_PRINT"));
        fs::write(&fixture.file,br#"{"command":"fixture","env":{"KEY":"SECRET_NEVER_PRINT"}}"#).unwrap();
        assert!(!format!("{:#}",spec(&fixture.file).err().unwrap()).contains("SECRET_NEVER_PRINT"));
        assert!(name("../escape").is_err());
    }

    #[test]
    fn inline_server_tables_are_supported_and_existing_names_never_overwritten() {
        let fixture=Fixture::new();let original=b"mcp_servers = { keep = { command = 'keep' } }\n";
        fs::write(&fixture.store.config,original).unwrap();let installed=fixture.install(&fixture.preview()).unwrap();
        assert!(execute_at(&fixture.store,&CodexMcpCommand::Preview{name:"sample".into(),file:fixture.file.clone()}).is_err());
        let value:toml::Value=std::str::from_utf8(&fixture.store.current().unwrap().unwrap()).unwrap().parse().unwrap();
        assert_eq!(value["mcp_servers"]["keep"]["command"].as_str(),Some("keep"));
        fixture.restore(&installed).unwrap();assert_eq!(fixture.store.current().unwrap().unwrap(),original);
    }

    #[test]
    fn native_mcp_parser_requires_both_review_digests_and_confirmation() {
        for args in [vec!["install","sample","--file","server.json"],vec!["remove","sample"],vec!["restore","00000000-0000-0000-0000-000000000001"]] {
            assert!(Cli::try_parse_from(["velella-cli","mcp","codex"].into_iter().chain(args)).is_err());
        }
    }
}
