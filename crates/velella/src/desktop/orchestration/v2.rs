//! Role-based scheduler over the existing durable collaboration service.
use super::*;
use model::v2::{Plan,RoleDefinition,Node as Step};
fn role<'a>(p:&'a Plan,id:&str)->Result<&'a RoleDefinition>{p.roles.iter().find(|r|r.id==id).context("角色不存在")}
fn credentials(doc:&Value)->Value{doc["credentials"].clone()}
fn retry_evidence(doc:&Value,node_id:&str)->Value{let value=&doc["retry_evidence"][node_id];if value.is_object(){value["evidence"].clone()}else{value.clone()}}
fn member_at(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,key:&str,name:&str,workspace:&str,role:&RoleDefinition)->Result<Value>{
    if let Some(remote)=&role.remote_codex {service.lock().unwrap().capabilities.bound_hybrid_profile(&remote.ssh_profile_id,&remote.ssh_profile_fingerprint)?;crate::collaboration::providers::validate(role)?;}
    let cred=credentials(doc);
    let opened=effect!(service,h,doc,&format!("{key}.open"),with_cred(&cred,json!({"cmd":"collab.launch","workspace_id":workspace,"launch":role.provider,"role_config":role})))?;
    doc["launch_preflight"][key]=opened["preflight"].clone();h.orchestration_put(doc)?;
    let invited=effect!(service,h,doc,&format!("{key}.invite"),with_cred(&cred,json!({"cmd":"collab.invite","session_id":opened["session_id"],"name":name})))?;
    let joined=effect!(service,h,doc,&format!("{key}.join"),json!({"cmd":"collab.join","session_id":opened["session_id"],"run_id":doc["collab_id"],"member_id":invited["member"]["id"],"invitation":invited["invitation"],"confirmed":true}))?;
    bind(service,&joined)?;doc["member_credentials"][text(&joined["member"],"id")?]=joined["credentials"].clone();h.orchestration_put(doc)?;Ok(joined["member"].clone())
}
fn close_member(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,id:&str,status:&Value)->Result<()> {
    let member=status["members"].as_array().context("members missing")?.iter().find(|m|m["id"]==id).context("member missing")?;
    if id==doc["credentials"]["member_id"]{return Ok(())}
    let cred=doc["member_credentials"][id].clone();
    effect!(service,h,doc,&format!("release.{id}"),with_cred(&cred,json!({"cmd":"collab.release"})))?;
    retire(service,h,doc,id)?;
    let sid=text(&member["identity"],"session_id")?;let wid=text(&member["identity"],"workspace_id")?;
    let dead={let s=service.lock().unwrap();s.workbench.session(wid,sid)?.is_some_and(|s|host::confirmed_dead(&s.descriptor.directory))};
    if !dead {effect!(service,h,doc,&format!("retire.stop.{id}"),json!({"cmd":"workbench.terminal.stop","workspace_id":wid,"session_id":sid,"confirmed":true}))?;}
    let descriptor={let s=service.lock().unwrap();s.workbench.session(wid,sid)?.context("member session missing")?.descriptor};
    if descriptor.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker") {
        anyhow::ensure!(host::confirmed_dead(&descriptor.directory),"cannot prove local remote-worker session stopped");
        crate::hybrid::ensure_worker_shutdown_known(&descriptor)?;
    }
    Ok(())
}
fn handoff_package(doc:&Value,status:&Value,member:&Value)->Value {
    let member_id=&member["id"];
    let active:Vec<Value>=status["work"].as_array().into_iter().flatten().filter(|work|work["assignee"]==*member_id&&matches!(work["state"].as_str(),Some("assigned"|"input_accepted"|"turn_started"))).map(|work|json!({"work_id":work["id"],"node_id":work["work_key"],"attempt_id":work["attempt_id"],"attempt_count":work["attempt_count"],"state":work["state"]})).collect();
    let reviews:Vec<Value>=status["reviews"].as_array().into_iter().flatten().filter(|review|review["reviewer"]==*member_id&&matches!(review["state"].as_str(),Some("requested"|"reported"))).map(|review|json!({"review_id":review["id"],"work_id":review["work_id"],"attempt_id":review["attempt_id"],"state":review["state"]})).collect();
    let remaining:Vec<Value>=doc["nodes"].as_array().into_iter().flatten().filter(|node|!doc["bindings"][node["id"].as_str().unwrap_or("")]["recorded"].as_bool().unwrap_or(false)).map(|node|json!({"node_id":node["id"],"gap":node["gap"],"depends_on":node["depends_on"]})).collect();
    json!({"version":1,"created_at":chrono::Utc::now().to_rfc3339(),"baseline":{"candidate":doc["baseline_candidate"],"snapshot":doc["baseline_snapshot"]},"remaining":remaining,"active":active,"reviews":reviews,"evidence":if doc["evidence"].is_array(){doc["evidence"].clone()}else{json!([])},"failure_ledger":if doc["failure_ledger"].is_array(){doc["failure_ledger"].clone()}else{json!([])},"stages":doc["stages"]})
}
fn handoff_role<'a>(p:&'a Plan,doc:&Value,status:&Value,member:&Value)->Result<&'a RoleDefinition>{
    if member["id"]==doc["credentials"]["member_id"] {return p.roles.iter().find(|role|role.kind=="coordinate").context("主控角色缺失")}
    if let Some(work)=status["work"].as_array().into_iter().flatten().find(|work|work["assignee"]==member["id"]&&matches!(work["state"].as_str(),Some("assigned"|"input_accepted"|"turn_started"))){let node=doc["nodes"].as_array().into_iter().flatten().find(|node|node["id"]==work["work_key"]).context("交接步骤缺失")?;return role(p,node["role_id"].as_str().context("交接步骤角色缺失")?)}
    if status["reviews"].as_array().into_iter().flatten().any(|review|review["reviewer"]==member["id"]&&matches!(review["state"].as_str(),Some("requested"|"reported"))){return role(p,&p.review_role)}
    anyhow::bail!("only an active assignee, reviewer, or coordinator can be handed off")
}
fn handoff_member(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,p:&Plan,status:&Value,member:&Value)->Result<()> {
    let id=text(member,"id")?;let record=&doc["handoffs"][id];
    if record["state"]=="completed" {return Ok(())}
    if !record.is_object() {doc["handoffs"][id]=json!({"state":"prepared","from_session":member["identity"]["session_id"],"package":handoff_package(doc,status,member)});h.orchestration_put(doc)?;}
    // A running native turn is allowed to finish, but it receives no further
    // automated input after the latch.  Identity moves only at a safe idle point.
    let old_session=text(&member["identity"],"session_id")?;let old=session_status(service,doc,old_session)?;
    if old.state!="waiting_input"||old.human_control||old.permission_input||!old.hooks_ready {doc["handoffs"][id]["state"]=json!("awaiting_safe_point");h.orchestration_put(doc)?;return Ok(())}
    let role=handoff_role(p,doc,status,member)?;let workspace=text(&member["identity"],"workspace_id")?;
    if let Some(remote)=&role.remote_codex {service.lock().unwrap().capabilities.bound_hybrid_profile(&remote.ssh_profile_id,&remote.ssh_profile_fingerprint)?;crate::collaboration::providers::validate(role)?;}
    let key=format!("handoff.{id}");let opened=effect!(service,h,doc,&format!("{key}.open"),with_cred(&doc["credentials"],json!({"cmd":"collab.launch","workspace_id":workspace,"launch":role.provider,"role_config":role})))?;
    let replacement=text(&opened,"session_id")?;ready(service,doc,replacement)?;
    let package=doc["handoffs"][id]["package"].clone();let moved=effect!(service,h,doc,&format!("{key}.transfer"),with_cred(&doc["credentials"],json!({"cmd":"collab.handoff","target_member_id":id,"session_id":replacement,"package":package})))?;
    bind(service,&moved)?;doc["member_credentials"][id]=moved["credentials"].clone();
    for binding in doc["bindings"].as_object_mut().into_iter().flat_map(|bindings|bindings.values_mut()){if binding["session_id"]==old_session {binding["session_id"]=json!(replacement);}}
    if doc["lead_session"]==old_session {doc["lead_session"]=json!(replacement);}
    doc["handoffs"][id]["state"]=json!("completed");doc["handoffs"][id]["to_session"]=json!(replacement);doc["handoffs"][id]["completed_at"]=json!(chrono::Utc::now().to_rfc3339());h.orchestration_put(doc)?;
    let notice=format!("你正在接任同一协作成员。先读取持久邮箱和交接包，再继续现有 work/review；不得创建重试、重置 attempt 或重发旧输入。交接包：{}",serde_json::to_string(&package)?);
    notify(service,h,doc,&format!("{key}.notify"),&moved["member"],notice)
}
fn plan(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    let p=model::v2::validate(&doc["plan"])?;
    for r in &p.roles{
        if let Some(remote)=&r.remote_codex {service.lock().unwrap().capabilities.bound_hybrid_profile(&remote.ssh_profile_id,&remote.ssh_profile_fingerprint)?;}
        crate::collaboration::providers::validate(r)?;
    }
    workspaces::baseline(service,h,doc)?;
    if !doc["credentials"].is_object(){
        let lead=p.roles.iter().find(|r|r.kind=="coordinate").context("方案需要主控角色")?;
        let node=Step{id:"coordinator".into(),title:"主控".into(),instruction:String::new(),role_id:lead.id.clone(),depends_on:vec![],scope:String::new(),gap:String::new(),checks:vec![],deliverables:vec![],human_gate:false};
        let w=workspaces::node_workspace(service,h,doc,&node)?;
        let opened=effect!(service,h,doc,"lead.open",json!({"cmd":"collab.start","workspace_id":w.id,"launch":lead.provider,"role_config":lead}))?;
        doc["lead_session"]=opened["session_id"].clone();doc["lead_preflight"]=opened["preflight"].clone();h.orchestration_put(doc)?;
        let created=effect!(service,h,doc,"lead.create",json!({"cmd":"collab.create","session_id":doc["lead_session"],"goal":doc["goal"],"name":"主控"}))?;
        bind(service,&created)?;doc["credentials"]=created["credentials"].clone();doc["collab_id"]=created["run"]["id"].clone();h.orchestration_put(doc)?;
    }
    let lead=lead_member(h,doc)?;
    let prompt=format!("只读分析以下目标，生成可实施的 v2 协作方案，不修改源码或派生 Agent。目标：{}\n总体验收：{}\n方案结构及选定角色：{}\n可以调整步骤和已有角色职责，但不能擅自换账户、模型、工具权限。每个步骤必须对应目标缺口，给出依赖、明确范围、交付物和可核验检查。系统增加集成与独立整体验证，不重复添加。通过 collaboration MCP 的 collab.plan.submit 提交 plan、revision={} 和 request_id(UUID)，或通过 collab plan-submit。提交后停止，等待用户确认。",doc["goal"],doc["acceptance"],doc["plan"],doc["revision"]);
    let key=format!("plan.prompt.{}",doc["refinement"]["id"].as_str().unwrap_or("initial"));
    let prompt=format!("{prompt}\n用户补充要求：{}",doc["refinement"]["instruction"].as_str().unwrap_or("无"));
    notify(service,h,doc,&key,&lead,prompt)
}
fn prompt(node:&Step,role:&RoleDefinition,doc:&Value,input_head:&str)->String {
    let dependencies:Vec<Value>=node.depends_on.iter().map(|id|json!({"node":id,"binding":doc["bindings"][id]})).collect();
    format!("执行已确认步骤：{}\n{}\n职责：{}\n原始目标：{}\n关闭缺口：{}\n范围：{}\n交付物：{}\n检查：{}\n输入提交：{}\n依赖证据：{}\n先读取持久邮箱和状态，用 collab.started 确认当前 attempt。不得派生 Agent、扩大范围、修改原工作区或全局配置。{}\n完成后以当前 attempt 提交 collab.report，包括实际检查命令、结果、交付物路径、未完成项；失败时必须给出稳定 cause_key（字母、数字、.-_）和根因说明。写入任务先取得 collab.candidate 并原样提交 candidate。报告只表示待审查，不能自行验收。提交后立即停止。技能指引见 VELELLA_ROLE_FILE。",node.title,node.instruction,role.instruction,doc["goal"],node.gap,node.scope,node.deliverables.join("；"),node.checks.join("；"),input_head,serde_json::to_string(&dependencies).unwrap_or_default(),if role.writes(){"只在分配工作树修改。完成约定验证后，把本步骤的源码改动提交到当前分支，再取得候选；不要提交 .velella 或测试产物。集成步骤须按依赖顺序合入每个已审查增量，处理冲突并核验所有目标。"}else{"只读源码；检查产物存到忽略目录或 VELELLA_DATA_DIR，不修改源码。"})
}
fn dispatch_node(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,p:&Plan,node:&Step,previous:Option<&Value>)->Result<()> {
    let r=role(p,&node.role_id)?;let attempt=previous.map_or(1,|w|w["attempt_count"].as_u64().unwrap_or(1)+1);
    let key=format!("v2.node.{}.{}",node.id,attempt);
    let w=workspaces::node_workspace(service,h,doc,node)?;
    let member=member_at(service,h,doc,&key,&node.title,&w.id,r)?;
    ready(service,doc,text(&member["identity"],"session_id")?)?;
    let input=workspaces::git_output(&w.path,&["rev-parse","HEAD"])?;
    let input_head=doc["bindings"][&node.id]["input_head"].as_str().unwrap_or(input.trim()).to_owned();
    if doc["bindings"][&node.id]["attempt"].as_u64()!=Some(attempt){doc["bindings"][&node.id]=json!({"attempt":attempt,"workspace_id":w.id,"session_id":member["identity"]["session_id"],"input_head":input_head,"role":r,"preflight":doc["launch_preflight"][&key],"read_candidate":crate::collaboration::source::candidate(&w.path)?});h.orchestration_put(doc)?;}
    let cred=credentials(doc);
    if r.writes(){effect!(service,h,doc,&format!("{key}.claim"),with_cred(&cred,json!({"cmd":"collab.claim","writer":member["id"]})))?;}
    let mut task_prompt=prompt(node,r,doc,&input_head);
    if r.remote_codex.is_some() {
        task_prompt=task_prompt.replace("技能指引见 VELELLA_ROLE_FILE。","通过会话绑定的 collaboration MCP 工具调用 collab.receive、collab.status、collab.started、collab.candidate 和 collab.report；修改请求使用稳定的 UUID request_id。本机文件与命令通过本机工具访问，写入和命令须逐项审批。不得假定远端存在本机路径或环境变量。");
        task_prompt=task_prompt.replace("只读源码；检查产物存到忽略目录或 VELELLA_DATA_DIR，不修改源码。","只读源码，不修改源码；通过 local_write_evidence 保存实际核验的证据并取得 path/sha256，仍须本机审批；不得用源码写入或命令工具绕过只读职责。");
    }
    let mut instruction=format!("{}\n所有角色报告前均须取得 collab.candidate 并原样提交。\n{}",task_prompt,model::evidence::instructions());
    let workspace_key=if node.id=="__integrate"{"integration"}else{node.id.as_str()};
    if r.kind=="integrate" && doc["workspace_bindings"][workspace_key]["pending_imports"].is_array() {
        instruction.push_str(&format!("\n此集成角色接管了依赖合入冲突；以下是从当前冲突开始尚待完成的有序提交：{}。若非空，先处理 CHERRY_PICK_HEAD 对应冲突并继续，再依次 cherry-pick -x 剩余提交。每个提交消息必须保留 (cherry picked from commit <原始SHA>) 回执。不得跳过依赖；完成后核对所有来源提交。",doc["workspace_bindings"][workspace_key]["pending_imports"]));
    }
    let assigned=effect!(service,h,doc,&format!("{key}.assign"),with_cred(&cred,json!({"cmd":"collab.assign","assignee":member["id"],"workspace_path":member["identity"]["workspace_path"],"work_key":node.id,"work_id":previous.map(|w|w["id"].clone()),"title":node.title,"instruction":instruction,"write":r.writes(),"new_evidence":retry_evidence(doc,&node.id)})))?;
    doc["bindings"][&node.id]["work_id"]=assigned["work"]["id"].clone();h.orchestration_put(doc)?;
    notify(service,h,doc,&format!("{key}.notify"),&member,instruction)
}
fn settled(doc:&Value,node:&Step,works:&HashMap<String,Value>)->bool {
    works.get(&node.id).is_some_and(|w|w["state"]=="accepted") && (!node.human_gate || doc["gates"][&node.id]["approved"]==true)
}
fn system_gate(doc:&Value,id:&str)->bool {doc["nodes"].as_array().is_some_and(|nodes|nodes.iter().any(|node|node["id"]==id && node["human_gate"]==true))}
fn finish(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,p:&Plan,w:&Value,status:&Value)->Result<()> {
    let key=format!("v2.review.{}.{}",text(w,"id")?,w["attempt_count"]);let node_id=text(w,"work_key")?;
    let actual=crate::collaboration::source::candidate(Path::new(text(w,"workspace_path")?))?;
    if w["write"]==true {
        anyhow::ensure!(w["candidate"]==actual,"提交后源码变化，冻结候选并停止验收");
        let clean=workspaces::git_output(Path::new(text(w,"workspace_path")?),&["status","--porcelain","--untracked-files=normal","--",".",":(exclude).velella"])?;
        anyhow::ensure!(clean.trim().is_empty(),"写入任务必须先提交源码，再报告候选");
        if node_id=="__integrate" {workspaces::verify_integration(doc,Path::new(text(w,"workspace_path")?))?;}else if role(p,&doc["nodes"].as_array().and_then(|nodes|nodes.iter().find(|node|node["id"]==node_id)).and_then(|node|node["role_id"].as_str()).context("步骤角色缺失")?)?.kind=="integrate" {workspaces::verify_imports(doc,node_id,Path::new(text(w,"workspace_path")?))?;}
        let author=status["members"].as_array().unwrap().iter().find(|m|m["id"]==w["assignee"]).context("作者会话缺失")?;
        if author["state"]!="left" {ready(service,doc,text(&author["identity"],"session_id")?)?;close_member(service,h,doc,text(w,"assignee")?,status)?;}
        let reviews:Vec<&Value>=status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==w["id"] && r["attempt_id"]==w["attempt_id"]).collect();
        if let Some(passed)=reviews.iter().find(|r|r["state"]=="passed"){
            let cred=credentials(doc);effect!(service,h,doc,&format!("{key}.accept"),with_cred(&cred,json!({"cmd":"collab.accept","work_id":w["id"],"summary":"独立审查通过，候选保持冻结"})))?;
            close_member(service,h,doc,text(passed,"reviewer")?,status)?;
        }else{
            let workspace=text(&doc["bindings"][node_id],"workspace_id")?.to_owned();
            let reviewer=member_at(service,h,doc,&key,"独立审查",&workspace,role(p,&p.review_role)?)?;
            ready(service,doc,text(&reviewer["identity"],"session_id")?)?;
            let cred=credentials(doc);let requested=effect!(service,h,doc,&format!("{key}.request"),with_cred(&cred,json!({"cmd":"collab.review.request","work_id":w["id"],"reviewer":reviewer["id"]})))?;
            return notify(service,h,doc,&format!("{key}.notify"),&reviewer,format!("{}\n检查和交付合同：{}\n{}",review_prompt(&requested["review"]),w["evidence_contract"],model::evidence::instructions()));
        }
    } else {
        anyhow::ensure!(actual==doc["bindings"][node_id]["read_candidate"],"只读步骤改变了源码，停止验收");
        let author=status["members"].as_array().unwrap().iter().find(|m|m["id"]==w["assignee"]).context("作者会话缺失")?;
        ready(service,doc,text(&author["identity"],"session_id")?)?;
        let cred=credentials(doc);effect!(service,h,doc,&format!("{key}.accept"),with_cred(&cred,json!({"cmd":"collab.accept","work_id":w["id"],"summary":"逐项证据、文件指纹和源码未变核对通过"})))?;
        close_member(service,h,doc,text(w,"assignee")?,status)?;
    }
    reconcile_accepted(service,h,doc,w,status)
}
fn reconcile_accepted(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,w:&Value,status:&Value)->Result<()> {
    let id=text(w,"work_key")?;
    if doc["bindings"][id]["recorded"]==true {
        if id=="__verify" && (!system_gate(doc,id) || doc["gates"][id]["approved"]==true) {return finalize_verification(service,h,doc,w);}
        return Ok(())
    }
    if w["write"]==true {doc["bindings"][id]["commit"]=w["candidate"]["git_head"].clone();doc["bindings"][id]["candidate"]=w["candidate"].clone();}
    if id=="__integrate"{doc["stages"]["integration"]=json!("passed");}
    // Without a human gate, validate the frozen final candidate before recording
    // verification evidence. A failed candidate comparison must not look like a
    // completed verification in the durable ledger.
    if id=="__verify" && !system_gate(doc,id) {finalize_verification(service,h,doc,w)?;}
    if !doc["evidence"].as_array().context("evidence records missing")?.iter().any(|e|e["attempt_id"]==w["attempt_id"]){
        let report=status["attempts"].as_array().into_iter().flatten().find(|a|a["id"]==w["attempt_id"]).cloned().unwrap_or(Value::Null);
        doc["evidence"].as_array_mut().unwrap().push(json!({"node_id":id,"attempt_id":w["attempt_id"],"candidate":w["candidate"],"report":report,"at":chrono::Utc::now().to_rfc3339()}));
    }
    doc["last_progress_at"]=json!(chrono::Utc::now().to_rfc3339());doc["bindings"][id]["recorded"]=json!(true);h.orchestration_put(doc)?;
    if id=="__verify" && system_gate(doc,id) && doc["gates"][id]["approved"]==true {finalize_verification(service,h,doc,w)}else{Ok(())}
}
fn finalize_verification(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,w:&Value)->Result<()> {
    let path={service.lock().unwrap().workbench.workspace(text(doc,"integration_workspace")?)?.path};
    let final_candidate=crate::collaboration::source::candidate(&path)?;
    anyhow::ensure!(final_candidate==doc["bindings"]["__integrate"]["candidate"],"集成候选在验证期间变化");
    anyhow::ensure!(w["candidate"]["revision"]==final_candidate["revision"] && w["candidate"]["git_head"]==final_candidate["git_head"],"独立验证未绑定同一集成提交和源码");
    workspaces::verify_integration(doc,&path)?;
    doc["final_candidate"]=final_candidate;doc["state"]=json!("awaiting_acceptance");
    doc["stages"]["implementation"]=json!("passed");
    for stage in ["development_validation","independent_testing","technical_acceptance"]{doc["stages"][stage]=json!("evidence_verified");}
    model::event(doc,"ready","集成与独立整体验证通过，交付工作区待用户验收");h.orchestration_put(doc)
}
fn run(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    let p=model::v2::validate(&doc["plan"])?;let nodes:Vec<Step>=serde_json::from_value(doc["nodes"].clone())?;
    let status=h.orchestration_status(doc)?;let works=model::work_by_key(&status);
    for w in nodes.iter().filter_map(|n|works.get(&n.id)).filter(|w|w["state"]=="accepted") {
        close_member(service,h,doc,text(w,"assignee")?,&status)?;
        for r in status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==w["id"] && r["state"]=="passed"){close_member(service,h,doc,text(r,"reviewer")?,&status)?;}
        reconcile_accepted(service,h,doc,w,&status)?;
    }
    if doc["state"]=="awaiting_acceptance"{return Ok(())}
    for n in &nodes {if let Some(w)=works.get(&n.id){
        if w["state"]=="reported"{return finish(service,h,doc,&p,w,&status)}
        if w["state"]=="failed"{
            for r in status["reviews"].as_array().into_iter().flatten().filter(|r|r["work_id"]==w["id"] && r["state"]=="failed"){close_member(service,h,doc,text(r,"reviewer")?,&status)?;}
            close_member(service,h,doc,text(w,"assignee")?,&status)?;
            anyhow::ensure!(w["attempt_count"]==1,"步骤两次尝试均失败，停止派发；需要新证据或范围决定：{}",n.title);
            if (doc["retry_evidence"][&n.id].as_str().is_some_and(|s|!s.trim().is_empty()) || doc["retry_evidence"][&n.id].is_object()) && doc["state"]=="running"{return dispatch_node(service,h,doc,&p,n,Some(w))}
            anyhow::bail!("步骤首次失败，请提供稳定根因标识、根因证据和调整后重试：{}",n.title)
        }
        if matches!(w["state"].as_str(),Some("assigned"|"input_accepted"|"turn_started")){
            let member=status["members"].as_array().unwrap().iter().find(|m|m["id"]==w["assignee"]).context("member missing")?;
            let s=session_status(service,doc,text(&member["identity"],"session_id")?)?;
            // This member must hand off before more input, but independent
            // reports and other members must still reach their scheduler path.
            if crate::collaboration::context_usage::guard(s.context_usage.as_ref()).is_err(){if let Err(error)=handoff_member(service,h,doc,&p,&status,member){let id=text(member,"id")?;doc["handoffs"][id]["state"]=json!("blocked");doc["handoffs"][id]["blocker"]=json!(error.to_string());h.orchestration_put(doc)?;}continue}
            anyhow::ensure!(!s.human_control && !s.permission_input && s.state!="waiting_permission","步骤等待原生授权或用户交还控制权：{}",n.title);
            if w["state"]=="assigned"{let instruction=text(w,"instruction")?.to_owned();notify(service,h,doc,&format!("v2.node.{}.{}.notify",n.id,w["attempt_count"]),member,instruction)?;}
        }
    }}
    if doc["state"]=="paused"{return Ok(())}
    let last=chrono::DateTime::parse_from_rfc3339(text(doc,"last_progress_at")?)?;
    if chrono::Utc::now().signed_duration_since(last).num_hours()>=24 {doc["state"]=json!("paused");model::event(doc,"convergence","24 小时无目标缺口关闭，停止派发；请根据已有证据决定唯一下一步");h.orchestration_put(doc)?;anyhow::bail!("24 小时无交付进展，等待收敛决定")}
    if let Some(lead)=doc["lead_session"].as_str(){
        let lead_status=session_status(service,doc,lead)?;
        if crate::collaboration::context_usage::guard(lead_status.context_usage.as_ref()).is_err(){let member=lead_member(h,doc)?;handoff_member(service,h,doc,&p,&status,&member)?;return Ok(())}
    }
    let count=works.values().filter(|w|active(w)).count();if count>=p.parallelism{return Ok(())}
    for n in &nodes {
        if doc["proposal"]["affected"].as_array().is_some_and(|ids|ids.contains(&json!(n.id))){continue}
        if works.contains_key(&n.id){continue}
        if !n.depends_on.iter().all(|id|nodes.iter().find(|n|&n.id==id).is_some_and(|n|settled(doc,n,&works))){continue}
        return dispatch_node(service,h,doc,&p,n,None)
    }Ok(())
}
pub(super) fn tick(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    match doc["state"].as_str(){Some("planning")=>plan(service,h,doc),Some("running"|"paused")=>run(service,h,doc),Some("stopping")=>stop(service,h,doc),_=>Ok(())}
}
pub(super) fn owned_sessions(doc:&Value)->(Vec<(String,String)>,Vec<String>){
    let mut sessions=Vec::new();let mut errors=Vec::new();for (key,effect) in doc["effects"].as_object().into_iter().flat_map(|effects|effects.iter()){
        if key.ends_with(".open") && effect["request"].is_null(){errors.push(format!("launch {key} has no ownership request"));continue}
        if !matches!(effect["request"]["cmd"].as_str(),Some("collab.start"|"collab.launch")){continue}
        let Some(workspace)=effect["request"]["workspace_id"].as_str().filter(|s|!s.is_empty()) else{errors.push(format!("launch {key} has no workspace identity"));continue};
        let Some(session)=effect["result"]["session_id"].as_str().filter(|s|!s.is_empty()).or_else(||effect["request"]["request_id"].as_str().filter(|s|!s.is_empty())) else{errors.push(format!("launch {key} has no session identity"));continue};
        if effect["request"]["request_id"].as_str().is_some_and(|id|id!=session){errors.push(format!("launch {key} session identity differs from request"));continue}
        if !sessions.iter().any(|(known_workspace,known_session)|known_workspace==workspace&&known_session==session){sessions.push((workspace.to_owned(),session.to_owned()));}
    }(sessions,errors)
}
pub(super) fn shutdown_effect_key(workspace:&str,session:&str)->String{format!("shutdown.{}",json!([workspace,session]))}
pub(super) fn shutdown_owned_sessions(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    let (sessions,mut errors)=owned_sessions(doc);
    for (workspace,session) in sessions {
        let result=(||->Result<()> {
            let descriptor={let state=service.lock().unwrap();
                anyhow::ensure!(!state.terminal_inflight.contains(&format!("starting:{session}"))&&!state.terminal_inflight.contains(&format!("session:{session}")),"run-owned session has an in-flight launch or operation: {session}");
                state.workbench.session(&workspace,&session)?.context("run-owned session is no longer registered; cannot prove it stopped")?.descriptor};
            if !host::confirmed_dead(&descriptor.directory){effect!(service,h,doc,&shutdown_effect_key(&workspace,&session),json!({"cmd":"workbench.terminal.stop","workspace_id":workspace,"session_id":session,"confirmed":true}))?;}
            let state=service.lock().unwrap();
            anyhow::ensure!(!state.terminal_inflight.contains(&format!("starting:{session}"))&&!state.terminal_inflight.contains(&format!("session:{session}"))&&host::confirmed_dead(&descriptor.directory),"cannot prove run-owned session stopped: {session}");
            crate::hybrid::ensure_worker_shutdown_known(&descriptor)?;
            doc["session_shutdown"][&workspace][&session]=json!({"status":"confirmed_dead","verified_at":chrono::Utc::now().to_rfc3339()});h.orchestration_put(doc)?;
            Ok(())
        })();
        if let Err(error)=result {
            if let Some(receipts)=doc.get_mut("session_shutdown").and_then(|v|v.get_mut(&workspace)).and_then(Value::as_object_mut){receipts.remove(&session);}
            errors.push(format!("{workspace}/{session}: {error:#}"));
        }
    }
    doc["shutdown_blockers"]=json!(errors);h.orchestration_put(doc)?;
    anyhow::ensure!(errors.is_empty(),"run-owned session shutdown incomplete: {}",errors.join("; "));
    Ok(())
}
fn stop(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    let status=h.orchestration_status(doc)?;let cred=credentials(doc);
    shutdown_owned_sessions(service,h,doc)?;
    for w in status["work"].as_array().into_iter().flatten().filter(|w|!matches!(w["state"].as_str(),Some("accepted"|"cancelled"))){effect!(service,h,doc,&format!("cancel.{}",w["id"]),with_cred(&cred,json!({"cmd":"collab.cancel","work_id":w["id"],"reason":"用户停止"})))?;}
    for m in status["members"].as_array().into_iter().flatten().filter(|m|m["id"]!=cred["member_id"]){close_member(service,h,doc,text(m,"id")?,&status)?;}
    if cred.is_object(){effect!(service,h,doc,"stop.end",with_cred(&cred,json!({"cmd":"collab.end","outcome":"cancelled","summary":"用户停止；成果和工作树保留"})))?;}
    doc["state"]=json!("stopped");h.orchestration_put(doc)
}
pub(super) fn accept(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,req:&Value)->Result<Value>{
    if doc["state"]=="accepted"{return Ok(json!({"ok":true,"id":doc["id"]}))}
    anyhow::ensure!(doc["state"]=="awaiting_acceptance","尚未完成独立整体验证");
    let path={service.lock().unwrap().workbench.workspace(text(doc,"integration_workspace")?)?.path};
    anyhow::ensure!(crate::collaboration::source::candidate(&path)?==doc["final_candidate"],"集成候选已变化，不能验收旧候选");
    shutdown_owned_sessions(service,h,doc)?;
    let cred=credentials(doc);effect!(service,h,doc,"accept.end",with_cred(&cred,json!({"cmd":"collab.end","outcome":"accepted","summary":text(req,"summary")?})))?;
    doc["state"]=json!("accepted");doc["user_acceptance"]=json!({"summary":req["summary"],"at":chrono::Utc::now().to_rfc3339(),"candidate":doc["final_candidate"]});h.orchestration_put(doc)?;
    Ok(json!({"ok":true,"id":doc["id"]}))
}
