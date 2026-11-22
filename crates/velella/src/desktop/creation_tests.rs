use super::*;

struct Fixture {
    service: Service,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("velella-create-api-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        let project: Project = serde_json::from_value(json!({"id":"p","path":root,"name":"fixture","setup":"original setup","direct":true,"terminals":[]})).unwrap();
        let service = Service { root:root.clone(), executable:std::env::current_exe().unwrap(), projects:vec![project], tasks:vec![], project_locks:HashMap::new(), workbench:crate::workbench::Workbench::open(&root).unwrap(), capabilities:capabilities::Capabilities::open(&root).unwrap(), terminal_gates:HashMap::new(), terminal_inflight:Default::default(), terminal_epoch:0, collaboration:Arc::new(Mutex::new(None)) };
        Self { service, root }
    }
    fn git_project() -> (Self, String) {
        let mut fixture = Self::new();
        git(&fixture.root, &["init", "--quiet", "--template=", "-b", "feature"]).unwrap();
        let main = fixture.commit("main");
        let feature = fixture.commit("feature");
        git(&fixture.root, &["update-ref", "refs/heads/main", &main]).unwrap();
        git(&fixture.root, &["update-ref", "refs/heads/feature", &feature]).unwrap();
        fixture.service.projects[0].direct = false;
        fixture.service.projects[0].setup.clear();
        (fixture, main)
    }
    fn commit(&self, message: &str) -> String {
        let tree = git(&self.root, &["mktree"]).unwrap();
        git(&self.root, &["-c", "user.name=Creation Fixture", "-c", "user.email=creation@example.invalid", "commit-tree", tree.trim(), "-m", message]).unwrap().trim().into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); }
}

#[test]
fn project_update_is_partial_and_optional_base_can_be_cleared() {
    let mut fixture = Fixture::new();
    fixture.service.dispatch(json!({"cmd":"project.update","project_id":"p","default_base":"release","setup_default":"run"})).unwrap();
    let project = fixture.service.project("p").unwrap();
    assert_eq!(project.setup, "original setup");
    assert_eq!(serde_json::to_value(project).unwrap()["default_base"], "release");
    fixture.service.dispatch(json!({"cmd":"project.update","project_id":"p","setup":"changed"})).unwrap();
    let project = serde_json::to_value(fixture.service.project("p").unwrap()).unwrap();
    assert_eq!(project["default_base"], "release");
    assert_eq!(project["setup_default"], "run");
    fixture.service.dispatch(json!({"cmd":"project.update","project_id":"p","default_base":null})).unwrap();
    let project = serde_json::to_value(fixture.service.project("p").unwrap()).unwrap();
    assert!(project["default_base"].is_null());
    assert_eq!(project["setup"], "changed");
}

#[test]
fn project_update_rejects_wrong_types_without_partial_changes() {
    let mut fixture = Fixture::new();
    let before = serde_json::to_value(fixture.service.project("p").unwrap()).unwrap();
    for fields in [json!({"setup":null}),json!({"setup":42}),json!({"setup":"changed","default_base":false}),json!({"setup":"changed","setup_default":null}),json!({"setup_default":"inherit"}),json!({"default_base":"--help"}),json!({"default_base":"main\n"})] {
        let mut request = fields;
        request["cmd"] = json!("project.update"); request["project_id"] = json!("p");
        assert!(fixture.service.dispatch(request).is_err());
        assert_eq!(serde_json::to_value(fixture.service.project("p").unwrap()).unwrap(), before);
    }
}

#[test]
fn failed_project_save_does_not_change_live_configuration() {
    let mut fixture = Fixture::new();
    std::fs::create_dir(fixture.root.join("projects.json")).unwrap();
    assert!(fixture.service.dispatch(json!({"cmd":"project.update","project_id":"p","setup":"changed"})).is_err());
    assert_eq!(fixture.service.project("p").unwrap().setup, "original setup");
}

