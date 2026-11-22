use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("velella-creation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
fn workspace(root: &Path, id: &str, parent: Option<&str>) -> Workspace {
    serde_json::from_value(json!({"id":id,"project_id":"project","name":id,"path":root,"kind":"worktree","archived":false,"favorite":false,"position":0,"parent_workspace_id":parent})).unwrap()
}

#[test]
fn parent_survives_restart_and_archive_without_hiding_child() {
    let fixture = Fixture::new();
    {
        let store = Workbench::open(&fixture.0).unwrap();
        store.put_workspace(&workspace(&fixture.0, "parent", None)).unwrap();
        store.put_workspace(&workspace(&fixture.0, "child", Some("parent"))).unwrap();
        let mut parent = store.workspace("parent").unwrap();
        parent.archived = true;
        store.put_workspace(&parent).unwrap();
    }
    let store = Workbench::open(&fixture.0).unwrap();
    let child = store.workspace("child").unwrap();
    assert_eq!(serde_json::to_value(&child).unwrap()["parent_workspace_id"], "parent");
    assert!(!child.archived);
    assert_eq!(store.workspaces().unwrap().len(), 2);
}

#[test]
fn parent_rejects_missing_self_cycle_and_primary_workspace() {
    let fixture = Fixture::new();
    let store = Workbench::open(&fixture.0).unwrap();
    assert!(store.put_workspace(&workspace(&fixture.0, "child", Some("missing"))).is_err());
    assert!(store.put_workspace(&workspace(&fixture.0, "self", Some("self"))).is_err());
    store.put_workspace(&workspace(&fixture.0, "a", None)).unwrap();
    store.put_workspace(&workspace(&fixture.0, "b", Some("a"))).unwrap();
    assert!(store.put_workspace(&workspace(&fixture.0, "a", Some("b"))).is_err());
    assert!(serde_json::to_value(store.workspace("a").unwrap()).unwrap()["parent_workspace_id"].is_null());
    assert!(store.put_workspace(&workspace(&fixture.0, "main:project", Some("a"))).is_err());
}

#[test]
fn migration_backs_up_wal_and_project_config_before_schema_change() {
    let fixture = Fixture::new();
    let old = Connection::open(fixture.0.join("workbench.db")).unwrap();
    old.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE workspaces(id TEXT PRIMARY KEY, document TEXT NOT NULL); PRAGMA user_version=2;").unwrap();
    let legacy = json!({"id":"old","project_id":"project","name":"old","path":fixture.0,"kind":"worktree","archived":false,"favorite":true,"position":4});
    old.execute("INSERT INTO workspaces VALUES ('old',?1)", [legacy.to_string()]).unwrap();
    std::fs::write(fixture.0.join("projects.json"), "[{\"id\":\"project\"}]").unwrap();
    let store = Workbench::open(&fixture.0).unwrap();
    assert!(store.workspace("old").unwrap().favorite);
    let entries: Vec<_> = std::fs::read_dir(fixture.0.join("migration-backups")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(entries.len(), 1);
    let backup = Connection::open(entries[0].join("workbench.db")).unwrap();
    assert_eq!(backup.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(backup.query_row("SELECT document FROM workspaces WHERE id='old'", [], |r| r.get::<_, String>(0)).unwrap(), legacy.to_string());
    assert_eq!(std::fs::read_to_string(entries[0].join("projects.json")).unwrap(), "[{\"id\":\"project\"}]");
    drop(store);
    let _reopened = Workbench::open(&fixture.0).unwrap();
    assert_eq!(std::fs::read_dir(fixture.0.join("migration-backups")).unwrap().count(), 1);
}

#[test]
fn future_workbench_schema_is_rejected_without_rewriting_it() {
    let fixture = Fixture::new();
    let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
    conn.execute_batch("PRAGMA user_version=999;").unwrap();
    assert!(Workbench::open(&fixture.0).is_err());
    assert_eq!(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 999);
}
