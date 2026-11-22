use super::*;
use interprocess::local_socket::prelude::*;

pub(super) fn mock(replies: Vec<Value>) -> (Client, std::thread::JoinHandle<Vec<Value>>, PathBuf) {
    let root = std::env::temp_dir().join(format!("velella-cli-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir(&root).unwrap();
    let endpoint = crate::ipc::endpoint(&root, "daemon");
    let listener = crate::ipc::listen(&endpoint).unwrap();
    let worker = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let stream = listener.incoming().next().unwrap().unwrap();
            crate::ipc::handle(stream, |request| {
                requests.push(request);
                Ok(reply)
            });
        }
        requests
    });
    (
        Client {
            root: root.clone(),
            remote: None,
            endpoint,
            timeout: Duration::from_secs(3),
        },
        worker,
        root,
    )
}

fn session(program: &str) -> Value {
    json!({"ok":true,"sessions":[{"id":"s1","program":program,"status":{"pid":42,"exit":null,"input_protocol":1,"input_sequence":7}}]})
}

fn send(text: &str, expected: Option<u64>) -> TerminalCommand {
    TerminalCommand::Send {
        target: Target {
            workspace: "w1".into(),
            terminal: "s1".into(),
        },
        text: Some(text.into()),
        text_file: None,
        enter: true,
        request_id: None,
        expected_sequence: expected,
        wait_submit: None,
    }
}

#[test]
fn nested_flags_work_and_stop_requires_confirmation() {
    let cli = Cli::try_parse_from([
        "velella-cli",
        "terminal",
        "read",
        "--workspace",
        "w",
        "--terminal",
        "s",
        "--json",
        "--data-dir",
        "profile",
        "--cursor",
        "12",
    ])
    .unwrap();
    assert!(cli.args.json);
    assert_eq!(cli.args.data_dir, Some(PathBuf::from("profile")));
    assert!(Cli::try_parse_from([
        "velella-cli",
        "terminal",
        "stop",
        "--workspace",
        "w",
        "--terminal",
        "s"
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "velella-cli",
        "terminal",
        "send",
        "--workspace",
        "w",
        "--terminal",
        "s"
    ])
    .is_err());
    assert!(Cli::try_parse_from(["velella-cli", "status", "--timeout-ms", "0"]).is_err());
}

#[test]
fn output_cursor_preserves_lossless_bytes_when_utf8_splits() {
    let bytes = "中文\u{1b}[32m".as_bytes();
    let page = output_page(json!({"from":100,"total":100+bytes.len(),"data":base64::engine::general_purpose::STANDARD.encode(bytes)}), 4).unwrap();
    assert_eq!(page["next_cursor"], 104);
    assert_eq!(page["has_more"], true);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(page["data"].as_str().unwrap())
            .unwrap(),
        bytes[..4]
    );
}

