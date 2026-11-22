//! Creation write-ahead journal. This module records intent and receipts only;
//! effects and reconciliation belong to the caller.
use super::*;

const CREATION_SCHEMA: u32 = 1;
const MAX_CREATION_BYTES: usize = 2 * 1024 * 1024;
const CREATION_STEP_ORDER: [CreationStepKind; 4] = [
    CreationStepKind::Checkout,
    CreationStepKind::Setup,
    CreationStepKind::Launch,
    CreationStepKind::Prompt,
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreationOperation {
    pub schema: u32,
    pub request_id: String,
    pub request: Value,
    pub revision: u64,
    pub frozen: Value,
    pub steps: Vec<CreationStep>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreationStep {
    pub kind: CreationStepKind,
    pub state: CreationStepState,
    pub attempt: u32,
    pub descriptor: Option<Descriptor>,
    pub receipt: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationStepKind {
    Checkout,
    Setup,
    Launch,
    Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationStepState {
    Pending,
    Running,
    Succeeded,
    Skipped,
    Failed,
    Uncertain,
}

impl CreationStepState {
    fn finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Skipped)
    }
}

impl CreationOperation {
    pub fn ready(&self) -> bool {
        self.validate().is_ok() && self.steps.iter().all(|step| step.state.finished())
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema == CREATION_SCHEMA,
            "unsupported creation operation schema"
        );
        uuid::Uuid::parse_str(&self.request_id).context("creation request id must be a UUID")?;
        anyhow::ensure!(
            self.request.is_object() && self.frozen.is_object(),
            "creation request and frozen snapshot must be objects"
        );
        anyhow::ensure!(
            self.revision <= i64::MAX as u64,
            "creation revision exceeds SQLite integer range"
        );
        anyhow::ensure!(
            self.steps.len() == CREATION_STEP_ORDER.len(),
            "creation steps must be checkout, setup, launch, prompt"
        );
        let mut previous_finished = true;
        for (step, kind) in self.steps.iter().zip(CREATION_STEP_ORDER) {
            anyhow::ensure!(step.kind == kind, "creation steps are out of order");
            anyhow::ensure!(
                previous_finished
                    || (step.state == CreationStepState::Pending && step.attempt == 0),
                "previous creation step is unfinished"
            );
            if step.state == CreationStepState::Pending {
                anyhow::ensure!(
                    step.descriptor.is_none() && step.receipt.is_none() && step.error.is_none(),
                    "pending creation step must have no attempt evidence"
                );
            }
            previous_finished &= step.state.finished();
        }
        Ok(())
    }
}

// Sort recursively even if another workspace dependency enables preserve_order.
// This changes neither the original JSON value nor array order/defaults.
fn canonical_creation_request(request: &Value) -> Result<String> {
    let mut canonical = request.clone();
    canonical.sort_all_objects();
    Ok(serde_json::to_string(&canonical)?)
}

fn check_creation_size(request: &str, document: &str) -> Result<()> {
    // Both serialized columns count toward the stored-content limit.
    anyhow::ensure!(
        request.len().saturating_add(document.len()) <= MAX_CREATION_BYTES,
        "creation journal content exceeds 2 MiB"
    );
    Ok(())
}

fn load_creation(
    conn: &Connection,
    id: &str,
    request: Option<&Value>,
) -> Result<Option<CreationOperation>> {
    uuid::Uuid::parse_str(id).context("creation request id must be a UUID")?;
    let row: Option<(String, i64, String)> = conn
        .query_row(
            "SELECT request,revision,document FROM creation_operations WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(stored_request, revision, document)| {
        check_creation_size(&stored_request, &document)?;
        let value: Value =
            serde_json::from_str(&document).context("invalid creation operation document")?;
        anyhow::ensure!(
            value.get("schema").and_then(Value::as_u64) == Some(CREATION_SCHEMA as u64),
            "unsupported creation operation schema"
        );
        let operation: CreationOperation = serde_json::from_value(value)?;
        operation.validate()?;
        anyhow::ensure!(
            operation.request_id == id,
            "creation operation identity does not match its row"
        );
        anyhow::ensure!(
            revision >= 0 && operation.revision == revision as u64,
            "creation operation revision does not match its row"
        );
        anyhow::ensure!(
            stored_request == canonical_creation_request(&operation.request)?,
            "creation operation request does not match its row"
        );
        if let Some(request) = request {
            anyhow::ensure!(
                operation.request == *request,
                "creation request id already belongs to a different request"
            );
        }
        Ok(operation)
    })
    .transpose()
}

