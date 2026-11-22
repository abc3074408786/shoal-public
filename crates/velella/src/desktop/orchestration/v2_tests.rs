use super::*;
use crate::desktop::orchestration::v2::{owned_sessions,shutdown_effect_key};
#[test]fn remote_role_preview_uses_existing_profile_and_detects_edits(){
    let f=fixture();let mut service=f.service.lock().unwrap();
    let saved=service.capability_command(json!({"cmd":"ssh.save","profile":{"name":"fixture","host":"agent"}})).unwrap();let id=saved["profile"]["id"].as_str().unwrap();
    let receipt=service.capability_command(json!({"cmd":"ssh.hybrid.preview","id":id})).unwrap();let fingerprint=receipt["ssh_profile_fingerprint"].as_str().unwrap();
    assert_eq!(service.capabilities.bound_hybrid_profile(id,fingerprint).unwrap(),receipt["profile"]);
    let mut updated=saved["profile"].clone();updated["host"]=json!("other");service.capability_command(json!({"cmd":"ssh.save","profile":updated})).unwrap();
    assert!(service.capabilities.bound_hybrid_profile(id,fingerprint).is_err());
}
#[test]fn remote_codex_role_cannot_start_a_local_terminal(){
    let f=fixture();let mut role=f.doc["plan"]["roles"][2].clone();
    role["remote_codex"]=json!({"ssh_profile_id":uuid::Uuid::new_v4().to_string(),"ssh_profile_fingerprint":format!("sha256:{}","a".repeat(64)),"ssh_target_fingerprint":format!("sha256:{}","e".repeat(64)),"remote_model":"deepseek-flash","provider_fingerprint":format!("sha256:{}","f".repeat(64))});
    let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":uuid::Uuid::now_v7().to_string(),"launch":"codex","collaboration":true,"role_config":role});
    let error=f.service.lock().unwrap().prepare_workbench_launch(&req).err().unwrap().to_string();
    assert!(error.contains("不能回退到本机"),"{error}");
    let mut remote=req.clone();remote["launch"]=json!("hybrid-codex");remote["ssh_profile_id"]=remote["role_config"]["remote_codex"]["ssh_profile_id"].clone();
    let error=f.service.lock().unwrap().prepare_workbench_launch(&remote).err().unwrap().to_string();assert!(error.contains("record not found")||error.contains("仅支持 Windows"),"{error}");
    assert_eq!(f.service.lock().unwrap().workbench.sessions(None).unwrap().len(),1);
}
#[test]fn orchestration_v2_handoff_preserves_attempt_and_rejects_the_old_native_session(){
    let mut f=fixture();let plan=f.doc["plan"].clone();confirm(&mut f,plan);seed(&mut f,"implement");f.tick();
    let before=f.status()["work"][0].clone();let assignee=text(&before,"assignee").unwrap().to_owned();let old_session=text(&f.doc["bindings"]["implement"],"session_id").unwrap().to_owned();
    let workspace=text(&f.doc["bindings"]["implement"],"workspace_id").unwrap().to_owned();let replacement=f.peer_in("handoff",&workspace);
    let response=collaboration_commands::managed(&f.service,with_cred(&f.doc["credentials"],json!({"cmd":"collab.handoff","request_id":uuid::Uuid::now_v7().to_string(),"target_member_id":assignee,"session_id":replacement,"package":{"baseline":f.doc["baseline_candidate"],"remaining":["implement"],"evidence":f.doc["evidence"],"failure_ledger":f.doc["failure_ledger"]}}))).unwrap();
    assert_eq!(response["member"]["identity"]["session_id"],replacement);assert_eq!(response["credentials"]["member_id"],before["assignee"]);
    let status=f.status();let after=status["work"][0].clone();assert_eq!(after["id"],before["id"]);assert_eq!(after["attempt_id"],before["attempt_id"]);assert_eq!(after["attempt_count"],1);
    let old=with_cred(&response["credentials"],json!({"cmd":"collab.started","request_id":uuid::Uuid::now_v7().to_string(),"attempt_id":before["attempt_id"],"native_session_id":old_session}));
    assert!(collaboration_commands::dispatch(&f.service,old).is_err(),"the prior native session must lose authority");
    let credentials=response["credentials"].clone();let started=f.call(&credentials,"collab.started",json!({"attempt_id":before["attempt_id"]}));assert_eq!(started["attempt"]["id"],before["attempt_id"]);
}
#[test]fn orchestration_v2_context_latch_persists_package_and_moves_an_idle_member(){
    let mut f=fixture();let plan=f.doc["plan"].clone();confirm(&mut f,plan);seed(&mut f,"implement");f.tick();
    let work=f.status()["work"][0].clone();let member=text(&work,"assignee").unwrap().to_owned();let old=text(&f.doc["bindings"]["implement"],"session_id").unwrap().to_owned();let workspace=text(&f.doc["bindings"]["implement"],"workspace_id").unwrap().to_owned();
    let receipt=crate::collaboration::context_usage::observe("omp",&old,"native",&json!({"tokens":40,"contextWindow":100}),None).unwrap();ipc::request(&f.peers[1].endpoint,&json!({"cmd":"fixture.usage","receipt":receipt})).unwrap();
    let replacement=f.peer_in("handoff",&workspace);f.doc["effects"][format!("handoff.{member}.open")]=json!({"request":{"cmd":"collab.launch","workspace_id":workspace,"request_id":replacement},"result":{"ok":true,"session_id":replacement}});f.h.orchestration_put(&f.doc).unwrap();
    f.tick();assert_eq!(f.doc["handoffs"][&member]["state"],"completed", "{}",f.doc["blocker"]);assert_eq!(f.doc["bindings"]["implement"]["session_id"],replacement);let after=f.status()["work"][0].clone();assert_eq!(after["id"],work["id"]);assert_eq!(after["attempt_id"],work["attempt_id"]);assert_eq!(after["attempt_count"],1);assert!(f.doc["handoffs"][&member]["package"]["baseline"].is_object());assert!(f.doc["handoffs"][&member]["package"]["failure_ledger"].is_array());let inputs=f.peers[2].inputs.load(std::sync::atomic::Ordering::SeqCst);f.tick();assert_eq!(f.peers[2].inputs.load(std::sync::atomic::Ordering::SeqCst),inputs,"recovery must not replay the handoff input");
}
#[test]fn native_context_blocked_member_does_not_block_other_report_acceptance(){
    let mut f=fixture();let mut plan=f.doc["plan"].clone();plan["parallelism"]=json!(2);plan["nodes"][0]["role_id"]=json!("explore");
    let mut other=plan["nodes"][0].clone();other["id"]=json!("other");other["title"]=json!("independent exploration");plan["nodes"].as_array_mut().unwrap().push(other);
    confirm(&mut f,plan);seed(&mut f,"implement");seed(&mut f,"other");f.tick();f.tick();
    let second=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="other").unwrap().clone();f.report(&second,"completed");
    let first=f.doc["bindings"]["implement"]["session_id"].as_str().unwrap();
    let receipt=crate::collaboration::context_usage::observe("omp",first,"native",&json!({"tokens":80,"contextWindow":100}),None).unwrap();
    ipc::request(&f.peers[1].endpoint,&json!({"cmd":"fixture.usage","receipt":receipt})).unwrap();
    f.tick();
    assert_eq!(f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="other").unwrap()["state"],"accepted","{}",f.doc["blocker"]);
}
#[test]fn orchestration_v2_shutdown_requires_owner_release_and_recovers_missing_launch_receipt(){
    let mut f=fixture();let sid=f.doc["lead_session"].as_str().unwrap().to_owned();
    f.doc["effects"]["lead.open"]=json!({"request":{"cmd":"collab.start","workspace_id":"w","request_id":sid}});
    assert_eq!(owned_sessions(&f.doc),(vec![("w".into(),sid.clone())],vec![]));
    let result=crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc);
    assert!(result.unwrap_err().to_string().contains("cannot prove"));
    assert!(f.doc["session_shutdown"]["w"][&sid].is_null());
    fs2::FileExt::unlock(&f.peers[0]._owner).unwrap();
    crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).unwrap();
    f.refresh();assert_eq!(f.doc["session_shutdown"]["w"][&sid]["status"],"confirmed_dead");
}
#[test]fn orchestration_v2_shutdown_does_not_treat_dead_ssh_as_remote_completion(){
    let mut f=fixture();let sid=f.doc["lead_session"].as_str().unwrap().to_owned();
    let directory={let state=f.service.lock().unwrap();let mut session=state.workbench.session("w",&sid).unwrap().unwrap();
        session.descriptor.spawn.adapter=Some("hybrid-orchestration-worker".into());
        state.workbench.put_session(&session).unwrap();session.descriptor.directory};
    f.doc["effects"]["lead.open"]=json!({"request":{"cmd":"collab.start","workspace_id":"w","request_id":sid}});
    crate::ipc::atomic_json(&directory.join("hybrid-turn-state.json"),&json!({"state":"uncertain","session_id":sid})).unwrap();
    fs2::FileExt::unlock(&f.peers[0]._owner).unwrap();
    assert!(crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).is_err());
    assert!(f.doc["session_shutdown"]["w"][&sid].is_null());
    crate::ipc::atomic_json(&directory.join("hybrid-turn-state.json"),&json!({"state":"complete","session_id":sid,"native_id":uuid::Uuid::new_v4().to_string()})).unwrap();
    crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).unwrap();
    assert_eq!(f.doc["session_shutdown"]["w"][&sid]["status"],"confirmed_dead");
}
#[test]fn orchestration_v2_shutdown_missing_registration_blocks_completion(){
    let mut f=fixture();let sid=uuid::Uuid::now_v7().to_string();f.doc["effects"]["unknown.open"]=json!({"request":{"cmd":"collab.launch","workspace_id":"w","request_id":sid}});
    assert!(crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).unwrap_err().to_string().contains("no longer registered"));
    assert_ne!(f.doc["session_shutdown"]["w"][&sid]["status"],"confirmed_dead");
}
#[test]fn orchestration_v2_shutdown_collects_all_failures_and_stops_other_owned_sessions(){
    let mut f=fixture();let other=f.peer("other");
    f.doc["effects"]["broken.open"]=json!({"request":{"cmd":"collab.launch","request_id":"missing-workspace"}});
    f.doc["effects"]["other.open"]=json!({"request":{"cmd":"collab.launch","workspace_id":"w","request_id":other},"result":{"session_id":other}});
    let result=crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc);assert!(result.is_err());
    assert_eq!(ipc::request(&f.peers[1].endpoint,&json!({"cmd":"status"})).unwrap()["status"]["exit"],0,"a preceding broken launch must not skip the other stop");
    assert_eq!(f.doc["shutdown_blockers"].as_array().unwrap().len(),3);
    for peer in &f.peers{fs2::FileExt::unlock(&peer._owner).unwrap();}
    assert!(crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).is_err());
    assert_eq!(f.doc["session_shutdown"]["w"][&other]["status"],"confirmed_dead");assert_eq!(f.doc["shutdown_blockers"].as_array().unwrap().len(),1);
}
#[test]fn orchestration_v2_shutdown_does_not_confirm_dead_before_inflight_launch_finishes(){
    let mut f=fixture();let sid=f.doc["lead_session"].as_str().unwrap().to_owned();let key=format!("starting:{sid}");fs2::FileExt::unlock(&f.peers[0]._owner).unwrap();
    f.service.lock().unwrap().terminal_inflight.insert(key.clone());
    assert!(crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).unwrap_err().to_string().contains("in-flight"));assert!(f.doc["session_shutdown"]["w"][&sid].is_null());
    f.service.lock().unwrap().terminal_inflight.remove(&key);
    crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).unwrap();assert_eq!(f.doc["session_shutdown"]["w"][&sid]["status"],"confirmed_dead");
    f.service.lock().unwrap().terminal_inflight.insert(key.clone());
    assert!(crate::desktop::orchestration::v2::shutdown_owned_sessions(&f.service,&f.h,&mut f.doc).is_err());assert!(f.doc["session_shutdown"]["w"][&sid].is_null(),"stale death receipt must be removed");
    f.service.lock().unwrap().terminal_inflight.remove(&key);
}
#[test]fn orchestration_v2_direct_terminal_open_rejects_invalid_mcp_catalog_before_spawn(){
    let f=fixture();let base=f.doc["plan"]["roles"][0].clone();
    for servers in [json!([{"id":"docs","command":"fixture","catalog":["read"],"tools":["write"]}]),json!([{"id":"velella_browser","command":"fixture","catalog":[],"tools":[]}]),json!([{"id":"docs","command":"fixture"},{"id":"docs","command":"fixture"}])]{
        let mut role=base.clone();role["mcp_servers"]=servers;let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":uuid::Uuid::now_v7().to_string(),"launch":"codex","collaboration":true,"role_config":role});
        assert!(f.service.lock().unwrap().prepare_workbench_launch(&req).err().unwrap().to_string().contains("MCP"));
    }
    assert_eq!(f.service.lock().unwrap().workbench.sessions(None).unwrap().len(),1);
}
pub(super) fn fixture_evidence(w:&Value,label:&str)->Value {
    use sha2::{Digest,Sha256};
    let root=Path::new(text(w,"workspace_path").unwrap());
    let output=git(root,&["diff","--check"]).unwrap();
    let record=format!("fixture git diff --check succeeded\n{output}\nsource.txt: {}",std::fs::read_to_string(root.join("source.txt")).unwrap());
    let path=format!(".velella/evidence/{}-{label}.log",text(w,"attempt_id").unwrap());
    std::fs::create_dir_all(root.join(".velella/evidence")).unwrap();std::fs::write(root.join(&path),&record).unwrap();
    json!({"version":1,"candidate":crate::collaboration::source::candidate(root).unwrap(),"checks":w["evidence_contract"]["checks"].as_array().unwrap().iter().enumerate().map(|(i,_)|json!({"requirement_index":i,"method":"fixture: git diff --check and read source.txt","result":"passed","detail":"fixture source and diff checked","artifact_ids":["log"]})).collect::<Vec<_>>(),"deliverables":w["evidence_contract"]["deliverables"].as_array().unwrap().iter().enumerate().map(|(i,_)|json!({"requirement_index":i,"artifact_ids":["log"]})).collect::<Vec<_>>(),"artifacts":[{"id":"log","path":path,"sha256":format!("{:x}",Sha256::digest(record.as_bytes())),"kind":"log"}],"gaps":[]})
}
fn fixture()->Fixture {
    let mut f=Fixture::new();let source=f.root.join("project");
    git(&source,&["init"]).unwrap();git(&source,&["config","user.name","Fixture"]).unwrap();git(&source,&["config","user.email","fixture@localhost"]).unwrap();
    std::fs::write(source.join(".gitignore"),".velella/\n").unwrap();git(&source,&["add","."]).unwrap();git(&source,&["commit","-m","initial"]).unwrap();
    let p=model::v2::seed("codex","source works");
    f.doc["effects"]["lead.open"]=json!({"request":{"cmd":"collab.start","workspace_id":"w","request_id":f.doc["lead_session"]},"result":{"ok":true,"session_id":f.doc["lead_session"]}});
    f.doc["schema_version"]=json!(2);f.doc["plan"]=p;f.doc["owned_workspaces"]=json!([]);f.doc["gates"]=json!({});f.doc["revisions"]=json!([]);f.doc["evidence"]=json!([]);f.doc["stages"]=json!({});f.doc["last_progress_at"]=json!(chrono::Utc::now().to_rfc3339());f.h.orchestration_put(&f.doc).unwrap();f
}
fn confirm(f:&mut Fixture,p:Value){
    dispatch(&f.service,json!({"cmd":"orchestration.plan.save","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":0,"plan":p})).unwrap();
    dispatch(&f.service,json!({"cmd":"orchestration.plan.confirm","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":1})).unwrap();f.refresh();
    workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();
}
fn seed(f:&mut Fixture,id:&str){
    seed_attempt(f,id,1);
}
fn seed_attempt(f:&mut Fixture,id:&str,attempt:u64){
    let nodes:Vec<model::v2::Node>=serde_json::from_value(f.doc["nodes"].clone()).unwrap();let n=nodes.iter().find(|n|n.id==id).unwrap();
    let w=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,n).unwrap();let sid=f.peer_in(id,&w.id);
    f.doc["effects"][format!("v2.node.{id}.{attempt}.open")]=json!({"request":{"cmd":"collab.launch","workspace_id":w.id,"request_id":sid},"result":{"ok":true,"session_id":sid}});f.h.orchestration_put(&f.doc).unwrap();
}
#[test]fn orchestration_v2_owned_session_receipts_only_include_completed_run_launches_once(){
    let doc=json!({"effects":{"lead.open":{"request":{"cmd":"collab.start","workspace_id":"one"},"result":{"session_id":"lead"}},"member.open":{"request":{"cmd":"collab.launch","workspace_id":"two"},"result":{"session_id":"member"}},"replay":{"request":{"cmd":"collab.launch","workspace_id":"two"},"result":{"session_id":"member"}},"pending":{"request":{"cmd":"collab.launch","workspace_id":"three"},"result":{}}}});let (sessions,errors)=owned_sessions(&doc);assert_eq!(sessions,vec![("one".into(),"lead".into()),("two".into(),"member".into())]);assert_eq!(errors.len(),1);assert!(errors[0].contains("no session identity"));
}
#[test]fn orchestration_v2_shutdown_effects_do_not_collide_across_workspaces(){assert_ne!(shutdown_effect_key("one","same"),shutdown_effect_key("two","same"));}
fn review(f:&mut Fixture,w:&Value){
    let workspace=f.doc["bindings"][w["work_key"].as_str().unwrap()]["workspace_id"].as_str().unwrap().to_owned();let sid=f.peer_in("review",&workspace);
    f.doc["effects"][format!("v2.review.{}.{}.open",w["id"].as_str().unwrap(),w["attempt_count"])]=json!({"request":{"cmd":"collab.launch","workspace_id":workspace,"request_id":sid},"result":{"ok":true,"session_id":sid}});f.h.orchestration_put(&f.doc).unwrap();f.tick();
    assert!(f.doc["blocker"].is_null(),"{}",f.doc["blocker"]);
    let r=f.status()["reviews"].as_array().unwrap().iter().find(|r|r["work_id"]==w["id"]).unwrap().clone();
    f.call(&f.doc["member_credentials"][r["reviewer"].as_str().unwrap()],"collab.review.report",json!({"review_id":r["id"],"candidate":r["candidate"],"verdict":"passed","summary":"independent fixture checked source","evidence":fixture_evidence(w,"reviewer")}));f.tick();
}
fn write_commit(w:&Value,content:&str){let path=Path::new(w["workspace_path"].as_str().unwrap());std::fs::write(path.join("source.txt"),content).unwrap();git(path,&["add","source.txt"]).unwrap();git(path,&["commit","--allow-empty","-m","fixture result"]).unwrap();}
#[test]fn orchestration_v2_review_gate_integration_and_final_candidate(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["nodes"][0]["human_gate"]=json!(true);confirm(&mut f,p);seed(&mut f,"implement");f.tick();let w=f.status()["work"][0].clone();write_commit(&w,"fixed");f.report(&w,"completed");review(&mut f,&w);
    f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),1,"gate must block integration");
    dispatch(&f.service,json!({"cmd":"orchestration.gate.resolve","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":1,"node_id":"implement","approved":true,"summary":"stage inspected"})).unwrap();f.refresh();
    seed(&mut f,"__integrate");f.tick();let integration=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__integrate").unwrap().clone();
    let path=Path::new(integration["workspace_path"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(path.join("source.txt")).unwrap(),"fixed","integration must import accepted implementation commits itself");
    f.report(&integration,"completed");review(&mut f,&integration);seed(&mut f,"__verify");f.tick();let verify=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__verify").unwrap().clone();
    assert_eq!(std::fs::read_to_string(Path::new(verify["workspace_path"].as_str().unwrap()).join("source.txt")).unwrap(),"fixed");
    f.report(&verify,"completed");
    let evidence_count=f.doc["evidence"].as_array().unwrap().len();let progress=f.doc["last_progress_at"].clone();
    std::fs::write(path.join("source.txt"),"drift during verification").unwrap();
    for _ in 0..2{f.tick();assert_ne!(f.doc["state"],"awaiting_acceptance");assert_eq!(f.doc["evidence"].as_array().unwrap().len(),evidence_count);assert_eq!(f.doc["last_progress_at"],progress);}
    std::fs::write(path.join("source.txt"),"fixed").unwrap();f.tick();assert_eq!(f.doc["state"],"awaiting_acceptance","{}",f.doc["blocker"]);
    assert_eq!(std::fs::read_to_string(f.root.join("project/source.txt")).unwrap(),"before");
    let req=json!({"cmd":"orchestration.accept","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"summary":"whole goal checked"});
    std::fs::write(path.join("source.txt"),"drift").unwrap();assert!(dispatch(&f.service,req.clone()).is_err());std::fs::write(path.join("source.txt"),"fixed").unwrap();
    assert!(dispatch(&f.service,req.clone()).is_err(),"fixture hosts still own their locks");
    for peer in &f.peers{fs2::FileExt::unlock(&peer._owner).unwrap();}
    dispatch(&f.service,req).unwrap();f.refresh();let (sessions,errors)=owned_sessions(&f.doc);assert!(errors.is_empty());assert_eq!(sessions.len(),f.peers.len());for (workspace,session) in sessions{assert_eq!(f.doc["session_shutdown"][workspace][session]["status"],"confirmed_dead");}
}
#[test]fn orchestration_v2_verification_gate_blocks_delivery_until_user_approves(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["verification_human_gate"]=json!(true);confirm(&mut f,p);
    seed(&mut f,"implement");f.tick();let implement=f.status()["work"][0].clone();write_commit(&implement,"fixed");f.report(&implement,"completed");review(&mut f,&implement);
    seed(&mut f,"__integrate");f.tick();let integration=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__integrate").unwrap().clone();f.report(&integration,"completed");review(&mut f,&integration);
    seed(&mut f,"__verify");f.tick();let verify=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__verify").unwrap().clone();f.report(&verify,"completed");f.tick();assert_ne!(f.doc["state"],"awaiting_acceptance","verification gate must block delivery");
    dispatch(&f.service,json!({"cmd":"orchestration.gate.resolve","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":1,"node_id":"__verify","approved":true,"summary":"final evidence inspected"})).unwrap();f.refresh();f.tick();assert_eq!(f.doc["state"],"awaiting_acceptance", "{}", f.doc["blocker"]);
}
#[test]fn orchestration_v2_retry_records_a_stable_failure_cause_and_second_failure(){
    let mut f=fixture();let p=f.doc["plan"].clone();confirm(&mut f,p);seed(&mut f,"implement");f.tick();let first=f.status()["work"][0].clone();f.report(&first,"failed");f.refresh();
    dispatch(&f.service,json!({"cmd":"orchestration.retry","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"node_id":"implement","cause_key":"fixture-failure","new_evidence":"fixture reproduced the configuration mismatch and changed the input"})).unwrap();f.refresh();assert_eq!(f.doc["failure_ledger"][0]["cause_key"],"fixture-failure");assert_eq!(f.doc["retry_evidence"]["implement"]["cause_key"],"fixture-failure");
    seed_attempt(&mut f,"implement",2);f.tick();let second=f.status()["work"][0].clone();assert_eq!(second["attempt_count"],2,"{}",f.doc);f.report(&second,"failed");f.refresh();assert_eq!(f.doc["failure_ledger"].as_array().unwrap().len(),2);assert!(f.doc["failure_ledger"].as_array().unwrap().iter().all(|entry|entry["cause_key"]=="fixture-failure"));
}
#[test]fn orchestration_v2_failure_ledger_stops_a_third_attempt_across_parallel_nodes(){
    let mut f=fixture();let mut plan=f.doc["plan"].clone();plan["parallelism"]=json!(2);let mut other=plan["nodes"][0].clone();other["id"]=json!("other");other["title"]=json!("other implementation");plan["nodes"].as_array_mut().unwrap().push(other);confirm(&mut f,plan);seed(&mut f,"implement");seed(&mut f,"other");f.tick();f.tick();let works=f.status()["work"].as_array().unwrap().clone();assert_eq!(works.len(),2);for work in &works{f.report(work,"failed");}f.refresh();assert_eq!(f.doc["failure_ledger"].as_array().unwrap().len(),2);
    dispatch(&f.service,json!({"cmd":"orchestration.retry","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"node_id":"implement","cause_key":"fixture-failure","new_evidence":"the same fixture cause was independently reproduced"})).unwrap();f.refresh();assert_eq!(f.doc["state"],"paused");assert!(f.doc["retry_evidence"]["implement"].is_null());f.tick();assert!(f.status()["work"].as_array().unwrap().iter().all(|work|work["attempt_count"]==1));
}
#[test]fn orchestration_v2_retry_rejects_a_work_from_another_run(){
    let mut first=fixture();let plan=first.doc["plan"].clone();confirm(&mut first,plan);seed(&mut first,"implement");first.tick();let first_work=first.status()["work"][0].clone();first.report(&first_work,"failed");first.refresh();
    let mut other=fixture();let plan=other.doc["plan"].clone();confirm(&mut other,plan);seed(&mut other,"implement");other.tick();let other_work=other.status()["work"][0].clone();other.report(&other_work,"failed");other.refresh();
    let req=with_cred(&first.doc["credentials"],json!({"cmd":"collab.retry.propose","request_id":uuid::Uuid::now_v7().to_string(),"work_id":other_work["id"],"cause_key":"fixture-failure","new_evidence":"fixture confirms the same root cause"}));
    assert!(collaboration_commands::dispatch(&first.service,req).is_err(),"a retry request must not accept a work identity outside the authenticated run");
}
#[test]fn orchestration_v2_failed_report_requires_a_stable_cause_key(){
    let mut f=fixture();let plan=f.doc["plan"].clone();confirm(&mut f,plan);seed(&mut f,"implement");f.tick();let work=f.status()["work"][0].clone();let cred=f.doc["member_credentials"][work["assignee"].as_str().unwrap()].clone();f.call(&cred,"collab.started",json!({"attempt_id":work["attempt_id"]}));
    let request=with_cred(&cred,json!({"cmd":"collab.report","request_id":uuid::Uuid::now_v7().to_string(),"attempt_id":work["attempt_id"],"outcome":"failed","summary":"fixture failure","candidate":crate::collaboration::source::candidate(Path::new(work["workspace_path"].as_str().unwrap())).unwrap(),"evidence":Value::Null}));
    assert!(collaboration_commands::dispatch(&f.service,request).unwrap_err().to_string().contains("cause_key"));assert_eq!(f.status()["work"][0]["state"],"turn_started");
}
#[test]fn orchestration_v2_reopen_creates_a_successor_without_mutating_the_accepted_predecessor(){
    let mut f=fixture();let candidate=crate::collaboration::source::candidate(&f.root.join("project")).unwrap();f.doc["state"]=json!("accepted");f.doc["integration_workspace"]=f.doc["workspace_id"].clone();f.doc["integration_branch"]=json!("codex/orch/fixture/integration");f.doc["final_candidate"]=candidate.clone();f.doc["user_acceptance"]=json!({"summary":"accepted fixture","candidate":candidate});f.h.orchestration_put(&f.doc).unwrap();let predecessor=f.doc.clone();
    let request=json!({"cmd":"orchestration.reopen","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"agent":"codex","reason_kind":"reproducible_defect","evidence":"fixture reproduces a defect after acceptance","goal":"fix the reproduced fixture defect","acceptance":"the successor verifies the fixture defect is fixed"});let opened=dispatch(&f.service,request.clone()).unwrap();assert_eq!(opened,dispatch(&f.service,request).unwrap());
    assert_eq!(f.h.orchestration_get(predecessor["id"].as_str().unwrap()).unwrap(),predecessor);let successor=f.h.orchestration_get(opened["id"].as_str().unwrap()).unwrap();assert_eq!(successor["state"],"planning");assert_eq!(successor["predecessor"]["id"],predecessor["id"]);assert_eq!(successor["predecessor"]["candidate"],predecessor["final_candidate"]);assert_eq!(successor["workspace_id"],predecessor["integration_workspace"]);
}
#[test]fn orchestration_v2_reopen_rejects_a_drifted_accepted_candidate(){
    let mut f=fixture();let candidate=crate::collaboration::source::candidate(&f.root.join("project")).unwrap();f.doc["state"]=json!("accepted");f.doc["integration_workspace"]=f.doc["workspace_id"].clone();f.doc["final_candidate"]=candidate.clone();f.doc["user_acceptance"]=json!({"candidate":candidate});f.h.orchestration_put(&f.doc).unwrap();std::fs::write(f.root.join("project/source.txt"),"drifted after acceptance").unwrap();
    let request=json!({"cmd":"orchestration.reopen","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"agent":"codex","reason_kind":"changed_requirement","evidence":"fixture changed the requirement","goal":"follow-on goal","acceptance":"follow-on acceptance"});assert!(dispatch(&f.service,request).unwrap_err().to_string().contains("漂移"));
}
#[test]fn orchestration_v2_baseline_preserves_dirty_index_deletions_and_new_files(){
    let mut f=fixture();let source=f.root.join("project");std::fs::write(source.join("staged-delete.txt"),"x").unwrap();git(&source,&["add","."]).unwrap();git(&source,&["commit","-m","file"]).unwrap();
    git(&source,&["rm","staged-delete.txt"]).unwrap();std::fs::write(source.join("source.txt"),"dirty").unwrap();std::fs::write(source.join("new.txt"),"new").unwrap();
    let before=git(&source,&["status","--porcelain"]).unwrap();let p=f.doc["plan"].clone();confirm(&mut f,p);
    let base=PathBuf::from(f.doc["workspace_bindings"]["baseline"]["path"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(base.join("source.txt")).unwrap(),"dirty");assert!(base.join("new.txt").is_file());assert!(!base.join("staged-delete.txt").exists());
    assert_eq!(git(&source,&["status","--porcelain"]).unwrap(),before);
}
#[test]fn orchestration_v2_baseline_recovers_a_proven_committed_snapshot_after_interruption(){
    let mut f=fixture();let source=f.root.join("project");std::fs::remove_file(source.join("source.txt")).unwrap();let source_status=git(&source,&["status","--porcelain"]).unwrap();workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();
    let committed=f.doc["baseline_commit"].clone();let snapshot=f.doc["baseline_snapshot"].clone();for key in ["baseline_commit","baseline_candidate","baseline_snapshot"]{f.doc.as_object_mut().unwrap().remove(key);}f.doc["baseline_capture_started"]=json!(true);f.h.orchestration_put(&f.doc).unwrap();
    workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();assert_eq!(f.doc["baseline_commit"],committed);assert_eq!(f.doc["baseline_snapshot"],snapshot);assert_eq!(git(&source,&["status","--porcelain"]).unwrap(),source_status);
}
#[test]fn orchestration_v2_baseline_recovery_preserves_an_unknown_interrupted_worktree(){
    let mut f=fixture();let source=f.root.join("project");workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();let baseline=PathBuf::from(text(&f.doc["workspace_bindings"]["baseline"],"path").unwrap());std::fs::write(baseline.join("source.txt"),"unknown interrupted content").unwrap();
    for key in ["baseline_commit","baseline_candidate","baseline_snapshot"]{f.doc.as_object_mut().unwrap().remove(key);}f.doc["baseline_capture_started"]=json!(true);f.h.orchestration_put(&f.doc).unwrap();
    let error=workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap_err().to_string();assert!(error.contains("未记录修改"),"{error}");assert!(f.doc["baseline_commit"].is_null());assert_eq!(std::fs::read_to_string(baseline.join("source.txt")).unwrap(),"unknown interrupted content");assert_eq!(std::fs::read_to_string(source.join("source.txt")).unwrap(),"before");
}
#[test]fn orchestration_v2_baseline_recovery_preserves_an_unknown_ignored_worktree_file(){
    let mut f=fixture();let source=f.root.join("project");std::fs::write(source.join(".gitignore"),".velella/\nignored-recovery.txt\n").unwrap();git(&source,&["add",".gitignore"]).unwrap();git(&source,&["commit","-m","ignore recovery fixture file"]).unwrap();workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();let baseline=PathBuf::from(text(&f.doc["workspace_bindings"]["baseline"],"path").unwrap());std::fs::write(baseline.join("ignored-recovery.txt"),"unknown ignored interrupted content").unwrap();
    for key in ["baseline_commit","baseline_candidate","baseline_snapshot"]{f.doc.as_object_mut().unwrap().remove(key);}f.doc["baseline_capture_started"]=json!(true);f.h.orchestration_put(&f.doc).unwrap();
    let error=workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap_err().to_string();assert!(error.contains("未记录修改"),"{error}");assert!(f.doc["baseline_commit"].is_null());assert_eq!(std::fs::read_to_string(baseline.join("ignored-recovery.txt")).unwrap(),"unknown ignored interrupted content");
}
#[test]fn orchestration_v2_baseline_recovery_rejects_source_drift_after_capture(){
    let mut f=fixture();let source=f.root.join("project");workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap();let baseline=PathBuf::from(text(&f.doc["workspace_bindings"]["baseline"],"path").unwrap());for key in ["baseline_commit","baseline_candidate","baseline_snapshot"]{f.doc.as_object_mut().unwrap().remove(key);}f.doc["baseline_capture_started"]=json!(true);std::fs::write(source.join("source.txt"),"source changed after capture").unwrap();f.h.orchestration_put(&f.doc).unwrap();
    let error=workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap_err().to_string();assert!(error.contains("快照与当前源码身份不一致"),"{error}");assert!(f.doc["baseline_commit"].is_null());assert_eq!(std::fs::read_to_string(source.join("source.txt")).unwrap(),"source changed after capture");assert_eq!(std::fs::read_to_string(baseline.join("source.txt")).unwrap(),"before");
}
#[test]fn orchestration_v2_baseline_checks_actual_worktree_when_binding_is_missing(){
    let mut f=fixture();let source=f.root.join("project");let path=source.join(".velella/orchestration").join(text(&f.doc,"id").unwrap()).join("baseline");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();let branch=format!("codex/orch/{}/baseline",text(&f.doc,"id").unwrap());
    git(&source,&["worktree","add","-b",&branch,path.to_str().unwrap(),"HEAD"]).unwrap();std::fs::write(path.join("source.txt"),"preserve unrecorded work").unwrap();
    let error=workspaces::baseline(&f.service,&f.h,&mut f.doc).unwrap_err().to_string();assert!(error.contains("未记录修改"),"{error}");
    assert_eq!(std::fs::read_to_string(path.join("source.txt")).unwrap(),"preserve unrecorded work");assert!(f.doc["baseline_capture_started"].is_null());
}
#[test]fn orchestration_v2_parallel_writers_use_different_checkouts(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["parallelism"]=json!(2);let mut second=p["nodes"][0].clone();second["id"]=json!("other");p["nodes"].as_array_mut().unwrap().push(second);confirm(&mut f,p);
    seed(&mut f,"implement");seed(&mut f,"other");f.tick();f.tick();let status=f.status();let works=status["work"].as_array().unwrap();assert_eq!(works.len(),2,"{}",f.doc);assert_ne!(works[0]["workspace_path"],works[1]["workspace_path"]);assert!(works.iter().all(|w|w["write"]==true));
    assert_eq!(std::fs::read_to_string(f.root.join("project/source.txt")).unwrap(),"before");
}
#[test]fn orchestration_v2_revisions_cannot_reset_dispatched_work(){
    let mut f=fixture();let p=f.doc["plan"].clone();confirm(&mut f,p);seed(&mut f,"implement");f.tick();
    let mut next=f.doc["plan"].clone();next["nodes"][0]["id"]=json!("renamed");
    let req=json!({"cmd":"orchestration.revision.propose","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":1,"reason":"new information","plan":next});
    assert!(dispatch(&f.service,req).is_err());
}
#[test]fn orchestration_v2_templates_are_revisioned_snapshots(){
    let f=fixture();let key=uuid::Uuid::now_v7().to_string();let request=json!({"cmd":"orchestration.templates.save","request_id":key,"name":"workflow","plan":f.doc["plan"]});
    let a=dispatch(&f.service,request.clone()).unwrap();assert_eq!(a,dispatch(&f.service,request).unwrap());
    let mut changed=f.doc["plan"].clone();changed["acceptance"]=json!("new template goal");
    dispatch(&f.service,json!({"cmd":"orchestration.templates.save","request_id":uuid::Uuid::now_v7().to_string(),"template_id":key,"revision":1,"name":"workflow","plan":changed})).unwrap();
    assert_eq!(f.h.orchestration_get(text(&f.doc,"id").unwrap()).unwrap()["plan"]["acceptance"],"source works");
    assert!(dispatch(&f.service,json!({"cmd":"orchestration.templates.save","request_id":uuid::Uuid::now_v7().to_string(),"template_id":key,"revision":1,"name":"stale","plan":f.doc["plan"]})).is_err());
}
#[test]fn orchestration_v2_completion_requires_complete_unchanged_evidence(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["nodes"][0]["role_id"]=json!("explore");confirm(&mut f,p);seed(&mut f,"implement");f.tick();
    let w=f.status()["work"][0].clone();let cred=f.doc["member_credentials"][text(&w,"assignee").unwrap()].clone();
    f.call(&cred,"collab.started",json!({"attempt_id":w["attempt_id"]}));
    let good=fixture_evidence(&w,"author");
    let send=|evidence:Value|collaboration_commands::dispatch(&f.service,with_cred(&cred,json!({"cmd":"collab.report","request_id":uuid::Uuid::now_v7().to_string(),"attempt_id":w["attempt_id"],"outcome":"completed","summary":"claimed complete","candidate":good["candidate"],"evidence":evidence})));
    assert!(send(Value::Null).is_err());
    for field in ["checks","deliverables"]{let mut bad=good.clone();bad[field]=json!([]);assert!(send(bad).is_err());}
    for state in ["not_run","failed"]{let mut bad=good.clone();bad["checks"][0]["result"]=json!(state);assert!(send(bad).is_err());}
    let mut bad=good.clone();bad["gaps"]=json!(["business requirement not checked"]);assert!(send(bad).is_err());
    bad=good.clone();bad["artifacts"][0]["path"]=json!(".velella/evidence/../../../source.txt");assert!(send(bad).is_err());
    bad=good.clone();bad["artifacts"][0]["sha256"]=json!("0".repeat(64));assert!(send(bad).is_err());
    assert_eq!(f.status()["work"][0]["state"],"turn_started");
    send(good.clone()).unwrap();
    std::fs::write(Path::new(text(&w,"workspace_path").unwrap()).join(text(&good["artifacts"][0],"path").unwrap()),"changed after report").unwrap();
    f.tick();assert_ne!(f.status()["work"][0]["state"],"accepted");assert!(f.doc["blocker"].as_str().unwrap().contains("证据文件已变化"));
}
#[test]fn orchestration_v2_review_cannot_pass_without_its_evidence(){
    let mut f=fixture();let p=f.doc["plan"].clone();confirm(&mut f,p);seed(&mut f,"implement");f.tick();
    let w=f.status()["work"][0].clone();write_commit(&w,"fixed");f.report(&w,"completed");
    let workspace=text(&f.doc["bindings"]["implement"],"workspace_id").unwrap().to_owned();let sid=f.peer_in("review",&workspace);
    f.doc["effects"][format!("v2.review.{}.{}.open",text(&w,"id").unwrap(),w["attempt_count"])]=json!({"result":{"ok":true,"session_id":sid}});f.h.orchestration_put(&f.doc).unwrap();f.tick();
    let r=f.status()["reviews"][0].clone();let cred=&f.doc["member_credentials"][text(&r,"reviewer").unwrap()];
    assert!(collaboration_commands::dispatch(&f.service,with_cred(cred,json!({"cmd":"collab.review.report","request_id":uuid::Uuid::now_v7().to_string(),"review_id":r["id"],"candidate":r["candidate"],"verdict":"passed","summary":"checks not run"}))).is_err());
    assert_eq!(f.status()["reviews"][0]["state"],"requested");assert_eq!(f.status()["work"][0]["state"],"reported");
}
fn dependency_fixture()->(Fixture,model::v2::Node,String){
    let mut f=fixture();let mut p=f.doc["plan"].clone();let mut downstream=p["nodes"][0].clone();downstream["id"]=json!("downstream");downstream["depends_on"]=json!(["implement"]);p["nodes"].as_array_mut().unwrap().push(downstream);confirm(&mut f,p);
    let nodes:Vec<model::v2::Node>=serde_json::from_value(f.doc["nodes"].clone()).unwrap();
    let w=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&nodes[0]).unwrap();let input=git(&w.path,&["rev-parse","HEAD"]).unwrap();
    std::fs::write(w.path.join("source.txt"),"dependency").unwrap();git(&w.path,&["add","source.txt"]).unwrap();git(&w.path,&["commit","-m","dependency"]).unwrap();let oid=git(&w.path,&["rev-parse","HEAD"]).unwrap().trim().to_owned();
    f.doc["bindings"]["implement"]=json!({"input_head":input.trim(),"commit":oid});f.h.orchestration_put(&f.doc).unwrap();
    (f,nodes[1].clone(),oid)
}
#[test]fn orchestration_v2_import_recovery_does_not_duplicate_committed_input(){
    let (mut f,node,oid)=dependency_fixture();
    let w=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&node).unwrap();let head=git(&w.path,&["rev-parse","HEAD"]).unwrap();
    let marker=format!("import.downstream.{oid}");f.doc["imports"][&marker]["state"]=json!("started");f.doc["imports"][&marker].as_object_mut().unwrap().remove("head");f.doc["workspace_bindings"]["downstream"].as_object_mut().unwrap().remove("input_head");f.h.orchestration_put(&f.doc).unwrap();
    workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&node).unwrap();assert_eq!(git(&w.path,&["rev-parse","HEAD"]).unwrap(),head);assert_eq!(f.doc["imports"][&marker]["state"],"done");
}
#[test]fn orchestration_v2_dependency_import_with_long_worktree_path(){
    let (mut f,mut node,oid)=dependency_fixture();
    // A valid revision range must never be stat'ed as a path. On Windows the
    // worktree plus two full OIDs exceeds MAX_PATH despite all real paths fitting.
    let parent=f.root.join("project/.velella/orchestration").join(text(&f.doc,"id").unwrap());
    let padding=200usize.saturating_sub(parent.to_string_lossy().len()+6);
    node.id=format!("deep-{}","x".repeat(padding));
    let workspace=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&node).unwrap();
    assert!(workspace.path.to_string_lossy().len()+81>260);
    assert_eq!(std::fs::read_to_string(workspace.path.join("source.txt")).unwrap(),"dependency");
    assert!(git(&workspace.path,&["log","-1","--format=%B"]).unwrap().contains(&format!("(cherry picked from commit {oid})")));
    workspaces::verify_imports(&f.doc,&node.id,&workspace.path).unwrap();
    let head=git(&workspace.path,&["rev-parse","HEAD"]).unwrap();
    workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&node).unwrap();
    assert_eq!(git(&workspace.path,&["rev-parse","HEAD"]).unwrap(),head,"recovery must not import twice");
}
#[test]fn orchestration_v2_import_rejects_a_non_descendant_candidate(){
    let (mut f,node,_)=dependency_fixture();let baseline=f.doc["baseline_commit"].clone();f.doc["bindings"]["implement"]["input_head"]=f.doc["bindings"]["implement"]["commit"].clone();f.doc["bindings"]["implement"]["commit"]=baseline;
    assert!(workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&node).is_err());
}
#[test]fn orchestration_v2_redundant_patches_keep_distinct_import_receipts(){
    let (mut f,_,first)=dependency_fixture();
    let plan=model::v2::validate(&f.doc["plan"]).unwrap();let mut independent=plan.nodes[1].clone();independent.depends_on.clear();
    let w=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&independent).unwrap();
    std::fs::write(w.path.join("source.txt"),"dependency").unwrap();git(&w.path,&["add","source.txt"]).unwrap();git(&w.path,&["commit","-m","same patch from a different implementation"]).unwrap();let second=git(&w.path,&["rev-parse","HEAD"]).unwrap().trim().to_owned();assert_ne!(first,second);
    f.doc["bindings"]["downstream"]=json!({"input_head":f.doc["baseline_commit"],"commit":second});
    let integration=model::v2::confirmed_nodes(&plan).into_iter().find(|n|n.id=="__integrate").unwrap();let w=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&integration).unwrap();
    workspaces::verify_integration(&f.doc,&w.path).unwrap();assert!(f.doc["workspace_bindings"]["integration"]["pending_imports"].is_null());
    assert_eq!(git(&w.path,&["rev-list","--count",&format!("{}..HEAD",text(&f.doc,"baseline_commit").unwrap())]).unwrap().trim(),"2");
}
#[test]fn orchestration_v2_integration_receives_conflicts_and_must_finish_all_imports(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["parallelism"]=json!(2);let mut b=p["nodes"][0].clone();b["id"]=json!("other");p["nodes"].as_array_mut().unwrap().push(b);confirm(&mut f,p);seed(&mut f,"implement");seed(&mut f,"other");f.tick();f.tick();
    let works=f.status()["work"].as_array().unwrap().clone();
    for (i,w) in works.iter().enumerate(){write_commit(w,&format!("change {i}"));f.report(w,"completed");review(&mut f,w);}
    seed(&mut f,"__integrate");f.tick();
    let integrated=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__integrate").unwrap().clone();
    let path=Path::new(text(&integrated,"workspace_path").unwrap());
    assert!(f.doc["blocker"].is_null(),"{}",f.doc["blocker"]);assert_eq!(f.doc["workspace_bindings"]["integration"]["pending_imports"].as_array().unwrap().len(),1);
    assert!(workspaces::verify_integration(&f.doc,path).is_err());
    let source=git(path,&["rev-parse","CHERRY_PICK_HEAD"]).unwrap().trim().to_owned();
    std::fs::write(path.join("source.txt"),"combined").unwrap();git(path,&["add","source.txt"]).unwrap();
    git(path,&["-c","core.editor=true","cherry-pick","--continue"]).unwrap();
    let message=format!("resolve both changes\n\n(cherry picked from commit {source})");git(path,&["commit","--amend","-m",&message]).unwrap();
    workspaces::verify_integration(&f.doc,path).unwrap();f.report(&integrated,"completed");review(&mut f,&integrated);
    assert_eq!(f.status()["work"].as_array().unwrap().iter().find(|w|w["id"]==integrated["id"]).unwrap()["state"],"accepted");
    assert_eq!(std::fs::read_to_string(f.root.join("project/source.txt")).unwrap(),"before");
}
#[test]fn orchestration_v2_nonfinal_integrator_receives_conflicts_and_verifies_imports(){
    let mut f=fixture();let mut p=f.doc["plan"].clone();p["parallelism"]=json!(2);p["nodes"][0]["id"]=json!("left");p["nodes"][0]["title"]=json!("left implementation");
    let mut right=p["nodes"][0].clone();right["id"]=json!("right");right["title"]=json!("right implementation");
    let mut combine=p["nodes"][0].clone();combine["id"]=json!("combine");combine["title"]=json!("combine conflicting implementations");combine["role_id"]=json!("integrate");combine["depends_on"]=json!(["left","right"]);combine["gap"]=json!("resolve the two accepted implementations");
    p["nodes"].as_array_mut().unwrap().push(right);p["nodes"].as_array_mut().unwrap().push(combine);confirm(&mut f,p);
    seed(&mut f,"left");seed(&mut f,"right");f.tick();f.tick();let works=f.status()["work"].as_array().unwrap().clone();let left=works.iter().find(|w|w["work_key"]=="left").unwrap().clone();let right=works.iter().find(|w|w["work_key"]=="right").unwrap().clone();
    write_commit(&left,"left change");f.report(&left,"completed");review(&mut f,&left);
    write_commit(&right,"right change");f.report(&right,"completed");review(&mut f,&right);
    let left_oid=text(&f.doc["bindings"]["left"],"commit").unwrap().to_owned();let right_oid=text(&f.doc["bindings"]["right"],"commit").unwrap().to_owned();
    // The planned integration role—not a normal implementation node—may receive
    // the conflicted checkout and its ordered unresolved remainder.
    seed(&mut f,"combine");assert_eq!(f.doc["workspace_bindings"]["combine"]["pending_imports"],json!([right_oid]));f.tick();
    let combined=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="combine").unwrap().clone();let path=Path::new(text(&combined,"workspace_path").unwrap());
    assert!(text(&combined,"instruction").unwrap().contains("CHERRY_PICK_HEAD"));assert!(text(&combined,"instruction").unwrap().contains(&right_oid));
    assert!(workspaces::verify_imports(&f.doc,"combine",path).is_err(),"an unresolved cherry-pick cannot be accepted");
    let source=git(path,&["rev-parse","CHERRY_PICK_HEAD"]).unwrap().trim().to_owned();assert_eq!(source,right_oid);
    std::fs::write(path.join("source.txt"),"combined change").unwrap();git(path,&["add","source.txt"]).unwrap();git(path,&["-c","core.editor=true","cherry-pick","--continue"]).unwrap();
    git(path,&["commit","--amend","-m","resolved without source receipt"]).unwrap();assert!(workspaces::verify_imports(&f.doc,"combine",path).is_err(),"a resolved conflict still needs its source trailer");
    let message=format!("resolved both changes\n\n(cherry picked from commit {source})");git(path,&["commit","--amend","-m",&message]).unwrap();workspaces::verify_imports(&f.doc,"combine",path).unwrap();
    f.report(&combined,"completed");review(&mut f,&combined);assert_eq!(f.status()["work"].as_array().unwrap().iter().find(|w|w["id"]==combined["id"]).unwrap()["state"],"accepted");
    assert_eq!(std::fs::read_to_string(f.root.join("project/source.txt")).unwrap(),"before");
    // The final system integrator retains its existing immutable-baseline and
    // all-accepted-input behavior; it has not been silently repurposed as this
    // intermediate node's working tree.
    let plan=model::v2::validate(&f.doc["plan"]).unwrap();let final_node=model::v2::confirmed_nodes(&plan).into_iter().find(|node|node.id=="__integrate").unwrap();let final_workspace=workspaces::node_workspace(&f.service,&f.h,&mut f.doc,&final_node).unwrap();
    let imports:Vec<&str>=f.doc["workspace_bindings"]["integration"]["imports"].as_array().unwrap().iter().map(|value|value.as_str().unwrap()).collect();let combine_oid=text(&f.doc["bindings"]["combine"],"commit").unwrap();assert_eq!(&imports[..2],&[left_oid.as_str(),right_oid.as_str()]);assert!(imports.contains(&combine_oid));
    assert_eq!(f.doc["workspace_bindings"]["integration"]["pending_imports"][0],right_oid);assert!(git(&final_workspace.path,&["merge-base","--is-ancestor",text(&f.doc,"baseline_commit").unwrap(),"HEAD"]).is_ok());assert_eq!(git(&final_workspace.path,&["rev-parse","CHERRY_PICK_HEAD"]).unwrap().trim(),right_oid);let conflict=std::fs::read_to_string(final_workspace.path.join("source.txt")).unwrap();assert!(conflict.contains("left change")&&conflict.contains("right change"));
}
