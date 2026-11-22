use super::*;

struct Fixture { service: Option<Service>, root: PathBuf }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("velella-automation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        let service = Service { root:root.clone(), executable:std::env::current_exe().unwrap(), projects:vec![], tasks:vec![], project_locks:HashMap::new(), workbench:crate::workbench::Workbench::open(&root).unwrap(), capabilities:Capabilities::open(&root).unwrap(), terminal_gates:HashMap::new(), terminal_inflight:Default::default(), terminal_epoch:0, collaboration:Arc::new(Mutex::new(None)) };
        Self { service:Some(service), root }
    }
    fn service(&mut self) -> &mut Service { self.service.as_mut().unwrap() }
}
impl Drop for Fixture {
    fn drop(&mut self) { self.service.take(); std::fs::remove_dir_all(&self.root).unwrap(); }
}
fn seed(c: &Capabilities, id: &str, task: &str, automation: &str, time: i64, state: &str) {
    c.db.execute("INSERT INTO runs(id,task_id,automation_id,started_at,state) VALUES(?1,?2,?3,?4,?5)", params![id,task,automation,time,state]).unwrap();
}
fn task(id: &str, phase: &str) -> Task {
    serde_json::from_value(json!({"id":id,"project_id":"p","spec":{"name":"fixture","agent":"codex","instruction":"never run"},"phase":phase,"worktree":"unused","branch":"b","base_oid":"oid","created_at":"fixture","reason":"fixture state","verified":false,"checks":[],"retries":0,"observed_turns":0,"allowed_commands":[]})).unwrap()
}

#[test]
fn automation_archive_restores_disabled_and_retains_history_across_reopen() {
    let mut f = Fixture::new(); let s = f.service();
    s.capabilities.put("automations", &json!({"id":"a","enabled":true,"next_run":1,"interval_minutes":1,"spec":{"name":"fixture"}})).unwrap();
    seed(&s.capabilities,"r","t","a",1,"delivered");
    assert!(s.capability_command(json!({"cmd":"automation.remove","id":"a"})).is_err());
    let result = s.capability_command(json!({"cmd":"automation.remove","id":"a","confirmed":true})).unwrap();
    assert_eq!(result["archived"], true);
    assert_eq!(result["automation"]["enabled"], false);
    assert!(s.capabilities.documents("automations").unwrap().is_empty());
    s.tick_automations().unwrap(); // due archived definition cannot dispatch
    assert_eq!(s.capabilities.runs().unwrap().len(),1);
    assert!(s.capability_command(json!({"cmd":"automation.run","id":"a","confirmed":true})).is_err());
    f.service.take();
    let mut c = Capabilities::open(&f.root).unwrap();
    assert_eq!(c.documents("archived_automations").unwrap().len(),1);
    let restored = c.move_automation("a",true).unwrap();
    assert_eq!(restored["enabled"],false);
    assert!(restored.get("archived_at").is_none());
    assert_eq!(c.runs().unwrap().len(),1);
    assert!(c.documents("archived_automations").unwrap().is_empty());
    drop(c);
}

#[test]
fn automation_archive_blocks_unfinished_and_uncertain_dispatch() {
    let mut f=Fixture::new(); let s=f.service();
    s.capabilities.put("automations",&json!({"id":"a","enabled":false})).unwrap();
    seed(&s.capabilities,"r","t","a",1,"queued");
    for phase in ["queued","preparing","running","verifying","review","needs_attention","needs_recovery"] {
        s.tasks=vec![task("t",phase)];
        assert!(s.capability_command(json!({"cmd":"automation.remove","id":"a","confirmed":true})).is_err(),"{phase}");
        assert!(s.capabilities.get("automations","a").is_ok());
    }
    s.tasks.clear();
    s.capabilities.db.execute("UPDATE runs SET state='dispatching'",[]).unwrap();
    assert!(s.capability_command(json!({"cmd":"automation.remove","id":"a","confirmed":true})).is_err());
    s.sync_automation_runs().unwrap();
    assert_eq!(s.capabilities.runs().unwrap()[0]["state"],"needs_attention");
    s.capability_command(json!({"cmd":"automation.remove","id":"a","confirmed":true})).unwrap();
}

