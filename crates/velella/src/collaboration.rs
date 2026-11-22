//! Durable local collaboration. No host, model, filesystem or network I/O is
//! performed while the store lock is held. The desktop adapter resolves native
//! session identity before entering this module. A report is never acceptance.
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};
pub mod cli;
pub(crate) mod mcp;
pub(crate) mod mcp_catalog;
pub(crate) mod context_usage;
pub(crate) mod providers;
pub(crate) mod orchestration;
pub(crate) mod source;

pub struct Hub {
    db: Mutex<Connection>,
    changed: Condvar,
}

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    let text = value[key]
        .as_str()
        .with_context(|| format!("{key} required"))?;
    ensure!(
        !text.trim().is_empty() && text.len() <= 16384,
        "invalid {key}"
    );
    Ok(text)
}
fn load(db: &Connection, kind: &str, key: &str) -> Result<Value> {
    let value: String = db
        .query_row(
            "SELECT document FROM objects WHERE kind=?1 AND id=?2",
            params![kind, key],
            |r| r.get(0),
        )
        .optional()?
        .with_context(|| format!("{kind} not found"))?;
    Ok(serde_json::from_str(&value)?)
}
fn put(db: &Connection, kind: &str, value: &Value) -> Result<()> {
    db.execute("INSERT INTO objects(kind,id,run_id,document) VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET document=excluded.document",
        params![kind,field(value,"id")?,value["run_id"].as_str().unwrap_or(""),value.to_string()])?;
    Ok(())
}
fn list(db: &Connection, kind: &str, run: &str) -> Result<Vec<Value>> {
    let mut statement =
        db.prepare("SELECT document FROM objects WHERE kind=?1 AND run_id=?2 ORDER BY rowid")?;
    let values = statement
        .query_map(params![kind, run], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    values
        .into_iter()
        .map(|v| Ok(serde_json::from_str(&v)?))
        .collect()
}
fn public(mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.remove("token");
        map.remove("invitation");
    }
    value
}
fn authenticate(db: &Connection, req: &Value, active: bool) -> Result<(Value, Value)> {
    let run = load(db, "run", field(req, "run_id")?)?;
    let member = load(db, "member", field(req, "member_id")?)?;
    ensure!(
        member["run_id"] == run["id"] && member["token"].as_str() == Some(field(req, "token")?),
        "member credential mismatch"
    );
    if active {
        ensure!(run["state"] == "active", "collaboration has ended");
        ensure!(
            member["state"] == "joined",
            "member has not joined or has left"
        );
    }
    Ok((run, member))
}
fn coordinator(run: &Value, member: &Value) -> Result<()> {
    ensure!(
        run["coordinator"] == member["id"],
        "only the coordinator may perform this action"
    );
    Ok(())
}
fn member_in(db: &Connection, run: &Value, key: &str) -> Result<Value> {
    let member = load(db, "member", key)?;
    ensure!(
        member["run_id"] == run["id"] && member["state"] == "joined",
        "recipient is not a joined member"
    );
    Ok(member)
}
fn message(
    db: &Connection,
    run: &Value,
    from: &str,
    to: &str,
    kind: &str,
    body: Value,
) -> Result<i64> {
    db.execute("INSERT INTO messages(run_id,sender,recipient,kind,body,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        params![field(run,"id")?,from,to,kind,body.to_string(),now()])?;
    Ok(db.last_insert_rowid())
}
fn active_work(db: &Connection, run: &str, member: &str, writing_only: bool) -> Result<bool> {
    Ok(list(db, "work", run)?.iter().any(|w| {
        w["assignee"] == member
            && (!writing_only || w["write"] == true)
            && ["assigned", "input_accepted", "turn_started"]
                .contains(&w["state"].as_str().unwrap_or(""))
    }))
}

impl Hub {
    /// Lookup uses the original wire request, before external identity/liveness
    /// checks. Lost success replies remain recoverable after the host exits.
    pub(crate) fn replay(&self, wire: &Value) -> Result<Option<Value>> {
        let Some(request_id) = wire["request_id"].as_str() else {
            return Ok(None);
        };
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        replay(&db, request_id, &wire.to_string())
    }
    pub(crate) fn actor_identity(&self, req: &Value) -> Result<Value> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        Ok(authenticate(&db, req, false)?.1["identity"].clone())
    }

    /// Verify coordinator authority without entering any native host. Desktop
    /// launch and notification adapters call this before their external work.
    pub(crate) fn coordinator_identity(&self, req: &Value) -> Result<Value> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        let (run, member) = authenticate(&db, req, true)?;
        coordinator(&run, &member)?;
        Ok(member["identity"].clone())
    }

    pub(crate) fn coordinator_recipient(&self, req: &Value, recipient: &str) -> Result<Value> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        let (run, member) = authenticate(&db, req, true)?;
        coordinator(&run, &member)?;
        member_in(&db, &run, recipient)
    }

    /// Resolve the work inside this run before the adapter reads any source.
    pub(crate) fn candidate_work(&self, req: &Value) -> Result<Option<Value>> {
        let cmd=field(req,"cmd")?;
        // Read-only mailbox/status/ack operations retain their own ended-run
        // semantics. A candidate check must not impose active-run authorization
        // on unrelated commands before execute_resolved can handle them.
        if !(matches!(cmd,"collab.candidate"|"collab.review.request"|"collab.accept")
            || (cmd=="collab.report" && req["outcome"]=="completed")
            || (cmd=="collab.review.report" && req["verdict"]=="passed")) {return Ok(None)}
        let db=self.db.lock().map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        let (run,actor)=authenticate(&db,req,true)?;
        let work=match cmd {
            "collab.candidate" | "collab.review.request" | "collab.accept" => load(&db,"work",field(req,"work_id")?)?,
            "collab.report" if req["outcome"]=="completed" => {
                let attempt=load(&db,"attempt",field(req,"attempt_id")?)?;
                ensure!(attempt["member_id"]==actor["id"],"attempt belongs to another member");
                load(&db,"work",field(&attempt,"work_id")?)?
            }
            "collab.review.report" if req["verdict"]=="passed" => {
                let review=load(&db,"review",field(req,"review_id")?)?;
                ensure!(review["reviewer"]==actor["id"],"review belongs to another member");
                load(&db,"work",field(&review,"work_id")?)?
            }
            _=>return Ok(None),
        };
        ensure!(work["run_id"]==run["id"],"work belongs to another collaboration");
        Ok(Some(work))
    }

    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let db = Connection::open(root.join("collaboration.db"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 1,
            "collaboration database is newer than this runtime"
        );
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS objects(kind TEXT NOT NULL,id TEXT NOT NULL,run_id TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(kind,id));
            CREATE INDEX IF NOT EXISTS objects_by_run ON objects(kind,run_id);
            CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,request TEXT NOT NULL,response TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS messages(seq INTEGER PRIMARY KEY AUTOINCREMENT,run_id TEXT NOT NULL,sender TEXT NOT NULL,recipient TEXT NOT NULL,kind TEXT NOT NULL,body TEXT NOT NULL,created_at TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS mailbox ON messages(run_id,recipient,seq);
            CREATE TABLE IF NOT EXISTS writers(workspace TEXT PRIMARY KEY,run_id TEXT NOT NULL,member_id TEXT NOT NULL);
            PRAGMA user_version=1;")?;
        Ok(Self {
            db: Mutex::new(db),
            changed: Condvar::new(),
        })
    }

    /// Desktop-only entry. create/join require a resolved `identity` supplied by
    /// the adapter, never trusted from an unvalidated wire request.
    #[cfg(test)]
    pub(crate) fn execute(&self, req: &Value) -> Result<Value> {
        self.execute_resolved(req, req)
    }

    pub(crate) fn execute_resolved(&self, wire: &Value, req: &Value) -> Result<Value> {
        let cmd = field(req, "cmd")?;
        if matches!(cmd, "collab.plan.submit" | "collab.revision.propose") { mcp_catalog::verify_plan(&req["plan"])?; }
        if cmd == "collab.receive" {
            return self.receive(req);
        }
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        if cmd == "collab.status" {
            let (run, _) = authenticate(&db, req, false)?;
            let managed=orchestration::find(&db,field(req,"run_id")?)?.map(|d|json!({"state":d["state"],"revision":d["revision"],"plan":d["plan"]}));
            return Ok(
                json!({"ok":true,"run":run,"orchestration":managed,"members":list(&db,"member",field(req,"run_id")?)?.into_iter().map(public).collect::<Vec<_>>(),"work":list(&db,"work",field(req,"run_id")?)?,"attempts":list(&db,"attempt",field(req,"run_id")?)?,"reviews":list(&db,"review",field(req,"run_id")?)?,"notifications":list(&db,"notification",field(req,"run_id")?)?}),
            );
        }
        let request_id = field(req, "request_id")?;
        ensure!(request_id.len() <= 200, "request_id too long");
        let serialized = wire.to_string();
        if let Some(response) = replay(&db, request_id, &serialized)? {
            return Ok(response);
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = mutate(&tx, req)?;
        tx.execute(
            "INSERT INTO requests(id,request,response) VALUES(?1,?2,?3)",
            params![request_id, serialized, result.to_string()],
        )?;
        tx.commit()?;
        // One mutex protects predicate inspection and waiting. Commit precedes
        // notification, so a receiver cannot miss a message between the two.
        self.changed.notify_all();
        Ok(result)
    }

    fn receive(&self, req: &Value) -> Result<Value> {
        let wait = req["wait_ms"].as_u64().unwrap_or(0).min(25000);
        let deadline = Instant::now() + Duration::from_millis(wait);
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("collaboration lock poisoned"))?;
        loop {
            let (run, mut member) = authenticate(&db, req, false)?;
            ensure!(
                member["state"] == "joined" || member["state"] == "left",
                "invitation must be accepted first"
            );
            let ack = member["ack"].as_i64().unwrap_or(0);
            // The server cursor, not a client-supplied 'since', controls replay.
            let values = {
                let mut statement = db.prepare("SELECT seq,sender,kind,body,created_at FROM messages WHERE run_id=?1 AND recipient=?2 AND seq>?3 ORDER BY seq LIMIT 64")?;
                let rows = statement
                    .query_map(
                        params![field(&run, "id")?, field(&member, "id")?, ack],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, String>(4)?,
                            ))
                        },
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().map(|(seq,sender,kind,body,created)| Ok(json!({"seq":seq,"sender":sender,"kind":kind,"body":serde_json::from_str::<Value>(&body)?,"created_at":created}))).collect::<Result<Vec<_>>>()?
            };
            if !values.is_empty()
                || Instant::now() >= deadline
                || run["state"] != "active"
                || member["state"] == "left"
            {
                if let Some(last) = values.last() {
                    member["delivered"] = json!(member["delivered"]
                        .as_i64()
                        .unwrap_or(0)
                        .max(last["seq"].as_i64().unwrap()));
                    put(&db, "member", &member)?;
                }
                return Ok(json!({"ok":true,"messages":values,"acked":ack,"state":run["state"]}));
            }
            let duration = deadline.saturating_duration_since(Instant::now());
            (db, _) = self
                .changed
                .wait_timeout(db, duration)
                .map_err(|_| anyhow::anyhow!("collaboration wait poisoned"))?;
        }
    }
}

