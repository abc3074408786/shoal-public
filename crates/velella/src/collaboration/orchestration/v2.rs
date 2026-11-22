//! Versioned workflow contracts. This module does not launch processes or touch source.
use super::*;

pub const PROVIDERS: [&str; 4] = ["codex", "claude-code", "omp", "opencode"];
const LEGACY_PROVIDERS: [&str; 2] = ["gemini", "kimi"];
pub fn default_model(provider:&str,kind:&str)->&'static str{if provider!="codex"{return ""}match kind {"coordinate"|"implement"|"integrate"=>"gpt-6-terra","explore"|"review"|"verify"=>"gpt-6-luna",_=>""}}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Skill { pub path: String, pub digest: String }
/// A user-confirmed external MCP catalog entry.  It deliberately contains no
/// environment or credential fields: account material stays outside plans and
/// role receipts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    pub id: String, pub command: String, #[serde(default)] pub args: Vec<String>,
    #[serde(default)] pub catalog: Vec<String>, #[serde(default)] pub tools: Vec<String>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub source: Option<super::super::mcp_catalog::Source>,
}
/// Frozen reference to an existing SSH profile, never credentials or key bytes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteCodex {
    pub ssh_profile_id: String,
    pub ssh_profile_fingerprint: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub ssh_target_fingerprint: Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub remote_model: Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub provider_fingerprint: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RoleDefinition {
    pub id: String, pub name: String, pub kind: String, pub instruction: String,
    pub provider: String, #[serde(default)] pub model: String,
    #[serde(default)] pub skills: Vec<Skill>, #[serde(default)] pub tools: Vec<String>,
    #[serde(default)] pub mcp_servers: Vec<McpServer>,
    #[serde(default,skip_serializing_if="Option::is_none")] pub remote_codex: Option<RemoteCodex>,
}
impl RoleDefinition { pub fn writes(&self) -> bool { matches!(self.kind.as_str(), "implement" | "integrate") } }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String, pub title: String, pub instruction: String, pub role_id: String,
    #[serde(default)] pub depends_on: Vec<String>, pub scope: String, pub gap: String,
    pub deliverables: Vec<String>, pub checks: Vec<String>, #[serde(default)] pub human_gate: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32, pub acceptance: String, #[serde(default="one")] pub parallelism: usize,
    pub roles: Vec<RoleDefinition>, pub nodes: Vec<Node>, pub review_role: String,
    pub integration_role: String, pub verification_role: String,
    #[serde(default)] pub integration_human_gate: bool,
    #[serde(default)] pub verification_human_gate: bool,
}
fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 80 && !id.starts_with("__") && id.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_' || c==b'-') }
fn valid_mcp_tool(name:&str)->bool{!name.is_empty()&&name.len()<=128&&name.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'_'|b'-'|b'.'|b'/'))}
fn validate_provider_scope(next:&Plan,prior:Option<&Plan>)->Result<()> {
    ensure!(next.roles.iter().all(|role|PROVIDERS.contains(&role.provider.as_str()) || prior.is_some_and(|old|old.roles.iter().any(|r|r.id==role.id&&r.provider==role.provider))), "新角色仅支持 OMP、Claude Code、Codex、OpenCode；历史角色可保留");Ok(())
}
pub(crate) fn validate_role(r:&RoleDefinition)->Result<()> {
    ensure!(valid_id(&r.id) && !r.name.trim().is_empty() && !r.instruction.trim().is_empty(), "角色需要有效标识、名称和职责");
    ensure!(PROVIDERS.contains(&r.provider.as_str()) || LEGACY_PROVIDERS.contains(&r.provider.as_str()), "不支持的 CLI");
    ensure!(["coordinate","explore","implement","review","integrate","verify"].contains(&r.kind.as_str()), "角色权限类型无效");
    ensure!(r.model.len()<=200 && r.skills.len()<=32 && r.tools.len()<=128 && r.mcp_servers.len()<=16, "角色配置超出限制");
    if let Some(remote)=&r.remote_codex {
        ensure!(r.provider=="codex", "远程执行仅支持 Codex 角色");
        ensure!(uuid::Uuid::parse_str(&remote.ssh_profile_id).is_ok(), "远程角色需要有效 SSH 配置标识");
        let hash=remote.ssh_profile_fingerprint.strip_prefix("sha256:").unwrap_or("");
        ensure!(hash.len()==64 && hash.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c)), "远程角色需要有效的 SSH 配置指纹");
        if let Some(target)=&remote.ssh_target_fingerprint {
            let hash=target.strip_prefix("sha256:").unwrap_or("");
            ensure!(hash.len()==64 && hash.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c)), "远程角色 SSH 目标指纹无效");
        }
        if let Some(model)=&remote.remote_model {ensure!(model.starts_with("deepseek")&&model.len()<=200&&!model.chars().any(char::is_control),"远程 Codex 模型配置无效");}
        if let Some(stamp)=&remote.provider_fingerprint {let hash=stamp.strip_prefix("sha256:").unwrap_or("");ensure!(hash.len()==64&&hash.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c)),"远程 provider 指纹无效");}
    }
    ensure!(r.skills.iter().all(|s|!s.path.is_empty() && s.digest.starts_with("sha256:")), "技能需要路径与版本指纹");
    let mut servers=HashSet::new();
    for server in &r.mcp_servers {
        if let Some(source)=&server.source {super::super::mcp_catalog::validate_source(source)?;}
        ensure!(valid_id(&server.id) && !matches!(server.id.as_str(),"velella"|"velella_browser") && servers.insert(&server.id),"外部 MCP 名称无效、保留或重复");
        ensure!(!server.command.trim().is_empty() && server.command.len()<=4096 && server.args.len()<=64 && server.args.iter().all(|arg|arg.len()<=4096),"外部 MCP 启动命令超出限制");
        ensure!(server.catalog.len()<=128 && server.tools.len()<=128 && server.catalog.iter().all(|tool|valid_mcp_tool(tool)) && server.tools.iter().all(|tool|valid_mcp_tool(tool)),"外部 MCP 工具名无效");
        let catalog:HashSet<_>=server.catalog.iter().collect();let selected:HashSet<_>=server.tools.iter().collect();
        ensure!(catalog.len()==server.catalog.len() && selected.len()==server.tools.len() && selected.is_subset(&catalog),"外部 MCP 只能选择目录中已声明且不重复的工具");
    }
    Ok(())
}
pub fn validate(value: &Value) -> Result<Plan> {
    let p: Plan=serde_json::from_value(value.clone())?;
    ensure!(p.version==2 && !p.acceptance.trim().is_empty(), "需要 v2 方案和总体验收条件");
    ensure!((1..=5).contains(&p.parallelism), "并发数必须为 1–5");
    ensure!(!p.roles.is_empty() && p.roles.len()<=32 && !p.nodes.is_empty() && p.nodes.len()<=64, "角色或步骤数量无效");
    let mut roles=HashMap::new();
    for r in &p.roles {
        ensure!(valid_id(&r.id) && roles.insert(r.id.clone(),r).is_none(), "角色标识无效或重复");
        validate_role(r)?;
    }
    for (id,kind) in [(&p.review_role,"review"),(&p.integration_role,"integrate"),(&p.verification_role,"verify")] {
        ensure!(roles.get(id).is_some_and(|r|r.kind==kind), "缺少独立审查、集成或验证角色");
    }
    ensure!(p.roles.iter().filter(|r|r.kind=="coordinate").count()==1,"方案需要且只能有一个主控角色");
    let mut ids=HashSet::new();
    for n in &p.nodes {
        ensure!(valid_id(&n.id) && ids.insert(n.id.clone()), "步骤标识无效或重复");
        ensure!(roles.contains_key(&n.role_id), "步骤引用了不存在的角色");
        ensure!([&n.title,&n.instruction,&n.scope,&n.gap].iter().all(|s|!s.trim().is_empty()), "步骤需要名称、说明、范围及目标缺口");
        ensure!(!n.checks.is_empty() && !n.deliverables.is_empty() && n.checks.iter().chain(&n.deliverables).all(|s|!s.trim().is_empty()), "步骤需要交付物及检查要求");
    }
    let mut done=HashSet::new();
    while done.len()<p.nodes.len() {
        let before=done.len();
        for n in &p.nodes {
            ensure!(n.depends_on.iter().all(|d|ids.contains(d)), "步骤依赖不存在");
            if n.depends_on.iter().all(|d|done.contains(d)) {done.insert(n.id.clone());}
        }
        ensure!(before<done.len(), "步骤依赖存在循环");
    }
    Ok(p)
}
pub fn confirmed_nodes(p:&Plan)->Vec<Node> {
    let mut nodes=p.nodes.clone();
    nodes.push(Node{id:"__integrate".into(),title:"集成固定候选".into(),instruction:"核对依赖成果，解决集成冲突，执行集成检查。只在本工作树操作；不得发布或改动原工作区。".into(),role_id:p.integration_role.clone(),depends_on:p.nodes.iter().map(|n|n.id.clone()).collect(),scope:"集成工作树".into(),gap:p.acceptance.clone(),deliverables:vec!["可运行集成候选".into()],checks:vec![p.acceptance.clone()],human_gate:p.integration_human_gate});
    nodes.push(Node{id:"__verify".into(),title:"独立整体验证".into(),instruction:"逐项验证原始目标及总体验收条件。报告实际执行、未执行与缺口，不修改源码。".into(),role_id:p.verification_role.clone(),depends_on:vec!["__integrate".into()],scope:"整个集成候选".into(),gap:p.acceptance.clone(),deliverables:vec!["逐项验收证据与剩余缺口".into()],checks:vec![p.acceptance.clone()],human_gate:p.verification_human_gate});
    nodes
}
fn valid_cause_key(value:&str)->bool{!value.is_empty()&&value.len()<=80&&value.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'-'|b'_'|b'.'))}
/// Records a human-classified actual failure. `true` means one bounded retry
/// remains for this root cause; `false` means two actual failures are now
/// durable and no third attempt may be created under another node or revision.
pub(crate) fn record_retry_failure(doc:&mut Value,work:&Value,cause_key:&str,evidence:&str)->Result<bool>{
    ensure!(valid_cause_key(cause_key),"根因标识只能使用字母、数字、.-_，且不超过 80 个字符");
    ensure!(!evidence.trim().is_empty()&&evidence.len()<=8192,"根因证据不能为空且不超过 8192 个字符");
    if !doc["failure_ledger"].is_array(){doc["failure_ledger"]=json!([]);}
    let attempt_id=field(work,"attempt_id")?.to_owned();let entry=json!({"cause_key":cause_key,"work_id":work["id"],"attempt_id":attempt_id,"node_id":work["work_key"],"plan_revision":doc["confirmed_revision"],"evidence":evidence,"recorded_at":now()});let entries=doc["failure_ledger"].as_array_mut().unwrap();
    if let Some(existing)=entries.iter().find(|entry|entry["attempt_id"]==attempt_id){ensure!(existing["cause_key"]==cause_key,"同一失败回执不能更换根因标识");return Ok(entries.iter().filter(|entry|entry["cause_key"]==cause_key).count()<2)}
    entries.push(entry);
    Ok(entries.iter().filter(|entry|entry["cause_key"]==cause_key).count()<2)
}
pub(crate) fn record_actual_failure(doc:&mut Value,work:&Value,cause_key:&str,summary:&str)->Result<()> {
    let retry=doc["retry_evidence"][&field(work,"work_key")?].clone();
    if let Some(expected)=retry["cause_key"].as_str(){ensure!(expected==cause_key,"重试失败必须保留原根因标识");}
    let evidence=retry["evidence"].as_str().unwrap_or(summary).to_owned();let _=record_retry_failure(doc,work,cause_key,&evidence)?;
    Ok(())
}
pub fn seed(provider:&str,acceptance:&str)->Value {
    let roles:Vec<Value>=[("lead","主控","coordinate"),("explore","探索","explore"),("implement","实现","implement"),("review","独立审查","review"),("integrate","集成","integrate"),("verify","独立验证","verify")].iter().map(|(id,name,kind)|json!({"id":id,"name":name,"kind":kind,"instruction":format!("负责{name}，提供可核验交付物，不派生 Agent"),"provider":provider,"model":"","skills":[],"tools":[],"mcp_servers":[]})).collect();
    json!({"version":2,"acceptance":acceptance,"parallelism":1,"roles":roles,"review_role":"review","integration_role":"integrate","verification_role":"verify","integration_human_gate":false,"verification_human_gate":false,"nodes":[{"id":"implement","title":"实现目标","instruction":"根据原始目标实现并验证","role_id":"implement","depends_on":[],"scope":"目标相关模块","gap":acceptance,"deliverables":["代码与验证结果"],"checks":[acceptance],"human_gate":false}]})
}