#[test]
fn automation_history_pages_are_complete_with_ties_and_concurrent_inserts() {
    let mut f=Fixture::new(); let c=&f.service().capabilities;
    for i in 0..605 { seed(c,&format!("r{i}"),&format!("t{i}"),if i%2==0 {"a"} else {"b"},i/3,"delivered"); }
    let first=c.history_page(&json!({"limit":37,"automation_id":"a"})).unwrap();
    seed(c,"new","new-task","a",0,"delivered"); // backdated insert must also be excluded
    let mut seen=first["runs"].as_array().unwrap().clone();
    let mut page=first;
    while page["has_more"]==true {
        page=c.history_page(&json!({"limit":37,"automation_id":"a","cursor":page["next_cursor"]})).unwrap();
        seen.extend(page["runs"].as_array().unwrap().iter().cloned());
    }
    let expected: Vec<_>=(0..605).rev().filter(|i|i%2==0).map(|i|json!(format!("r{i}"))).collect();
    assert_eq!(seen.iter().map(|r|r["id"].clone()).collect::<Vec<_>>(),expected);
    assert!(page["next_cursor"].is_null());
    let fresh=c.history_page(&json!({"limit":500})).unwrap();
    assert_eq!(fresh["runs"].as_array().unwrap().len(),500);
    assert_eq!(fresh["has_more"],true);
}

#[test]
fn automation_history_rejects_invalid_limits_and_cross_query_cursors() {
    let mut f=Fixture::new(); let c=&f.service().capabilities;
    seed(c,"1","t1","a",1,"delivered"); seed(c,"2","t2","a",1,"delivered");
    let page=c.history_page(&json!({"limit":1,"automation_id":"a"})).unwrap();
    for req in [json!({"limit":0}),json!({"limit":501}),json!({"limit":-1}),json!({"limit":"10"}),json!({"cursor":"garbage"}),json!({"cursor":false}),json!({"cursor":page["next_cursor"]}),json!({"cursor":page["next_cursor"],"automation_id":"b"})] {
        assert!(c.history_page(&req).is_err(),"{req}");
    }
    assert_eq!(c.history_page(&json!({"automation_id":"missing"})).unwrap()["runs"],json!([]));
}

#[test]
fn automation_history_sync_includes_old_tasks_and_orphans_beyond_200() {
    let mut f=Fixture::new(); let s=f.service();
    s.capabilities.put("automations",&json!({"id":"a","enabled":true,"next_run":4100000000i64})).unwrap();
    seed(&s.capabilities,"old-task","t","a",0,"queued");
    seed(&s.capabilities,"old-orphan","missing","a",0,"dispatching");
    seed(&s.capabilities,"deleted-orphan","missing2","deleted",0,"dispatching");
    for i in 0..220 { seed(&s.capabilities,&format!("r{i}"),&format!("t{i}"),"b",i+1,"delivered"); }
    s.tasks=vec![task("t","delivered")];
    s.tick_automations().unwrap();
    let rows=s.capability_command(json!({"cmd":"automation.runs","automation_id":"a"})).unwrap();
    assert_eq!(rows["runs"][0]["state"],"needs_attention");
    assert_eq!(rows["runs"][1]["state"],"delivered");
    assert_eq!(s.capabilities.get("automations","a").unwrap()["enabled"],false);
}

#[test]
fn automation_archive_conflict_rolls_back_and_restore_requires_confirmation() {
    let mut f=Fixture::new(); let s=f.service();
    s.capabilities.put("automations",&json!({"id":"a","enabled":true})).unwrap();
    s.capabilities.put("archived_automations",&json!({"id":"a","enabled":false})).unwrap();
    assert!(s.capabilities.move_automation("a",false).is_err());
    assert_eq!(s.capabilities.get("automations","a").unwrap()["enabled"],true);
    assert!(s.capability_command(json!({"cmd":"automation.restore","id":"a"})).is_err());
    assert!(s.capabilities.move_automation("missing",true).is_err());
}
