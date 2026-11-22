use super::*;
use super::tests::mock;
use interprocess::local_socket::prelude::*;

fn command() -> Command {
    Cli::try_parse_from(["velella-cli", "agent", "switch", "--workspace", "w", "--terminal", "s", "--json"]).unwrap().args.command
}
fn workspace(archived: bool) -> Value { json!({"ok":true,"workspaces":[{"id":"w","archived":archived}]}) }
fn sessions() -> Value { json!({"ok":true,"sessions":[{"id":"s","kind":"terminal"}]}) }

#[test]
fn switch_requires_exact_desktop_focus_ack_and_preserves_ids() {
    for accepted in [true, false] {
        let (client,daemon,root)=mock(vec![workspace(false),sessions()]);
        let listener=crate::ipc::listen(&crate::ipc::endpoint(&root,"browser")).unwrap();
        let desktop=std::thread::spawn(move|| {
            crate::ipc::handle(listener.accept().unwrap(),|request| {
                assert_eq!(request["op"],"terminal_switch");
                assert_eq!(request["workspace_id"],"w");assert_eq!(request["session_id"],"s");
                assert!(request["expires_at_ms"].as_u64().is_some());
                Ok(json!({"ok":accepted,"code":"desktop_switch_rejected","error":"dialog open","protocol":"velella-terminal-switch-v1","workspace_id":"w","session_id":"s","focused":true}))
            });
        });
        let result=execute(&client,&command());
        if accepted { assert_eq!(result.unwrap()["focused"],true); }
        else { assert_eq!(result.unwrap_err().downcast_ref::<Failure>().unwrap().detail["code"],"desktop_switch_rejected"); }
        desktop.join().unwrap();
        let requests=daemon.join().unwrap();
        assert_eq!(requests.len(),2);
        assert_eq!(requests[0]["cmd"],"workbench.snapshot");assert_eq!(requests[1]["cmd"],"workbench.sessions");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn archived_missing_or_nonterminal_targets_never_reach_desktop() {
    for replies in [vec![workspace(true)],vec![workspace(false),json!({"ok":true,"sessions":[]})],vec![workspace(false),json!({"ok":true,"sessions":[{"id":"s","kind":"agent"}]})]] {
        let (client,daemon,root)=mock(replies);
        assert!(execute(&client,&command()).is_err());daemon.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn incompatible_ack_and_unavailable_desktop_cannot_report_success() {
    let (client,daemon,root)=mock(vec![workspace(false),sessions()]);
    let listener=crate::ipc::listen(&crate::ipc::endpoint(&root,"browser")).unwrap();
    let desktop=std::thread::spawn(move||crate::ipc::handle(listener.accept().unwrap(),|_|Ok(json!({"ok":true,"protocol":"velella-terminal-switch-v1","workspace_id":"other","session_id":"s","focused":true}))));
    assert!(execute(&client,&command()).unwrap_err().to_string().contains("exact terminal focus"));
    desktop.join().unwrap();daemon.join().unwrap();std::fs::remove_dir_all(root).unwrap();
    let (client,daemon,root)=mock(vec![workspace(false),sessions()]);
    assert_eq!(execute(&client,&command()).unwrap_err().downcast_ref::<Failure>().unwrap().detail["code"],"desktop_switch_unconfirmed");
    daemon.join().unwrap();std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn switch_rejects_ssh_before_inspecting_any_saved_profile() {
    let args=Cli::try_parse_from(["velella-cli","--ssh-profile","missing","--server-id","00000000-0000-0000-0000-000000000001","--remote-data-dir","/missing","terminal","switch","--workspace","w","--terminal","s","--json"]).unwrap().args;
    assert_eq!(run(args),1);
}
