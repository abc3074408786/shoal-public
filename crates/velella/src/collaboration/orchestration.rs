//! Durable, desktop-owned plans. Credentials and effect receipts never enter UI snapshots.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub(crate) mod v2;
pub(crate) mod evidence;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub instruction: String,
    pub role: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub scope: String,
    pub checks: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub acceptance: String,
    #[serde(default = "one")]
    pub parallelism: usize,
    pub nodes: Vec<Node>,
}
fn one() -> usize { 1 }

pub fn validate(value: &Value) -> Result<Plan> {
    let plan: Plan = serde_json::from_value(value.clone())?;
    ensure!(!plan.acceptance.trim().is_empty(), "请填写总体验收条件");
    ensure!((1..=5).contains(&plan.parallelism), "探子并发数必须为 1–5");
    ensure!(!plan.nodes.is_empty() && plan.nodes.len() <= 64, "计划需要 1–64 个任务");
    let mut ids = HashSet::new();
    for n in &plan.nodes {
        ensure!(!n.id.is_empty() && n.id.len() <= 80 && n.id != "__final" && n.id.bytes().all(|c|c.is_ascii_alphanumeric() || c == b'_' || c == b'-'), "任务标识无效或使用了保留名称");
        ensure!(ids.insert(n.id.as_str()), "任务标识重复");
        ensure!(matches!(n.role.as_str(), "lead" | "scout"), "任务角色必须为 lead 或 scout");
        ensure!(!n.title.trim().is_empty() && !n.instruction.trim().is_empty() && !n.scope.trim().is_empty(), "任务需要标题、说明和范围");
        ensure!(!n.checks.is_empty() && n.checks.iter().all(|c|!c.trim().is_empty()), "每个任务需要验收要求");
    }
    let mut done = HashSet::new();
    for n in &plan.nodes { for dep in &n.depends_on { ensure!(ids.contains(dep.as_str()), "依赖的任务不存在: {dep}"); } }
    loop {
        let before = done.len();
        for n in &plan.nodes { if n.depends_on.iter().all(|d|done.contains(d.as_str())) { done.insert(n.id.as_str()); } }
        if done.len() == plan.nodes.len() { break; }
        ensure!(done.len() > before, "任务依赖存在循环");
    }
    Ok(plan)
}
pub fn confirmed_nodes(plan: &Plan) -> Vec<Node> {
    let mut nodes = plan.nodes.clone();
    nodes.push(Node { id: "__final".into(), title: "整体验证与交付".into(), role: "lead".into(),
        depends_on: nodes.iter().map(|n|n.id.clone()).collect(), scope: "整个已确认目标与代码变更".into(),
        instruction: format!("对整个目标执行约定验证，整理交付说明和未完成事项。不得扩大范围。总体验收条件：{}", plan.acceptance), checks: vec![plan.acceptance.clone()] });
    nodes
}
pub fn event(doc: &mut Value, kind: &str, text: &str) {
    if !doc["events"].is_array() { doc["events"] = json!([]); }
    let events=doc["events"].as_array_mut().unwrap();
    events.push(json!({"at":now(),"kind":kind,"text":text}));
    if events.len()>500 { events.remove(0); }
    doc["updated_at"] = json!(now());
}
fn all(db: &Connection) -> Result<Vec<Value>> {
    let mut s=db.prepare("SELECT document FROM objects WHERE kind='orchestration' ORDER BY rowid DESC")?;
    let rows=s.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|s|Ok(serde_json::from_str(&s)?)).collect()
}
pub(super) fn find(db: &Connection, run: &str) -> Result<Option<Value>> {
    Ok(all(db)?.into_iter().find(|d|d["collab_id"]==run))
}
pub(super) fn submit(db: &Connection, req: &Value) -> Result<Value> {
    let (run,actor)=authenticate(db,req,true)?; coordinator(&run,&actor)?;
    let mut doc=find(db,field(&run,"id")?)?.context("not a managed orchestration")?;
    if doc["schema_version"]==2 && matches!(req["cmd"].as_str(),Some("collab.plan.submit"|"collab.revision.propose"|"collab.retry.propose")) {
        v2::mutate(db,req,&mut doc)?;put(db,"orchestration",&doc)?;
        return Ok(json!({"ok":true,"revision":doc["revision"]}));
    }
    match field(req,"cmd")? {
        "collab.plan.submit" => {
            ensure!(matches!(doc["state"].as_str(),Some("planning"|"draft")),"计划已确认，不能由 Agent 修改");
            ensure!(req["revision"]==doc["revision"],"计划版本已变化，请先读取最新状态");
            validate(&req["plan"])?;
            doc["plan"]=req["plan"].clone(); doc["revision"]=json!(doc["revision"].as_u64().unwrap_or(0)+1);
            doc["state"]=json!("draft"); doc["blocker"]=Value::Null;
            event(&mut doc,"draft","主 Agent 已提交方案，等待确认");
        }
        "collab.retry.propose" => {
            let work=load(db,"work",field(req,"work_id")?)?;
            ensure!(work["run_id"]==run["id"] && work["state"]=="failed" && work["attempt_count"]==1,"只能为首次失败提交重试证据");
            let key=field(&work,"work_key")?;
            doc["retry_evidence"][key]=json!(field(req,"new_evidence")?);
            event(&mut doc,"retry","已记录根因与修正依据，允许一次重试");
        }
        _=>bail!("unsupported plan command"),
    }
    put(db,"orchestration",&doc)?;
    Ok(json!({"ok":true,"revision":doc["revision"]}))
}
/// The collaboration service calls this only after a failed report is durable.
/// First failures become cross-node ledger entries when the coordinator later
/// supplies an explicit root-cause key; retry failures retain that key here.
pub(crate) fn record_failure(db:&Connection,work:&Value,attempt:&Value,cause_key:Option<&str>)->Result<()> {
    let Some(mut doc)=find(db,field(work,"run_id")?)? else{return Ok(())};
    if doc["schema_version"]==2 {v2::record_actual_failure(&mut doc,work,cause_key.context("v2 失败报告需要稳定根因标识 cause_key")?,attempt["report"]["summary"].as_str().unwrap_or("failure report"))?;put(db,"orchestration",&doc)?;}
    Ok(())
}

