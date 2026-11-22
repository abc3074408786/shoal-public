//! Native-host fixtures verify the actual desktop adapter; these are not model UAT.
use super::*;
use crate::workbench::{Session,Workspace};
use std::sync::atomic::{AtomicUsize,Ordering};
#[path="orchestration/v2_tests.rs"]
mod v2_tests;
struct Peer {endpoint:String,thread:Option<std::thread::JoinHandle<()>>,_owner:File,inputs:Arc<AtomicUsize>}
impl Drop for Peer {fn drop(&mut self){let _=ipc::request(&self.endpoint,&json!({"cmd":"fixture.stop"}));if let Some(t)=self.thread.take(){t.join().unwrap();}}}
struct Fixture {service:Arc<Mutex<Service>>,h:Arc<Hub>,doc:Value,peers:Vec<Peer>,root:PathBuf}
#[test]fn orchestration_preflight_reports_missing_commit_and_data_overlap_without_creating_run(){
    let f=Fixture::new();let count=f.h.orchestration_records().unwrap().len();
    let missing=dispatch(&f.service,json!({"cmd":"orchestration.preflight","workspace_id":"w"})).unwrap();
    assert_eq!(missing["ready"],false);assert_eq!(missing["issues"][0]["code"],"git_baseline");
    {let s=f.service.lock().unwrap();let mut workspace=s.workbench.workspace("w").unwrap();workspace.path=f.root.clone();s.workbench.put_workspace(&workspace).unwrap();}
    let overlap=dispatch(&f.service,json!({"cmd":"orchestration.preflight","workspace_id":"w"})).unwrap();
    assert!(overlap["issues"].as_array().unwrap().iter().any(|i|i["code"]=="data_overlap"));
    assert_eq!(f.h.orchestration_records().unwrap().len(),count);
}
impl Fixture {
    fn new()->Self{
        let root=std::env::temp_dir().join(format!("orch-desktop-{}",uuid::Uuid::now_v7()));let data=root.join("data");let project=root.join("project");std::fs::create_dir_all(&project).unwrap();std::fs::write(project.join("source.txt"),"before").unwrap();
        std::fs::create_dir_all(&data).unwrap();let wb=crate::workbench::Workbench::open(&data).unwrap();
        wb.put_workspace(&Workspace{id:"w".into(),project_id:"p".into(),name:"fixture".into(),path:project.clone(),kind:"folder".into(),archived:false,favorite:false,position:0,parent_workspace_id:None}).unwrap();
        let service=Arc::new(Mutex::new(Service{root:data.clone(),executable:std::env::current_exe().unwrap(),projects:vec![Project{id:"p".into(),path:project,name:"fixture".into(),setup:String::new(),default_base:None,setup_default:SetupPolicy::Skip,direct:true,terminals:vec![]}],tasks:vec![],project_locks:HashMap::new(),workbench:wb,capabilities:capabilities::Capabilities::open(&data).unwrap(),terminal_gates:HashMap::new(),terminal_inflight:Default::default(),terminal_epoch:0,collaboration:Arc::new(Mutex::new(None))}));
        let h=hub(&service).unwrap();let rid=uuid::Uuid::now_v7().to_string();
        dispatch(&service,json!({"cmd":"orchestration.create","request_id":rid,"workspace_id":"w","goal":"fix source","acceptance":"result verified","agent":"codex","scout_agent":"claude-code"})).unwrap();
        let doc=h.orchestration_get(&rid).unwrap();let mut f=Self{service,h,doc,peers:vec![],root};
        let sid=f.peer("lead");let r=collaboration_commands::managed(&f.service,json!({"cmd":"collab.create","request_id":uuid::Uuid::now_v7().to_string(),"session_id":sid,"goal":"fix source"})).unwrap();
        f.doc["lead_session"]=json!(sid);f.doc["credentials"]=r["credentials"].clone();f.doc["collab_id"]=r["run"]["id"].clone();f.h.orchestration_put(&f.doc).unwrap();f
    }
    fn peer(&mut self,label:&str)->String{
        self.peer_in(label,"w")
    }
    fn peer_in(&mut self,label:&str,workspace:&str)->String{
        let id=uuid::Uuid::now_v7().to_string();let directory=self.root.join(&id);std::fs::create_dir_all(&directory).unwrap();let s=self.service.lock().unwrap();
        let w=s.workbench.workspace(workspace).unwrap();
        let desc=Descriptor{id:id.clone(),endpoint:ipc::endpoint(&s.root,&id),directory:directory.clone(),spawn:SpawnRequest{sandbox:Default::default(),program:if label=="lead"{"codex"}else{"claude"}.into(),args:vec![],cwd:Some(w.path.to_string_lossy().into_owned()),env:HashMap::new(),adapter:Some("codex".into())}};
        s.workbench.put_session(&Session{id:id.clone(),workspace_id:workspace.into(),title:label.into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:desc.clone()}).unwrap();drop(s);
        let owner=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(directory.join("owner.lock")).unwrap();owner.try_lock_exclusive().unwrap();let listener=ipc::listen(&desc.endpoint).unwrap();let inputs=Arc::new(AtomicUsize::new(0));let counter=inputs.clone();
        let thread=std::thread::spawn(move||{let mut killed=false;let mut usage=None;loop{let mut stop=false;ipc::handle(listener.accept().unwrap(),|req|{if req["cmd"]=="fixture.stop"{stop=true;return Ok(json!({"ok":true}))}if req["cmd"]=="fixture.usage"{usage=Some(serde_json::from_value(req["receipt"].clone())?);return Ok(json!({"ok":true}))}if req["cmd"]=="kill"{killed=true;return Ok(json!({"ok":true}))}if req["cmd"]=="input"{counter.fetch_add(1,Ordering::SeqCst);return Ok(json!({"ok":true}))}Ok(json!({"ok":true,"status":Status{state:"waiting_input".into(),hooks_ready:true,input_sequence:Some(0),context_usage:usage.clone(),exit:if killed{Some(0)}else{None},..Status::default()}}))});if stop{break}}});
        self.peers.push(Peer{endpoint:desc.endpoint,thread:Some(thread),_owner:owner,inputs});id
    }
    fn seed_member(&mut self,key:&str){let sid=self.peer(key);self.doc["effects"][format!("{key}.open")]=json!({"result":{"ok":true,"session_id":sid}});self.h.orchestration_put(&self.doc).unwrap();}
    fn confirm(&mut self,role:&str){
        let p=json!({"acceptance":"result verified","parallelism":1,"nodes":[{"id":"work","title":"work","instruction":"do agreed work","role":role,"depends_on":[],"scope":"source.txt","checks":["read source and verify"]}]});
        let rid=self.doc["id"].clone();dispatch(&self.service,json!({"cmd":"orchestration.plan.save","request_id":uuid::Uuid::now_v7().to_string(),"id":rid,"revision":0,"plan":p})).unwrap();
        dispatch(&self.service,json!({"cmd":"orchestration.plan.confirm","request_id":uuid::Uuid::now_v7().to_string(),"id":rid,"revision":1})).unwrap();self.refresh();
    }
    fn refresh(&mut self){self.doc=self.h.orchestration_get(text(&self.doc,"id").unwrap()).unwrap();}
    fn tick(&mut self){tick(&self.service).unwrap();self.refresh();}
    fn status(&self)->Value{self.h.orchestration_status(&self.doc).unwrap()}
    fn call(&self,cred:&Value,cmd:&str,args:Value)->Value{let mut req=with_cred(cred,args);req["cmd"]=json!(cmd);req["request_id"]=json!(uuid::Uuid::now_v7().to_string());collaboration_commands::dispatch(&self.service,req).unwrap()}
    fn report(&self,w:&Value,outcome:&str){
        let c=if w["assignee"]==self.doc["credentials"]["member_id"]{self.doc["credentials"].clone()}else{self.doc["member_credentials"][text(w,"assignee").unwrap()].clone()};
        self.call(&c,"collab.started",json!({"attempt_id":w["attempt_id"]}));
        let candidate=if w["write"]==true || w["evidence_contract"].is_object(){crate::collaboration::source::candidate(Path::new(text(w,"workspace_path").unwrap())).unwrap()}else{Value::Null};
        let evidence=if w["evidence_contract"].is_object() && outcome=="completed"{v2_tests::fixture_evidence(w,"author")}else{Value::Null};
        self.call(&c,"collab.report",json!({"attempt_id":w["attempt_id"],"outcome":outcome,"summary":"fixture checked actual source","candidate":candidate,"evidence":evidence,"cause_key":if outcome=="failed" {json!("fixture-failure")}else{Value::Null}}));
    }
    fn review(&mut self,w:&Value){
        self.seed_member(&format!("review.{}.{}",text(w,"id").unwrap(),w["attempt_count"]));self.tick();
        let r=self.status()["reviews"].as_array().unwrap().iter().find(|r|r["work_id"]==w["id"]).unwrap().clone();
        self.call(&self.doc["member_credentials"][text(&r,"reviewer").unwrap()],"collab.review.report",json!({"review_id":r["id"],"candidate":r["candidate"],"verdict":"passed","summary":"independent fixture source check"}));
    }
}
#[test]fn orchestration_confirmation_gate_and_single_writer(){
    let mut f=Fixture::new();let req=with_cred(&f.doc["credentials"],json!({"cmd":"collab.assign","request_id":uuid::Uuid::now_v7().to_string(),"trusted":true}));assert!(collaboration_commands::dispatch(&f.service,req).unwrap_err().to_string().contains("托管"));
    f.confirm("lead");f.tick();assert!(f.doc["blocker"].is_null(),"{}",f.doc);let w=f.status()["work"][0].clone();assert_eq!(w["assignee"],f.doc["credentials"]["member_id"]);assert_eq!(w["write"],true);f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),1);assert_eq!(f.peers[0].inputs.load(Ordering::SeqCst),1);
}
#[test]fn orchestration_scout_modification_blocks_acceptance(){
    let mut f=Fixture::new();f.confirm("scout");f.seed_member("node.work.1");f.tick();let w=f.status()["work"][0].clone();std::fs::write(f.root.join("project/source.txt"),"unexpected scout edit").unwrap();f.report(&w,"completed");f.tick();assert!(f.doc["blocker"].as_str().unwrap().contains("源码发生变化"));assert_eq!(f.status()["work"][0]["state"],"reported");
}
#[test]fn orchestration_retry_requires_evidence_and_keeps_work_identity(){
    let mut f=Fixture::new();f.confirm("lead");f.tick();let w=f.status()["work"][0].clone();f.report(&w,"failed");f.tick();assert_eq!(f.status()["work"][0]["attempt_count"],1);
    f.call(&f.doc["credentials"],"collab.retry.propose",json!({"work_id":w["id"],"new_evidence":"fixture proved configuration mismatch; corrected configuration"}));f.tick();let next=f.status()["work"][0].clone();assert_eq!(next["id"],w["id"]);assert_eq!(next["attempt_count"],2);f.report(&next,"failed");f.tick();assert!(f.doc["blocker"].as_str().unwrap().contains("两次"));
}
#[test]fn orchestration_pause_prevents_new_dispatch(){
    let mut f=Fixture::new();f.confirm("lead");dispatch(&f.service,json!({"cmd":"orchestration.pause","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string()})).unwrap();f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),0);
    dispatch(&f.service,json!({"cmd":"orchestration.resume","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string()})).unwrap();f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),1);
}
#[test]fn orchestration_uncertain_input_is_not_replayed(){
    let mut f=Fixture::new();let m=lead_member(&f.h,&f.doc).unwrap();f.doc["effects"]["uncertain"]=json!({"request":{"cmd":"collab.notify","request_id":uuid::Uuid::now_v7().to_string()}});f.h.orchestration_put(&f.doc).unwrap();
    for _ in 0..2{assert!(notify(&f.service,&f.h,&mut f.doc,"uncertain",&m,"test".into()).unwrap_err().to_string().contains("禁止自动重发"));}
    assert_eq!(f.peers[0].inputs.load(Ordering::SeqCst),0);
}
#[test]fn orchestration_stop_error_does_not_skip_other_sessions_and_can_be_reconciled(){
    let mut f=Fixture::new();let other=f.peer("other");let lead=text(&f.doc,"lead_session").unwrap().to_owned();
    f.doc["effects"]["lead.open"]=json!({"result":{"ok":true,"session_id":lead}});f.doc["effects"]["other.open"]=json!({"result":{"ok":true,"session_id":other}});
    f.doc["effects"][format!("stop.{lead}")]=json!({"request":{"cmd":"workbench.terminal.stop","workspace_id":"w","session_id":lead,"confirmed":true},"error":"temporary host error"});f.h.orchestration_put(&f.doc).unwrap();
    dispatch(&f.service,json!({"cmd":"orchestration.stop","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string()})).unwrap();f.tick();assert_eq!(f.doc["state"],"stopping");assert_eq!(f.doc["effects"][format!("stop.{other}")]["result"]["ok"],true);
    dispatch(&f.service,json!({"cmd":"orchestration.resume","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string()})).unwrap();f.refresh();assert!(f.doc["effects"][format!("stop.{lead}")]["error"].is_null());assert_eq!(f.doc["state"],"stopping");
}
#[test]fn orchestration_parallel_scouts_hold_the_lead_until_both_finish(){
    let mut f=Fixture::new();
    let node=|id:&str,role:&str,deps:Vec<&str>|json!({"id":id,"title":id,"instruction":"inspect","role":role,"depends_on":deps,"scope":"source.txt","checks":["verify"]});
    let plan=json!({"acceptance":"verified","parallelism":2,"nodes":[node("a","scout",vec![]),node("b","scout",vec![]),node("lead","lead",vec!["a"])]});
    dispatch(&f.service,json!({"cmd":"orchestration.plan.save","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":0,"plan":plan})).unwrap();
    dispatch(&f.service,json!({"cmd":"orchestration.plan.confirm","id":f.doc["id"],"request_id":uuid::Uuid::now_v7().to_string(),"revision":1})).unwrap();f.refresh();f.seed_member("node.a.1");f.seed_member("node.b.1");f.tick();f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),2);
    let a=f.status()["work"][0].clone();f.report(&a,"completed");f.tick();f.tick();assert_eq!(f.status()["work"].as_array().unwrap().len(),2);
    let b=f.status()["work"][1].clone();f.report(&b,"completed");f.tick();f.tick();assert_eq!(f.status()["work"][2]["work_key"],"lead");
}
#[test]fn orchestration_review_notice_resumes_after_request_and_does_not_duplicate(){
    let mut f=Fixture::new();f.confirm("lead");f.tick();let w=f.status()["work"][0].clone();f.report(&w,"completed");let key=format!("review.{}.{}",text(&w,"id").unwrap(),w["attempt_count"]);f.seed_member(&key);
    let m=member(&f.service,&f.h,&mut f.doc,&key,"review").unwrap();let credentials=f.doc["credentials"].clone();effect!(&f.service,&f.h,&mut f.doc,&format!("{key}.request"),with_cred(&credentials,json!({"cmd":"collab.review.request","work_id":w["id"],"reviewer":m["id"]}))).unwrap();
    // Restart after durable review.request, before notifying the reviewer.
    let reopened=Arc::new(Hub::open(&f.service.lock().unwrap().root).unwrap());*f.service.lock().unwrap().collaboration.lock().unwrap()=Some(reopened.clone());f.h=reopened;f.tick();assert_eq!(f.peers[1].inputs.load(Ordering::SeqCst),1);f.tick();assert_eq!(f.peers[1].inputs.load(Ordering::SeqCst),1);
}
#[test]fn orchestration_final_acceptance_recovers_and_checks_current_candidate(){
    let mut f=Fixture::new();f.confirm("lead");f.tick();let w=f.status()["work"][0].clone();f.report(&w,"completed");f.review(&w);f.tick();f.tick();let final_work=f.status()["work"].as_array().unwrap().iter().find(|w|w["work_key"]=="__final").unwrap().clone();f.report(&final_work,"completed");f.review(&final_work);
    // Simulate crash immediately after durable collab.accept, before controller update.
    collaboration_commands::managed(&f.service,with_cred(&f.doc["credentials"],json!({"cmd":"collab.accept","request_id":uuid::Uuid::now_v7().to_string(),"work_id":final_work["id"],"summary":"review passed"}))).unwrap();f.tick();assert_eq!(f.doc["state"],"awaiting_acceptance");
    let req=json!({"cmd":"orchestration.accept","id":f.doc["id"],"summary":"user accepted","request_id":uuid::Uuid::now_v7().to_string()});std::fs::write(f.root.join("project/source.txt"),"changed").unwrap();assert!(dispatch(&f.service,req.clone()).is_err());std::fs::write(f.root.join("project/source.txt"),"before").unwrap();dispatch(&f.service,req).unwrap();f.refresh();assert_eq!(f.doc["state"],"accepted");
}
