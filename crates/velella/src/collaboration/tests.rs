use super::*;
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    root: PathBuf,
    hub: Arc<Hub>,
    leader: Value,
}
fn identity(session: &str, workspace: &str) -> Value {
    json!({"session_id":session,"host_id":session,"endpoint":format!("pipe-{session}"),"workspace_id":workspace,"workspace_path":workspace})
}
fn request(credentials: &Value, cmd: &str, args: Value) -> Value {
    let mut req = credentials.clone();
    req["cmd"] = json!(format!("collab.{cmd}"));
    req["request_id"] = json!(id());
    for (key, value) in args.as_object().unwrap() {
        req[key] = value.clone();
    }
    req
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("velella-collab-test-{}", id()));
        let hub = Arc::new(Hub::open(&root).unwrap());
        let result = hub.execute(&json!({"cmd":"collab.create","request_id":id(),"goal":"deliver a reviewed candidate","identity":identity("lead","workspace")})).unwrap();
        Self {
            root,
            hub,
            leader: result["credentials"].clone(),
        }
    }
    fn call(&self, who: &Value, cmd: &str, args: Value) -> Result<Value> {
        self.hub.execute(&request(who, cmd, args))
    }
    fn member(&self, name: &str, workspace: &str) -> Value {
        let identity = identity(name, workspace);
        let invite = self
            .call(
                &self.leader,
                "invite",
                json!({"name":name,"identity":identity}),
            )
            .unwrap();
        self.hub.execute(&json!({"cmd":"collab.join","request_id":id(),"run_id":self.leader["run_id"],"member_id":invite["member"]["id"],"invitation":invite["invitation"],"confirmed":true,"identity":identity})).unwrap()["credentials"].clone()
    }
    fn assign(&self, member: &Value, writing: bool) -> Value {
        if writing {
            self.call(&self.leader, "claim", json!({"writer":member["member_id"]}))
                .unwrap();
        }
        self.call(&self.leader,"assign",json!({"assignee":member["member_id"],"work_key":"small-change","title":"small change","instruction":"implement and verify","workspace_path":"workspace","write":writing})).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Connections must close before Windows can remove the isolated files.
        let placeholder = Hub {
            db: Mutex::new(Connection::open_in_memory().unwrap()),
            changed: Condvar::new(),
        };
        let previous = std::mem::replace(&mut self.hub, Arc::new(placeholder));
        drop(previous);
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn duplicate_send_is_durable_replay_is_acknowledged_only_explicitly() {
    let f = Fixture::new();
    let member = f.member("worker", "workspace");
    let req = request(
        &f.leader,
        "send",
        json!({"to":member["member_id"],"text":"once"}),
    );
    let first = f.hub.execute(&req).unwrap();
    assert_eq!(first, f.hub.execute(&req).unwrap());
    let mut changed = req.clone();
    changed["text"] = json!("different");
    assert!(f
        .hub
        .execute(&changed)
        .unwrap_err()
        .to_string()
        .contains("different content"));
    let receive = request(&member, "receive", json!({"since":999999}));
    let batch = f.hub.execute(&receive).unwrap();
    assert_eq!(batch["messages"].as_array().unwrap().len(), 1);
    assert_eq!(batch, f.hub.execute(&receive).unwrap());
    assert!(f
        .call(
            &member,
            "ack",
            json!({"seq":first["seq"].as_i64().unwrap()+1})
        )
        .is_err());
    // A second store instance represents a full daemon restart, not an in-memory replay.
    let reopened = Hub::open(&f.root).unwrap();
    assert_eq!(batch, reopened.execute(&receive).unwrap());
    reopened
        .execute(&request(&member, "ack", json!({"seq":first["seq"]})))
        .unwrap();
    assert!(reopened.execute(&receive).unwrap()["messages"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(first, reopened.execute(&req).unwrap());
}

#[test]
fn notification_intent_is_durable_and_input_acceptance_is_separate() {
    let f = Fixture::new();
    let member = f.member("worker", "workspace");
    let intent = f.call(&f.leader, "notify", json!({"to":member["member_id"],"text":"inspect the candidate"})).unwrap();
    assert_eq!(intent["notification"]["state"], "intent");
    assert!(!intent.to_string().contains(member["token"].as_str().unwrap()));
    assert!(f.call(&member, "input.accepted", json!({"notification_id":intent["notification"]["id"]})).is_err());
    let accepted = f.call(&f.leader, "input.accepted", json!({"notification_id":intent["notification"]["id"]})).unwrap();
    assert_eq!(accepted["notification"]["state"], "input_accepted");
    assert!(f.call(&f.leader, "input.accepted", json!({"notification_id":intent["notification"]["id"]})).is_err());
    let inbox = f.call(&member, "receive", json!({})).unwrap();
    assert_eq!(inbox["messages"][0]["kind"], "notification_intent");
}

#[test]
fn join_requires_exact_native_identity_and_explicit_confirmation() {
    let f = Fixture::new();
    let identity = identity("existing", "workspace");
    let invite = f
        .call(
            &f.leader,
            "invite",
            json!({"name":"existing","identity":identity}),
        )
        .unwrap();
    let mut req = json!({"cmd":"collab.join","request_id":id(),"run_id":f.leader["run_id"],"member_id":invite["member"]["id"],"invitation":invite["invitation"],"identity":identity});
    assert!(f.hub.execute(&req).is_err());
    req["confirmed"] = json!(true);
    req["identity"]["host_id"] = json!("replacement-host");
    assert!(f.hub.execute(&req).is_err());
    req["identity"]["host_id"] = json!("existing");
    let joined = f.hub.execute(&req).unwrap();
    assert_eq!(joined, f.hub.execute(&req).unwrap());
    req["request_id"] = json!(id());
    assert!(f.hub.execute(&req).is_err());
    let public = f.call(&f.leader, "status", json!({})).unwrap();
    assert!(!public
        .to_string()
        .contains(joined["credentials"]["token"].as_str().unwrap()));
}

#[test]
fn coordinator_only_delegation_and_six_member_cap() {
    let f = Fixture::new();
    let worker = f.member("one", "workspace");
    assert!(f
        .call(
            &worker,
            "invite",
            json!({"name":"nested","identity":identity("nested","workspace")})
        )
        .is_err());
    for name in ["two", "three", "four", "five"] {
        f.member(name, "workspace");
    }
    assert!(f
        .call(
            &f.leader,
            "invite",
            json!({"name":"six","identity":identity("six","workspace")})
        )
        .is_err());
    assert!(f
        .call(&worker, "assign", json!({"assignee":worker["member_id"]}))
        .is_err());
    let mut forged = worker.clone();
    forged["member_id"] = f.leader["member_id"].clone();
    assert!(f.call(&forged, "status", json!({})).is_err());
}

#[test]
fn stale_reports_wrong_workspace_and_unchanged_retries_are_rejected() {
    let f = Fixture::new();
    let worker = f.member("worker", "workspace");
    assert!(f
        .call(
            &f.leader,
            "assign",
            json!({"assignee":worker["member_id"],"workspace_path":"elsewhere"})
        )
        .is_err());
    let first = f.assign(&worker, false);
    let work = first["work"]["id"].clone();
    let report =
        json!({"attempt_id":first["attempt"]["id"],"outcome":"failed","summary":"missing fixture"});
    f.call(&worker, "report", report.clone()).unwrap();
    let mut retry =
        json!({"work_id":work,"assignee":worker["member_id"],"workspace_path":"workspace"});
    assert!(f.call(&f.leader, "assign", retry.clone()).is_err());
    retry["new_evidence"] = json!("fixture now exists");
    let second = f.call(&f.leader, "assign", retry.clone()).unwrap();
    assert!(f
        .call(&worker, "report", report)
        .unwrap_err()
        .to_string()
        .contains("stale attempt"));
    f.call(
        &worker,
        "report",
        json!({"attempt_id":second["attempt"]["id"],"outcome":"failed","summary":"still fails"}),
    )
    .unwrap();
    assert!(f
        .call(&f.leader, "assign", retry)
        .unwrap_err()
        .to_string()
        .contains("two attempts"));
    for (key, title) in [
        ("small-change", "different wording"),
        ("replacement-key", "small change"),
    ] {
        assert!(f.call(&f.leader,"assign",json!({"assignee":worker["member_id"],"work_key":key,"title":title,"instruction":"implement and verify","workspace_path":"workspace"})).unwrap_err().to_string().contains("work already exists"));
    }
}

#[test]
fn writer_handover_requires_release_and_review_binds_fixed_candidate() {
    let f = Fixture::new();
    let writer = f.member("writer", "workspace");
    let reviewer = f.member("reviewer", "workspace");
    let assigned = f.assign(&writer, true);
    let work = assigned["work"]["id"].clone();
    assert!(f
        .call(&f.leader, "claim", json!({"writer":reviewer["member_id"]}))
        .is_err());
    assert!(f.call(&writer, "release", json!({})).is_err());
    assert!(f
        .call(
            &f.leader,
            "accept",
            json!({"work_id":work,"summary":"not yet"})
        )
        .is_err());
    f.call(
        &writer,
        "started",
        json!({"attempt_id":assigned["attempt"]["id"]}),
    )
    .unwrap();
    let candidate = json!({"workspace_path":"workspace","revision":"fixed-sha"});
    f.call(&writer,"report",json!({"attempt_id":assigned["attempt"]["id"],"outcome":"completed","summary":"ready","candidate":candidate})).unwrap();
    assert!(f
        .call(
            &f.leader,
            "accept",
            json!({"work_id":work,"summary":"not yet"})
        )
        .is_err());
    assert!(f
        .call(
            &f.leader,
            "review.request",
            json!({"work_id":work,"reviewer":writer["member_id"]})
        )
        .is_err());
    let review = f
        .call(
            &f.leader,
            "review.request",
            json!({"work_id":work,"reviewer":reviewer["member_id"]}),
        )
        .unwrap();
    let mut report = json!({"review_id":review["review"]["id"],"verdict":"passed","summary":"independently checked","candidate":{"workspace_path":"workspace","revision":"wrong-sha"}});
    assert!(f.call(&reviewer, "review.report", report.clone()).is_err());
    report["candidate"] = candidate;
    f.call(&reviewer, "review.report", report).unwrap();
    f.call(
        &f.leader,
        "accept",
        json!({"work_id":work,"summary":"verified"}),
    )
    .unwrap();
    assert!(f
        .call(
            &f.leader,
            "end",
            json!({"outcome":"accepted","summary":"done"})
        )
        .is_err());
    f.call(&writer, "release", json!({})).unwrap();
    f.call(&f.leader, "claim", json!({"writer":reviewer["member_id"]}))
        .unwrap();
    f.call(&reviewer, "release", json!({})).unwrap();
    let ended = f
        .call(
            &f.leader,
            "end",
            json!({"outcome":"accepted","summary":"done"}),
        )
        .unwrap();
    assert_eq!(ended["terminals_preserved"], true);
    assert!(f
        .call(
            &writer,
            "send",
            json!({"to":reviewer["member_id"],"text":"late"})
        )
        .is_err());
    assert!(
        !f.call(&reviewer, "receive", json!({})).unwrap()["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn waiter_is_woken_after_commit_and_does_not_block_other_requests() {
    let f = Fixture::new();
    let worker = f.member("worker", "workspace");
    let receiver = f.hub.clone();
    let credentials = worker.clone();
    let waiting = std::thread::spawn(move || {
        receiver
            .execute(&request(&credentials, "receive", json!({"wait_ms":25000})))
            .unwrap()
    });
    let start = Instant::now();
    // Either ordering is valid: pre-wait messages must also be found.
    f.call(
        &f.leader,
        "send",
        json!({"to":worker["member_id"],"text":"wake"}),
    )
    .unwrap();
    let result = waiting.join().unwrap();
    assert_eq!(result["messages"][0]["body"]["text"], "wake");
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancellation_and_leave_do_not_release_an_active_writer_implicitly() {
    let f = Fixture::new();
    let writer = f.member("writer", "workspace");
    let assigned = f.assign(&writer, true);
    assert!(f.call(&writer, "leave", json!({})).is_err());
    f.call(
        &f.leader,
        "cancel",
        json!({"work_id":assigned["work"]["id"],"reason":"stop work"}),
    )
    .unwrap();
    assert!(f
        .call(&f.leader, "end", json!({"outcome":"cancelled"}))
        .is_err());
    assert!(f
        .call(
            &writer,
            "report",
            json!({"attempt_id":assigned["attempt"]["id"],"outcome":"completed","summary":"late"})
        )
        .is_err());
    assert!(f
        .call(&writer, "leave", json!({}))
        .unwrap_err()
        .to_string()
        .contains("explicitly release"));
    f.call(&writer, "release", json!({})).unwrap();
    assert_eq!(
        f.call(&writer, "leave", json!({})).unwrap()["terminals_preserved"],
        true
    );
    f.call(&f.leader, "end", json!({"outcome":"cancelled"}))
        .unwrap();
}
