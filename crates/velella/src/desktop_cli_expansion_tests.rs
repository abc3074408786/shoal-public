use super::tests::mock;
use super::*;
use interprocess::local_socket::prelude::*;

fn command(args: &[&str]) -> Command {
    Cli::try_parse_from(std::iter::once("velella-cli").chain(args.iter().copied()))
        .unwrap()
        .args
        .command
}
fn sessions(state: &str, permission: bool) -> Value {
    json!({"ok":true,"sessions":[{"id":"s1","status":{"state":state,"exit":null,"permission_input":permission}}]})
}
fn empty_layout() -> Value {
    json!({"ok":true,"layout":crate::workbench::Layout::default()})
}
fn page(from: u64, total: u64, bytes: &[u8], geometry: Value) -> Value {
    json!({"ok":true,"from":from,"total":total,"data":base64::engine::general_purpose::STANDARD.encode(bytes),"geometry":geometry})
}

#[test]
fn new_management_writes_require_review_and_mcp_requires_tools() {
    for args in [
        vec!["automation", "remove", "a1"],
        vec!["automation", "restore", "a1"],
        vec!["automation", "run", "a1"],
        vec![
            "host",
            "connect",
            "h1",
            "--workspace",
            "w1",
            "--reviewed-file",
            "profile.json",
        ],
        vec!["host", "remove", "h1"],
        vec!["workspace", "archive", "w1"],
        vec!["file", "remove", "--workspace", "w1", "README.md"],
        vec![
            "git",
            "commit",
            "--workspace",
            "w1",
            "--message",
            "reviewed",
        ],
        vec![
            "terminal",
            "recover",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
        ],
        vec!["browser", "forget", "--workspace", "w1", "--page", "b1"],
        vec![
            "mcp",
            "confirm",
            "catalog.json",
            "--digest",
            "abc",
            "--entry",
            "fixture",
            "--confirm",
        ],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("velella-cli").chain(args)).is_err());
    }
}