fn replay(db: &Connection, request_id: &str, serialized: &str) -> Result<Option<Value>> {
    if let Some((previous, response)) = db
        .query_row(
            "SELECT request,response FROM requests WHERE id=?1",
            [request_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure!(
            serialized == previous,
            "request_id reused with different content"
        );
        return Ok(Some(serde_json::from_str(&response)?));
    }
    Ok(None)
}

fn mutate(db: &Connection, req: &Value) -> Result<Value> {
    let cmd = field(req, "cmd")?;
    if matches!(cmd,"collab.plan.submit"|"collab.revision.propose"|"collab.retry.propose") { return orchestration::submit(db,req); }
    if cmd == "collab.create" {
        let identity = &req["identity"];
        for key in [
            "session_id",
            "host_id",
            "endpoint",
            "workspace_id",
            "workspace_path",
        ] {
            field(identity, key)?;
        }
        let run_id = id();
        let member_id = id();
        let token = id();
        let run = json!({"id":run_id,"run_id":run_id,"goal":field(req,"goal")?,"workspace_id":identity["workspace_id"],"workspace_path":identity["workspace_path"],"source":req["source"],"baseline":req["baseline"],"coordinator":member_id,"state":"active","created_at":now()});
        let member = json!({"id":member_id,"run_id":run_id,"name":req["name"].as_str().unwrap_or("coordinator"),"role":"coordinator","state":"joined","identity":identity,"token":token,"ack":0,"delivered":0});
        put(db, "run", &run)?;
        put(db, "member", &member)?;
        return Ok(
            json!({"ok":true,"run":run,"member":public(member),"credentials":{"run_id":run_id,"member_id":member_id,"token":token}}),
        );
    }
    if cmd == "collab.join" {
        let run = load(db, "run", field(req, "run_id")?)?;
        ensure!(run["state"] == "active", "collaboration has ended");
        let mut member = load(db, "member", field(req, "member_id")?)?;
        ensure!(
            member["run_id"] == run["id"]
                && member["state"] == "invited"
                && member["invitation"].as_str() == Some(field(req, "invitation")?),
            "invalid or consumed invitation"
        );
        ensure!(
            req["confirmed"] == true,
            "explicit join confirmation required"
        );
        let expected = &member["identity"];
        let actual = &req["identity"];
        for key in [
            "session_id",
            "host_id",
            "endpoint",
            "workspace_id",
            "workspace_path",
        ] {
            ensure!(
                field(expected, key)? == field(actual, key)?,
                "invitation {key} mismatch"
            );
        }
        member["state"] = json!("joined");
        member["token"] = json!(id());
        member["invitation"] = Value::Null;
        put(db, "member", &member)?;
        message(
            db,
            &run,
            field(&member, "id")?,
            field(&run, "coordinator")?,
            "joined",
            json!({"member_id":member["id"]}),
        )?;
        return Ok(
            json!({"ok":true,"member":public(member.clone()),"credentials":{"run_id":run["id"],"member_id":member["id"],"token":member["token"]}}),
        );
    }
    let (mut run, mut actor) = authenticate(db, req, !matches!(cmd, "collab.ack"))?;
    let run_id = field(&run, "id")?.to_owned();
    let actor_id = field(&actor, "id")?.to_owned();
    match cmd {
        "collab.invite" => {
            coordinator(&run, &actor)?;
            let members = list(db, "member", &run_id)?;
            ensure!(
                members.iter().filter(|m| m["state"] != "left").count() < 6,
                "at most one coordinator and five members"
            );
            let identity = &req["identity"];
            for key in [
                "session_id",
                "host_id",
                "endpoint",
                "workspace_id",
                "workspace_path",
            ] {
                field(identity, key)?;
            }
            ensure!(
                !members.iter().any(|m| m["state"] != "left"
                    && m["identity"]["session_id"] == identity["session_id"]),
                "session already enrolled or invited"
            );
            let member = json!({"id":id(),"run_id":run_id,"name":field(req,"name")?,"role":"member","state":"invited","identity":identity,"invitation":id(),"ack":0,"delivered":0});
            put(db, "member", &member)?;
            Ok(json!({"ok":true,"member":public(member.clone()),"invitation":member["invitation"]}))
        }
        "collab.handoff" => {
            coordinator(&run,&actor)?;
            let mut member=member_in(db,&run,field(req,"target_member_id")?)?;
            let identity=&req["identity"];
            for key in ["session_id","host_id","endpoint","workspace_id","workspace_path"] {field(identity,key)?;}
            ensure!(member["identity"]["session_id"]!=identity["session_id"],"handoff requires a different native session");
            ensure!(member["identity"]["workspace_id"]==identity["workspace_id"] && member["identity"]["workspace_path"]==identity["workspace_path"],"handoff cannot change a member workspace");
            ensure!(!list(db,"member",&run_id)?.iter().any(|other|other["id"]!=member["id"]&&other["state"]!="left"&&other["identity"]["session_id"]==identity["session_id"]),"replacement session is already enrolled");
            let package=&req["package"];
            ensure!(package.is_object(),"handoff package required");
            let previous=member["identity"].clone();
            member["identity"]=identity.clone();
            member["handoff"]=json!({"from":previous,"package":package,"at":now()});
            put(db,"member",&member)?;
            message(db,&run,&actor_id,field(&member,"id")?,"handoff",json!({"member_id":member["id"],"package":package}))?;
            Ok(json!({"ok":true,"member":public(member.clone()),"credentials":{"run_id":run["id"],"member_id":member["id"],"token":member["token"]}}))
        }
        "collab.send" => {
            let to = field(req, "to")?;
            member_in(db, &run, to)?;
            let text = field(req, "text")?;
            let seq = message(db, &run, &actor_id, to, "message", json!({"text":text}))?;
            Ok(json!({"ok":true,"seq":seq,"state":"durable"}))
        }
        "collab.notify" => {
            coordinator(&run, &actor)?;
            let recipient = member_in(db, &run, field(req, "to")?)?;
            let notification = json!({"id":id(),"run_id":run_id,"sender":actor_id,"recipient":recipient["id"],"text":field(req,"text")?,"state":"intent","created_at":now()});
            put(db, "notification", &notification)?;
            let seq = message(db, &run, &actor_id, field(&recipient,"id")?, "notification_intent", notification.clone())?;
            Ok(json!({"ok":true,"seq":seq,"notification":notification,"recipient":public(recipient)}))
        }
        "collab.input.accepted" => {
            coordinator(&run, &actor)?;
            let mut notification = load(db, "notification", field(req,"notification_id")?)?;
            ensure!(notification["run_id"]==run_id && notification["state"]=="intent", "notification is not pending input");
            notification["state"] = json!("input_accepted");
            notification["accepted_at"] = json!(now());
            put(db,"notification",&notification)?;
            Ok(json!({"ok":true,"notification":notification}))
        }
        "collab.ack" => {
            let seq = req["seq"].as_i64().context("seq required")?;
            ensure!(
                seq >= 0 && seq <= actor["delivered"].as_i64().unwrap_or(0),
                "cannot acknowledge undelivered messages"
            );
            actor["ack"] = json!(seq.max(actor["ack"].as_i64().unwrap_or(0)));
            put(db, "member", &actor)?;
            Ok(json!({"ok":true,"acked":actor["ack"]}))
        }
        "collab.claim" => {
            coordinator(&run, &actor)?;
            let member = member_in(db, &run, field(req, "writer")?)?;
            let workspace = field(&member["identity"], "workspace_path")?;
            let previous: Option<(String, String)> = db
                .query_row(
                    "SELECT run_id,member_id FROM writers WHERE workspace=?1",
                    [workspace],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            ensure!(
                previous.is_none()
                    || previous == Some((run_id.clone(), field(&member, "id")?.into())),
                "workspace already has a writer; current writer must release it"
            );
            db.execute(
                "INSERT OR IGNORE INTO writers(workspace,run_id,member_id) VALUES(?1,?2,?3)",
                params![workspace, run_id, field(&member, "id")?],
            )?;
            Ok(json!({"ok":true,"writer":member["id"],"workspace":workspace}))
        }
        "collab.release" => {
            ensure!(
                !active_work(db, &run_id, &actor_id, true)?,
                "report or cancel current writing work before releasing ownership"
            );
            db.execute(
                "DELETE FROM writers WHERE run_id=?1 AND member_id=?2",
                params![run_id, actor_id],
            )?;
            Ok(json!({"ok":true,"released":true}))
        }
        "collab.assign" => {
            coordinator(&run, &actor)?;
            let member = member_in(db, &run, field(req, "assignee")?)?;
            let assignee = field(&member, "id")?;
            ensure!(
                !active_work(db, &run_id, assignee, false)?,
                "member already has an active attempt"
            );
            let workspace = field(&member["identity"], "workspace_path")?;
            ensure!(
                field(req, "workspace_path")? == workspace,
                "assignment workspace mismatch"
            );
            let writing = req["write"] == true;
            if writing {
                let holder: Option<String> = db
                    .query_row(
                        "SELECT member_id FROM writers WHERE workspace=?1 AND run_id=?2",
                        params![workspace, run_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                ensure!(
                    holder.as_deref() == Some(assignee),
                    "assignee does not own the workspace writing lease"
                );
            }
            let mut work = if let Some(key) = req["work_id"].as_str() {
                let mut previous = load(db, "work", key)?;
                ensure!(
                    previous["run_id"] == run["id"] && previous["state"] == "failed",
                    "only failed work may be retried"
                );
                ensure!(
                    previous["attempt_count"].as_u64().unwrap_or(0) < 2,
                    "two attempts exhausted; a decision is required"
                );
                field(req, "new_evidence")?;
                ensure!(
                    previous["workspace_path"] == workspace && previous["write"] == writing,
                    "retry cannot change the work scope or writing requirement"
                );
                previous["attempt_count"] = json!(2);
                previous
            } else {
                let key = field(req, "work_key")?;
                let title = field(req, "title")?;
                let instruction = field(req, "instruction")?;
                ensure!(
                    !list(db, "work", &run_id)?
                        .iter()
                        .any(|w| w["work_key"] == key
                            || (w["workspace_path"] == workspace
                                && w["title"] == title
                                && w["instruction"] == instruction)),
                    "work already exists; retain its work_id for retries and its two-attempt limit"
                );
                json!({"id":id(),"run_id":run_id,"work_key":key,"title":title,"instruction":instruction,"attempt_count":1})
            };
            let attempt = json!({"id":id(),"run_id":run_id,"work_id":work["id"],"member_id":assignee,"state":"assigned","new_evidence":req["new_evidence"],"created_at":now()});
            work["assignee"] = member["id"].clone();
            work["workspace_path"] = json!(workspace);
            work["write"] = json!(writing);
            work["attempt_id"] = attempt["id"].clone();
            work["state"] = json!("assigned");
            work["candidate"] = Value::Null;
            if let Some(doc)=orchestration::find(db,&run_id)?.filter(|d|d["schema_version"]==2) {
                let node=doc["nodes"].as_array().context("confirmed nodes missing")?.iter().find(|n|n["id"]==work["work_key"]).context("step is outside the confirmed plan")?;
                if work["evidence_contract"].is_null() {
                    work["evidence_contract"]=json!({"version":1,"revision":doc["confirmed_revision"],"checks":node["checks"],"deliverables":node["deliverables"]});
                }
            }
            put(db, "attempt", &attempt)?;
            put(db, "work", &work)?;
            message(
                db,
                &run,
                &actor_id,
                assignee,
                "assignment",
                json!({"work":work,"attempt":attempt}),
            )?;
            Ok(json!({"ok":true,"work":work,"attempt":attempt}))
        }
        "collab.started" | "collab.report" => {
            let mut attempt = load(db, "attempt", field(req, "attempt_id")?)?;
            let mut work = load(db, "work", field(&attempt, "work_id")?)?;
            ensure!(
                work["run_id"] == run["id"]
                    && work["attempt_id"] == attempt["id"]
                    && attempt["member_id"] == actor["id"],
                "stale attempt or wrong member"
            );
            ensure!(
                ["assigned", "input_accepted", "turn_started"]
                    .contains(&work["state"].as_str().unwrap_or("")),
                "attempt already reported or cancelled"
            );
            if cmd == "collab.started" {
                // Explicit member acknowledgement proves it is executing this
                // attempt. Transport input acceptance remains a separate event.
                if work["state"] == "turn_started" {
                    return Ok(
                        json!({"ok":true,"work":work,"attempt":attempt,"already_started":true}),
                    );
                }
                work["state"] = json!("turn_started");
                attempt["started_at"] = json!(now());
            } else {
                let outcome = field(req, "outcome")?;
                ensure!(
                    ["completed", "failed"].contains(&outcome),
                    "outcome must be completed or failed"
                );
                field(req, "summary")?;
                ensure!(
                    outcome == "failed" || work["state"] == "turn_started",
                    "acknowledge starting the current attempt before reporting completion"
                );
                if outcome == "completed" && work["write"] == true {
                    field(&req["candidate"], "revision")?;
                    ensure!(
                        req["candidate"]["workspace_path"] == work["workspace_path"],
                        "candidate workspace mismatch"
                    );
                }
                work["state"] = json!(if outcome == "completed" {
                    "reported"
                } else {
                    "failed"
                });
                work["candidate"] = req["candidate"].clone();
                if outcome=="completed" && work["evidence_contract"].is_object() {orchestration::evidence::validate(&work["evidence_contract"],&req["evidence"],&req["candidate"])?;}
                attempt["report"] = json!({"summary":req["summary"],"outcome":outcome,"candidate":req["candidate"],"evidence":req["evidence"],"evidence_source":"agent_report","reported_at":now()});
            }
            attempt["state"] = work["state"].clone();
            put(db, "attempt", &attempt)?;
            put(db, "work", &work)?;
            if cmd=="collab.report" && work["state"]=="failed" {orchestration::record_failure(db,&work,&attempt,req["cause_key"].as_str())?;}
            message(
                db,
                &run,
                &actor_id,
                field(&run, "coordinator")?,
                if cmd == "collab.started" {
                    "started"
                } else {
                    "report"
                },
                json!({"work":work,"attempt":attempt}),
            )?;
            Ok(json!({"ok":true,"work":work,"attempt":attempt}))
        }
        "collab.review.request" => {
            coordinator(&run, &actor)?;
            let work = load(db, "work", field(req, "work_id")?)?;
            ensure!(
                work["run_id"] == run["id"] && work["state"] == "reported",
                "work must be reported before review"
            );
            let reviewer = member_in(db, &run, field(req, "reviewer")?)?;
            ensure!(
                reviewer["identity"]["session_id"]
                    != load(db, "member", field(&work, "assignee")?)?["identity"]["session_id"],
                "review needs a different native session"
            );
            let review = json!({"id":id(),"run_id":run_id,"work_id":work["id"],"attempt_id":work["attempt_id"],"reviewer":reviewer["id"],"candidate":work["candidate"],"state":"requested"});
            put(db, "review", &review)?;
            message(
                db,
                &run,
                &actor_id,
                field(&reviewer, "id")?,
                "review",
                review.clone(),
            )?;
            Ok(json!({"ok":true,"review":review}))
        }
        "collab.review.report" => {
            let mut review = load(db, "review", field(req, "review_id")?)?;
            let mut work = load(db, "work", field(&review, "work_id")?)?;
            ensure!(
                review["run_id"] == run["id"]
                    && review["reviewer"] == actor["id"]
                    && review["state"] == "requested",
                "review ownership or state mismatch"
            );
            ensure!(
                work["state"] == "reported"
                    && work["attempt_id"] == review["attempt_id"]
                    && req["candidate"] == review["candidate"],
                "review must bind the current fixed candidate"
            );
            let verdict = field(req, "verdict")?;
            ensure!(
                ["passed", "failed"].contains(&verdict),
                "verdict must be passed or failed"
            );
            review["state"] = json!(verdict);
            if verdict=="passed" && work["evidence_contract"].is_object() {orchestration::evidence::validate(&work["evidence_contract"],&req["evidence"],&req["candidate"])?;}
            review["evidence"]=req["evidence"].clone();
            review["evidence_source"]=json!("independent_agent_report");
            review["summary"] = json!(field(req, "summary")?);
            put(db, "review", &review)?;
            if verdict == "failed" {
                work["state"] = json!("failed");
                put(db, "work", &work)?;
                let attempt=load(db,"attempt",field(&review,"attempt_id")?)?;
                orchestration::record_failure(db,&work,&attempt,req["cause_key"].as_str())?;
            }
            message(
                db,
                &run,
                &actor_id,
                field(&run, "coordinator")?,
                "review_report",
                review.clone(),
            )?;
            Ok(json!({"ok":true,"review":review}))
        }
        "collab.accept" => {
            coordinator(&run, &actor)?;
            let mut work = load(db, "work", field(req, "work_id")?)?;
            ensure!(
                work["run_id"] == run["id"] && work["state"] == "reported",
                "only reported work can be accepted"
            );
            if work["write"] == true {
                ensure!(
                    list(db, "review", &run_id)?
                        .iter()
                        .any(|r| r["work_id"] == work["id"]
                            && r["attempt_id"] == work["attempt_id"]
                            && r["candidate"] == work["candidate"]
                            && r["state"] == "passed"),
                    "independent review of the fixed candidate must pass"
                );
            }
            if work["evidence_contract"].is_object() {
                let report=load(db,"attempt",field(&work,"attempt_id")?)?;
                orchestration::evidence::validate(&work["evidence_contract"],&report["report"]["evidence"],&work["candidate"])?;
                if work["write"]==true {
                    let reviews=list(db,"review",&run_id)?;
                    let review=reviews.iter().find(|r|r["work_id"]==work["id"] && r["attempt_id"]==work["attempt_id"] && r["state"]=="passed").context("review missing")?;
                    orchestration::evidence::validate(&work["evidence_contract"],&review["evidence"],&work["candidate"])?;
                }
            }
            work["state"] = json!("accepted");
            work["acceptance"] = json!(field(req, "summary")?);
            put(db, "work", &work)?;
            Ok(json!({"ok":true,"work":work}))
        }
        "collab.cancel" => {
            coordinator(&run, &actor)?;
            let mut work = load(db, "work", field(req, "work_id")?)?;
            ensure!(
                work["run_id"] == run["id"] && work["state"] != "accepted",
                "cannot cancel another run or accepted candidate"
            );
            work["state"] = json!("cancelled");
            work["reason"] = json!(field(req, "reason")?);
            let mut attempt = load(db, "attempt", field(&work, "attempt_id")?)?;
            attempt["state"] = json!("cancelled");
            put(db, "attempt", &attempt)?;
            put(db, "work", &work)?;
            message(
                db,
                &run,
                &actor_id,
                field(&work, "assignee")?,
                "cancel",
                work.clone(),
            )?;
            Ok(json!({"ok":true,"work":work,"writer_release_required":work["write"]}))
        }
        "collab.leave" => {
            ensure!(
                actor["id"] != run["coordinator"],
                "coordinator must end the collaboration"
            );
            ensure!(
                !active_work(db, &run_id, &actor_id, false)?,
                "report or cancel active work before leaving"
            );
            let writers: i64 = db.query_row(
                "SELECT count(*) FROM writers WHERE run_id=?1 AND member_id=?2",
                params![run_id, actor_id],
                |r| r.get(0),
            )?;
            ensure!(
                writers == 0,
                "explicitly release writing ownership before leaving"
            );
            actor["state"] = json!("left");
            put(db, "member", &actor)?;
            message(
                db,
                &run,
                &actor_id,
                field(&run, "coordinator")?,
                "left",
                json!({"member_id":actor_id}),
            )?;
            Ok(json!({"ok":true,"terminals_preserved":true}))
        }
        "collab.end" => {
            coordinator(&run, &actor)?;
            let outcome = field(req, "outcome")?;
            ensure!(
                ["accepted", "cancelled"].contains(&outcome),
                "end outcome must be accepted or cancelled"
            );
            let work = list(db, "work", &run_id)?;
            if outcome == "accepted" {
                ensure!(
                    !work.is_empty() && work.iter().all(|w| w["state"] == "accepted"),
                    "overall acceptance requires all work accepted"
                );
                field(req, "summary")?;
            }
            // Ending does not silently revoke a writer that may still be
            // editing. Each writer explicitly releases before finalization.
            let writers: i64 = db.query_row(
                "SELECT count(*) FROM writers WHERE run_id=?1",
                [&run_id],
                |r| r.get(0),
            )?;
            ensure!(writers == 0, "writers must release ownership before ending");
            run["state"] = json!(outcome);
            run["summary"] = req["summary"].clone();
            run["ended_at"] = json!(now());
            put(db, "run", &run)?;
            for member in list(db, "member", &run_id)?
                .iter()
                .filter(|m| m["state"] == "joined")
            {
                message(
                    db,
                    &run,
                    &actor_id,
                    field(member, "id")?,
                    "ended",
                    json!({"outcome":outcome,"summary":req["summary"]}),
                )?;
            }
            Ok(json!({"ok":true,"run":run,"terminals_preserved":true}))
        }
        _ => bail!("unknown collaboration command: {cmd}"),
    }
}

#[cfg(test)]
mod tests;