pub(super) fn mutate(db:&Connection,req:&Value,doc:&mut Value)->Result<()> {
    // Pure DB provenance checks; file checks run before acquiring the store lock.
    let validate = |value:&Value| super::super::mcp_catalog::confirmed(db, self::validate(value)?);
    match field(req,"cmd")? {
        "orchestration.create" | "orchestration.reopen" => {
            ensure!(PROVIDERS.contains(&field(req,"agent")?), "不支持的 CLI");
            doc["schema_version"]=json!(2); doc["plan"]=req.get("plan").cloned().unwrap_or_else(||seed(field(req,"agent").unwrap(),field(req,"acceptance").unwrap()));
            let plan=validate(&doc["plan"])?;
            validate_provider_scope(&plan,None)?;
            doc["gates"]=json!({});doc["revisions"]=json!([]);doc["owned_workspaces"]=json!([]);doc["launch_preflight"]=json!({});
            doc["evidence"]=json!([]);doc["failure_ledger"]=json!([]);doc["last_progress_at"]=json!(now());
            doc["stages"]=json!({"implementation":"pending","development_validation":"pending","independent_testing":"pending","technical_acceptance":"pending","integration":"pending","deployment":"not_run","online":"not_verified","business_uat":"not_run"});
        }
        "orchestration.plan.save" | "collab.plan.submit" => {
            ensure!(matches!(doc["state"].as_str(),Some("planning"|"draft")), "已启动方案请提交变更提议");
            ensure!(req["revision"]==doc["revision"], "方案版本已变化");validate_provider_scope(&validate(&req["plan"])?,Some(&validate(&doc["plan"])?))?;
            doc["plan"]=req["plan"].clone();doc["revision"]=json!(doc["revision"].as_u64().unwrap()+1);doc["state"]=json!("draft");
        }
        "orchestration.plan.refine" => {
            ensure!(matches!(doc["state"].as_str(),Some("planning"|"draft")) && req["revision"]==doc["revision"],"只能调整当前未确认方案");
            doc["refinement"]=json!({"id":field(req,"request_id")?,"instruction":field(req,"instruction")?});doc["state"]=json!("planning");
        }
        "orchestration.plan.confirm" => {
            ensure!(doc["state"]=="draft" && req["revision"]==doc["revision"], "请确认当前版本方案");
            ensure!(doc["collab_id"].is_string(), "主控会话尚未建立");
            doc["nodes"]=serde_json::to_value(confirmed_nodes(&validate(&doc["plan"])?))?;
            doc["confirmed_revision"]=doc["revision"].clone();doc["state"]=json!("running");doc["last_progress_at"]=json!(now());
        }
        "orchestration.revision.propose" | "collab.revision.propose" => {
            ensure!(matches!(doc["state"].as_str(),Some("running"|"paused")), "当前状态不能提议变更");
            ensure!(doc["proposal"].is_null(), "已有待决定变更");ensure!(req["revision"]==doc["revision"], "方案版本已变化");
            let next=validate(&req["plan"])?;let old=validate(&doc["plan"])?;
            validate_provider_scope(&next,Some(&old))?;
            let works=list(db,"work",field(doc,"collab_id")?)?;
            // Persisted work identities may not be removed/renamed to reset an attempt budget.
            for w in &works {
                let key=field(w,"work_key")?;
                ensure!(!key.starts_with("__"), "集成已开始，新增目标请创建协作");
                let before=old.nodes.iter().find(|n|n.id==key).context("旧步骤缺失")?;
                let after=next.nodes.iter().find(|n|n.id==key).context("已派发步骤不能删除或重命名")?;
                ensure!(before==after && old.roles.iter().find(|r|r.id==before.role_id)==next.roles.iter().find(|r|r.id==after.role_id), "已派发步骤及角色已冻结；缺陷修复使用原步骤的有界重试");
            }
            let mut affected:HashSet<String>=old.nodes.iter().filter(|n|next.nodes.iter().find(|x|x.id==n.id)!=Some(*n) || old.roles.iter().find(|r|r.id==n.role_id)!=next.roles.iter().find(|r|r.id==n.role_id)).map(|n|n.id.clone()).collect();
            loop {let before=affected.len();for n in &old.nodes{if n.depends_on.iter().any(|d|affected.contains(d)){affected.insert(n.id.clone());}}if affected.len()==before{break}}
            affected.insert("__integrate".into());affected.insert("__verify".into());
            doc["proposal"]=json!({"id":field(req,"request_id")?,"base_revision":doc["revision"],"reason":field(req,"reason")?,"plan":req["plan"],"affected":affected,"created_at":now()});
        }
        "orchestration.revision.resolve" => {
            ensure!(!doc["proposal"].is_null() && doc["proposal"]["id"]==req["proposal_id"] && req["revision"]==doc["revision"], "变更提议已变化");
            if req["approved"]==true {
                let plan=validate(&doc["proposal"]["plan"])?;
                doc["plan"]=doc["proposal"]["plan"].clone();doc["nodes"]=serde_json::to_value(confirmed_nodes(&plan))?;
                doc["revision"]=json!(doc["revision"].as_u64().unwrap()+1);doc["confirmed_revision"]=doc["revision"].clone();
            }
            let mut receipt=doc["proposal"].clone();receipt["approved"]=json!(req["approved"]==true);receipt["resolved_at"]=json!(now());
            doc["revisions"].as_array_mut().unwrap().push(receipt);doc["proposal"]=Value::Null;
        }
        "orchestration.gate.resolve" => {
            ensure!(req["revision"]==doc["confirmed_revision"], "确认点属于旧版本");
            let node=doc["nodes"].as_array().context("尚未启动")?.iter().find(|n|n["id"]==req["node_id"]).context("步骤不存在")?;
            ensure!(node["human_gate"]==true, "步骤没有人工确认点");
            let node_id=field(req,"node_id")?;
            ensure!(list(db,"work",field(doc,"collab_id")?)?.iter().any(|w|w["work_key"]==node_id && w["state"]=="accepted"), "技术验收尚未通过");
            ensure!(req["approved"].is_boolean(), "需要明确决定");
            doc["gates"][node_id]=json!({"approved":req["approved"],"revision":req["revision"],"summary":field(req,"summary")?,"at":now()});
            if req["approved"]!=true {doc["state"]=json!("paused");}
        }
        "orchestration.retry" | "collab.retry.propose" => {
            let work_id=if req["cmd"]=="orchestration.retry" {field(&doc["bindings"][&field(req,"node_id")?],"work_id")?}else{field(req,"work_id")?};
            let work=load(db,"work",work_id)?;
            ensure!(work["run_id"]==doc["collab_id"],"重试步骤不属于当前协作");
            ensure!(work["state"]=="failed"&&work["attempt_count"]==1,"两次尝试已用尽，或步骤不在首次失败状态");
            let cause_key=field(req,"cause_key")?;let evidence=field(req,"new_evidence")?;
            if record_retry_failure(doc,&work,cause_key,evidence)? {doc["retry_evidence"][&field(&work,"work_key")?]=json!({"cause_key":cause_key,"evidence":evidence});event(doc,"retry","已记录根因与修正依据，允许一次重试");}
            else {doc["state"]=json!("paused");doc["blocker"]=json!(format!("根因 {cause_key} 已有两次实际失败，停止第三次尝试；请根据失败账本决定唯一后续方案"));event(doc,"convergence","同一根因已达到两次失败上限，停止派发");}
        }
        _=>bail!("unsupported v2 mutation"),
    }
    if !matches!(field(req,"cmd")?,"orchestration.retry"|"collab.retry.propose") || doc["state"]!="paused" {doc["blocker"]=Value::Null;}
    event(doc,field(req,"cmd")?,"协作方案或确认记录已更新");Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn remote_role_contract_is_bound_and_cannot_fall_back_to_local(){
        let mut draft=seed("codex","goal works");let profile=uuid::Uuid::new_v4().to_string();
        draft["roles"][2]["remote_codex"]=json!({"ssh_profile_id":profile,"ssh_profile_fingerprint":format!("sha256:{}","a".repeat(64))});
        let legacy=validate(&draft).unwrap();
        assert!(crate::collaboration::providers::effective_model(&legacy.roles[2]).is_empty());
        assert!(crate::collaboration::providers::validate_remote(&legacy.roles[2]).is_err());
        draft["roles"][2]["remote_codex"]["remote_model"]=json!("deepseek-flash");
        draft["roles"][2]["remote_codex"]["provider_fingerprint"]=json!(format!("sha256:{}","f".repeat(64)));
        draft["roles"][2]["remote_codex"]["ssh_target_fingerprint"]=json!(format!("sha256:{}","e".repeat(64)));
        let plan=validate(&draft).unwrap();let role=&plan.roles[2];
        assert_eq!(crate::collaboration::providers::effective_model(role),"deepseek-flash");
        let mut wrong_model=role.clone();wrong_model.model="gpt-6-terra".into();
        assert!(crate::collaboration::providers::validate_remote(&wrong_model).is_err());
        assert_eq!(crate::collaboration::providers::validate(role).is_ok(),cfg!(windows));
        crate::collaboration::providers::validate_remote(role).unwrap();
        let mut excessive=role.clone();excessive.tools.push("shell".into());assert!(crate::collaboration::providers::validate_remote(&excessive).is_err());
        excessive=role.clone();excessive.kind="coordinate".into();assert!(crate::collaboration::providers::validate_remote(&excessive).is_err());
        let original=crate::collaboration::providers::role_fingerprint(role).unwrap();
        draft["roles"][2]["remote_codex"]["ssh_profile_fingerprint"]=json!(format!("sha256:{}","b".repeat(64)));
        let changed=validate(&draft).unwrap();assert_ne!(original,crate::collaboration::providers::role_fingerprint(&changed.roles[2]).unwrap());
        draft["roles"][2]["remote_codex"]["ssh_profile_fingerprint"]=json!("sha256:bad");assert!(validate(&draft).is_err());
        draft["roles"][2]["remote_codex"]["ssh_profile_fingerprint"]=json!(format!("sha256:{}","a".repeat(64)));draft["roles"][2]["provider"]=json!("claude-code");assert!(validate(&draft).is_err());
        let old=validate(&seed("codex","goal works")).unwrap();assert!(serde_json::to_value(&old.roles[0]).unwrap().get("remote_codex").is_none());
    }
    #[test]fn orchestration_v2_new_provider_scope_preserves_only_existing_legacy_roles(){
        for provider in PROVIDERS {assert!(validate_provider_scope(&validate(&seed(provider,"goal")).unwrap(),None).is_ok());}
        let old=validate(&seed("kimi","goal")).unwrap();assert!(validate_provider_scope(&old,None).is_err());assert!(validate_provider_scope(&old,Some(&old)).is_ok());
        let mut changed=old.clone();changed.roles[0].provider="gemini".into();assert!(validate_provider_scope(&changed,Some(&old)).is_err());changed.roles[0].provider="omp".into();assert!(validate_provider_scope(&changed,Some(&old)).is_ok());
    }
    #[test]fn orchestration_v2_contract_rejects_cycles_and_wrong_review_authority(){
        let p=seed("kimi","goal works");assert!(validate(&p).is_ok());
        let mut bad=p.clone();bad["nodes"][0]["depends_on"]=json!(["implement"]);assert!(validate(&bad).is_err());
        bad=p;bad["review_role"]=json!("implement");assert!(validate(&bad).is_err());
    }
    #[test]fn orchestration_v2_final_validation_is_separate_from_integration(){
        let p=validate(&seed("codex","complete goal")).unwrap();let nodes=confirmed_nodes(&p);
        assert_eq!(nodes.last().unwrap().role_id,p.verification_role);
        assert_eq!(nodes.last().unwrap().depends_on,vec!["__integrate"]);
    }
    #[test]fn orchestration_v2_system_stage_gates_are_part_of_the_confirmed_contract(){
        let mut draft=seed("codex","complete goal");
        draft["integration_human_gate"]=json!(true);draft["verification_human_gate"]=json!(true);
        let nodes=confirmed_nodes(&validate(&draft).unwrap());
        assert!(nodes.iter().find(|node|node.id=="__integrate").unwrap().human_gate);
        assert!(nodes.iter().find(|node|node.id=="__verify").unwrap().human_gate);
    }
    #[test]fn orchestration_v2_external_mcp_requires_a_declared_selected_catalog_tool(){
        let mut plan=seed("codex","goal");plan["roles"][0]["mcp_servers"]=json!([{"id":"docs","command":"docs-mcp","args":[],"catalog":["search","read"],"tools":["read"]}]);assert!(validate(&plan).is_ok());
        plan["roles"][0]["mcp_servers"][0]["tools"]=json!(["write"]);assert!(validate(&plan).is_err());
        plan["roles"][0]["mcp_servers"][0]["tools"]=json!(["read"]);plan["roles"][0]["mcp_servers"][0]["id"]=json!("velella");assert!(validate(&plan).is_err());
    }
    #[test]fn orchestration_v2_codex_defaults_are_derived_not_persisted_as_user_models(){
        let plan=seed("codex","goal");assert!(plan["roles"].as_array().unwrap().iter().all(|role|role["model"]==""));assert_eq!(default_model("codex","implement"),"gpt-6-terra");assert_eq!(default_model("codex","review"),"gpt-6-luna");
    }
    #[test]fn orchestration_v2_failure_ledger_blocks_a_third_attempt_for_the_same_cause(){
        let mut doc=json!({"failure_ledger":[]});let first=json!({"id":"work-a","work_key":"first","attempt_id":"attempt-a"});let second=json!({"id":"work-b","work_key":"second","attempt_id":"attempt-b"});
        assert!(record_retry_failure(&mut doc,&first,"compiler-missing","first verified observation").unwrap());
        assert!(!record_retry_failure(&mut doc,&second,"compiler-missing","same verified observation").unwrap());
        assert_eq!(doc["failure_ledger"].as_array().unwrap().len(),2);
    }
}