#[test]
fn shell_send_is_scoped_and_uses_host_and_sequence_guards() {
    let (client, worker, root) = mock(vec![
        session("powershell"),
        json!({"ok":true,"input_sequence":8}),
    ]);
    let response = terminal(&client, &send("Write-Output '中文'", Some(7))).unwrap();
    assert_eq!(response["input_accepted"], true);
    assert_eq!(response["completion_verified"], false);
    let requests = worker.join().unwrap();
    assert_eq!(requests[1]["cmd"], "workbench.terminal.input");
    assert_eq!(requests[1]["workspace_id"], "w1");
    assert_eq!(requests[1]["session_id"], "s1");
    assert_eq!(requests[1]["expected_host_pid"], 42);
    assert_eq!(requests[1]["expected_input_sequence"], 7);
    assert_eq!(requests[1]["text"], "Write-Output '中文'");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_sequence_rejects_before_any_shell_input() {
    let (client, worker, root) = mock(vec![session("powershell")]);
    assert!(terminal(&client, &send("dangerous-if-duplicated", Some(6))).is_err());
    assert_eq!(worker.join().unwrap().len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_replay_can_recover_receipt_even_when_host_is_no_longer_available() {
    let (client, worker, root) = mock(vec![
        json!({"ok":true,"sessions":[{"id":"s1","program":"codex","status":null}]}),
        json!({"ok":true,"receipt":{"state":"submitted","request_id":"saved"}}),
    ]);
    let id = uuid::Uuid::now_v7();
    let mut command = send("prompt", Some(0));
    if let TerminalCommand::Send { request_id, .. } = &mut command {
        *request_id = Some(id);
    }
    let response = terminal(&client, &command).unwrap();
    assert_eq!(response["turn_started_confirmed"], false);
    let requests = worker.join().unwrap();
    assert_eq!(requests[1]["cmd"], "workbench.feedback.send");
    assert_eq!(requests[1]["request_id"], id.to_string());
    assert_eq!(requests[1]["expected_input_sequence"], 0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn uncertain_agent_receipt_is_failure_without_resending() {
    let (client, worker, root) = mock(vec![
        session("claude"),
        json!({"ok":true,"receipt":{"state":"uncertain","error":"paste timeout"}}),
    ]);
    let error = terminal(&client, &send("prompt", None)).unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().unwrap().detail["code"],
        "delivery_uncertain"
    );
    assert_eq!(worker.join().unwrap().len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_creation_keeps_request_uuid_and_is_not_retried() {
    let (client, worker, root) = mock(vec![json!({"ok":false,"error":"folder locked"})]);
    let id = uuid::Uuid::now_v7();
    let error = terminal(
        &client,
        &TerminalCommand::Create {
            workspace: "w1".into(),
            launch: "terminal".into(),
            track_submission: false,
            hybrid_host: None,
            request_id: Some(id),
        },
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Failure>().unwrap().detail["request_id"],
        id.to_string()
    );
    assert_eq!(worker.join().unwrap().len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn listings_do_not_expose_descriptor_environments() {
    let (client, worker, root) = mock(vec![
        json!({"ok":true,"projects":[{"id":"p","name":"project","terminals":[{"spawn":{"env":{"SECRET":"sensitive"}}}]}],"tasks":[]}),
    ]);
    let response = execute(
        &client,
        &Command::Project {
            command: ProjectCommand::List,
        },
    )
    .unwrap();
    assert!(!response.to_string().contains("sensitive"));
    assert!(!response.to_string().contains("terminals"));
    worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn input_rejects_control_sequences_and_limits_bytes() {
    assert!(input_text(Some("\u{1b}[200~unexpected"), None).is_err());
    assert!(input_text(Some(&"中".repeat(22000)), None).is_err());
    assert_eq!(
        input_text(Some("中文\nline 2"), None).unwrap(),
        "中文\nline 2"
    );
}

fn management_command(args: &[&str]) -> Command {
    Cli::try_parse_from(std::iter::once("velella-cli").chain(args.iter().copied()))
        .unwrap().args.command
}

#[test]
fn management_decisions_require_explicit_flags_and_valid_arguments() {
    for args in [
        vec!["task", "cancel", "t1"],
        vec!["swarm", "stop", "r1"],
        vec!["swarm", "accept", "r1", "--summary", "checked"],
        vec!["swarm", "plan", "confirm", "r1", "--revision", "1"],
        vec!["swarm", "gate", "r1", "--node", "n1", "--revision", "1", "--decision", "maybe", "--summary", "checked"],
        vec!["swarm", "retry", "r1", "--node", "n1"],
        vec!["task", "events", "t1", "--since", "-1"],
        vec!["swarm", "pause", "r1", "--request-id", "not-a-uuid"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("velella-cli").chain(args)).is_err());
    }
    assert!(matches!(management_command(&["agent", "list", "--workspace", "w1"]), Command::Terminal { .. }));
    assert!(matches!(management_command(&["orchestration", "list"]), Command::Swarm { .. }));
}

#[test]
fn task_creation_preserves_omitted_base_and_rejects_automatic_delivery() {
    let (client, worker, root) = mock(vec![json!({"ok":true,"task":{"id":"task1","spec":{"name":"中文任务"}}})]);
    let file = root.join("任务.yaml");
    std::fs::write(&file, "\u{feff}name: 中文任务\nagent: codex\ninstruction: Verify a useful result\non_done: review\n").unwrap();
    let id = uuid::Uuid::now_v7();
    let command = management::TaskCommand::Create { project:"p1".into(), file:file.clone(), request_id:Some(id) };
    let value = management::task(&client, &command).unwrap();
    assert_eq!(value["request_id"], id.to_string());
    let requests = worker.join().unwrap();
    assert_eq!(requests[0]["cmd"], "task.create");
    assert_eq!(requests[0]["setup"], "skip");
    assert!(requests[0]["spec"].get("base").is_none());
    assert_eq!(requests[0]["spec"]["name"], "中文任务");
    std::fs::write(&file, "name: task\nagent: codex\ninstruction: check\non_done: pr\n").unwrap();
    assert!(management::task(&client, &command).unwrap_err().to_string().contains("on_done: review"));
    std::fs::write(&file, "name: task\nagent: codex\ninstruction: check\nunknown_setting: true\n").unwrap();
    assert!(management::task(&client, &command).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_feedback_failure_preserves_receipt_and_never_resends() {
    for state in ["uncertain", "rejected"] {
        let (client, worker, root) = mock(vec![json!({"ok":true,"receipt":{"state":state,"task_id":"t1"}})]);
        let id = uuid::Uuid::now_v7();
        let command = management::TaskCommand::Feedback { id:"t1".into(), text:Some("中文意见\r\n下一行".into()), text_file:None, request_id:Some(id) };
        let error = management::task(&client, &command).unwrap_err();
        let detail = &error.downcast_ref::<Failure>().unwrap().detail;
        assert_eq!(detail["receipt"]["state"], state);
        assert_eq!(detail["request_id"], id.to_string());
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["text"], "中文意见\n下一行");
        assert_eq!(requests[0]["request_id"], id.to_string());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn management_transport_failure_reports_uncertainty_with_recovery_identity() {
    let (client, worker, root) = mock(vec![]);
    worker.join().unwrap();
    let id = uuid::Uuid::now_v7();
    let command = management_command(&["swarm", "pause", "r1", "--request-id", &id.to_string()]);
    let error = execute(&client, &command).unwrap_err();
    let detail = &error.downcast_ref::<Failure>().unwrap().detail;
    assert_eq!(detail["code"], "delivery_uncertain");
    assert_eq!(detail["id"], "r1");
    assert_eq!(detail["request_id"], id.to_string());
    let path = dunce::canonicalize(&root).unwrap();
    let error = execute(&client, &Command::Project { command:ProjectCommand::Add { path:path.clone() } }).unwrap_err();
    let detail = &error.downcast_ref::<Failure>().unwrap().detail;
    assert_eq!(detail["code"], "delivery_uncertain");
    assert_eq!(detail["path"], json!(path));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_details_include_checks_without_exposing_host_descriptors() {
    let (client, worker, root) = mock(vec![json!({"ok":true,"tasks":[{"id":"t1","spec":{"name":"task"},"verified":false,"checks":[{"command":"cargo test","passed":false}],"agent":{"spawn":{"env":{"SECRET":"private"}}}}]})]);
    let value = execute(&client, &management_command(&["task", "show", "t1"])).unwrap();
    assert_eq!(value["task"]["checks"][0]["passed"], false);
    assert!(!value.to_string().contains("private"));
    worker.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn swarm_cli_uses_real_ipc_and_durable_plan_state_machine_without_agents() {
    let root = std::env::temp_dir().join(format!("velella-management-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir(&root).unwrap();
    let endpoint = crate::ipc::endpoint(&root, "daemon");
    let listener = crate::ipc::listen(&endpoint).unwrap();
    let server_root = root.clone();
    let server = std::thread::spawn(move || {
        let hub = crate::collaboration::Hub::open(&server_root).unwrap();
        for _ in 0..11 {
            let stream = listener.incoming().next().unwrap().unwrap();
            crate::ipc::handle(stream, |request| {
                let id = request["id"].as_str().unwrap_or("");
                if request["cmd"] == "orchestration.snapshot" {
                    return Ok(json!({"ok":true,"run":hub.orchestration_public(&hub.orchestration_get(id)?)?}));
                }
                // Stand in only for the native lead's readiness. No scheduler,
                // native process, model or user profile participates in this test.
                if request["cmd"] == "orchestration.plan.confirm" {
                    let mut doc = hub.orchestration_get(id)?;
                    doc["collab_id"] = json!("synthetic-ready-lead");
                    hub.orchestration_put(&doc)?;
                }
                hub.orchestration_mutate(&request)
            });
        }
    });
    let client = Client { root:root.clone(), remote:None, endpoint, timeout:Duration::from_secs(3) };
    let id = uuid::Uuid::now_v7().to_string();
    let create = management_command(&["swarm", "create", "--workspace", "w1", "--goal", "useful result", "--acceptance", "checks pass", "--request-id", &id]);
    let first = execute(&client, &create).unwrap();
    assert_eq!(execute(&client, &create).unwrap(), first);
    let show = management_command(&["swarm", "show", &id]);
    let doc = execute(&client, &show).unwrap()["run"].clone();
    assert_eq!(doc["revision"], 0);
    let file = root.join("plan.json");
    std::fs::write(&file, doc["plan"].to_string()).unwrap();
    execute(&client, &management_command(&["swarm", "plan", "save", &id, "--revision", "0", "--file", file.to_str().unwrap()])).unwrap();
    let stale = management_command(&["swarm", "plan", "confirm", &id, "--revision", "0", "--confirm"]);
    assert!(execute(&client, &stale).is_err());
    execute(&client, &management_command(&["swarm", "plan", "confirm", &id, "--revision", "1", "--confirm"])).unwrap();
    execute(&client, &management_command(&["swarm", "pause", &id])).unwrap();
    assert_eq!(execute(&client, &show).unwrap()["run"]["state"], "paused");
    execute(&client, &management_command(&["swarm", "resume", &id])).unwrap();
    execute(&client, &management_command(&["swarm", "stop", &id, "--confirm"])).unwrap();
    assert_eq!(execute(&client, &show).unwrap()["run"]["state"], "stopping");
    server.join().unwrap();
    let reopened = crate::collaboration::Hub::open(&root).unwrap();
    let persisted = reopened.orchestration_get(&id).unwrap();
    assert_eq!(persisted["revision"], 1);
    assert_eq!(persisted["confirmed_revision"], 1);
    assert_eq!(persisted["state"], "stopping");
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