impl Hub {
    /// Gather under the DB lock; verify files outside it in the desktop adapter.
    pub(crate) fn orchestration_evidence(&self,work:&Value,cmd:&str)->Result<Vec<Value>> {
        if !work["evidence_contract"].is_object(){return Ok(vec![])}
        let db=self.db.lock().unwrap();let mut values=Vec::new();
        if matches!(cmd,"collab.review.request"|"collab.review.report"|"collab.accept") {
            values.push(load(&db,"attempt",field(work,"attempt_id")?)?["report"]["evidence"].clone());
        }
        if cmd=="collab.accept" && work["write"]==true {
            let reviews=list(&db,"review",field(work,"run_id")?)?;
            let review=reviews.iter().find(|r|r["work_id"]==work["id"] && r["attempt_id"]==work["attempt_id"] && r["state"]=="passed").context("独立审查尚未通过")?;
            values.push(review["evidence"].clone());
        }
        Ok(values)
    }
    pub(crate) fn orchestration_templates(&self)->Result<Vec<Value>> {
        let db=self.db.lock().unwrap();let mut s=db.prepare("SELECT document FROM objects WHERE kind='orchestration_template' ORDER BY rowid DESC")?;
        let rows=s.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|s|Ok(serde_json::from_str(&s)?)).collect()
    }
    pub(crate) fn orchestration_records(&self) -> Result<Vec<Value>> { all(&self.db.lock().unwrap()) }
    pub(crate) fn orchestration_get(&self, id: &str) -> Result<Value> { load(&self.db.lock().unwrap(),"orchestration",id) }
    pub(crate) fn orchestration_put(&self, doc: &Value) -> Result<()> { put(&self.db.lock().unwrap(),"orchestration",doc) }
    pub(crate) fn orchestration_status(&self, doc: &Value) -> Result<Value> {
        let db=self.db.lock().unwrap();
        let Some(run)=doc["collab_id"].as_str() else { return Ok(json!({"members":[],"work":[],"reviews":[],"attempts":[]})); };
        Ok(json!({"members":list(&db,"member",run)?.into_iter().map(public).collect::<Vec<_>>(),"work":list(&db,"work",run)?,"reviews":list(&db,"review",run)?,"attempts":list(&db,"attempt",run)?}))
    }
    pub(crate) fn orchestration_public(&self, doc: &Value) -> Result<Value> {
        let mut output=doc.clone();
        for key in ["effects","credentials","member_credentials","retry_evidence"] { output.as_object_mut().unwrap().remove(key); }
        output["status"]=self.orchestration_status(doc)?;
        Ok(output)
    }
    /// Called only on the public adapter path; internal scheduler authority is a Rust call,
    /// never a forgeable JSON flag or a renderer-visible credential.
    pub(crate) fn orchestration_guard(&self, req: &Value) -> Result<()> {
        let db=self.db.lock().unwrap();
        let cmd=field(req,"cmd")?;
        let managed=if let Some(run)=req["run_id"].as_str(){find(&db,run)?.is_some()}else{false};
        if managed {
            ensure!(matches!(cmd,"collab.status"|"collab.receive"|"collab.ack"|"collab.started"|"collab.report"|"collab.review.report"|"collab.candidate"|"collab.reconcile"|"collab.plan.submit"|"collab.revision.propose"|"collab.retry.propose"), "托管协作必须由已确认计划调度，不能直接派发、邀请或验收");
        }
        if cmd=="collab.create" {
            ensure!(!all(&db)?.iter().any(|d|d["lead_session"]==req["session_id"] && !d["lead_session"].is_null()),"该会话已由编排服务管理");
        }
        Ok(())
    }
    pub(crate) fn orchestration_mutate(&self, req: &Value) -> Result<Value> {
        super::mcp_catalog::verify_plan(&req["plan"])?;
        if matches!(req["cmd"].as_str(),Some("orchestration.plan.confirm"|"orchestration.revision.resolve")) {
            let current=self.orchestration_get(field(req,"id")?)?;
            let plan=if req["cmd"]=="orchestration.plan.confirm" {&current["plan"]}else if req["approved"]==true {&current["proposal"]["plan"]}else {&Value::Null};
            super::mcp_catalog::verify_plan(plan)?;
        }
        let mut db=self.db.lock().unwrap();
        let rid=field(req,"request_id")?;
        if let Some(r)=replay(&db,rid,&req.to_string())? {return Ok(r)}
        let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cmd=field(req,"cmd")?;
        if cmd=="orchestration.templates.save" {
            super::mcp_catalog::confirmed(&tx,v2::validate(&req["plan"])?)?;
            let key=req["template_id"].as_str().unwrap_or(rid);
            let prior=load(&tx,"orchestration_template",key).ok();
            let revision=prior.as_ref().and_then(|p|p["revision"].as_u64()).unwrap_or(0);
            ensure!(req["revision"].as_u64().unwrap_or(0)==revision,"模板版本已变化");
            let doc=json!({"id":key,"name":field(req,"name")?,"revision":revision+1,"plan":req["plan"],"updated_at":now()});
            put(&tx,"orchestration_template",&doc)?;let result=json!({"ok":true,"template":doc});
            tx.execute("INSERT INTO requests(id,request,response) VALUES(?1,?2,?3)",params![rid,req.to_string(),result.to_string()])?;tx.commit()?;return Ok(result);
        }
        let mut doc=if matches!(cmd,"orchestration.create"|"orchestration.reopen") {
            let predecessor=if cmd=="orchestration.reopen" {let source=load(&tx,"orchestration",field(req,"id")?)?;ensure!(source["schema_version"]==2&&source["state"]=="accepted"&&source["user_acceptance"]["candidate"]==source["final_candidate"],"前任候选不可重开");ensure!(matches!(req["reason_kind"].as_str(),Some("reproducible_defect"|"changed_requirement")),"重开原因无效");ensure!(!field(req,"evidence")?.trim().is_empty(),"重开依据不能为空");ensure!(req["agent"]==source["agent"],"继任主控 CLI 与前任不一致");json!({"id":source["id"],"accepted_at":source["user_acceptance"]["at"],"candidate":source["final_candidate"],"integration_workspace":source["integration_workspace"],"integration_branch":source["integration_branch"],"reason_kind":req["reason_kind"],"evidence":req["evidence"]})}else{Value::Null};
            let workspace=if cmd=="orchestration.reopen" {field(&predecessor,"integration_workspace")?}else{field(req,"workspace_id")?};
            ensure!(!all(&tx)?.iter().any(|d|d["workspace_id"]==workspace && !matches!(d["state"].as_str(),Some("accepted"|"stopped"))),"该工作区已有未结束的协作，请先完成或停止它");
            ensure!(matches!(req["agent"].as_str(),Some("codex"|"claude-code")) || req["schema_version"]==2 && v2::PROVIDERS.contains(&req["agent"].as_str().unwrap_or("")),"选择支持的 CLI");
            json!({"id":rid,"run_id":rid,"workspace_id":workspace,"goal":field(req,"goal")?,"acceptance":field(req,"acceptance")?,"agent":req["agent"],"scout_agent":req["scout_agent"].as_str().unwrap_or(field(req,"agent")?),"state":"planning","revision":0,"created_at":now(),"effects":{},"bindings":{},"member_credentials":{},"retry_evidence":{},"events":[],"predecessor":predecessor})
        } else {load(&tx,"orchestration",field(req,"id")?)?};
        if (doc["schema_version"]==2 || cmd=="orchestration.reopen" || cmd=="orchestration.create" && req["schema_version"]==2) && matches!(cmd,"orchestration.create"|"orchestration.reopen"|"orchestration.plan.save"|"orchestration.plan.refine"|"orchestration.plan.confirm"|"orchestration.revision.propose"|"orchestration.revision.resolve"|"orchestration.gate.resolve"|"orchestration.retry") {
            v2::mutate(&tx,req,&mut doc)?;
        } else { match cmd {
            "orchestration.create"=>{ensure!(matches!(doc["scout_agent"].as_str(),Some("codex"|"claude-code")),"探子必须使用 Codex 或 Claude Code");event(&mut doc,"created","正在准备主 Agent，随后生成方案");}
            "orchestration.plan.save"=>{
                ensure!(matches!(doc["state"].as_str(),Some("planning"|"draft")),"已启动的计划不可修改");
                ensure!(req["revision"]==doc["revision"],"计划版本已变化，请刷新后编辑"); validate(&req["plan"])?;
                doc["plan"]=req["plan"].clone();doc["revision"]=json!(doc["revision"].as_u64().unwrap()+1);doc["state"]=json!("draft");event(&mut doc,"saved","方案已保存");
            }
            "orchestration.plan.confirm"=>{
                ensure!(doc["state"]=="draft" && req["revision"]==doc["revision"],"请确认当前版本的方案");
                ensure!(doc["collab_id"].is_string(),"主 Agent 尚未就绪");
                let plan=validate(&doc["plan"])?;doc["nodes"]=serde_json::to_value(confirmed_nodes(&plan))?;
                doc["confirmed_revision"]=doc["revision"].clone();doc["state"]=json!("running");doc["blocker"]=Value::Null;
                event(&mut doc,"confirmed","方案已确认，开始按依赖执行");
            }
            "orchestration.pause"=>{ensure!(doc["state"]=="running","只能暂停执行中的协作");doc["state"]=json!("paused");event(&mut doc,"paused","已暂停新派发，当前任务可以完成");}
            "orchestration.resume"=>{
                ensure!(matches!(doc["state"].as_str(),Some("running"|"paused"|"planning"|"stopping")),"当前状态不能继续");
                if doc["state"]=="paused"{doc["state"]=json!("running")}
                // Explicit recovery permits one replay of an idempotent action after
                // the user has resolved its prerequisite. Input is never resent.
                for e in doc["effects"].as_object_mut().unwrap().values_mut(){
                    if e["error"].is_string() && e["request"]["cmd"]!="collab.notify" && e["recovery_count"].as_u64().unwrap_or(0)==0 {
                        e.as_object_mut().unwrap().remove("error");e["recovery_count"]=json!(1);
                    }
                }
                doc["blocker"]=Value::Null;event(&mut doc,"resume","重新核对会话与回执后继续");
            }
            "orchestration.stop"=>{ensure!(!matches!(doc["state"].as_str(),Some("accepted"|"stopped")),"协作已结束");doc["state"]=json!("stopping");event(&mut doc,"stopping","正在停止本次协作的会话");}
            "orchestration.retry"=>{
                let key=field(req,"node_id")?;let work=load(&tx,"work",field(&doc["bindings"][key],"work_id")?)?;
                ensure!(work["state"]=="failed" && work["attempt_count"]==1,"两次尝试已用尽，或任务不在首次失败状态");
                doc["retry_evidence"][key]=json!(field(req,"new_evidence")?);doc["blocker"]=Value::Null;event(&mut doc,"retry","用户提供了根因与方案调整，允许一次重试");
            }
            _=>bail!("unknown orchestration command"),
        }}
        put(&tx,"orchestration",&doc)?;
        let result=json!({"ok":true,"id":doc["id"],"revision":doc["revision"]});
        tx.execute("INSERT INTO requests(id,request,response) VALUES(?1,?2,?3)",params![rid,req.to_string(),result.to_string()])?;tx.commit()?;Ok(result)
    }
}

