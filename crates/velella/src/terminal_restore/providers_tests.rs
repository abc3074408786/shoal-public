use super::*;

#[test]fn every_agent_in_the_desktop_catalog_has_native_restore(){
    let catalog=include_str!("../../../../desktop/src/agent-preferences.ts");
    let mut count=0;for line in catalog.lines().filter(|line|line.contains("program:'")){
        let name=line.split("program:'").nth(1).unwrap().split('\'').next().unwrap();let d=tests::fixture(name);assert!(supported(&d),"catalog Agent {name} has no restoration backend");fs::remove_dir_all(d.directory).unwrap();count+=1;
    }assert_eq!(count,8);
}
#[test]fn kimi_owned_session_binding_is_not_limited_to_the_recent_ui_page(){
    let mut d=tests::fixture("kimi");let home=d.directory.join("home");fs::create_dir_all(home.join("locks/sessions")).unwrap();d.spawn.env.insert("KIMI_CODE_HOME".into(),home.to_string_lossy().into_owned());let id=format!("session_{}",uuid::Uuid::now_v7());
    let mut text=format!("{}\n",json!({"sessionId":id,"workDir":d.spawn.cwd}));for _ in 0..150{text.push_str(&format!("{}\n",json!({"sessionId":format!("session_{}",uuid::Uuid::now_v7()),"workDir":d.spawn.cwd})));}fs::write(home.join("session_index.jsonl"),text).unwrap();
    ipc::atomic_json(&home.join("locks/sessions").join(format!("{id}.lock")),&json!({"version":1,"pid":123})).unwrap();
    assert_eq!(candidates(&d).unwrap().len(),100);assert!(!candidates(&d).unwrap().iter().any(|row|row["id"]==id));assert_eq!(providers::poll_kimi(&d,|pid|pid==123,0).unwrap(),Some(id));fs::remove_dir_all(d.directory).unwrap();
}
#[test]fn legacy_opencode_role_hooks_cannot_persist_subagent_as_main(){
    let mut d=tests::fixture("opencode");d.spawn.env.insert("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),"fixture".into());
    observe_hook(&d,&json!({"session_id":"ses_child","hook_event_name":"SessionStart"})).unwrap();assert!(native_id(&d,None).is_none());
    ipc::atomic_json(&d.directory.join("status.json"),&Status{pid:1,native_id:Some("ses_child".into()),..Status::default()}).unwrap();assert!(native_id(&d,None).is_none(),"legacy status does not prove root ownership");
    observe_hook(&d,&json!({"session_id":"ses_main","cwd":d.spawn.cwd,"hook_event_name":"SessionStart"})).unwrap();observe_hook(&d,&json!({"session_id":"ses_child","agent_id":"ses_child","cwd":d.spawn.cwd})).unwrap();assert_eq!(native_id(&d,None).as_deref(),Some("ses_main"));fs::remove_dir_all(d.directory).unwrap();
}
#[test]fn kimi_index_changes_invalidate_cached_workspace_binding(){
    let mut d=tests::fixture("kimi");let home=d.directory.join("home");fs::create_dir_all(home.join("locks/sessions")).unwrap();d.spawn.env.insert("KIMI_CODE_HOME".into(),home.to_string_lossy().into_owned());let id=format!("session_{}",uuid::Uuid::now_v7());let index=home.join("session_index.jsonl");
    fs::write(&index,format!("{}\n",json!({"sessionId":id,"workDir":d.spawn.cwd}))).unwrap();ipc::atomic_json(&home.join("locks/sessions").join(format!("{id}.lock")),&json!({"version":1,"pid":123})).unwrap();assert_eq!(providers::poll_kimi(&d,|pid|pid==123,0).unwrap(),Some(id.clone()));
    fs::write(&index,format!("{}\n",json!({"sessionId":id,"workDir":"/other-workspace"}))).unwrap();assert!(providers::poll_kimi(&d,|pid|pid==123,0).unwrap().is_none());assert!(native_id(&d,None).is_none(),"changed native metadata cannot reuse an old cached workspace proof");fs::remove_dir_all(d.directory).unwrap();
}
#[test]fn unbound_legacy_role_hooks_are_rejected_before_updating_host_status(){
    let mut d=tests::fixture("opencode");d.spawn.env.insert("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),"fixture".into());let child=json!({"session_id":"ses_child"});assert!(!hook_is_bound(&d,&child));d.spawn.env.insert("VELELLA_RESTORE_NATIVE_ID".into(),"ses_main".into());assert!(!hook_is_bound(&d,&child));assert!(hook_is_bound(&d,&json!({"session_id":"ses_main"})));fs::remove_dir_all(d.directory).unwrap();
}
#[test]fn observers_preserve_native_permissions_and_are_idempotent(){
    for program in ["gemini","opencode","omp","kimi"]{
        let mut d=tests::fixture(program);d.spawn.env.insert("HOME".into(),d.directory.to_string_lossy().into_owned());d.spawn.env.insert("USERPROFILE".into(),d.directory.to_string_lossy().into_owned());
        let runtime=d.directory.join("runtime.exe");attach(&mut d,&runtime).unwrap();let first=serde_json::to_value(&d.spawn).unwrap();attach(&mut d,&runtime).unwrap();assert_eq!(serde_json::to_value(&d.spawn).unwrap(),first,"observer duplicated or changed native settings");
        assert!(d.spawn.args.windows(2).any(|args|args==["--model","unchanged"]));assert!(!d.spawn.args.iter().any(|arg|arg=="--yolo"||arg=="--no-extensions"));fs::remove_dir_all(d.directory).unwrap();
    }
}
#[test]fn kimi_identity_requires_owned_pid_and_matching_cwd_not_recency(){
    let mut d=tests::fixture("kimi");let home=d.directory.join("home");fs::create_dir_all(home.join("locks/sessions")).unwrap();d.spawn.env.insert("KIMI_CODE_HOME".into(),home.to_string_lossy().into_owned());
    let id=format!("session_{}",uuid::Uuid::now_v7());let other=format!("session_{}",uuid::Uuid::now_v7());
    fs::write(home.join("session_index.jsonl"),format!("{}\n{}\n",json!({"sessionId":id,"workDir":d.spawn.cwd}),json!({"sessionId":other,"workDir":d.spawn.cwd}))).unwrap();
    ipc::atomic_json(&home.join("locks/sessions").join(format!("{id}.lock")),&json!({"version":1,"pid":123})).unwrap();ipc::atomic_json(&home.join("locks/sessions").join(format!("{other}.lock")),&json!({"version":1,"pid":456})).unwrap();
    assert!(providers::poll_kimi(&d,|_|false,0).unwrap().is_none());assert!(native_id(&d,None).is_none());
    assert!(providers::poll_kimi(&d,|_|true,0).unwrap().is_none(),"two owned native sessions must remain ambiguous");
    assert_eq!(providers::poll_kimi(&d,|pid|pid==123,0).unwrap(),Some(id.clone()));assert_eq!(native_id(&d,None),Some(id));fs::remove_dir_all(d.directory).unwrap();
}
#[test]fn native_restore_removes_conflicting_continue_and_creation_flags(){
    for (program,old) in [("gemini",vec!["--session-id=old","--resume=older"]),("opencode",vec!["--session=ses_old","--continue","--fork"]),("kimi",vec!["--session=session_old","--continue","--agent=old","--agent-file=old"]),("omp",vec!["--resume=old","--restore=old","--continue"]) ]{
        let mut d=tests::fixture(program);let id=match program{"opencode"=>"ses_original".to_string(),"kimi"=>format!("session_{}",uuid::Uuid::now_v7()),_=>uuid::Uuid::now_v7().to_string()};
        observe(&d,&json!({"session_id":id,"cwd":d.spawn.cwd})).unwrap();d.spawn.args.extend(old.into_iter().map(String::from));resume(&mut d,None).unwrap();
        assert_eq!(d.spawn.args[1],id);assert_eq!(d.spawn.args.iter().filter(|arg|arg.contains("old")||arg.as_str()=="--continue"||arg.as_str()=="--fork").count(),0);fs::remove_dir_all(d.directory).unwrap();
    }
}
#[test]fn legacy_provider_lists_are_scoped_to_the_original_workspace(){
    for program in ["gemini","opencode","omp","kimi"]{
        let mut d=tests::fixture(program);let home=d.directory.join("home");fs::create_dir_all(&home).unwrap();d.spawn.env.insert("HOME".into(),home.to_string_lossy().into_owned());d.spawn.env.insert("USERPROFILE".into(),home.to_string_lossy().into_owned());
        let id=match program{"opencode"=>"ses_original".into(),"kimi"=>format!("session_{}",uuid::Uuid::now_v7()),_=>uuid::Uuid::now_v7().to_string()};
        match program{
            "gemini"=>{let custom=home.join("custom-gemini");d.spawn.env.insert("GEMINI_CLI_HOME".into(),custom.to_string_lossy().into_owned());let base=custom.join(".gemini");fs::create_dir_all(base.join("tmp/project/chats")).unwrap();ipc::atomic_json(&base.join("projects.json"),&json!({"projects":{d.spawn.cwd.as_deref().unwrap():"project"}})).unwrap();fs::write(base.join("tmp/project/chats/session.jsonl"),format!("{}\n",json!({"sessionId":id}))).unwrap();},
            "opencode"=>{let db=home.join("native.db");d.spawn.env.insert("OPENCODE_DB".into(),db.to_string_lossy().into_owned());let c=rusqlite::Connection::open(db).unwrap();c.execute_batch("CREATE TABLE session(id TEXT,title TEXT,time_created INTEGER,directory TEXT,parent_id TEXT);").unwrap();for (sid,cwd,parent) in [(id.as_str(),d.spawn.cwd.as_deref().unwrap(),None),("ses_child",d.spawn.cwd.as_deref().unwrap(),Some(id.as_str())),("ses_foreign","/foreign",None)]{c.execute("INSERT INTO session VALUES(?1,'fixture',1,?2,?3)",rusqlite::params![sid,cwd,parent]).unwrap();}},
            "omp"=>{let sessions=home.join("sessions");fs::create_dir_all(&sessions).unwrap();d.spawn.args.extend(["--session-dir".into(),sessions.to_string_lossy().into_owned()]);fs::write(sessions.join("original.jsonl"),format!("{}\n",json!({"type":"session","id":id,"cwd":d.spawn.cwd}))).unwrap();fs::write(sessions.join("foreign.jsonl"),format!("{}\n",json!({"type":"session","id":uuid::Uuid::now_v7(),"cwd":"/foreign"}))).unwrap();},
            "kimi"=>{d.spawn.env.insert("KIMI_CODE_HOME".into(),home.to_string_lossy().into_owned());fs::write(home.join("session_index.jsonl"),format!("{}\n{}\n",json!({"sessionId":id,"workDir":d.spawn.cwd}),json!({"sessionId":format!("session_{}",uuid::Uuid::now_v7()),"workDir":"/foreign"}))).unwrap();},_=>unreachable!()
        }
        let rows=candidates(&d).unwrap();assert_eq!(rows.len(),1,"{program} must not list another directory or subagent");assert_eq!(rows[0]["id"],id);bind(&d,&id).unwrap();assert_eq!(native_id(&d,None).as_deref(),Some(id.as_str()));fs::remove_dir_all(d.directory).unwrap();
    }
}
