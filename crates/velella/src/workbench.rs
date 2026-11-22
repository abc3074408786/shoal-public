//! Durable workspace/session/layout metadata. Execution remains owned by hosts.
use crate::{desktop::{Project, Task}, host::Descriptor, ipc};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, path::{Path, PathBuf}};

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: String,
    pub archived: bool,
    pub favorite: bool,
    pub position: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_workspace_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub kind: String,
    pub task_id: Option<String>,
    pub native_id: Option<String>,
    pub descriptor: Descriptor,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub title: String,
    #[serde(default, rename="terminalTree", skip_serializing_if="Option::is_none")]
    pub terminal_tree: Option<SplitNode>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag="kind", rename_all="lowercase")]
pub enum SplitNode {
    Leaf { id: String },
    Split { id: String, axis: String, ratio: f64, first: Box<SplitNode>, second: Box<SplitNode> },
}
impl SplitNode {
    fn leaves(&self) -> Result<HashSet<String>> {
        fn visit(node:&SplitNode,depth:usize,ids:&mut HashSet<String>,leaves:&mut HashSet<String>)->Result<()> {
            anyhow::ensure!(depth<=16,"split tree is too deep");
            let id=match node {SplitNode::Leaf{id}|SplitNode::Split{id,..}=>id};
            anyhow::ensure!(!id.is_empty()&&id.len()<=200&&ids.insert(id.clone()),"invalid or duplicate split node");
            match node {
                SplitNode::Leaf{id}=>{leaves.insert(id.clone());anyhow::ensure!(leaves.len()<=16,"too many split panes");}
                SplitNode::Split{axis,ratio,first,second,..}=>{
                    anyhow::ensure!(["horizontal","vertical"].contains(&axis.as_str())&&ratio.is_finite()&&(0.15..=0.85).contains(ratio),"invalid split boundary");
                    visit(first,depth+1,ids,leaves)?;visit(second,depth+1,ids,leaves)?;
                }
            }Ok(())
        }
        let mut leaves=HashSet::new();visit(self,0,&mut HashSet::new(),&mut leaves)?;Ok(leaves)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pane {
    pub id: String,
    pub active: Option<String>,
    pub tabs: Vec<Tab>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Layout {
    pub revision: u64,
    pub axis: String,
    pub ratio: f64,
    pub panes: Vec<Pane>,
    pub closed: Vec<Tab>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub tree: Option<SplitNode>,
}
impl Default for Layout {
    fn default() -> Self {
        Self { revision: 0, axis: "horizontal".into(), ratio: 0.5,
            panes: vec![Pane { id: "main".into(), active: None, tabs: vec![] }], closed: vec![], tree:None }
    }
}

pub struct Workbench { conn: Connection, root: PathBuf }
#[path = "workbench_creation.rs"]
mod creation;
pub(crate) use creation::{CreationOperation, CreationStepState};
#[cfg(test)]
mod session_lookup_tests {
    use super::*;

    #[test]
    fn targeted_session_lookup_keeps_ownership_and_observes_updates() {
        let root=std::env::temp_dir().join(format!("velella-session-lookup-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        {
            let store=Workbench::open(&root).unwrap();
            for id in ["a","b"] {
                store.put_workspace(&Workspace{id:id.into(),project_id:"p".into(),name:id.into(),path:root.clone(),kind:"folder".into(),archived:false,favorite:false,position:0,parent_workspace_id:None}).unwrap();
            }
            let mut session=Session{id:"s".into(),workspace_id:"a".into(),title:"original".into(),kind:"terminal".into(),task_id:None,native_id:None,
                descriptor:Descriptor{id:"s".into(),endpoint:"unused".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{program:"fixture".into(),args:vec![],cwd:None,env:Default::default(),adapter:None,sandbox:Default::default()}}};
            store.put_session(&session).unwrap();
            assert_eq!(store.session("a","s").unwrap().unwrap().title,"original");
            assert!(store.session("b","s").unwrap().is_none());
            assert!(store.session("a","missing").unwrap().is_none());
            session.title="updated".into();store.put_session(&session).unwrap();
            assert_eq!(store.session("a","s").unwrap().unwrap().title,"updated");
            // Unrelated historical metadata must not be decoded to read s.
            store.conn.execute("INSERT INTO sessions VALUES ('other','a','invalid json')",[]).unwrap();
            assert!(store.session("a","s").unwrap().is_some());
            assert!(store.session("a","other").is_err());
            session.workspace_id="b".into();
            store.conn.execute("UPDATE sessions SET document=?1 WHERE id='s'",[serde_json::to_string(&session).unwrap()]).unwrap();
            assert!(store.session("a","s").is_err());
            store.conn.execute("DELETE FROM sessions WHERE id='s'",[]).unwrap();
            assert!(store.session("a","s").unwrap().is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[cfg(test)]
mod feedback_tests {
    use super::*;
    #[test]
    fn receipt_survives_restart_without_replay_and_is_workspace_bound() {
        let root=std::env::temp_dir().join(format!("velella-feedback-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        let sending=json!({"state":"sending","text":"only once"});
        {
            let store=Workbench::open(&root).unwrap();
            for id in ["a","b"] {store.put_workspace(&Workspace{id:id.into(),project_id:"p".into(),name:id.into(),path:root.clone(),kind:"main".into(),archived:false,favorite:false,position:0,parent_workspace_id:None}).unwrap();}
            store.begin_feedback("a","request",&sending).unwrap();
        }
        {
            let store=Workbench::open(&root).unwrap();
            assert_eq!(store.feedback("a","request").unwrap(),Some(sending.clone()));
            assert!(store.begin_feedback("a","request",&sending).is_err());
            assert!(store.feedback("b","request").is_err());
            assert!(store.finish_feedback("b","request",&json!({"state":"submitted"})).is_err());
            store.finish_feedback("a","request",&json!({"state":"uncertain"})).unwrap();
        }
        {
            let store=Workbench::open(&root).unwrap();
            assert_eq!(store.feedback("a","request").unwrap().unwrap()["state"],"uncertain");
            assert!(store.begin_feedback("a","request",&sending).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
impl Workbench {
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("workbench.db");
        let existed = path.is_file();
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        anyhow::ensure!(version <= 3, "workbench database is newer than this runtime");
        let backup = if existed && version < 3 {
            let dir = root.join("migration-backups").join(uuid::Uuid::now_v7().to_string());
            std::fs::create_dir_all(&dir)?;
            conn.execute("VACUUM INTO ?1", [dir.join("workbench.db").to_string_lossy().as_ref()])?;
            let mut files = vec!["workbench.db"];
            if root.join("projects.json").exists() {
                std::fs::copy(root.join("projects.json"), dir.join("projects.json"))?;
                files.push("projects.json");
            }
            ipc::atomic_json(&dir.join("manifest.json"), &json!({"source":"workbench","from_schema":version,"to_schema":3,"files":files,"created_at":chrono::Utc::now().to_rfc3339()}))?;
            Some(dir)
        } else { None };
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS workspaces(id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id), document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS layouts(workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id), document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS migrations(source TEXT PRIMARY KEY, backup TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, workspace_id TEXT NOT NULL REFERENCES workspaces(id), kind TEXT NOT NULL, payload TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS events_workspace_seq ON events(workspace_id,seq);
            CREATE TABLE IF NOT EXISTS feedback_deliveries(id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id), document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS creation_operations(id TEXT PRIMARY KEY, request TEXT NOT NULL, revision INTEGER NOT NULL, document TEXT NOT NULL);
            PRAGMA user_version=3; COMMIT;")?;
        if let Some(backup) = backup {
            conn.execute("INSERT OR IGNORE INTO migrations(source,backup) VALUES(?1,?2)", params![format!("workbench-v{version}"), backup.to_string_lossy()])?;
        }
        Ok(Self { conn, root: root.into() })
    }

    // Called while the daemon/project ownership locks are held, before opening
    // the legacy Store (which can migrate its own schema). VACUUM includes WAL.
    pub fn backup_legacy(&self, project: Option<&Project>) -> Result<()> {
        let source = project.map(|p| p.path.to_string_lossy().into_owned()).unwrap_or_else(|| "projects.json".into());
        if self.conn.query_row("SELECT 1 FROM migrations WHERE source=?1", [&source], |_| Ok(())).optional()?.is_some() { return Ok(()); }
        let dir = self.root.join("migration-backups").join(uuid::Uuid::now_v7().to_string());
        std::fs::create_dir_all(&dir)?;
        let mut files = vec![];
        if let Some(project) = project {
            let path = project.path.join(".velella/state.db");
            if path.exists() {
                let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                conn.busy_timeout(std::time::Duration::from_secs(5))?;
                conn.execute("VACUUM INTO ?1", [dir.join("state.db").to_string_lossy().as_ref()])?;
                files.push("state.db");
            }
        } else if self.root.join("projects.json").exists() {
            std::fs::copy(self.root.join("projects.json"), dir.join("projects.json"))?;
            files.push("projects.json");
        }
        ipc::atomic_json(&dir.join("manifest.json"), &json!({"source":source,"files":files,"created_at":chrono::Utc::now().to_rfc3339(),"schema":1}))?;
        self.conn.execute("INSERT INTO migrations(source,backup) VALUES(?1,?2)", params![source,dir.to_string_lossy()])?;
        Ok(())
    }

    pub fn sync_legacy(&mut self, projects: &[Project], tasks: &[Task]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for p in projects {
            let w = Workspace { id: format!("main:{}",p.id), project_id:p.id.clone(), name:p.name.clone(), path:p.path.clone(), kind:if p.direct {"folder"} else {"primary"}.into(), archived:false, favorite:false, position:0, parent_workspace_id:None };
            tx.execute("INSERT OR IGNORE INTO workspaces(id,document) VALUES(?1,?2)", params![w.id,serde_json::to_string(&w)?])?;
            for d in &p.terminals {
                let s = Session { id:d.id.clone(), workspace_id:w.id.clone(), title:if cfg!(windows){"PowerShell"}else{"Shell"}.into(), kind:"terminal".into(), task_id:None, native_id:None, descriptor:d.clone() };
                tx.execute("INSERT OR IGNORE INTO sessions(id,workspace_id,document) VALUES(?1,?2,?3)",params![s.id,s.workspace_id,serde_json::to_string(&s)?])?;
            }
        }
        for t in tasks {
            // Queued worktrees don't exist yet; avoid presenting unusable roots.
            if !t.worktree.is_dir() { continue; }
            let wid = if t.direct {format!("main:{}",t.project_id)} else {format!("task:{}",t.id)};
            if !t.direct {
                let w = Workspace { id:wid.clone(), project_id:t.project_id.clone(), name:t.spec.name.clone(), path:t.worktree.clone(), kind:"task".into(), archived:false, favorite:false, position:0, parent_workspace_id:t.parent_workspace_id.clone() };
                tx.execute("INSERT OR IGNORE INTO workspaces(id,document) VALUES(?1,?2)",params![w.id,serde_json::to_string(&w)?])?;
            }
            for (kind, d) in [("agent", &t.agent),("command", &t.command),("terminal", &t.shell)] {
                if let Some(d) = d {
                    let s = Session { id:d.id.clone(), workspace_id:wid.clone(), title:format!("{} · {}",t.spec.name,kind), kind:kind.into(), task_id:Some(t.id.clone()), native_id:if kind=="agent" {t.native_id.clone()} else {None}, descriptor:d.clone() };
                    tx.execute("INSERT INTO sessions(id,workspace_id,document) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET document=excluded.document",params![s.id,s.workspace_id,serde_json::to_string(&s)?])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn creation_operations(&self) -> Result<Vec<CreationOperation>> {
        let mut stmt=self.conn.prepare("SELECT id FROM creation_operations ORDER BY rowid")?;
        let ids=stmt.query_map([],|row|row.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        ids.iter().map(|id|self.creation(id,None)?.context("creation operation missing")).collect()
    }
    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare("SELECT document FROM workspaces ORDER BY rowid")?;
        let docs = stmt.query_map([], |r| r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        docs.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
    pub fn workspace(&self, id: &str) -> Result<Workspace> {
        let s: String = self.conn.query_row("SELECT document FROM workspaces WHERE id=?1",[id],|r|r.get(0)).optional()?.context("workspace not found")?;
        Ok(serde_json::from_str(&s)?)
    }
    pub fn put_workspace(&self, w: &Workspace) -> Result<()> {
        anyhow::ensure!(!w.name.trim().is_empty() && w.name.len() <= 240,"workspace name required (max 240 bytes)");
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate)?;
        Self::validate_parent(&tx, &w.id, w.parent_workspace_id.as_deref())?;
        tx.execute("INSERT INTO workspaces(id,document) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET document=excluded.document",params![w.id,serde_json::to_string(w)?])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn check_parent(&self, id: &str, parent: Option<&str>) -> Result<()> {
        Self::validate_parent(&self.conn, id, parent)
    }
    fn validate_parent(conn: &Connection, id: &str, parent: Option<&str>) -> Result<()> {
        anyhow::ensure!(!id.starts_with("main:") || parent.is_none(), "主工作区必须位于顶级");
        let mut visited = HashSet::from([id.to_owned()]);
        let mut next = parent.map(str::to_owned);
        while let Some(parent) = next {
            anyhow::ensure!(visited.insert(parent.clone()), "父级关系不能包含自身或形成循环");
            let document: String = conn.query_row("SELECT document FROM workspaces WHERE id=?1", [&parent], |row| row.get(0)).optional()?.context("父工作区不存在于当前执行主机")?;
            let workspace: Workspace = serde_json::from_str(&document)?;
            next = workspace.parent_workspace_id;
        }
        Ok(())
    }
    pub fn sessions(&self, workspace_id: Option<&str>) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare("SELECT document FROM sessions WHERE (?1 IS NULL OR workspace_id=?1) ORDER BY rowid")?;
        let docs = stmt.query_map([workspace_id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        docs.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
    pub fn put_session(&self, s: &Session) -> Result<()> {
        self.conn.execute("INSERT INTO sessions(id,workspace_id,document) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET document=excluded.document",params![s.id,s.workspace_id,serde_json::to_string(s)?])?;
        Ok(())
    }
    /// Resolve one session by its primary key without decoding the workspace's
    /// history on every terminal poll. Ownership stays part of the SQL predicate.
    pub(crate) fn session(&self, workspace_id: &str, id: &str) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare_cached("SELECT document FROM sessions WHERE id=?1 AND workspace_id=?2")?;
        let document: Option<String> = stmt.query_row(params![id, workspace_id], |row| row.get(0)).optional()?;
        document.map(|document| {
            let session: Session = serde_json::from_str(&document)?;
            anyhow::ensure!(session.id == id && session.workspace_id == workspace_id, "session metadata does not match its owner");
            Ok(session)
        }).transpose()
    }
    pub fn layout(&self, id: &str) -> Result<Layout> {
        self.workspace(id)?;
        let s: Option<String> = self.conn.query_row("SELECT document FROM layouts WHERE workspace_id=?1",[id],|r|r.get(0)).optional()?;
        Ok(s.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default())
    }
    pub fn save_layout(&mut self, id: &str, mut layout: Layout) -> Result<Layout> {
        let current = self.layout(id)?;
        anyhow::ensure!(current.revision == layout.revision,"layout changed in another window; reload before saving");
        anyhow::ensure!((1..=16).contains(&layout.panes.len()) && ["horizontal","vertical"].contains(&layout.axis.as_str()) && (0.15..=0.85).contains(&layout.ratio),"invalid split layout");
        if let Some(tree)=&layout.tree {
            let expected:HashSet<String>=layout.panes.iter().map(|p|p.id.clone()).collect();
            anyhow::ensure!(tree.leaves()?==expected,"split tree does not match panes");
        } else {anyhow::ensure!(layout.panes.len()<=2,"nested panes need a split tree");}
        anyhow::ensure!(layout.closed.len() <= 30,"too many closed tabs");
        let sessions = self.sessions(Some(id))?;
        let mut ids = HashSet::new();
        let mut pane_ids = HashSet::new();
        anyhow::ensure!(layout.panes.iter().map(|pane|pane.tabs.len()).sum::<usize>()<=512,"too many workspace tabs");
        for pane in &layout.panes {
            // A merge must accept all tabs already allowed across 16 groups.
            anyhow::ensure!(pane_ids.insert(&pane.id),"invalid pane");
            anyhow::ensure!(pane.active.as_ref().is_none_or(|a|pane.tabs.iter().any(|t| &t.id==a)),"active tab missing");
            for tab in &pane.tabs {
                anyhow::ensure!(ids.insert(&tab.id),"duplicate tab");
            }
        }
        for tab in layout.panes.iter().flat_map(|p| &p.tabs).chain(&layout.closed) {
            anyhow::ensure!(["terminal","agent","files","diff","browser"].contains(&tab.kind.as_str()),"unsupported tab kind");
            if tab.kind=="browser" {anyhow::ensure!(tab.target.starts_with("browser-")&&tab.target.len()<=200,"invalid browser page target");}
            if tab.kind=="terminal" { anyhow::ensure!(sessions.iter().any(|s|s.id==tab.target),"session belongs to another workspace"); }
            if let Some(tree)=&tab.terminal_tree {
                anyhow::ensure!(tab.kind=="terminal","only terminal tabs can contain terminal splits");
                let leaves=tree.leaves()?;
                anyhow::ensure!(leaves.contains(&tab.target),"terminal split target is missing");
                for sid in leaves {
                    anyhow::ensure!(sessions.iter().any(|s|s.id==sid&&!s.descriptor.is_legacy_chat()),"split session belongs to another workspace or is not a terminal");
                }
            }
        }
        layout.revision += 1;
        self.conn.execute("INSERT INTO layouts(workspace_id,document) VALUES(?1,?2) ON CONFLICT(workspace_id) DO UPDATE SET document=excluded.document",params![id,serde_json::to_string(&layout)?])?;
        self.record_event(id,"layout.saved",json!({"revision":layout.revision,"panes":layout.panes.len()}))?;
        Ok(layout)
    }
    pub fn record_event(&self, workspace_id:&str, kind:&str, payload:Value)->Result<()> {
        self.workspace(workspace_id)?;
        anyhow::ensure!(!kind.is_empty() && kind.len()<=120,"event kind is required");
        self.conn.execute("INSERT INTO events(workspace_id,kind,payload,created_at) VALUES(?1,?2,?3,?4)",params![workspace_id,kind,serde_json::to_string(&payload)?,chrono::Utc::now().to_rfc3339()])?;
        Ok(())
    }
    pub fn feedback(&self,workspace:&str,id:&str)->Result<Option<Value>> {
        let row:Option<(String,String)>=self.conn.query_row("SELECT workspace_id,document FROM feedback_deliveries WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        row.map(|(owner,document)|{anyhow::ensure!(owner==workspace,"feedback belongs to another workspace");Ok(serde_json::from_str(&document)?)}).transpose()
    }
    pub fn begin_feedback(&self,workspace:&str,id:&str,document:&Value)->Result<()> {
        self.conn.execute("INSERT INTO feedback_deliveries(id,workspace_id,document) VALUES(?1,?2,?3)",params![id,workspace,serde_json::to_string(document)?])?;
        Ok(())
    }
    pub fn finish_feedback(&self,workspace:&str,id:&str,document:&Value)->Result<()> {
        anyhow::ensure!(self.conn.execute("UPDATE feedback_deliveries SET document=?3 WHERE id=?1 AND workspace_id=?2",params![id,workspace,serde_json::to_string(document)?])?==1,"feedback receipt missing");
        Ok(())
    }
    pub fn events(&self, workspace_id:&str, since:i64)->Result<Vec<Value>> {
        self.workspace(workspace_id)?;
        let mut stmt=self.conn.prepare("SELECT seq,kind,payload,created_at FROM events WHERE workspace_id=?1 AND seq>?2 ORDER BY seq LIMIT 500")?;
        let rows=stmt.query_map(params![workspace_id,since.max(0)],|r|Ok(json!({"seq":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null),"created_at":r.get::<_,String>(3)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(rows)
    }
    pub fn backups(&self) -> Result<Value> {
        let mut stmt=self.conn.prepare("SELECT source,backup FROM migrations")?;
        let rows=stmt.query_map([],|r|Ok(json!({"source":r.get::<_,String>(0)?,"backup":r.get::<_,String>(1)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(json!(rows))
    }
}

#[cfg(test)]
#[path="workbench_split_tests.rs"]
mod split_tests;

#[cfg(test)]
#[path="workbench_creation_tests.rs"]
mod creation_tests;