#[test]
fn automation_management_checks_capabilities_before_mutation() {
    let (client, worker, root) = mock(vec![json!({"ok":true,"capabilities":[]})]);
    let error=execute(&client,&command(&["automation","remove","a1","--confirm"])).unwrap_err();
    assert!(error.to_string().contains("需要升级 runtime"));
    assert_eq!(worker.join().unwrap(),vec![json!({"cmd":"protocol"})]);
    std::fs::remove_dir_all(root).unwrap();
    let (client, worker, root) = mock(vec![json!({"ok":true,"capabilities":["automation-archive-v1"]}),json!({"ok":true,"archived":false})]);
    execute(&client,&command(&["automation","restore","a1","--confirm"])).unwrap();
    assert_eq!(worker.join().unwrap()[1],json!({"cmd":"automation.restore","id":"a1","confirmed":true}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn automation_history_legacy_is_explicit_and_new_paging_is_forwarded() {
    let (client, worker, root)=mock(vec![json!({"ok":true,"capabilities":[]}),json!({"ok":true,"runs":[{"automation_id":"a"},{"automation_id":"b"}]})]);
    let reply=execute(&client,&command(&["automation","runs","--automation","a"])).unwrap();
    assert_eq!(reply["history_scope"],"legacy_global_latest_200");
    assert_eq!(reply["runs"].as_array().unwrap().len(),1);
    assert_eq!(worker.join().unwrap().len(),2);
    std::fs::remove_dir_all(root).unwrap();
    let (client,worker,root)=mock(vec![json!({"ok":true,"capabilities":[]})]);
    assert!(execute(&client,&command(&["automation","runs","--limit","50"])).is_err());
    assert_eq!(worker.join().unwrap().len(),1);
    std::fs::remove_dir_all(root).unwrap();
    let (client,worker,root)=mock(vec![json!({"ok":true,"capabilities":["automation-history-v1"]}),json!({"ok":true,"runs":[],"next_cursor":null})]);
    execute(&client,&command(&["automation","runs","--automation","a","--limit","50","--cursor","token"])).unwrap();
    assert_eq!(worker.join().unwrap()[1],json!({"cmd":"automation.runs","automation_id":"a","limit":50,"cursor":"token"}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_false_and_null_updates_are_not_lost() {
    let (client, worker, root) = mock(vec![json!({"ok":true}), json!({"ok":true})]);
    if let Command::Workspace { command } = command(&[
        "workspace",
        "update",
        "w1",
        "--favorite",
        "false",
        "--no-parent",
    ]) {
        services::workspace(&client, &command).unwrap();
    }
    if let Command::Workspace { command } = command(&["workspace", "archive", "w1", "--confirm"]) {
        services::workspace(&client, &command).unwrap();
    }
    let requests = worker.join().unwrap();
    assert_eq!(requests[0]["favorite"], false);
    assert!(requests[0].get("parent_workspace_id").unwrap().is_null());
    assert!(requests[0].get("archived").is_none());
    assert_eq!(requests[1]["confirmed"], true);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn automation_creation_is_disabled_by_default_and_uses_explicit_timezone() {
    let (client, worker, root) = mock(vec![json!({"ok":true,"automation":{"id":"a1"}})]);
    let spec = root.join("task.yaml");
    std::fs::write(
        &spec,
        "name: fixture\nagent: codex\ninstruction: perform reviewed work\n",
    )
    .unwrap();
    execute(
        &client,
        &command(&[
            "automation",
            "create",
            "--project",
            "p1",
            "--file",
            spec.to_str().unwrap(),
            "--start-at",
            "2026-10-30T09:00:00+08:00",
            "--interval-minutes",
            "60",
            "--confirm",
        ]),
    )
    .unwrap();
    let request = worker.join().unwrap().remove(0);
    assert_eq!(request["automation"]["enabled"], false);
    assert_eq!(request["confirmed"], true);
    assert_eq!(
        request["automation"]["next_run"],
        chrono::DateTime::parse_from_rfc3339("2026-10-30T01:00:00Z")
            .unwrap()
            .timestamp()
    );
    assert!(request["automation"]["spec"].get("base").is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_write_preserves_previous_content_and_reports_conflict() {
    let (client, worker, root) = mock(vec![
        json!({"ok":false,"error":"file changed on disk; reload before saving"}),
    ]);
    let old = root.join("old.txt");
    let new = root.join("new.txt");
    std::fs::write(&old, "before\n").unwrap();
    std::fs::write(&new, "after\n").unwrap();
    let error = execute(
        &client,
        &command(&[
            "file",
            "write",
            "--workspace",
            "w1",
            "value.txt",
            "--file",
            new.to_str().unwrap(),
            "--previous-file",
            old.to_str().unwrap(),
        ]),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().unwrap().detail["code"],
        "request_rejected"
    );
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["workspace_id"], "w1");
    assert_eq!(requests[0]["previous"], "before\n");
    assert_eq!(requests[0]["content"], "after\n");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_wait_matches_utf8_across_pages_without_input() {
    let bytes = "pre中文post".as_bytes();
    let (client, worker, root) = mock(vec![
        page(0, bytes.len() as u64, &bytes[..5], json!([])),
        page(5, bytes.len() as u64, &bytes[5..], json!([])),
    ]);
    let value = execute(
        &client,
        &command(&[
            "terminal",
            "wait",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
            "--for",
            "output",
            "--contains",
            "中文",
            "--deadline-ms",
            "1000",
        ]),
    )
    .unwrap();
    assert_eq!(value["matched"], true);
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["since"], 5);
    assert!(requests
        .iter()
        .all(|r| r["cmd"] == "workbench.terminal.output"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_wait_does_not_treat_a_permission_menu_as_idle() {
    let (client, worker, root) = mock(vec![
        sessions("waiting_input", true),
        sessions("waiting_input", false),
    ]);
    let value = execute(
        &client,
        &command(&[
            "terminal",
            "wait",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
            "--for",
            "idle",
            "--deadline-ms",
            "1000",
        ]),
    )
    .unwrap();
    assert_eq!(value["matched"], true);
    assert_eq!(worker.join().unwrap().len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn screen_replay_honors_reset_resize_page_boundaries_and_fixed_end() {
    let bytes = "old\r\n\u{1b}[2J\u{1b}[H新画面".as_bytes();
    let total = bytes.len() as u64;
    let mut tail = bytes[7..].to_vec();
    tail.extend_from_slice(b"LATER");
    let (client, worker, root) = mock(vec![
        page(
            0,
            total,
            &bytes[..7],
            json!([
                {"offset":0,"rows":24,"cols":80,"reset":true,"sequence":0},
                {"offset":5,"rows":10,"cols":40,"reset":true,"sequence":1}
            ]),
        ),
        page(
            7,
            total + 5,
            &tail,
            json!([
                {"offset":5,"rows":10,"cols":40,"reset":false,"sequence":1},
                {"offset":total,"rows":12,"cols":50,"reset":false,"sequence":2}
            ]),
        ),
    ]);
    let value = execute(
        &client,
        &command(&[
            "terminal",
            "screen",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
        ]),
    )
    .unwrap();
    assert!(value["screen"].as_str().unwrap().contains("新画面"));
    assert!(!value["screen"].as_str().unwrap().contains("old"));
    assert!(!value["screen"].as_str().unwrap().contains("LATER"));
    assert_eq!(value["next_cursor"], total);
    assert_eq!(
        (value["rows"].as_u64(), value["cols"].as_u64()),
        (Some(12), Some(50))
    );
    assert_eq!(value["complete_history"], true);
    worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_split_keeps_created_identity_when_layout_conflicts() {
    let id = uuid::Uuid::now_v7();
    let (client, worker, root) = mock(vec![
        sessions("waiting_input", false),
        empty_layout(),
        json!({"ok":true,"session_id":id}),
        json!({"ok":false,"error":"layout changed in another window"}),
    ]);
    let error = execute(
        &client,
        &command(&[
            "terminal",
            "split",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
            "--request-id",
            &id.to_string(),
        ]),
    )
    .unwrap_err();
    let detail = &error.downcast_ref::<Failure>().unwrap().detail;
    assert_eq!(detail["session_created"], true);
    assert_eq!(detail["session_id"], id.to_string());
    assert_eq!(detail["request_id"], id.to_string());
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3]["layout"]["revision"], 0);
    assert_eq!(
        requests[3]["layout"]["panes"][0]["tabs"][0]["terminalTree"]["second"]["id"],
        id.to_string()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn host_connect_forwards_the_reviewed_profile_without_auto_inspect_or_retry() {
    let (client, worker, root) = mock(vec![json!({"ok":true,"session_id":"ssh1"})]);
    let file = root.join("review.json");
    let profile = json!({"id":"h1","host":"example.invalid","user":"fixture"});
    std::fs::write(&file, profile.to_string()).unwrap();
    execute(
        &client,
        &command(&[
            "host",
            "connect",
            "h1",
            "--workspace",
            "w1",
            "--reviewed-file",
            file.to_str().unwrap(),
            "--confirm",
        ]),
    )
    .unwrap();
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["reviewed_profile"], profile);
    assert_eq!(requests[0]["confirmed"], true);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn browser_stale_reference_returns_failure_without_replay() {
    let (client, worker, root) = mock(vec![]);
    worker.join().unwrap();
    let listener = crate::ipc::listen(&crate::ipc::endpoint(&root, "browser")).unwrap();
    let peer = std::thread::spawn(move || {
        let stream = listener.incoming().next().unwrap().unwrap();
        let mut seen = Value::Null;
        crate::ipc::handle(stream, |request| {
            seen = request;
            Ok(json!({"ok":false,"error":"browser_snapshot_stale"}))
        });
        seen
    });
    let error = execute(
        &client,
        &command(&[
            "browser",
            "click",
            "--workspace",
            "w1",
            "--page",
            "b1",
            "--snapshot",
            "snapshot1",
            "--element",
            "@e2",
        ]),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().unwrap().detail["code"],
        "browser_rejected"
    );
    let request = peer.join().unwrap();
    assert_eq!(request["snapshot_id"], "snapshot1");
    assert_eq!(request["workspace_id"], "w1");
    assert_eq!(request["id"], "b1");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mcp_confirmation_keeps_original_uuid_and_tool_scope_on_rejection() {
    let (client, worker, root) = mock(vec![json!({"ok":false,"error":"source changed"})]);
    let file = root.join("catalog.json");
    std::fs::write(&file, "{}").unwrap();
    let id = uuid::Uuid::now_v7();
    let error = execute(
        &client,
        &command(&[
            "mcp",
            "confirm",
            file.to_str().unwrap(),
            "--digest",
            "old-digest",
            "--entry",
            "fixture",
            "--tool",
            "read",
            "--request-id",
            &id.to_string(),
            "--confirm",
        ]),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().unwrap().detail["request_id"],
        id.to_string()
    );
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["tools"], json!(["read"]));
    assert_eq!(requests[0]["confirmed"], true);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_configuration_returns_reviewable_policy_without_host_secrets() {
    let (client, worker, root) = mock(vec![
        json!({"ok":true,"project":{"id":"p1","setup":"prepare command","setup_default":"skip","terminals":[{"spawn":{"env":{"SECRET":"private"}}}]}}),
    ]);
    let value = execute(
        &client,
        &command(&[
            "project",
            "configure",
            "p1",
            "--default-base",
            "main",
            "--confirm",
        ]),
    )
    .unwrap();
    assert_eq!(value["project"]["setup"], "prepare command");
    assert!(!value.to_string().contains("private"));
    assert!(!value.to_string().contains("terminals"));
    worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remote_target_cannot_be_selected_without_both_identity_and_data_root() {
    for args in [
        vec!["--ssh-profile", "h1", "status"],
        vec![
            "--server-id",
            "019a12a0-0000-7000-8000-000000000001",
            "status",
        ],
        vec!["--remote-data-dir", "/fixture", "status"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("velella-cli").chain(args)).is_err());
    }
}

#[test]
fn regression_screen_fails_if_a_later_geometry_page_reports_corruption() {
    let first = page(
        0,
        2,
        b"a",
        json!([{"offset":0,"rows":24,"cols":80,"reset":true,"sequence":0}]),
    );
    let mut second = page(1, 2, b"b", json!([]));
    second["geometry_error"] = json!("invalid geometry journal");
    let (client, worker, root) = mock(vec![first, second]);
    let result = execute(
        &client,
        &command(&[
            "terminal",
            "screen",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
        ]),
    );
    worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        result.is_err(),
        "a geometry error must not produce a successful screen"
    );
}

#[test]
fn regression_file_write_preserves_bom_for_exact_compare_and_write() {
    let (client, worker, root) = mock(vec![json!({"ok":true})]);
    let previous = root.join("previous.txt");
    let next = root.join("next.txt");
    std::fs::write(&previous, "\u{feff}before\r\n").unwrap();
    std::fs::write(&next, "\u{feff}after\r\n").unwrap();
    execute(
        &client,
        &command(&[
            "file",
            "write",
            "--workspace",
            "w1",
            "note.txt",
            "--file",
            next.to_str().unwrap(),
            "--previous-file",
            previous.to_str().unwrap(),
        ]),
    )
    .unwrap();
    let request = worker.join().unwrap().remove(0);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(request["previous"], "\u{feff}before\r\n");
    assert_eq!(request["content"], "\u{feff}after\r\n");
}

#[test]
fn regression_split_validates_inconsistent_tree_before_starting_a_session() {
    let mut invalid = empty_layout();
    invalid["layout"]["panes"][0]["tabs"] = json!([{"id":"terminal:s1","kind":"terminal","target":"s1","title":"Fixture","terminalTree":{"kind":"leaf","id":"other-session"}}]);
    let id = uuid::Uuid::now_v7();
    let (client, finished, root) = mock(vec![]);
    finished.join().unwrap();
    let listener = crate::ipc::listen(&client.endpoint).unwrap();
    let worker = std::thread::spawn(move || {
        let mut replies = vec![
            sessions("waiting_input", false),
            invalid,
            json!({"ok":true,"session_id":id}),
        ]
        .into_iter();
        let mut requests = Vec::new();
        loop {
            let stream = listener.incoming().next().unwrap().unwrap();
            let mut finished = false;
            crate::ipc::handle(stream, |request| {
                if request["cmd"] == "fixture.finish" {
                    finished = true;
                    return Ok(json!({"ok":true}));
                }
                requests.push(request);
                Ok(replies.next().unwrap())
            });
            if finished {
                break;
            }
        }
        requests
    });
    let result = execute(
        &client,
        &command(&[
            "terminal",
            "split",
            "--workspace",
            "w1",
            "--terminal",
            "s1",
            "--request-id",
            &id.to_string(),
        ]),
    );
    client.request(json!({"cmd":"fixture.finish"})).unwrap();
    let requests = worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(result.is_err());
    assert_eq!(
        requests.len(),
        2,
        "reject the invalid layout before starting another process"
    );
}