pub fn work_by_key(status: &Value) -> HashMap<String,Value> {
    status["work"].as_array().into_iter().flatten().filter_map(|w|Some((w["work_key"].as_str()?.into(),w.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn initial_snapshot_has_complete_renderer_status(){
        let root=std::env::temp_dir().join(format!("orchestration-initial-{}",id()));
        let h=Hub::open(&root).unwrap();let rid=id();
        h.orchestration_mutate(&json!({"cmd":"orchestration.create","request_id":rid,"workspace_id":"replay-workspace","goal":"fix add","acceptance":"tests pass","agent":"codex"})).unwrap();
        let snapshot=h.orchestration_public(&h.orchestration_get(&rid).unwrap()).unwrap();
        if let Ok(output)=std::env::var("VELELLA_INITIAL_SNAPSHOT_OUTPUT"){
            std::fs::write(output,serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
        }
        assert_eq!(snapshot["state"],"planning");
        for field in ["members","work","reviews","attempts"]{
            assert_eq!(snapshot["status"][field],json!([]),"initial status field {field}");
        }
        drop(h);std::fs::remove_dir_all(root).unwrap();
    }
    fn plan()->Value{json!({"acceptance":"tests pass","parallelism":1,"nodes":[{"id":"inspect","title":"inspect","instruction":"read","role":"scout","depends_on":[],"scope":"src","checks":["file:line"]},{"id":"fix","title":"fix","instruction":"fix and test","role":"lead","depends_on":["inspect"],"scope":"src","checks":["cargo test"]}]})}
    #[test]fn rejects_cycles_missing_dependencies_and_reserved_final(){
        let good=plan();assert!(validate(&good).is_ok());
        let mut bad=good.clone();bad["nodes"][0]["depends_on"]=json!(["fix"]);assert!(validate(&bad).unwrap_err().to_string().contains("循环"));
        bad=good.clone();bad["nodes"][1]["depends_on"]=json!(["missing"]);assert!(validate(&bad).is_err());
        bad=good.clone();bad["nodes"][0]["id"]=json!("__final");assert!(validate(&bad).is_err());
        bad=good;bad["parallelism"]=json!(6);assert!(validate(&bad).is_err());
    }
    #[test]fn confirmation_is_versioned_persistent_and_keeps_credentials_private(){
        let root=std::env::temp_dir().join(format!("orchestration-store-{}",id()));let h=Hub::open(&root).unwrap();
        let rid=id();let req=json!({"cmd":"orchestration.create","request_id":rid,"workspace_id":"w","goal":"fix","acceptance":"tests","agent":"codex"});
        let first=h.orchestration_mutate(&req).unwrap();assert_eq!(first,h.orchestration_mutate(&req).unwrap());
        let mut conflict=req.clone();conflict["goal"]=json!("other");assert!(h.orchestration_mutate(&conflict).is_err());
        let save=json!({"cmd":"orchestration.plan.save","request_id":id(),"id":rid,"revision":0,"plan":plan()});h.orchestration_mutate(&save).unwrap();
        let stale=json!({"cmd":"orchestration.plan.confirm","request_id":id(),"id":rid,"revision":0});assert!(h.orchestration_mutate(&stale).is_err());
        let mut doc=h.orchestration_get(&rid).unwrap();doc["collab_id"]=json!("native-run");doc["credentials"]=json!({"token":"secret"});doc["member_credentials"]=json!({"member":{"token":"secret"}});doc["effects"]=json!({"launch":{"request":{"token":"secret"}}});h.orchestration_put(&doc).unwrap();
        h.orchestration_mutate(&json!({"cmd":"orchestration.plan.confirm","request_id":id(),"id":rid,"revision":1})).unwrap();
        assert!(h.orchestration_mutate(&json!({"cmd":"orchestration.plan.save","request_id":id(),"id":rid,"revision":1,"plan":plan()})).is_err());
        assert!(h.orchestration_guard(&json!({"cmd":"collab.assign","run_id":"native-run","trusted":true})).is_err());
        assert!(h.orchestration_guard(&json!({"cmd":"collab.report","run_id":"native-run"})).is_ok());
        drop(h);let h=Hub::open(&root).unwrap();let doc=h.orchestration_get(&rid).unwrap();assert_eq!(doc["state"],"running");assert_eq!(doc["nodes"][2]["id"],"__final");assert!(!h.orchestration_public(&doc).unwrap().to_string().contains("secret"));drop(h);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn recovery_replays_safe_effects_once_but_never_input(){
        let root=std::env::temp_dir().join(format!("orchestration-recovery-{}",id()));let h=Hub::open(&root).unwrap();let rid=id();
        h.orchestration_mutate(&json!({"cmd":"orchestration.create","request_id":rid,"workspace_id":"w","goal":"fix","acceptance":"tests","agent":"codex"})).unwrap();
        let mut doc=h.orchestration_get(&rid).unwrap();doc["effects"]=json!({"launch":{"request":{"cmd":"collab.launch","request_id":"stable"},"error":"temporary"},"input":{"request":{"cmd":"collab.notify"},"error":"uncertain"}});h.orchestration_put(&doc).unwrap();
        h.orchestration_mutate(&json!({"cmd":"orchestration.resume","request_id":id(),"id":rid})).unwrap();
        doc=h.orchestration_get(&rid).unwrap();assert!(doc["effects"]["launch"]["error"].is_null());assert_eq!(doc["effects"]["launch"]["request"]["request_id"],"stable");assert_eq!(doc["effects"]["input"]["error"],"uncertain");
        doc["effects"]["launch"]["error"]=json!("again");h.orchestration_put(&doc).unwrap();h.orchestration_mutate(&json!({"cmd":"orchestration.resume","request_id":id(),"id":rid})).unwrap();assert_eq!(h.orchestration_get(&rid).unwrap()["effects"]["launch"]["error"],"again");drop(h);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn same_workspace_creation_is_atomic_across_connections(){
        let root=std::env::temp_dir().join(format!("orchestration-race-{}",id()));let first=Hub::open(&root).unwrap();let second=Hub::open(&root).unwrap();let barrier=std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles=[first,second].into_iter().map(|h|{let b=barrier.clone();std::thread::spawn(move||{b.wait();h.orchestration_mutate(&json!({"cmd":"orchestration.create","request_id":id(),"workspace_id":"w","goal":"fix","acceptance":"tests","agent":"codex"})).is_ok()})}).collect::<Vec<_>>();
        assert_eq!(handles.into_iter().map(|h|h.join().unwrap() as usize).sum::<usize>(),1);let h=Hub::open(&root).unwrap();assert_eq!(h.orchestration_records().unwrap().len(),1);drop(h);std::fs::remove_dir_all(root).unwrap();
    }
}
