//! Read-only, credential-free progress across providers in the same managed run.
use super::*;

pub(super) fn read(service:&Arc<Mutex<Service>>,h:&Hub,req:&Value)->Result<Value>{
    let workspace=text(req,"workspace_id")?;
    {let s=service.lock().unwrap();anyhow::ensure!(!s.workbench.workspace(workspace)?.archived,"workspace is archived");}
    let mut runs=Vec::new();
    for doc in h.orchestration_records()?{
        if related(&doc,workspace){runs.push(snapshot(&doc,&h.orchestration_status(&doc)?)?)}
    }
    Ok(json!({"ok":true,"workspace_id":workspace,"runs":runs,"observed_at":chrono::Utc::now().to_rfc3339()}))
}
fn related(doc:&Value,workspace:&str)->bool{
    doc["workspace_id"]==workspace||doc["integration_workspace"]==workspace||doc["bindings"].as_object().is_some_and(|bindings|bindings.values().any(|binding|binding["workspace_id"]==workspace))
}
fn settled(doc:&Value,node:&Value,works:&HashMap<String,&Value>)->bool{
    node["id"].as_str().and_then(|id|works.get(id)).is_some_and(|work|work["state"]=="accepted")&&
        (node["human_gate"]!=true||node["id"].as_str().is_some_and(|id|doc["gates"][id]["approved"]==true))
}
fn snapshot(doc:&Value,status:&Value)->Result<Value>{
    let confirmed=doc["nodes"].as_array().filter(|nodes|!nodes.is_empty());
    let nodes=confirmed.or_else(||doc["plan"]["nodes"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
    anyhow::ensure!(nodes.len()<=200,"swarm plan exceeds 200 nodes");
    let works=status["work"].as_array().into_iter().flatten().filter_map(|work|work["work_key"].as_str().map(|key|(key.to_owned(),work))).collect::<HashMap<_,_>>();
    let roles=doc["plan"]["roles"].as_array();
    let mut steps=Vec::new();
    for node in nodes{
        let id=text(node,"id")?;let work=works.get(id).copied();
        let deps=node["depends_on"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|dep|!nodes.iter().any(|other|other["id"]==*dep&&settled(doc,other,&works))).collect::<Vec<_>>();
        let state=if confirmed.is_none(){"planned"}
            else if settled(doc,node,&works){"completed"}
            else {match work.and_then(|work|work["state"].as_str()){
                Some("accepted")=>"waiting_gate",Some("reported")=>"awaiting_review",Some("failed")=>"failed",Some("cancelled")=>"cancelled",
                Some("turn_started")=>"in_progress",Some("assigned"|"input_accepted")=>"assigned",Some("queued")|None=>if deps.is_empty(){"pending"}else{"blocked"},_=>"unknown",
            }};
        let role=roles.into_iter().flatten().find(|role|role["id"]==node["role_id"]);
        let provider=role.and_then(|role|role["provider"].as_str()).unwrap_or_else(||if node["role"]=="scout"{doc["scout_agent"].as_str().unwrap_or("unknown")}else{doc["agent"].as_str().unwrap_or("unknown")});
        let binding=&doc["bindings"][id];
        steps.push(json!({"id":id,"title":node["title"],"status":state,"provider":provider,"blocked_by":deps,"session_id":binding["session_id"],"workspace_id":binding["workspace_id"]}));
    }
    let completed=steps.iter().filter(|step|step["status"]=="completed").count();
    Ok(json!({"id":doc["id"],"goal":doc["goal"],"state":doc["state"],"blocker":doc["blocker"],"updated_at":doc["updated_at"],"confirmed":confirmed.is_some(),"steps":steps,"completed":completed,"total":steps.len()}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn reporting_is_not_acceptance_and_gates_block_dependents(){
        let mut doc=json!({"id":"run","workspace_id":"root","state":"running","plan":{"roles":[{"id":"a","provider":"codex"},{"id":"b","provider":"omp"}]},"nodes":[{"id":"first","title":"implement","role_id":"a","human_gate":true,"depends_on":[]},{"id":"second","title":"verify","role_id":"b","depends_on":["first"]}],"bindings":{"first":{"workspace_id":"child","session_id":"s"}},"gates":{}});
        let mut status=json!({"work":[{"id":"w","work_key":"first","state":"reported"}]});let actual=snapshot(&doc,&status).unwrap();
        assert_eq!(actual["completed"],0);assert_eq!(actual["steps"][0]["status"],"awaiting_review");assert_eq!(actual["steps"][1]["status"],"blocked");assert_eq!(actual["steps"][1]["provider"],"omp");
        status["work"][0]["state"]=json!("accepted");assert_eq!(snapshot(&doc,&status).unwrap()["steps"][0]["status"],"waiting_gate");
        doc["gates"]["first"]=json!({"approved":true});let actual=snapshot(&doc,&status).unwrap();assert_eq!(actual["completed"],1);assert_eq!(actual["steps"][1]["status"],"pending");
        assert!(related(&doc,"root"));assert!(related(&doc,"child"));assert!(!related(&doc,"foreign"));assert_eq!(actual["state"],"running");
        doc["nodes"]=Value::Null;doc["plan"]["nodes"]=json!([{"id":"first","title":"draft","depends_on":[]}]);assert_eq!(snapshot(&doc,&status).unwrap()["completed"],0);
    }
}