fn validate_creation_transition(current: &CreationStep, next: &CreationStep) -> Result<()> {
    use CreationStepState::*;
    if current.state == Failed && next.state == Pending {
        anyhow::ensure!(
            current.attempt.checked_add(1) == Some(next.attempt),
            "failed creation retry must increment attempt by exactly one"
        );
        anyhow::ensure!(
            next.descriptor.is_none() && next.receipt.is_none() && next.error.is_none(),
            "failed creation retry must clear descriptor, receipt and error"
        );
        return Ok(());
    }
    anyhow::ensure!(
        current.attempt == next.attempt,
        "creation attempt can change only on an explicit failed retry"
    );
    anyhow::ensure!(
        matches!(
            (current.state, next.state),
            (Pending, Running | Skipped)
                | (Running, Succeeded | Failed | Uncertain)
                | (Uncertain, Succeeded | Failed)
        ),
        "invalid creation step transition"
    );
    if !(current.state == Pending && next.state == Running) {
        // Also reject None -> Some after Running: intent must be durable before
        // the effect, not substituted or filled in retrospectively.
        anyhow::ensure!(
            serde_json::to_value(&current.descriptor)? == serde_json::to_value(&next.descriptor)?,
            "creation descriptor is immutable for the current attempt"
        );
    }
    Ok(())
}

impl Workbench {
    /// Look up the original request before recomputing any frozen snapshot.
    pub fn creation(&self, id: &str, request: Option<&Value>) -> Result<Option<CreationOperation>> {
        load_creation(&self.conn, id, request)
    }

