//! Managed orchestration adapter. Native I/O is never performed under Service's lock.
use super::*;
use crate::collaboration::{Hub, orchestration::{self as model, Node}};
mod progress;

fn hub(service:&Arc<Mutex<Service>>) -> Result<Arc<Hub>> {
    let (root,slot)={let s=service.lock().unwrap();(s.root.clone(),s.collaboration.clone())};
    let mut h=slot.lock().unwrap();
    if h.is_none(){*h=Some(Arc::new(Hub::open(&root)?));}
    Ok(h.as_ref().unwrap().clone())
}
fn gate(service:&Arc<Mutex<Service>>, id:&str)->Arc<Mutex<()>> {
    static GATES:std::sync::OnceLock<Mutex<HashMap<String,Arc<Mutex<()>>>>>=std::sync::OnceLock::new();
    let root=service.lock().unwrap().root.to_string_lossy().into_owned();
    GATES.get_or_init(Default::default).lock().unwrap().entry(format!("{root}/{id}")).or_default().clone()
}
pub(super) fn agent_gate(service:&Arc<Mutex<Service>>,req:&Value)->Result<Option<Arc<Mutex<()>>>> {
    if !matches!(req["cmd"].as_str(),Some("collab.plan.submit"|"collab.revision.propose"|"collab.retry.propose")){return Ok(None)}
    Ok(hub(service)?.orchestration_records()?.iter().find(|d|d["collab_id"]==req["run_id"]).map(|d|gate(service,d["id"].as_str().unwrap())))
}
fn text<'a>(v:&'a Value,key:&str)->Result<&'a str>{v[key].as_str().with_context(||format!("missing {key}"))}
fn bind(service:&Arc<Mutex<Service>>,response:&Value)->Result<()> {
    let root=service.lock().unwrap().root.clone();
    let cred=&response["credentials"];let member=text(cred,"member_id")?;
    let session=text(&response["member"]["identity"],"session_id")?;
    let clients=root.join("collaboration-clients");let sessions=root.join("collaboration-sessions");
    std::fs::create_dir_all(&clients)?;std::fs::create_dir_all(&sessions)?;
    ipc::atomic_json(&clients.join(format!("{member}.json")),cred)?;
    ipc::atomic_json(&sessions.join(format!("{session}.json")),&json!({"session_id":session,"member_id":member}))?;
    Ok(())
}
fn with_cred(cred:&Value,mut request:Value)->Value {
    for key in ["run_id","member_id","token"] {request[key]=cred[key].clone();}request
}
// The journal is saved BEFORE external effects. Reconcile idempotent requests with
// their original UUID. Native input is never replayed after a missing receipt.
macro_rules! effect {
    ($service:expr,$hub:expr,$doc:expr,$key:expr,$req:expr) => {{
        let request=$req; let key=$key;
        run_effect($service,$hub,$doc,key,request)
    }};
}
mod v2;
mod workspaces;
fn run_effect(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,key:&str,mut req:Value)->Result<Value> {
    let prior=doc["effects"][key].clone();
    if let Some(result)=prior.get("result") {return Ok(result.clone())}
    if let Some(error)=prior["error"].as_str(){anyhow::bail!("{error}")}
    if prior.is_object() {
        req=prior["request"].clone();
        anyhow::ensure!(req["cmd"]!="collab.notify","输入发送结果不确定，禁止自动重发；请打开会话核对");
    }else{
        req["request_id"]=json!(uuid::Uuid::now_v7().to_string());
        doc["effects"][key]=json!({"request":req,"issued_at":chrono::Utc::now().to_rfc3339()});h.orchestration_put(doc)?;
    }
    let result=if req["cmd"].as_str().unwrap_or("").starts_with("collab."){collaboration_commands::managed(service,req)}else{terminal_io::dispatch(service,req)};
    match result {
        Ok(result)=>{anyhow::ensure!(result["ok"]==true,"后台动作未确认成功");doc["effects"][key]["result"]=result.clone();h.orchestration_put(doc)?;Ok(result)}
        Err(error)=>{doc["effects"][key]["error"]=json!(error.to_string());h.orchestration_put(doc)?;Err(error)}
    }
}
fn session_status(service:&Arc<Mutex<Service>>,doc:&Value,session:&str)->Result<Status>{
    let desc={let s=service.lock().unwrap();s.workbench.sessions(None)?.into_iter().find(|s|s.id==session && (s.workspace_id==doc["workspace_id"] || doc["owned_workspaces"].as_array().is_some_and(|ws|ws.contains(&json!(s.workspace_id))))).context("协作会话不存在")?.descriptor};
    anyhow::ensure!(!host::confirmed_dead(&desc.directory),"会话已退出，需要人工恢复");
    let response=ipc::request_with_timeout(&desc.endpoint,&json!({"cmd":"status"}),Duration::from_secs(2))?;
    anyhow::ensure!(response["ok"]==true,"无法确认会话状态");
    let status:Status=serde_json::from_value(response["status"].clone())?;
    anyhow::ensure!(status.exit.is_none(),"会话已退出，需要人工恢复");
    Ok(status)
}
fn context_usage_snapshot(service:&Arc<Mutex<Service>>,doc:&Value)->Result<Value>{
    let sessions={let state=service.lock().unwrap();v2::owned_sessions(doc).0.into_iter().map(|(workspace,session)|{
        let descriptor=state.workbench.session(&workspace,&session)?.map(|s|s.descriptor);
        Ok((workspace,session,descriptor))
    }).collect::<Result<Vec<_>>>()?};
    let mut observations=Vec::new();
    for (workspace,session,descriptor) in sessions {
        let Some(desc)=descriptor else{continue};
        let provider=desc.spawn.env.get("VELELLA_PROVIDER").cloned().unwrap_or_default();
        let result=crate::collaboration::context_usage::read(&desc.directory,&session);
        let (receipt,state)=match result {Ok(Some(r))=>(serde_json::to_value(r)?,"observed"),Ok(None)=>(Value::Null,"unknown"),Err(_)=>(Value::Null,"unavailable")};
        observations.push(json!({"session_id":session,"workspace_id":workspace,"provider":provider,"state":state,"receipt":receipt}));
    }
    Ok(json!(observations))
}
fn ready(service:&Arc<Mutex<Service>>,doc:&Value,session:&str)->Result<()> {
    let status=session_status(service,doc,session)?;
    anyhow::ensure!(!status.permission_input && status.state!="waiting_permission","等待原生授权，请打开会话处理");
    anyhow::ensure!(!status.human_control,"终端正由用户控制，请在终端交还控制权");
    anyhow::ensure!(status.hooks_ready||status.hybrid_worker_ready,"原生 hooks 或远程任务接收器尚未就绪；请打开会话检查状态");
    anyhow::ensure!(status.state=="waiting_input","Agent 尚未空闲，正在等待当前回合结束");
    Ok(())
}
fn notify(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,key:&str,member:&Value,message:String)->Result<()> {
    if doc["effects"][key]["result"].is_null(){
        let session=text(&member["identity"],"session_id")?;
        ready(service,doc,session)?;
        crate::collaboration::context_usage::guard(session_status(service,doc,session)?.context_usage.as_ref())?;
    }
    let result=effect!(service,h,doc,key,with_cred(&doc["credentials"],json!({"cmd":"collab.notify","to":member["id"],"text":message})))?;
    anyhow::ensure!(result["input"]["state"]=="accepted","输入未确认送达（{}）。打开会话核对，禁止自动重发。",result["input"]["reason"].as_str().unwrap_or("回执不确定"));
    Ok(())
}
fn lead_member(h:&Hub,doc:&Value)->Result<Value>{
    h.orchestration_status(doc)?["members"].as_array().context("members missing")?.iter().find(|m|m["id"]==doc["credentials"]["member_id"]).cloned().context("lead missing")
}
fn ensure_lead(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    if doc["credentials"].is_object(){return Ok(())}
    let opened=effect!(service,h,doc,"lead.open",json!({"cmd":"collab.start","workspace_id":doc["workspace_id"],"launch":doc["agent"]}))?;
    doc["lead_session"]=opened["session_id"].clone();h.orchestration_put(doc)?;
    let created=effect!(service,h,doc,"lead.create",json!({"cmd":"collab.create","session_id":doc["lead_session"],"goal":doc["goal"],"name":"主 Agent"}))?;
    bind(service,&created)?;doc["credentials"]=created["credentials"].clone();doc["collab_id"]=created["run"]["id"].clone();
    model::event(doc,"lead","主 Agent 会话已建立");h.orchestration_put(doc)
}
fn member(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,key:&str,name:&str)->Result<Value>{
    let launched=effect!(service,h,doc,&format!("{key}.open"),with_cred(&doc["credentials"],json!({"cmd":"collab.launch","workspace_id":doc["workspace_id"],"launch":doc["scout_agent"]})))?;
    let invited=effect!(service,h,doc,&format!("{key}.invite"),with_cred(&doc["credentials"],json!({"cmd":"collab.invite","session_id":launched["session_id"],"name":name})))?;
    let joined=effect!(service,h,doc,&format!("{key}.join"),json!({"cmd":"collab.join","session_id":launched["session_id"],"run_id":doc["collab_id"],"member_id":invited["member"]["id"],"invitation":invited["invitation"],"confirmed":true}))?;
    bind(service,&joined)?;doc["member_credentials"][text(&joined["member"],"id")?]=joined["credentials"].clone();h.orchestration_put(doc)?;
    Ok(joined["member"].clone())
}
fn retire(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,member_id:&str)->Result<()> {
    if doc["credentials"]["member_id"]==member_id{return Ok(())}
    let creds=doc["member_credentials"][member_id].clone();
    effect!(service,h,doc,&format!("leave.{member_id}"),with_cred(&creds,json!({"cmd":"collab.leave"})))?;Ok(())
}
fn planning(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    ensure_lead(service,h,doc)?;
    let member=lead_member(h,doc)?;
    let prompt=format!("你是本次研发协作的主 Agent。当前只允许只读探索和起草方案，不修改项目文件，不派生子代理。目标：{}\n总体验收：{}\n将 JSON 保存到 $VELELLA_DATA_DIR 下的方案文件（项目之外），再执行 velella-runtime collab --request-id <新UUID> plan-submit --revision {} --file <绝对路径>。提交后停止，等待用户确认。JSON 格式：{{\"acceptance\":\"总体验收条件\",\"parallelism\":1,\"nodes\":[{{\"id\":\"inspect\",\"title\":\"定位实现\",\"instruction\":\"只读探索具体问题并返回出处\",\"role\":\"scout\",\"depends_on\":[],\"scope\":\"相关源码\",\"checks\":[\"提供 file:line 与结论\"]}},{{\"id\":\"implement\",\"title\":\"实现目标\",\"instruction\":\"具体修改与验证\",\"role\":\"lead\",\"depends_on\":[\"inspect\"],\"scope\":\"明确文件或模块\",\"checks\":[\"实际运行必要测试\"]}}]}}。按目标调整任务；lead 是唯一写入者，scout 只读。系统会自动增加整体验证和独立审查，不要自行调度/验收。不得改变账户、模型、权限、全局配置或发布代码。",doc["goal"].as_str().unwrap(),doc["acceptance"].as_str().unwrap(),doc["revision"]);
    notify(service,h,doc,"plan.prompt",&member,prompt)
}
fn active(w:&Value)->bool{matches!(w["state"].as_str(),Some("assigned"|"input_accepted"|"turn_started"|"reported"))}
fn assignment_prompt(node:&Node)->String{
    format!("执行已确认任务：{}\n{}\n范围：{}\n验收要求：{}\n先读持久邮箱，找到当前 assignment 并执行 started --attempt <id>。{}不得派生子代理、改动范围、模型或权限。完成后提交实际检查结果，执行 report --attempt <id> --outcome completed --summary <结果> {}。失败则 report --outcome failed 并说明根因。提交报告后停止，不自行派发/验收。候选/报告临时文件须保存在 VELELLA_DATA_DIR 下、项目源码之外。",node.title,node.instruction,node.scope,node.checks.join("；"),if node.role=="scout"{"只读探索，禁止修改项目文件。"}else{"你是唯一代码写入者。"},if node.role=="lead"{"--candidate <由 candidate --work 取得的JSON文件>"}else{""})
}
fn dispatch_node(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,node:&Node,previous:Option<&Value>)->Result<()> {
    let attempt=previous.map_or(1,|w|w["attempt_count"].as_u64().unwrap_or(1)+1);
    let key=format!("node.{}.{}",node.id,attempt);
    let assignee=if node.role=="lead"{lead_member(h,doc)?}else{member(service,h,doc,&key,&node.title)?};
    if node.role=="lead"{effect!(service,h,doc,&format!("{key}.claim"),with_cred(&doc["credentials"],json!({"cmd":"collab.claim","writer":assignee["id"]})))?;}
    ready(service,doc,text(&assignee["identity"],"session_id")?)?;
    if doc["bindings"][&node.id]["attempt"].as_u64()!=Some(attempt){
        let baseline=if node.role=="scout"{crate::collaboration::source::candidate(Path::new(text(&assignee["identity"],"workspace_path")?))?}else{Value::Null};
        doc["bindings"][&node.id]=json!({"attempt":attempt,"session_id":assignee["identity"]["session_id"],"read_candidate":baseline});h.orchestration_put(doc)?;
    }
    let result=effect!(service,h,doc,&format!("{key}.assign"),with_cred(&doc["credentials"],json!({"cmd":"collab.assign","assignee":assignee["id"],"workspace_path":assignee["identity"]["workspace_path"],"work_key":node.id,"work_id":previous.map(|w|w["id"].clone()),"title":node.title,"instruction":assignment_prompt(node),"write":node.role=="lead","new_evidence":doc["retry_evidence"][&node.id]})))?;
    doc["bindings"][&node.id]["work_id"]=result["work"]["id"].clone();h.orchestration_put(doc)?;
    notify(service,h,doc,&format!("{key}.notify"),&assignee,assignment_prompt(node))?;
    Ok(())
}
fn finish_work(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,work:&Value,status:&Value)->Result<()> {
    let key=format!("review.{}.{}",text(work,"id")?,work["attempt_count"]);
    if work["write"]==true {
        let reviews:Vec<Value>=status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==work["id"] && r["attempt_id"]==work["attempt_id"]).cloned().collect();
        if let Some(passed)=reviews.iter().find(|r|r["state"]=="passed"){
            effect!(service,h,doc,&format!("{key}.accept"),with_cred(&doc["credentials"],json!({"cmd":"collab.accept","work_id":work["id"],"summary":"固定候选独立审查通过；总体验收仍由用户确认"})))?;
            retire(service,h,doc,text(passed,"reviewer")?)?;
            if work["work_key"]=="__final"{doc["final_candidate"]=work["candidate"].clone();doc["state"]=json!("awaiting_acceptance");model::event(doc,"ready","整体验证与独立审查完成，等待用户验收");}
            return h.orchestration_put(doc)
        }
        if !reviews.is_empty(){
            let reviewer=&reviews[0]["reviewer"];
            let m=status["members"].as_array().unwrap().iter().find(|m|m["id"]==*reviewer).context("reviewer missing")?;
            session_status(service,doc,text(&m["identity"],"session_id")?)?;
            notify(service,h,doc,&format!("{key}.notify"),m,review_prompt(&reviews[0]))?;
            return Ok(())
        }
        // Freeze before starting a separate native reviewer, using existing source checks.
        ready(service,doc,text(doc,"lead_session")?)?;
        let reviewer=member(service,h,doc,&key,"独立审查")?;
        ready(service,doc,text(&reviewer["identity"],"session_id")?)?;
        let review=effect!(service,h,doc,&format!("{key}.request"),with_cred(&doc["credentials"],json!({"cmd":"collab.review.request","work_id":work["id"],"reviewer":reviewer["id"]})))?;
        let prompt=review_prompt(&review["review"]);
        notify(service,h,doc,&format!("{key}.notify"),&reviewer,prompt)?;
    }else{
        let baseline=&doc["bindings"][text(work,"work_key")?]["read_candidate"];
        anyhow::ensure!(!baseline.is_null() && *baseline==crate::collaboration::source::candidate(Path::new(text(work,"workspace_path")?))?,"只读探索期间源码发生变化，停止推进并核对修改来源");
        effect!(service,h,doc,&format!("{key}.accept"),with_cred(&doc["credentials"],json!({"cmd":"collab.accept","work_id":work["id"],"summary":"探索报告已保存，后续主 Agent 按确认计划使用"})))?;
        retire(service,h,doc,text(work,"assignee")?)?;
    }
    Ok(())
}
fn review_prompt(review:&Value)->String{
    format!("你是独立只读审查者，不修改源码、不派生子代理。先读取持久邮箱 review 和 collab status 中的 work/attempts。审查已确认任务的范围、实际代码、验证证据，并独立复核约定检查。review ID: {}。将邮箱中的 candidate 原样保存到 VELELLA_DATA_DIR 下的文件，使用 review-report --review <id> --verdict passed|failed --candidate <文件> --summary <实际检查与缺口> 提交。无实际证据不能声称通过。检查失败必须附稳定 cause_key（字母、数字、.-_）和可复现根因。提交后停止。",review["id"])
}
fn running(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    let status=h.orchestration_status(doc)?;let works=model::work_by_key(&status);
    let nodes:Vec<Node>=serde_json::from_value(doc["nodes"].clone())?;
    // Reconcile durable acceptance before any new dispatch (including a crash between
    // collab.accept and updating the managed record / retiring a one-shot member).
    for w in works.values().filter(|w|w["state"]=="accepted"){
        retire(service,h,doc,text(w,"assignee")?)?;
        for r in status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==w["id"] && r["attempt_id"]==w["attempt_id"] && r["state"]=="passed") {retire(service,h,doc,text(r,"reviewer")?)?;}
        if w["work_key"]=="__final" {doc["final_candidate"]=w["candidate"].clone();doc["state"]=json!("awaiting_acceptance");model::event(doc,"ready","整体验证与独立审查完成，等待用户验收");return h.orchestration_put(doc)}
    }
    // Completion and failures take priority over dispatch, preserving the fixed review candidate.
    for n in &nodes {
        if let Some(w)=works.get(&n.id){
            if w["state"]=="reported" {return finish_work(service,h,doc,w,&status)}
            if w["state"]=="failed" {
                for r in status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==w["id"] && r["state"]=="failed") {retire(service,h,doc,text(r,"reviewer")?)?;}
                retire(service,h,doc,text(w,"assignee")?)?;
                anyhow::ensure!(w["attempt_count"]==1,"任务“{}”两次尝试均失败；自动推进已停止，需要决定后续方案",n.title);
                if doc["retry_evidence"][&n.id].as_str().is_some_and(|s|!s.trim().is_empty()){return dispatch_node(service,h,doc,n,Some(w))}
                let lead=lead_member(h,doc)?;
                notify(service,h,doc,&format!("diagnose.{}",n.id),&lead,format!("任务 {} 首次失败。只读定位根因，结合 collab status 的 work/attempt/review 报告，说明改变了什么前置条件或方案；有新证据才执行 retry-propose --work {} --new-evidence <根因、新证据及方案调整>。此回合不得修改源码。没有新证据则停止并说明唯一决策项。",n.title,w["id"]))?;
                anyhow::bail!("任务“{}”首次失败，等待根因与重试依据",n.title)
            }
        }
    }
    let active_nodes:Vec<&Node>=nodes.iter().filter(|n|works.get(&n.id).is_some_and(active)).collect();
    for n in &active_nodes {
        let w=&works[&n.id];
        let m=status["members"].as_array().unwrap().iter().find(|m|m["id"]==w["assignee"]).context("assignee missing")?;
        let s=session_status(service,doc,text(&m["identity"],"session_id")?)?;
        anyhow::ensure!(!s.permission_input && s.state!="waiting_permission","任务“{}”等待原生授权，请打开会话处理",n.title);
        anyhow::ensure!(!s.human_control,"任务“{}”已由用户接管",n.title);
        // Repair only the dispatch continuation, never repeat the native input effect.
        if w["state"]=="assigned" {
            if doc["bindings"][&n.id]["work_id"].is_null(){doc["bindings"][&n.id]["work_id"]=w["id"].clone();h.orchestration_put(doc)?;}
            let key=format!("node.{}.{}.notify",n.id,w["attempt_count"]);
            notify(service,h,doc,&key,m,assignment_prompt(n))?;
        }
    }
    let parallelism=doc["plan"]["parallelism"].as_u64().unwrap_or(1) as usize;
    for node in &nodes {
        if works.contains_key(&node.id) || !node.depends_on.iter().all(|id|works.get(id).is_some_and(|w|w["state"]=="accepted")){continue}
        if !active_nodes.is_empty() && (node.role=="lead" || active_nodes.iter().any(|n|n.role=="lead") || active_nodes.len()>=parallelism){continue}
        return dispatch_node(service,h,doc,node,None)
    }
    Ok(())
}
fn stopping(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    // Even a launch interrupted before membership must be stopped: effects own its session.
    let mut sessions=std::collections::HashSet::new();
    for e in doc["effects"].as_object().into_iter().flat_map(|e|e.values()) {
        if let Some(id)=e["result"]["session_id"].as_str(){sessions.insert(id.to_owned());}
        // Launch IDs are native session IDs. Recover ownership if the host was
        // persisted/launched before the effect's result could be saved.
        if matches!(e["request"]["cmd"].as_str(),Some("collab.start"|"collab.launch")) {
            if let Some(id)=e["request"]["request_id"].as_str(){
                let s=service.lock().unwrap();
                if s.workbench.session(text(doc,"workspace_id")?,id)?.is_some_and(|s|s.descriptor.spawn.env.contains_key("VELELLA_COLLABORATION_SKILL")){sessions.insert(id.to_owned());}
            }
        }
    }
    let mut failures=Vec::new();
    for session in sessions {
        let key=format!("stop.{session}");
        let already_dead={let s=service.lock().unwrap();s.workbench.session(text(doc,"workspace_id")?,&session)?.is_some_and(|s|host::confirmed_dead(&s.descriptor.directory))};
        if already_dead {
            if !doc["effects"][&key].is_object(){doc["effects"][&key]=json!({});}
            doc["effects"][&key]["result"]=json!({"ok":true,"confirmed_dead":true});h.orchestration_put(doc)?;
        } else if let Err(error)=effect!(service,h,doc,&key,json!({"cmd":"workbench.terminal.stop","workspace_id":doc["workspace_id"],"session_id":session,"confirmed":true})) {failures.push(format!("{session}: {error}"));}
    }
    anyhow::ensure!(failures.is_empty(),"停止未完成，已继续处理其余会话。解决原因后点继续核对：{}",failures.join("；"));
    if doc["credentials"].is_object(){
        let status=h.orchestration_status(doc)?;
        for w in status["work"].as_array().into_iter().flatten().filter(|w|!matches!(w["state"].as_str(),Some("accepted"|"cancelled"))){
            effect!(service,h,doc,&format!("cancel.{}",w["id"]),with_cred(&doc["credentials"],json!({"cmd":"collab.cancel","work_id":w["id"],"reason":"用户停止协作"})))?;
        }
        effect!(service,h,doc,"stop.release",with_cred(&doc["credentials"],json!({"cmd":"collab.release"})))?;
        effect!(service,h,doc,"stop.end",with_cred(&doc["credentials"],json!({"cmd":"collab.end","outcome":"cancelled","summary":"用户停止"})))?;
    }
    doc["state"]=json!("stopped");doc["blocker"]=Value::Null;model::event(doc,"stopped","本次协作已停止，代码与历史保留");h.orchestration_put(doc)
}
pub(super) fn tick(service:&Arc<Mutex<Service>>)->Result<()> {
    let h=hub(service)?;
    for entry in h.orchestration_records()? {
        if !matches!(entry["state"].as_str(),Some("planning"|"running"|"stopping")) && !(entry["schema_version"]==2 && entry["state"]=="paused"){continue}
        let g=gate(service,text(&entry,"id")?);let Ok(_lock)=g.try_lock() else{continue};
        let mut doc=h.orchestration_get(text(&entry,"id")?)?;
        let result=if doc["schema_version"]==2 {v2::tick(service,&h,&mut doc)}else{match doc["state"].as_str(){Some("planning")=>planning(service,&h,&mut doc),Some("running")=>running(service,&h,&mut doc),Some("stopping")=>stopping(service,&h,&mut doc),_=>continue}};
        match result {
            Ok(())=>{if !doc["blocker"].is_null(){doc["blocker"]=Value::Null;h.orchestration_put(&doc)?;}}
            Err(error)=>{let reason=error.to_string();if doc["blocker"]!=reason{doc["blocker"]=json!(reason);model::event(&mut doc,"blocked",&reason);h.orchestration_put(&doc)?;}}
        }
    }
    Ok(())
}
fn creation_preflight(service:&Arc<Mutex<Service>>,workspace:&str)->Result<Value> {
    let (data,w)={let s=service.lock().unwrap();(s.root.clone(),s.workbench.workspace(workspace)?)};
    let root=dunce::canonicalize(&w.path).context("项目目录不可访问，请重新选择工作区")?;
    let data=dunce::canonicalize(data)?;
    let mut issues=Vec::new();
    if w.archived {issues.push(json!({"code":"archived","message":"此工作区已归档，请先恢复工作区或选择其他项目。"}));}
    if workspaces::git_output(&root,&["rev-parse","--verify","HEAD"]).is_err(){issues.push(json!({"code":"git_baseline","message":"此项目还没有 Git 提交。请先初始化仓库并完成首次提交，再创建协作。"}));}
    if data.starts_with(&root)&&!data.starts_with(root.join(".velella")){issues.push(json!({"code":"data_overlap","message":"运行数据目录位于项目源码内。请将候选实例的数据目录移到项目之外后再启动。"}));}
    Ok(json!({"ok":true,"ready":issues.is_empty(),"workspace_id":workspace,"path":root,"issues":issues}))
}
pub(super) fn dispatch(service:&Arc<Mutex<Service>>,req:Value)->Result<Value> {
    let h=hub(service)?;let cmd=text(&req,"cmd")?;
    if cmd=="orchestration.progress" {return progress::read(service,&h,&req)}
    if cmd=="orchestration.preflight" {return creation_preflight(service,text(&req,"workspace_id")?)}
    if cmd=="orchestration.capabilities"{
        let (data,project)={let s=service.lock().unwrap();let p=req["workspace_id"].as_str().map(|id|s.workbench.workspace(id).map(|w|w.path)).transpose()?;(s.root.clone(),p)};
        return crate::collaboration::providers::scoped_capabilities(&data,project.as_deref())
    }
    if cmd=="orchestration.mcp.preview" {return Ok(json!({"ok":true,"catalog":crate::collaboration::mcp_catalog::preview(Path::new(text(&req,"path")?))?}))}
    if cmd=="orchestration.mcp.confirm" {return h.confirm_mcp_source(&req)}
    if cmd=="orchestration.templates.list"{return Ok(json!({"ok":true,"templates":h.orchestration_templates()?}))}
    if cmd=="orchestration.templates.save"{return h.orchestration_mutate(&req)}
    if cmd=="orchestration.list" {
        let runs=h.orchestration_records()?.iter().map(|d|Ok(json!({"id":d["id"],"schema_version":d["schema_version"],"goal":d["goal"],"workspace_id":d["workspace_id"],"state":d["state"],"blocker":d["blocker"],"updated_at":d["updated_at"]}))).collect::<Result<Vec<Value>>>()?;
        return Ok(json!({"ok":true,"runs":runs}))
    }
    if cmd=="orchestration.snapshot"{
        let doc=h.orchestration_get(text(&req,"id")?)?;
        let mut public=h.orchestration_public(&doc)?;
        if doc["schema_version"]==2 {public["context_usage"]=context_usage_snapshot(service,&doc)?;}
        return Ok(json!({"ok":true,"run":public}))
    }
    let id=if cmd=="orchestration.create"{text(&req,"request_id")?}else{text(&req,"id")?};
    let g=gate(service,id);let _lock=g.lock().unwrap();
    if cmd=="orchestration.create" {
        uuid::Uuid::parse_str(id)?;
        if req["schema_version"]==2 {let check=creation_preflight(service,text(&req,"workspace_id")?)?;anyhow::ensure!(check["ready"]==true,"{}",check["issues"].as_array().unwrap().iter().filter_map(|i|i["message"].as_str()).collect::<Vec<_>>().join("；"));}
        let s=service.lock().unwrap();let w=s.workbench.workspace(text(&req,"workspace_id")?)?;anyhow::ensure!(!w.archived,"工作区已归档");
        if req["schema_version"]==2 {anyhow::ensure!(crate::collaboration::source::candidate(&w.path)?["git_head"].is_string(),"v2 编排需要已有提交的 Git 项目");}
        anyhow::ensure!(!h.orchestration_records()?.iter().any(|d|d["id"]!=id && d["workspace_id"]==req["workspace_id"] && !matches!(d["state"].as_str(),Some("accepted"|"stopped"))),"该工作区已有未结束的协作，请先完成或停止它");
    }
    if cmd=="orchestration.reopen" {
        uuid::Uuid::parse_str(text(&req,"request_id")?)?;
        let predecessor=h.orchestration_get(id)?;anyhow::ensure!(predecessor["schema_version"]==2&&predecessor["state"]=="accepted","只能从已验收的 v2 候选发起后续协作");
        anyhow::ensure!(predecessor["user_acceptance"]["candidate"]==predecessor["final_candidate"],"前任验收记录与候选不一致");
        anyhow::ensure!(matches!(req["reason_kind"].as_str(),Some("reproducible_defect"|"changed_requirement")),"受控重开需要可复现缺陷或明确需求变更");
        let evidence=text(&req,"evidence")?;anyhow::ensure!(!evidence.trim().is_empty()&&evidence.len()<=8192,"重开依据不能为空且不超过 8192 个字符");
        anyhow::ensure!(!text(&req,"goal")?.trim().is_empty()&&!text(&req,"acceptance")?.trim().is_empty(),"继任协作需要新的目标与验收条件");
        anyhow::ensure!(req["agent"]==predecessor["agent"],"继任协作必须先沿用前任主控 CLI");
        let workspace=text(&predecessor,"integration_workspace")?;let path={service.lock().unwrap().workbench.workspace(workspace)?.path};
        anyhow::ensure!(crate::collaboration::source::candidate(&path)?==predecessor["final_candidate"],"已验收集成候选已变化，不能从漂移版本重开");
    }
    if cmd=="orchestration.accept" {
        let mut doc=h.orchestration_get(id)?;
        if doc["schema_version"]==2{return v2::accept(service,&h,&mut doc,&req)}
        if doc["state"]=="accepted" {return Ok(json!({"ok":true,"id":id}))}
        anyhow::ensure!(doc["state"]=="awaiting_acceptance","尚未完成整体独立审查");
        let path={service.lock().unwrap().workbench.workspace(text(&doc,"workspace_id")?)?.path};
        let candidate=crate::collaboration::source::candidate(Path::new(&path))?;
        anyhow::ensure!(candidate==doc["final_candidate"],"最终候选已变化，不能验收旧版本");
        effect!(service,&h,&mut doc,"accept.release",with_cred(&h.orchestration_get(id)?["credentials"],json!({"cmd":"collab.release"})))?;
        let credentials=doc["credentials"].clone();
        effect!(service,&h,&mut doc,"accept.end",with_cred(&credentials,json!({"cmd":"collab.end","outcome":"accepted","summary":text(&req,"summary")?})))?;
        doc["state"]=json!("accepted");model::event(&mut doc,"accepted",text(&req,"summary")?);h.orchestration_put(&doc)?;
        return Ok(json!({"ok":true,"id":id}))
    }
    h.orchestration_mutate(&req)
}

#[cfg(test)]
#[path="orchestration_tests.rs"]
mod tests;