#[test]
fn ordinary_creation_uses_default_base_and_replays_before_resolving_configuration() {
    let (mut fixture, main) = Fixture::git_project();
    let id = uuid::Uuid::now_v7().to_string();
    let request = json!({"cmd":"workbench.create","project_id":"p","name":"child","request_id":id,"parent_workspace_id":"main:p"});
    let first = fixture.service.dispatch(request.clone()).unwrap();
    let path = PathBuf::from(first["workspace"]["path"].as_str().unwrap());
    assert_eq!(git(&path, &["rev-parse", "HEAD"]).unwrap().trim(), main);
    assert_eq!(first["workspace"]["parent_workspace_id"], "main:p");
    fixture.service.projects[0].default_base = Some("missing".into());
    let again = fixture.service.dispatch(request.clone()).unwrap();
    assert_eq!(again["workspace"]["id"], id);
    let mut conflict = request;
    conflict["name"] = json!("different");
    assert!(fixture.service.dispatch(conflict).is_err());
    assert_eq!(fixture.service.workbench.workspaces().unwrap().len(), 2);
}

#[test]
fn managed_creation_freezes_project_base_and_deduplicates_request_id() {
    let (mut fixture, _) = Fixture::git_project();
    fixture.service.projects[0].default_base = Some("feature".into());
    let expected = git(&fixture.root, &["rev-parse", "feature"]).unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let request = json!({"cmd":"task.create","project_id":"p","request_id":id,"spec":{"name":"task","agent":"codex","instruction":"test"}});
    let first = fixture.service.dispatch(request.clone()).unwrap();
    assert_eq!(first["task"]["base_oid"], expected.trim());
    assert_eq!(first["task"]["id"], id);
    fixture.service.projects[0].default_base = Some("missing".into());
    let second = fixture.service.dispatch(request.clone()).unwrap();
    assert_eq!(first["task"], second["task"]);
    assert_eq!(fixture.service.tasks.len(), 1);
    let mut conflict = request;
    conflict["spec"]["instruction"] = json!("changed");
    assert!(fixture.service.dispatch(conflict).is_err());
    assert_eq!(fixture.service.tasks.len(), 1);
}

#[test]
fn native_launch_replay_binds_model_and_effort_without_creating_a_task() {
    let mut fixture = Fixture::new();
    fixture.service.workbench.sync_legacy(&fixture.service.projects,&[]).unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let request = json!({"cmd":"workbench.terminal.open","workspace_id":"main:p","launch":"codex","request_id":id,"launch_options":{"model":"custom/model","effort":"high"}});
    let (descriptor, first) = fixture.service.prepare_workbench_launch(&request).unwrap();
    let descriptor = descriptor.unwrap();
    assert!(descriptor.spawn.args.windows(2).any(|args|args==["--model","custom/model"]));
    assert!(descriptor.spawn.args.windows(2).any(|args|args==["-c","model_reasoning_effort=\"high\""]));
    let (descriptor, repeated) = fixture.service.prepare_workbench_launch(&request).unwrap();
    assert!(descriptor.is_none());
    assert_eq!(first["session_id"], repeated["session_id"]);
    let mut conflict = request;
    conflict["launch_options"]["effort"] = json!("low");
    assert!(fixture.service.prepare_workbench_launch(&conflict).is_err());
    assert!(fixture.service.tasks.is_empty());
    assert_eq!(fixture.service.workbench.sessions(None).unwrap().len(), 1);
}

#[test]
fn completed_setup_session_cannot_be_replayed_as_terminal_recovery() {
    let mut fixture = Fixture::new();
    fixture.service.workbench.sync_legacy(&fixture.service.projects, &[]).unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let request = json!({"cmd":"workbench.terminal.open","workspace_id":"main:p","launch":"terminal","request_id":id});
    fixture.service.prepare_workbench_launch(&request).unwrap();
    let mut session = fixture.service.workbench.session("main:p", &id).unwrap().unwrap();
    session.kind = "command".into();
    fixture.service.workbench.put_session(&session).unwrap();
    let recovered = fixture.service.prepare_workbench_launch(&json!({"cmd":"workbench.terminal.recover","workspace_id":"main:p","session_id":id}));
    assert!(recovered.is_err(), "command recovery must not bypass creation attempt review");
}

#[test]
fn ordinary_creation_does_not_ignore_terminal_sandbox_field() {
    let (mut fixture, _) = Fixture::git_project();
    fixture.service.projects[0].setup = "exit 7".into();
    let request = json!({"cmd":"workbench.create","project_id":"p","name":"restricted","setup":"run","reviewed_setup":"exit 7","sandbox":{"mode":"restricted"}});
    assert!(fixture.service.dispatch(request).is_err());
}
