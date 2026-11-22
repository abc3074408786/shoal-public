use super::*;

fn parse(args: &[&str]) -> Args {
    Cli::try_parse_from(std::iter::once("velella-cli").chain(args.iter().copied()))
        .unwrap().args
}

fn offline_client() -> Client {
    let root = std::env::temp_dir().join(format!("velella-offline-{}", uuid::Uuid::now_v7()));
    Client { endpoint: crate::ipc::endpoint(&root, "daemon"), root, remote: None, timeout: Duration::from_millis(20) }
}

#[test]
fn collaboration_flags_work_at_nested_positions_without_clap_collisions() {
    for args in [
        vec!["--json", "collab", "--credentials", "member.json", "--request-id", "stable", "send", "--to", "m", "--text", "hello"],
        vec!["collaboration", "send", "--to", "m", "--text", "hello", "--credentials", "member.json", "--request-id", "stable", "--json"],
    ] {
        let args = parse(&args);
        assert!(args.json);
        match args.command {
            Command::Collab { credentials, request_id, command: crate::collaboration::cli::Command::Send { .. } } => {
                assert_eq!(credentials, Some(PathBuf::from("member.json")));
                assert_eq!(request_id.as_deref(), Some("stable"));
            }
            _ => panic!("wrong collaboration command"),
        }
    }
    Cli::command().debug_assert();
}

#[test]
fn bundled_guides_and_discovery_work_offline_without_creating_a_profile() {
    let client = offline_client();
    for name in ["velella-cli", "velella-collaboration"] {
        let result = execute(&client, &parse(&["skills", "get", name]).command).unwrap();
        assert!(result["guide"].as_str().unwrap().contains("velella-cli"));
        assert_eq!(result["source"], "bundled-cli");
        assert_eq!(result["cli_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(result["guide_revision"], GUIDE_REVISION);
    }
    let catalog = execute(&client, &parse(&["skills", "bundled"]).command).unwrap();
    assert_eq!(catalog["skills"].as_array().unwrap().len(), 2);
    let error = execute(&client, &parse(&["skills", "get", "unavailable"]).command).unwrap_err();
    assert_eq!(error.downcast_ref::<Failure>().unwrap().detail["code"], "unknown_bundled_skill");
    let schema = execute(&client, &Command::Schema).unwrap();
    let commands = schema["command"]["commands"].as_array().unwrap();
    let collab = commands.iter().find(|c| c["name"] == "collab").unwrap();
    for name in ["send", "receive", "assign", "report", "review-report", "request"] {
        assert!(collab["commands"].as_array().unwrap().iter().any(|c| c["name"] == name));
    }
    assert_eq!(schema["bundled_skills"], catalog);
    assert!(matches!(parse(&["skills", "installed"]).command, Command::Skills { command: services::SkillsCommand::List }));
    assert!(!client.root.exists());
}

#[test]
fn collaboration_requires_stable_id_and_keeps_mcp_stream_separate() {
    let client = offline_client();
    let session = uuid::Uuid::now_v7().to_string();
    let error = execute(&client, &parse(&["collab", "create", "--session", &session, "--goal", "no write"]).command).unwrap_err();
    assert!(format!("{error:#}").contains("mutations require --request-id"));
    let error = execute(&client, &parse(&["collab", "mcp"]).command).unwrap_err();
    assert!(error.to_string().contains("velella-runtime collab mcp"));
    assert!(!client.root.exists());
}

#[test]
fn collaboration_request_uses_public_timeout_and_never_retries() {
    use interprocess::local_socket::prelude::*;
    let client = offline_client();
    let listener = crate::ipc::listen(&client.endpoint).unwrap();
    let worker = std::thread::spawn(move || {
        crate::ipc::handle(listener.accept().unwrap(), |request| {
            assert_eq!(request["cmd"], "collab.create");
            assert_eq!(request["request_id"], "fixed-timeout-request");
            std::thread::sleep(Duration::from_millis(200));
            Ok(json!({"ok":true}))
        });
    });
    let session = uuid::Uuid::now_v7().to_string();
    let error = execute(&client, &parse(&["collab", "create", "--session", &session, "--goal", "timeout fixture", "--request-id", "fixed-timeout-request"]).command).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("timed out"), "{message}");
    assert!(message.contains("same request ID"));
    worker.join().unwrap();
}
