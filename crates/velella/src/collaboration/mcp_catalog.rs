//! Explicit local catalog selection. Loading never executes a server or imports accounts.
use super::*;
use super::orchestration::v2::{McpServer, Plan};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read};

const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub digest: String,
    pub entry_id: String,
    pub confirmation_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub command: String,
    #[serde(default)] pub args: Vec<String>,
    pub catalog: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest { version: u32, entries: Vec<Entry> }

#[derive(Serialize)]
pub struct Preview { pub path: String, pub digest: String, pub entries: Vec<Entry> }

pub fn preview(path: &Path) -> Result<Preview> {
    ensure!(path.is_absolute(), "MCP 来源需要本地绝对路径");
    let meta = std::fs::symlink_metadata(path).context("无法读取 MCP 来源文件")?;
    ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_BYTES, "MCP 来源必须是不超过 1 MiB 的普通 JSON 文件");
    let canonical = dunce::canonicalize(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&canonical)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "MCP 来源超出大小限制");
    // Do not echo parser input: a mistaken account/config file may contain credentials.
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("MCP 来源格式无效；需要 version、entries，且不允许账户或环境字段"))?;
    ensure!(manifest.version == 1 && manifest.entries.len() <= 64, "MCP 来源版本必须为 1，最多 64 个条目");
    let mut ids = HashSet::new();
    for entry in &manifest.entries {
        ensure!(ids.insert(&entry.id), "MCP 来源条目标识重复");
        let mut role = orchestration::v2::validate(&orchestration::v2::seed("codex", "catalog validation"))?.roles.remove(0);
        role.mcp_servers.push(entry.server(None));
        orchestration::v2::validate_role(&role)?;
    }
    Ok(Preview { path: canonical.to_string_lossy().into_owned(), digest: format!("sha256:{:x}", Sha256::digest(&bytes)), entries: manifest.entries })
}

impl Entry {
    fn server(&self, source: Option<Source>) -> McpServer {
        McpServer { id: self.id.clone(), command: self.command.clone(), args: self.args.clone(), catalog: self.catalog.clone(), tools: vec![], source }
    }
}

pub fn validate_source(source: &Source) -> Result<()> {
    ensure!(Path::new(&source.path).is_absolute() && source.path.len() <= 4096, "MCP 来源路径无效");
    ensure!(source.digest.len() == 71 && source.digest.starts_with("sha256:") && source.digest[7..].bytes().all(|c| c.is_ascii_hexdigit()), "MCP 来源摘要无效");
    ensure!(!source.entry_id.is_empty() && source.entry_id.len() <= 80 && uuid::Uuid::parse_str(&source.confirmation_id).is_ok(), "MCP 来源确认记录无效");
    Ok(())
}

pub fn verify(server: &McpServer) -> Result<()> {
    let Some(source) = &server.source else { return Ok(()) }; // Legacy/custom is explicitly unverified.
    validate_source(source)?;
    let current = preview(Path::new(&source.path))?;
    ensure!(current.path == source.path && current.digest == source.digest, "MCP 来源已变化，请重新载入、预览并确认");
    let entry = current.entries.iter().find(|e| e.id == source.entry_id).context("MCP 来源条目已不存在")?;
    ensure!(entry.id == server.id && entry.command == server.command && entry.args == server.args && entry.catalog == server.catalog, "MCP 配置与已确认的来源条目不匹配");
    ensure!(server.tools.iter().all(|tool| entry.catalog.contains(tool)), "MCP 选择了来源目录以外的工具");
    Ok(())
}

/// File I/O stays outside the collaboration store lock. Later launch/proxy checks
/// close the gap if the source changes after a plan is saved.
pub fn verify_plan(value: &Value) -> Result<()> {
    if value["version"] == 2 {
        for role in orchestration::v2::validate(value)?.roles {
            for server in role.mcp_servers { verify(&server)?; }
        }
    }
    Ok(())
}