    /// Accept once. Replays ignore the supplied frozen snapshot entirely.
    pub fn accept_creation(
        &self,
        id: &str,
        request: &Value,
        frozen: &Value,
    ) -> Result<CreationOperation> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(operation) = load_creation(&tx, id, Some(request))? {
            tx.commit()?;
            return Ok(operation);
        }
        let operation = CreationOperation {
            schema: CREATION_SCHEMA,
            request_id: id.to_owned(),
            request: request.clone(),
            revision: 0,
            frozen: frozen.clone(),
            steps: CREATION_STEP_ORDER
                .into_iter()
                .map(|kind| CreationStep {
                    kind,
                    state: CreationStepState::Pending,
                    attempt: 0,
                    descriptor: None,
                    receipt: None,
                    error: None,
                })
                .collect(),
        };
        operation.validate()?;
        let request = canonical_creation_request(request)?;
        let document = serde_json::to_string(&operation)?;
        check_creation_size(&request, &document)?;
        tx.execute(
            "INSERT INTO creation_operations(id,request,revision,document) VALUES(?1,?2,0,?3)",
            params![id, request, document],
        )?;
        tx.commit()?;
        Ok(operation)
    }

    /// Commit exactly one state transition using the caller's current revision.
    /// Callers must reconcile an Uncertain effect before saving Succeeded/Failed;
    /// this journal never probes, reruns effects, or infers successful completion.
    pub fn save_creation(&self, operation: &CreationOperation) -> Result<CreationOperation> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = load_creation(&tx, &operation.request_id, None)?
            .context("creation operation not found")?;
        anyhow::ensure!(
            operation.revision == current.revision,
            "creation operation changed; reload before saving"
        );
        anyhow::ensure!(
            operation.schema == current.schema
                && operation.request_id == current.request_id
                && operation.request == current.request
                && operation.frozen == current.frozen,
            "creation identity, request, frozen snapshot and schema are immutable"
        );
        operation.validate()?;
        let mut changed = None;
        for (index, (before, after)) in current.steps.iter().zip(&operation.steps).enumerate() {
            // Descriptor deliberately need not implement PartialEq; compare its
            // complete serialized value, including spawn configuration.
            if serde_json::to_value(before)? != serde_json::to_value(after)? {
                anyhow::ensure!(
                    changed.is_none(),
                    "save exactly one creation step at a time"
                );
                changed = Some(index);
            }
        }
        let index = changed.context("save must change exactly one creation step")?;
        anyhow::ensure!(
            current.steps[..index]
                .iter()
                .all(|step| step.state.finished()),
            "previous creation step is unfinished"
        );
        validate_creation_transition(&current.steps[index], &operation.steps[index])?;
        let mut saved = operation.clone();
        saved.revision = current
            .revision
            .checked_add(1)
            .context("creation revision overflow")?;
        let revision = i64::try_from(saved.revision)
            .context("creation revision exceeds SQLite integer range")?;
        let document = serde_json::to_string(&saved)?;
        check_creation_size(&canonical_creation_request(&saved.request)?, &document)?;
        anyhow::ensure!(tx.execute(
            "UPDATE creation_operations SET revision=?2,document=?3 WHERE id=?1 AND revision=?4",
            params![saved.request_id, revision, document, current.revision as i64],
        )? == 1, "creation operation changed; reload before saving");
        tx.commit()?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CreationStepState::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("velella-creation-journal-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn open(&self) -> Workbench {
            // Only an isolated SQLite fixture: no production DB, sessions or hosts.
            let conn = Connection::open(self.0.join("journal.db")).unwrap();
            conn.busy_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                CREATE TABLE IF NOT EXISTS creation_operations(
                    id TEXT PRIMARY KEY, request TEXT NOT NULL,
                    revision INTEGER NOT NULL, document TEXT NOT NULL);",
            )
            .unwrap();
            Workbench {
                conn,
                root: self.0.clone(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn accept(store: &Workbench) -> CreationOperation {
        store.accept_creation(
            &uuid::Uuid::now_v7().to_string(),
            &json!({"name":"fixture", "setup":null, "launch":{"args":["a","b"]}}),
            &json!({"workspace":{"id":"w"}, "task":{"id":"t"}, "base":"main", "oid":"frozen-oid", "setup":null, "launch":{"program":"fixture"}}),
        ).unwrap()
    }

    fn descriptor(id: &str) -> Descriptor {
        serde_json::from_value(json!({
            "id":id, "endpoint":format!("fixture-{id}"), "directory":"fixture-only",
            "spawn":{"program":"fixture", "args":[], "cwd":null, "env":{}, "adapter":null}
        }))
        .unwrap()
    }

    fn document(operation: &CreationOperation) -> Value {
        serde_json::to_value(operation).unwrap()
    }

    fn stored(store: &Workbench, id: &str) -> (String, i64, String) {
        store
            .conn
            .query_row(
                "SELECT request,revision,document FROM creation_operations WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn save_state(
        store: &Workbench,
        operation: &CreationOperation,
        index: usize,
        state: CreationStepState,
    ) -> CreationOperation {
        let mut next = operation.clone();
        next.steps[index].state = state;
        store.save_creation(&next).unwrap()
    }

    fn rejected_unchanged(
        store: &Workbench,
        current: &CreationOperation,
        proposed: &CreationOperation,
    ) {
        let before = stored(store, &current.request_id);
        assert!(store.save_creation(proposed).is_err());
        assert_eq!(stored(store, &current.request_id), before);
    }

    #[test]
    fn identical_replay_preserves_original_request_frozen_and_progress() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let id = uuid::Uuid::now_v7().to_string();
        let request: Value =
            serde_json::from_str(r#"{"z":[{"z":2,"a":1}],"name":"  unchanged  ","a":null}"#)
                .unwrap();
        let reordered: Value =
            serde_json::from_str(r#"{ "a": null, "name":"  unchanged  ", "z":[{"a":1,"z":2}] }"#)
                .unwrap();
        let frozen = json!({"oid":"original", "task":{"id":"original-task"}});
        let first = store.accept_creation(&id, &request, &frozen).unwrap();
        assert_eq!(first.schema, 1);
        assert_eq!(first.revision, 0);
        assert_eq!(first.request, request);
        assert!(first.steps.iter().all(|step| step.state == Pending
            && step.attempt == 0
            && step.descriptor.is_none()
            && step.receipt.is_none()
            && step.error.is_none()));
        let first = save_state(&store, &first, 0, Running);
        let replay = store
            .accept_creation(
                &id,
                &reordered,
                &json!({"oid":"moved", "task":{"id":"different"}}),
            )
            .unwrap();
        assert_eq!(document(&replay), document(&first));
        assert_eq!(replay.frozen, frozen);
        assert_eq!(
            document(&store.accept_creation(&id, &request, &Value::Null).unwrap()),
            document(&first)
        );
        assert_eq!(
            document(&store.creation(&id, Some(&reordered)).unwrap().unwrap()),
            document(&first)
        );
        assert_eq!(
            stored(&store, &id).0,
            r#"{"a":null,"name":"  unchanged  ","z":[{"a":1,"z":2}]}"#
        );
    }

    #[test]
    fn changed_original_request_rejects_without_normalizing_defaults_or_arrays() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let operation = accept(&store);
        let before = stored(&store, &operation.request_id);
        let mut changed = operation.request.clone();
        changed.as_object_mut().unwrap().remove("setup");
        for request in [
            changed,
            json!({"name":"fixture", "setup":null, "launch":{"args":["b","a"]}}),
            json!({"name":"different"}),
        ] {
            assert!(store
                .creation(&operation.request_id, Some(&request))
                .is_err());
            assert!(store
                .accept_creation(&operation.request_id, &request, &operation.frozen)
                .is_err());
            assert_eq!(stored(&store, &operation.request_id), before);
        }
    }

    #[test]
    fn requires_uuid_and_new_object_snapshots() {
        let fixture = Fixture::new();
        let store = fixture.open();
        for id in ["", "not-a-uuid", "../request"] {
            assert!(store.creation(id, None).is_err());
            assert!(store.accept_creation(id, &json!({}), &json!({})).is_err());
        }
        let id = uuid::Uuid::now_v7().to_string();
        assert!(store.creation(&id, None).unwrap().is_none());
        for bad in [Value::Null, json!([]), json!("request"), json!(1)] {
            assert!(store.accept_creation(&id, &bad, &json!({})).is_err());
            assert!(store.accept_creation(&id, &json!({}), &bad).is_err());
            assert!(store.creation(&id, None).unwrap().is_none());
        }
        assert_eq!(
            store
                .accept_creation(&id, &json!({}), &json!({}))
                .unwrap()
                .revision,
            0
        );
    }

    #[test]
    fn stale_cas_from_another_connection_fails_without_losing_progress() {
        let fixture = Fixture::new();
        let first = fixture.open();
        let second = fixture.open();
        let operation = accept(&first);
        let mut stale = second
            .creation(&operation.request_id, None)
            .unwrap()
            .unwrap();
        let current = save_state(&first, &operation, 0, Running);
        assert_eq!(current.revision, 1);
        stale.steps[0].state = Skipped;
        rejected_unchanged(&second, &current, &stale);
        let mut forged_revision = current.clone();
        forged_revision.revision += 1;
        forged_revision.steps[0].state = Succeeded;
        rejected_unchanged(&second, &current, &forged_revision);
        assert_eq!(
            document(
                &second
                    .accept_creation(&operation.request_id, &operation.request, &json!({}))
                    .unwrap()
            ),
            document(&current)
        );
    }

    #[test]
    fn uncertain_survives_reopen_and_never_automatically_retries() {
        let fixture = Fixture::new();
        let uncertain = {
            let store = fixture.open();
            let mut operation = accept(&store);
            operation.steps[0].state = Running;
            operation.steps[0].descriptor = Some(descriptor("original"));
            let mut operation = store.save_creation(&operation).unwrap();
            operation.steps[0].state = Uncertain;
            operation.steps[0].receipt = Some(json!({"observation":"connection_lost"}));
            operation.steps[0].error = Some("Outcome unknown, reconcile first".into());
            store.save_creation(&operation).unwrap()
        };
        let store = fixture.open();
        let restored = store
            .creation(&uncertain.request_id, None)
            .unwrap()
            .unwrap();
        assert_eq!(document(&restored), document(&uncertain));
        assert!(!restored.ready());
        assert_eq!(
            document(
                &store
                    .accept_creation(
                        &restored.request_id,
                        &restored.request,
                        &json!({"oid":"new"})
                    )
                    .unwrap()
            ),
            document(&restored)
        );
        for state in [Pending, Running, Skipped] {
            let mut next = restored.clone();
            next.steps[0].state = state;
            rejected_unchanged(&store, &restored, &next);
            next.steps[0].attempt += 1;
            next.steps[0].descriptor = None;
            next.steps[0].receipt = None;
            next.steps[0].error = None;
            rejected_unchanged(&store, &restored, &next);
        }
        let mut reconciled = restored.clone();
        reconciled.steps[0].state = Succeeded;
        reconciled.steps[0].receipt = Some(json!({"reconciled":true}));
        reconciled.steps[0].error = None;
        let reconciled = store.save_creation(&reconciled).unwrap();
        assert_eq!(reconciled.steps[0].attempt, 0);
        assert_eq!(
            reconciled.steps[0].descriptor.as_ref().unwrap().id,
            "original"
        );
        assert!(!reconciled.ready());
    }

    #[test]
    fn fixed_order_immutable_snapshots_and_one_step_per_save() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let operation = accept(&store);
        rejected_unchanged(&store, &operation, &operation);
        let mut next = operation.clone();
        next.steps[0].state = Skipped;
        for field in ["schema", "request_id", "request", "frozen"] {
            let mut altered = document(&next);
            altered[field] = match field {
                "schema" => json!(2),
                "request_id" => json!(uuid::Uuid::now_v7().to_string()),
                _ => json!({"changed":true}),
            };
            rejected_unchanged(
                &store,
                &operation,
                &serde_json::from_value(altered).unwrap(),
            );
        }
        let mut reordered = next.clone();
        reordered.steps.swap(0, 1);
        rejected_unchanged(&store, &operation, &reordered);
        let mut missing = next.clone();
        missing.steps.pop();
        rejected_unchanged(&store, &operation, &missing);
        let mut extra = next.clone();
        extra.steps.push(extra.steps[3].clone());
        rejected_unchanged(&store, &operation, &extra);
        let mut ahead = operation.clone();
        ahead.steps[1].state = Running;
        rejected_unchanged(&store, &operation, &ahead);
        ahead.steps[1].state = Skipped;
        rejected_unchanged(&store, &operation, &ahead);
        next.steps[1].state = Skipped;
        rejected_unchanged(&store, &operation, &next);
        let running = save_state(&store, &operation, 0, Running);
        let mut two = running.clone();
        two.steps[0].state = Succeeded;
        two.steps[1].state = Running;
        rejected_unchanged(&store, &running, &two);
        let complete = save_state(&store, &running, 0, Succeeded);
        assert_eq!(
            save_state(&store, &complete, 1, Running).steps[1].state,
            Running
        );
    }

    #[test]
    fn descriptor_is_written_before_effects_and_frozen_for_the_attempt() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let pending = accept(&store);
        let mut next = pending.clone();
        next.steps[0].descriptor = Some(descriptor("original"));
        rejected_unchanged(&store, &pending, &next);
        next.steps[0].state = Running;
        let running = store.save_creation(&next).unwrap();
        assert_eq!(running.steps[0].descriptor.as_ref().unwrap().id, "original");
        for state in [Running, Succeeded, Failed, Uncertain] {
            for replacement in [None, Some(descriptor("replacement"))] {
                let mut next = running.clone();
                next.steps[0].state = state;
                next.steps[0].descriptor = replacement;
                rejected_unchanged(&store, &running, &next);
            }
        }
        for field in ["endpoint", "directory", "spawn"] {
            let mut next = running.clone();
            next.steps[0].state = Succeeded;
            let mut altered =
                serde_json::to_value(next.steps[0].descriptor.as_ref().unwrap()).unwrap();
            if field == "spawn" {
                altered["spawn"]["program"] = json!("substituted");
            } else {
                altered[field] = json!("substituted");
            }
            next.steps[0].descriptor = Some(serde_json::from_value(altered).unwrap());
            rejected_unchanged(&store, &running, &next);
        }
        let uncertain = save_state(&store, &running, 0, Uncertain);
        let mut next = uncertain.clone();
        next.steps[0].state = Succeeded;
        next.steps[0].descriptor = Some(descriptor("replacement"));
        rejected_unchanged(&store, &uncertain, &next);
        let terminal = save_state(&store, &uncertain, 0, Succeeded);
        let mut next = terminal.clone();
        next.steps[0].receipt = Some(json!({"rewritten":true}));
        rejected_unchanged(&store, &terminal, &next);

        let pending = accept(&store);
        let without_descriptor = save_state(&store, &pending, 0, Running);
        for state in [Running, Succeeded, Failed, Uncertain] {
            let mut next = without_descriptor.clone();
            next.steps[0].state = state;
            next.steps[0].descriptor = Some(descriptor("too-late"));
            rejected_unchanged(&store, &without_descriptor, &next);
        }
    }

    #[test]
    fn failed_retry_requires_exact_increment_and_clears_attempt_evidence() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let mut operation = accept(&store);
        operation.steps[0].state = Running;
        operation.steps[0].descriptor = Some(descriptor("first-attempt"));
        let mut operation = store.save_creation(&operation).unwrap();
        operation.steps[0].state = Failed;
        operation.steps[0].receipt = Some(json!({"exit_code":1}));
        operation.steps[0].error = Some("setup failed".into());
        let failed = store.save_creation(&operation).unwrap();
        assert!(!failed.ready());
        let mut retry = failed.clone();
        retry.steps[0].state = Pending;
        retry.steps[0].attempt += 1;
        retry.steps[0].descriptor = None;
        retry.steps[0].receipt = None;
        retry.steps[0].error = None;
        for attempt in [0, 2] {
            let mut invalid = retry.clone();
            invalid.steps[0].attempt = attempt;
            rejected_unchanged(&store, &failed, &invalid);
        }
        for field in ["descriptor", "receipt", "error"] {
            let mut invalid = document(&retry);
            invalid["steps"][0][field] = document(&failed)["steps"][0][field].clone();
            rejected_unchanged(&store, &failed, &serde_json::from_value(invalid).unwrap());
        }
        for state in [Running, Succeeded, Skipped, Uncertain] {
            let mut invalid = retry.clone();
            invalid.steps[0].state = state;
            rejected_unchanged(&store, &failed, &invalid);
        }
        let pending = store.save_creation(&retry).unwrap();
        assert_eq!(pending.steps[0].attempt, 1);
        let mut next = pending.clone();
        next.steps[0].state = Running;
        next.steps[0].descriptor = Some(descriptor("second-attempt"));
        next.steps[0].attempt += 1;
        rejected_unchanged(&store, &pending, &next);
        next.steps[0].attempt = 1;
        let running = store.save_creation(&next).unwrap();
        let uncertain = save_state(&store, &running, 0, Uncertain);
        let failed = save_state(&store, &uncertain, 0, Failed);
        assert_eq!(failed.steps[0].attempt, 1);
        assert_eq!(
            failed.steps[0].descriptor.as_ref().unwrap().id,
            "second-attempt"
        );
    }

    #[test]
    fn ready_is_derived_only_after_every_step_is_succeeded_or_skipped() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let mut operation = accept(&store);
        for index in 0..4 {
            assert!(!operation.ready());
            let mut invalid = operation.clone();
            invalid.steps[index].state = Succeeded;
            rejected_unchanged(&store, &operation, &invalid);
            if index % 2 == 0 {
                operation = save_state(&store, &operation, index, Running);
                for state in [Pending, Skipped] {
                    let mut invalid = operation.clone();
                    invalid.steps[index].state = state;
                    rejected_unchanged(&store, &operation, &invalid);
                }
                operation = save_state(&store, &operation, index, Succeeded);
            } else {
                operation = save_state(&store, &operation, index, Skipped);
            }
        }
        assert!(operation.ready());
        assert_eq!(operation.revision, 6);
        let wire = document(&operation);
        assert_eq!(wire["steps"][0]["kind"], "checkout");
        assert_eq!(wire["steps"][1]["state"], "skipped");
        assert!(wire.get("ready").is_none());
        let mut invalid = operation.clone();
        invalid.steps.clear();
        assert!(!invalid.ready());
        invalid = operation.clone();
        invalid.schema = 2;
        assert!(!invalid.ready());
    }

    #[test]
    fn future_record_schema_is_rejected_without_rewriting() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let operation = accept(&store);
        let mut future = document(&operation);
        future["schema"] = json!(999);
        store
            .conn
            .execute(
                "UPDATE creation_operations SET document=?2 WHERE id=?1",
                params![operation.request_id, future.to_string()],
            )
            .unwrap();
        let before = stored(&store, &operation.request_id);
        assert!(store.creation(&operation.request_id, None).is_err());
        assert!(store
            .accept_creation(&operation.request_id, &operation.request, &operation.frozen)
            .is_err());
        assert!(store.save_creation(&operation).is_err());
        assert_eq!(stored(&store, &operation.request_id), before);
    }

    #[test]
    fn corrupt_record_identity_request_revision_or_steps_is_not_repaired() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let operation = accept(&store);
        let original = document(&operation);
        let mutations = [
            ("request_id", json!(uuid::Uuid::now_v7().to_string())),
            ("request", json!({"different":true})),
            ("revision", json!(17)),
            ("steps", json!([])),
            ("frozen", Value::Null),
            ("schema", Value::Null),
        ];
        for (field, value) in mutations {
            let mut corrupted = original.clone();
            corrupted[field] = value;
            store
                .conn
                .execute(
                    "UPDATE creation_operations SET document=?2 WHERE id=?1",
                    params![operation.request_id, corrupted.to_string()],
                )
                .unwrap();
            let before = stored(&store, &operation.request_id);
            assert!(
                store.creation(&operation.request_id, None).is_err(),
                "accepted corrupt {field}"
            );
            assert!(store
                .accept_creation(&operation.request_id, &operation.request, &operation.frozen)
                .is_err());
            assert!(store.save_creation(&operation).is_err());
            assert_eq!(stored(&store, &operation.request_id), before);
        }
        store
            .conn
            .execute(
                "UPDATE creation_operations SET document='invalid json' WHERE id=?1",
                [&operation.request_id],
            )
            .unwrap();
        assert!(store.creation(&operation.request_id, None).is_err());
    }

    #[test]
    fn serialized_content_limit_rejects_accept_and_save_atomically() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let id = uuid::Uuid::now_v7().to_string();
        let oversized = json!({"payload":"x".repeat(MAX_CREATION_BYTES)});
        assert!(store.accept_creation(&id, &oversized, &json!({})).is_err());
        assert!(store.accept_creation(&id, &json!({}), &oversized).is_err());
        assert!(store.creation(&id, None).unwrap().is_none());
        let operation = accept(&store);
        let running = save_state(&store, &operation, 0, Running);
        let mut next = running.clone();
        next.steps[0].state = Succeeded;
        next.steps[0].receipt = Some(oversized);
        rejected_unchanged(&store, &running, &next);
        assert!(!store
            .creation(&running.request_id, None)
            .unwrap()
            .unwrap()
            .ready());
    }

    #[test]
    fn sqlite_write_failure_never_marks_ready_and_transaction_is_rolled_back() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let mut operation = accept(&store);
        for index in 0..3 {
            operation = save_state(&store, &operation, index, Skipped);
        }
        let running = save_state(&store, &operation, 3, Running);
        store.conn.execute_batch("CREATE TEMP TRIGGER reject_creation_update
            BEFORE UPDATE ON creation_operations BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
        let mut next = running.clone();
        next.steps[3].state = Succeeded;
        rejected_unchanged(&store, &running, &next);
        assert!(!store
            .creation(&running.request_id, None)
            .unwrap()
            .unwrap()
            .ready());
        assert!(store.conn.is_autocommit());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_creation_update;")
            .unwrap();
        assert!(store.save_creation(&next).unwrap().ready());
    }

    #[test]
    fn concurrent_acceptance_has_one_frozen_winner() {
        let fixture = Fixture::new();
        let stores = [fixture.open(), fixture.open()];
        let id = uuid::Uuid::now_v7().to_string();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = stores
            .into_iter()
            .enumerate()
            .map(|(index, store)| {
                let id = id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .accept_creation(&id, &json!({"request":"same"}), &json!({"winner":index}))
                        .unwrap()
                })
            })
            .collect();
        let operations: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(document(&operations[0]), document(&operations[1]));
        assert_eq!(operations[0].revision, 0);
        let store = fixture.open();
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM creation_operations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            document(&store.creation(&id, None).unwrap().unwrap()),
            document(&operations[0])
        );
    }

    #[test]
    fn attempt_and_revision_exhaustion_reject_without_wrapping_or_writing() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let operation = accept(&store);
        let running = save_state(&store, &operation, 0, Running);
        let mut failed = save_state(&store, &running, 0, Failed);
        failed.steps[0].attempt = u32::MAX;
        store
            .conn
            .execute(
                "UPDATE creation_operations SET document=?2 WHERE id=?1",
                params![failed.request_id, document(&failed).to_string()],
            )
            .unwrap();
        let exhausted = store.creation(&failed.request_id, None).unwrap().unwrap();
        for attempt in [0, u32::MAX] {
            let mut retry = exhausted.clone();
            retry.steps[0].state = Pending;
            retry.steps[0].attempt = attempt;
            rejected_unchanged(&store, &exhausted, &retry);
        }
        let operation = accept(&store);
        let mut running = save_state(&store, &operation, 0, Running);
        running.revision = i64::MAX as u64;
        store
            .conn
            .execute(
                "UPDATE creation_operations SET revision=?2,document=?3 WHERE id=?1",
                params![running.request_id, i64::MAX, document(&running).to_string()],
            )
            .unwrap();
        let exhausted = store.creation(&running.request_id, None).unwrap().unwrap();
        let mut next = exhausted.clone();
        next.steps[0].state = Succeeded;
        rejected_unchanged(&store, &exhausted, &next);
        assert!(store.conn.is_autocommit());
    }
}
