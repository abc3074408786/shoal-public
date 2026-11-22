use super::*;
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ExecutionHost {
    schema: u32,
    id: String,
    revision: u64,
    name: String,
    ssh_profile_id: String,
    data_root: Option<String>,
    confirmation: Option<Confirmation>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Confirmation {
    server_id: String,
    data_root: String,
    os: String,
    profile_fingerprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    #[serde(default)] id: Option<String>,
    #[serde(default)] expected_revision: Option<u64>,
    name: String,
    ssh_profile_id: String,
    #[serde(default)] data_root: Option<String>,
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

pub(super) fn migrate(db: &Connection, root: &Path, existed: bool) -> Result<()> {
    let present: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='execution_hosts')", [], |row| row.get(0))?;
    if present { return Ok(()); }
    if existed {
        let backup = root.join("backups").join(format!("execution-hosts-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&backup)?;
        db.execute("VACUUM INTO ?1", [backup.join("capabilities.sqlite").to_string_lossy().as_ref()])?;
    }
    db.execute_batch("CREATE TABLE execution_hosts(id TEXT PRIMARY KEY, profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT, revision INTEGER NOT NULL, document TEXT NOT NULL);")?;
    Ok(())
}

fn decode(id: &str, revision: i64, document: &str) -> Result<ExecutionHost> {
    anyhow::ensure!(document.len() <= 16384, "execution host record too large");
    let host: ExecutionHost = serde_json::from_str(document)?;
    anyhow::ensure!(host.schema == SCHEMA, "unsupported execution host schema");
    uuid::Uuid::parse_str(&host.id)?;
    anyhow::ensure!(host.id == id && revision >= 0 && host.revision == revision as u64, "execution host identity mismatch");
    Ok(host)
}

impl Capabilities {
    fn execution_host(&self, id: &str) -> Result<ExecutionHost> {
        uuid::Uuid::parse_str(id)?;
        let (revision, document): (i64, String) = self.db.query_row(
            "SELECT revision,document FROM execution_hosts WHERE id=?1", [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?.context("execution host not found")?;
        decode(id, revision, &document)
    }

    fn execution_profile(&self, id: &str) -> Result<(Value, String)> {
        let profile: SshProfile = serde_json::from_value(self.get("profiles", id)?)?;
        profile.validate()?;
        let profile = serde_json::to_value(profile)?;
        let fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&profile)?));
        Ok((profile, fingerprint))
    }

    fn save_execution_host(&self, host: &ExecutionHost, expected: Option<u64>) -> Result<()> {
        let document = serde_json::to_string(host)?;
        anyhow::ensure!(document.len() <= 16384 && host.revision <= i64::MAX as u64, "execution host record exceeds limits");
        if let Some(revision) = expected {
            let changed = self.db.execute("UPDATE execution_hosts SET profile_id=?1,revision=?2,document=?3 WHERE id=?4 AND revision=?5",
                params![host.ssh_profile_id, host.revision as i64, document, host.id, revision as i64])?;
            anyhow::ensure!(changed == 1, "execution host changed; review again");
        } else {
            self.db.execute("INSERT INTO execution_hosts(id,profile_id,revision,document) VALUES(?1,?2,?3,?4)",
                params![host.id, host.ssh_profile_id, host.revision as i64, document])?;
        }
        Ok(())
    }

    pub(super) fn execution_host_command(&mut self, req: &Value) -> Result<Value> {
        match req["cmd"].as_str().unwrap_or("") {
            "execution_hosts.list" => {
                let mut statement = self.db.prepare("SELECT id,revision,document FROM execution_hosts ORDER BY rowid")?;
                let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))?;
                let mut hosts = Vec::new();
                for row in rows { let (id, revision, document) = row?; hosts.push(decode(&id, revision, &document)?); }
                Ok(json!({"ok":true,"hosts":hosts}))
            }
            "execution_hosts.get" => {
                let host = self.execution_host(req["id"].as_str().context("execution host ID required")?)?;
                let (profile, fingerprint) = self.execution_profile(&host.ssh_profile_id)?;
                let valid = host.confirmation.as_ref().is_some_and(|c| c.profile_fingerprint == fingerprint);
                Ok(json!({"ok":true,"host":host,"profile":profile,"profile_fingerprint":fingerprint,"confirmation_valid":valid}))
            }
            "execution_hosts.save" => {
                let draft: Draft = serde_json::from_value(req["host"].clone())?;
                anyhow::ensure!(text(&draft.name, 160), "请输入执行主机名称（最多160字节）");
                if let Some(root) = &draft.data_root { anyhow::ensure!(text(root, 4096), "远端数据根包含无效字符"); }
                self.execution_profile(&draft.ssh_profile_id)?;
                let (id, revision, confirmation) = if let Some(id) = draft.id {
                    let previous = self.execution_host(&id)?;
                    anyhow::ensure!(draft.expected_revision == Some(previous.revision), "execution host changed; review again");
                    let confirmation = if previous.ssh_profile_id == draft.ssh_profile_id && previous.data_root == draft.data_root { previous.confirmation } else { None };
                    (id, previous.revision.checked_add(1).context("execution host revision exhausted")?, confirmation)
                } else {
                    anyhow::ensure!(draft.expected_revision.is_none(), "new execution host cannot specify a revision");
                    (uuid::Uuid::now_v7().to_string(), 0, None)
                };
                let host = ExecutionHost { schema: SCHEMA, id, revision, name: draft.name, ssh_profile_id: draft.ssh_profile_id, data_root: draft.data_root, confirmation };
                self.save_execution_host(&host, draft.expected_revision)?;
                Ok(json!({"ok":true,"host":host}))
            }
            "execution_hosts.confirm" => {
                anyhow::ensure!(req["confirmed"] == true, "请确认目标 SSH 主机、服务器身份和远端数据根");
                let mut host = self.execution_host(req["id"].as_str().context("execution host ID required")?)?;
                let (profile, fingerprint) = self.execution_profile(&host.ssh_profile_id)?;
                anyhow::ensure!(req["reviewed_host"] == serde_json::to_value(&host)? && req["reviewed_profile"] == profile, "execution host or SSH profile changed; review again");
                let identity = &req["identity"];
                anyhow::ensure!(identity["protocol"] == 1, "unsupported remote runtime protocol");
                let server_id = identity["server_id"].as_str().context("server ID required")?;
                uuid::Uuid::parse_str(server_id)?;
                let root = identity["data_root"].as_str().context("remote data root required")?;
                anyhow::ensure!(text(root, 4096), "invalid remote data root");
                let os = identity["os"].as_str().context("remote OS required")?;
                let absolute = match os {
                    "linux" => root.starts_with('/'),
                    "windows" => root.starts_with(r"\\") || (root.as_bytes().get(1) == Some(&b':') && root.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) && root.as_bytes().get(2).is_some_and(|c| matches!(*c, b'/' | b'\\'))),
                    _ => false,
                };
                anyhow::ensure!(absolute, "remote data root must be absolute on the execution host");
                let previous = host.revision;
                host.revision = previous.checked_add(1).context("execution host revision exhausted")?;
                host.confirmation = Some(Confirmation { server_id: server_id.into(), data_root: root.into(), os: os.into(), profile_fingerprint: fingerprint });
                self.save_execution_host(&host, Some(previous))?;
                Ok(json!({"ok":true,"host":host}))
            }
            "execution_hosts.delete" => {
                anyhow::ensure!(req["confirmed"] == true, "请确认删除执行主机记录；远端任务不会停止");
                let host = self.execution_host(req["id"].as_str().context("execution host ID required")?)?;
                anyhow::ensure!(req["expected_revision"].as_u64() == Some(host.revision), "execution host changed; review again");
                let changed = self.db.execute("DELETE FROM execution_hosts WHERE id=?1 AND revision=?2", params![host.id, host.revision as i64])?;
                anyhow::ensure!(changed == 1, "execution host changed; review again");
                Ok(json!({"ok":true}))
            }
            _ => anyhow::bail!("unsupported execution host command"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("velella-host-registry-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap(); Self(root)
        }
        fn db(&self) -> Capabilities { Capabilities::open(&self.0).unwrap() }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn profile(db: &Capabilities) {
        db.put("profiles", &json!({"id":"ssh-fixture","name":"test","host":"fixture.invalid"})).unwrap();
    }
    fn add(db: &mut Capabilities) -> Value {
        db.execution_host_command(&json!({"cmd":"execution_hosts.save","host":{"name":"Linux","ssh_profile_id":"ssh-fixture","data_root":"/srv/中文 space"}})).unwrap()["host"].clone()
    }
    fn confirm(db: &mut Capabilities, host: &Value) -> Value {
        let profile = db.execution_profile("ssh-fixture").unwrap().0;
        db.execution_host_command(&json!({"cmd":"execution_hosts.confirm","id":host["id"],"confirmed":true,"reviewed_host":host,"reviewed_profile":profile,"identity":{"protocol":1,"server_id":"018fa61b-4db2-7000-8000-000000000001","os":"linux","data_root":"/srv/中文 space"}})).unwrap()["host"].clone()
    }
    #[test]
    fn execution_hosts_persist_identity_and_remote_path_without_local_normalization() {
        let f=Fixture::new();let mut db=f.db();profile(&db);let added=add(&mut db);let confirmed=confirm(&mut db,&added);drop(db);
        let mut db=f.db();let result=db.execution_host_command(&json!({"cmd":"execution_hosts.get","id":added["id"]})).unwrap();
        assert_eq!(result["host"],confirmed);assert_eq!(result["confirmation_valid"],true);assert_eq!(result["host"]["data_root"],"/srv/中文 space");
    }
    #[test]
    fn execution_hosts_reject_stale_review_and_invalidate_changed_profiles() {
        let f=Fixture::new();let mut db=f.db();profile(&db);let added=add(&mut db);let confirmed=confirm(&mut db,&added);
        assert!(db.execution_host_command(&json!({"cmd":"execution_hosts.save","host":{"id":added["id"],"name":"stale","ssh_profile_id":"ssh-fixture","expected_revision":0}})).is_err());
        db.put("profiles",&json!({"id":"ssh-fixture","name":"test","host":"other.invalid"})).unwrap();
        let result=db.execution_host_command(&json!({"cmd":"execution_hosts.get","id":added["id"]})).unwrap();assert_eq!(result["confirmation_valid"],false);
        let updated=db.execution_host_command(&json!({"cmd":"execution_hosts.save","host":{"id":added["id"],"name":"changed root","ssh_profile_id":"ssh-fixture","data_root":"/other","expected_revision":confirmed["revision"]}})).unwrap();
        assert!(updated["host"]["confirmation"].is_null());
        assert!(db.db.execute("DELETE FROM profiles WHERE id='ssh-fixture'",[]).is_err());
    }
    #[test]
    fn execution_hosts_validate_types_and_reject_future_schema() {
        let f=Fixture::new();let mut db=f.db();profile(&db);
        for host in [json!({"name":"bad","ssh_profile_id":"missing"}),json!({"name":"bad","ssh_profile_id":"ssh-fixture","data_root":3}),json!({"name":"bad","ssh_profile_id":"ssh-fixture","server_id":"forged"})] {
            assert!(db.execution_host_command(&json!({"cmd":"execution_hosts.save","host":host})).is_err());
        }
        let mut host=add(&mut db);host["schema"]=json!(2);db.db.execute("UPDATE execution_hosts SET document=?1",[serde_json::to_string(&host).unwrap()]).unwrap();
        assert!(db.execution_host_command(&json!({"cmd":"execution_hosts.list"})).is_err());
    }
    #[test]
    fn execution_hosts_migration_backs_up_existing_wal_and_retains_profiles() {
        let f=Fixture::new();let db=Connection::open(f.0.join("capabilities.db")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;CREATE TABLE profiles(id TEXT PRIMARY KEY,document TEXT NOT NULL);").unwrap();
        db.execute("INSERT INTO profiles VALUES('keep','{}')",[]).unwrap();let cap=f.db();
        assert_eq!(cap.get("profiles","keep").unwrap(),json!({}));
        let backup=std::fs::read_dir(f.0.join("backups")).unwrap().next().unwrap().unwrap().path().join("capabilities.sqlite");
        let snapshot=Connection::open(backup).unwrap();assert_eq!(snapshot.query_row("SELECT COUNT(*) FROM profiles",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        drop(snapshot);drop(cap);drop(db);let reopened=f.db();drop(reopened);assert_eq!(std::fs::read_dir(f.0.join("backups")).unwrap().count(),1);
    }
}