pub(super) fn confirmed(db: &Connection, plan: Plan) -> Result<Plan> {
    for role in &plan.roles {
        for server in &role.mcp_servers {
            if let Some(source) = &server.source {
                let receipt = load(db, "mcp_source_confirmation", &source.confirmation_id).context("请先在界面预览并确认 MCP 来源条目")?;
                ensure!(receipt["source"] == serde_json::to_value(source)?, "MCP 来源与确认记录不一致");
                let allowed=receipt["tools"].as_array().context("旧来源确认未记录工具范围，请重新确认")?;
                ensure!(server.tools.iter().all(|tool|allowed.contains(&json!(tool))), "新增 MCP 工具需要重新预览并确认来源");
            }
        }
    }
    Ok(plan)
}

impl Hub {
    /// Desktop management route, not exposed through collaboration MCP commands.
    /// Native processes under the same OS user share desktop IPC authority; this
    /// receipt is workflow provenance, not a sandbox against hostile local code.
    pub(crate) fn confirm_mcp_source(&self, req: &Value) -> Result<Value> {
        ensure!(req["confirmed"] == true, "需要明确确认 MCP 来源条目");
        let request_id = field(req, "request_id")?;
        uuid::Uuid::parse_str(request_id)?;
        let current = preview(Path::new(field(req, "path")?))?;
        ensure!(current.digest == field(req, "digest")?, "预览后 MCP 来源已变化，请重新载入");
        let entry = current.entries.iter().find(|e| e.id == req["entry_id"]).context("MCP 来源条目不存在")?;
        let tools:Vec<String>=serde_json::from_value(req["tools"].clone()).context("需要明确选择 MCP 工具")?;
        let unique:HashSet<_>=tools.iter().collect();
        ensure!(!tools.is_empty() && tools.len()<=128 && unique.len()==tools.len() && tools.iter().all(|tool|entry.catalog.contains(tool)), "只能确认来源目录中明确勾选且不重复的工具");
        let source = Source { path: current.path, digest: current.digest, entry_id: entry.id.clone(), confirmation_id: request_id.into() };
        let db = self.db.lock().unwrap();
        let receipt = json!({"id":request_id,"source":source,"tools":tools,"confirmed_at":now()});
        if let Ok(previous) = load(&db, "mcp_source_confirmation", request_id) {
            ensure!(previous["source"] == receipt["source"] && previous["tools"]==receipt["tools"], "MCP 确认请求标识已用于其他来源或工具");
        } else { put(&db, "mcp_source_confirmation", &receipt)?; }
        let mut server=entry.server(Some(source));server.tools=tools;
        Ok(json!({"ok":true,"server":server}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Fixture { root: PathBuf, path: PathBuf, hub: Hub }
    impl Fixture {
        fn new() -> Self {
            let root=std::env::temp_dir().join(format!("velella-mcp-source-{}",id()));
            let hub=Hub::open(&root).unwrap();let path=root.join("catalog.json");
            std::fs::write(&path,serde_json::to_vec(&json!({"version":1,"entries":[{"id":"docs","command":"fixture-only","args":["--stdio"],"catalog":["read","search"]}]})).unwrap()).unwrap();
            Self {root,path,hub}
        }
        fn request(&self)->Value {
            let p=preview(&self.path).unwrap();
            json!({"request_id":id(),"confirmed":true,"path":p.path,"digest":p.digest,"entry_id":"docs","tools":["read"]})
        }
        fn server(&self)->McpServer {
            let mut server:McpServer=serde_json::from_value(self.hub.confirm_mcp_source(&self.request()).unwrap()["server"].clone()).unwrap();
            server.tools=vec!["read".into()];server
        }
        fn finish(self) {drop(self.hub);std::fs::remove_dir_all(self.root).unwrap();}
    }
    #[test] fn mcp_catalog_requires_explicit_current_preview_confirmation() {
        let f=Fixture::new();let mut request=f.request();request["confirmed"]=json!(false);
        assert!(f.hub.confirm_mcp_source(&request).is_err());request["confirmed"]=json!(true);
        let first=f.hub.confirm_mcp_source(&request).unwrap();assert_eq!(first,f.hub.confirm_mcp_source(&request).unwrap());
        let mut bytes=std::fs::read(&f.path).unwrap();bytes.push(b' ');std::fs::write(&f.path,bytes).unwrap();
        assert!(f.hub.confirm_mcp_source(&request).unwrap_err().to_string().contains("预览后"));
        f.finish();
    }
    #[test] fn mcp_catalog_binds_entry_and_requires_durable_confirmation_on_save() {
        let f=Fixture::new();let server=f.server();verify(&server).unwrap();
        let mut plan=orchestration::v2::seed("codex","goal");plan["roles"][0]["mcp_servers"]=json!([server]);
        let rid=id();let request=json!({"cmd":"orchestration.create","request_id":rid,"workspace_id":"fixture","goal":"goal","acceptance":"goal","agent":"codex","schema_version":2,"plan":plan});
        f.hub.orchestration_mutate(&request).unwrap();
        let mut forged=plan.clone();forged["roles"][0]["mcp_servers"][0]["source"]["confirmation_id"]=json!(id());
        let save=|p:Value|json!({"cmd":"orchestration.plan.save","request_id":id(),"id":rid,"revision":0,"plan":p});
        assert!(f.hub.orchestration_mutate(&save(forged)).unwrap_err().to_string().contains("界面预览"));
        let mut expanded=plan.clone();expanded["roles"][0]["mcp_servers"][0]["tools"]=json!(["read","search"]);
        assert!(f.hub.orchestration_mutate(&save(expanded)).unwrap_err().to_string().contains("新增 MCP 工具"));
        let mut narrowing=server.clone();narrowing.tools.clear();let mut narrow_plan=plan.clone();narrow_plan["roles"][0]["mcp_servers"]=json!([narrowing]);
        assert!(confirmed(&f.hub.db.lock().unwrap(),orchestration::v2::validate(&narrow_plan).unwrap()).is_ok());
        let mut altered=server.clone();altered.command="other-command".into();assert!(verify(&altered).is_err());
        altered=server.clone();altered.catalog.push("other".into());assert!(verify(&altered).is_err());
        altered=server.clone();altered.tools=vec!["unselected".into()];assert!(verify(&altered).is_err());
        f.hub.orchestration_mutate(&save(plan.clone())).unwrap();
        std::fs::write(&f.path,"{\"version\":1,\"entries\":[]}").unwrap();
        assert!(f.hub.orchestration_mutate(&save(plan.clone())).unwrap_err().to_string().contains("来源已变化"));
        assert!(f.hub.orchestration_mutate(&json!({"cmd":"orchestration.plan.confirm","request_id":id(),"id":rid,"revision":1})).unwrap_err().to_string().contains("来源已变化"));
        // Agent submission verifies the same file before entering the store.
        assert!(f.hub.execute(&json!({"cmd":"collab.plan.submit","plan":plan})).unwrap_err().to_string().contains("来源已变化"));
        f.finish();
    }
    #[test] fn mcp_catalog_rejects_account_fields_and_bounds_without_echoing_secrets() {
        let f=Fixture::new();
        std::fs::write(&f.path,r#"{"version":1,"entries":[],"token":"sentinel-secret"}"#).unwrap();
        let message=preview(&f.path).err().unwrap().to_string();assert!(!message.contains("sentinel-secret"));
        std::fs::write(&f.path,vec![b' ';MAX_BYTES as usize+1]).unwrap();assert!(preview(&f.path).is_err());
        std::fs::write(&f.path,r#"{"version":2,"entries":[]}"#).unwrap();assert!(preview(&f.path).is_err());
        std::fs::write(&f.path,r#"{"version":1,"entries":[]}"#).unwrap();assert!(preview(&f.path).unwrap().entries.is_empty());
        assert!(preview(Path::new("catalog.json")).is_err());f.finish();
    }
    #[test] fn mcp_catalog_legacy_custom_role_retains_its_serialized_fingerprint() {
        let value=json!({"id":"custom","command":"fixture","args":[],"catalog":["read"],"tools":["read"]});
        let server:McpServer=serde_json::from_value(value.clone()).unwrap();assert!(server.source.is_none());
        assert_eq!(serde_json::to_value(&server).unwrap(),value);verify(&server).unwrap();
    }
}
