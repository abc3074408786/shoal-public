//! Persistent, multi-project task supervisor. No PTY belongs to the GUI or daemon.
use crate::{
    host::{self, Descriptor, Status},
    ipc, process,
    sessions::SpawnRequest,
};
use anyhow::{Context, Result};
use fs2::FileExt;
use interprocess::local_socket::prelude::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use velella_core::task::{parse_wall_clock, OnDone, Permissions, TaskSpec};
mod workbench_commands;
mod delivery;
mod capabilities;
mod cli_inspection;
mod account_launch;
mod project_creation;
mod terminal_io;
mod task_execution;
mod task_sandbox;
mod collaboration_commands;
mod orchestration;
mod creation_base;
mod creation;
mod creation_setup;
mod creation_launch;
#[cfg(test)]
mod creation_tests;

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub setup: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_base: Option<String>,
    #[serde(default)]
    pub setup_default: SetupPolicy,
    #[serde(default)]
    pub direct: bool,
    #[serde(default)]
    pub terminals: Vec<Descriptor>,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetupPolicy {
    Run,
    #[default]
    Skip,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SetupSnapshot {
    pub decision: SetupPolicy,
    pub script: String,
    pub digest: String,
    pub reviewed_at: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Preparing,
    Running,
    Verifying,
    Review,
    NeedsAttention,
    NeedsRecovery,
    Cancelled,
    Delivered,
}
impl Phase {
    fn active(&self) -> bool {
        matches!(self, Self::Preparing | Self::Running | Self::Verifying)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Check {
    pub command: String,
    pub passed: Option<bool>,
    pub output: String,
    pub exit: Option<i32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub spec: TaskSpec,
    pub phase: Phase,
    pub worktree: PathBuf,
    pub branch: String,
    pub base_oid: String,
    pub agent: Option<Descriptor>,
    pub command: Option<Descriptor>,
    pub native_id: Option<String>,
    pub deadline: Option<i64>,
    pub created_at: String,
    pub reason: String,
    pub verified: bool,
    pub checks: Vec<Check>,
    pub retries: u32,
    pub observed_turns: u64,
    pub status: Option<Status>,
    pub pr_url: Option<String>,
    pub allowed_commands: Vec<String>,
    #[serde(default)]
    pub shell: Option<Descriptor>,
    #[serde(default)]
    pub direct: bool,
    #[serde(default)]
    pub delivery: Option<delivery::DeliveryProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_snapshot: Option<SetupSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_id: Option<String>,
}

struct Service {
    root: PathBuf,
    executable: PathBuf,
    projects: Vec<Project>,
    tasks: Vec<Task>,
    project_locks: HashMap<String, File>,
    workbench: crate::workbench::Workbench,
    capabilities: capabilities::Capabilities,
    terminal_gates: HashMap<String,Arc<Mutex<()>>>,
    terminal_inflight: std::collections::HashSet<String>,
    terminal_epoch: u64,
    collaboration: Arc<Mutex<Option<Arc<crate::collaboration::Hub>>>>,
}

pub fn lock_project(root: &Path) -> Result<File> {
    std::fs::create_dir_all(root.join(".velella"))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".velella/runtime.lock"))?;
    file.try_lock_exclusive()
        .context("another runtime owns this project")?;
    Ok(file)
}

fn db(project: &Project) -> Result<Connection> {
    let path = project.path.join(".velella/state.db");
    let _ = velella_core::store::Store::open(&path)?;
    let conn = Connection::open(&path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(version <= 4, "database is newer than this runtime");
    if version < 4 {
        let backup = path.with_extension(format!(
            "db.pre-desktop-{}.bak",
            chrono::Utc::now().timestamp_millis()
        ));
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS desktop_tasks (id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS desktop_events (seq INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL, ts TEXT NOT NULL, phase TEXT NOT NULL, reason TEXT NOT NULL);
            PRAGMA user_version=4; COMMIT;")?;
    }
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

fn save(project: &Project, task: &Task) -> Result<()> {
    let mut conn = db(project)?;
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO desktop_tasks(id,document) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET document=excluded.document", params![task.id,serde_json::to_string(task)?])?;
    tx.execute(
        "INSERT INTO desktop_events(task_id,ts,phase,reason) VALUES(?1,?2,?3,?4)",
        params![
            task.id,
            chrono::Utc::now().to_rfc3339(),
            serde_json::to_string(&task.phase)?,
            task.reason
        ],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn serve(root: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("daemon.lock"))?;
    lock.try_lock_exclusive()
        .context("another Velella daemon owns this data directory")?;
    crate::remote_stdio::initialize_identity(&root)?;
    let listener = ipc::listen_recovering(&ipc::endpoint(&root, "daemon"))?;
    let projects: Vec<Project> = if root.join("projects.json").exists() {
        serde_json::from_slice(&std::fs::read(root.join("projects.json"))?)?
    } else {
        vec![]
    };
    let mut tasks = vec![];
    let mut project_locks = HashMap::new();
    let mut workbench = crate::workbench::Workbench::open(&root)?;
    workbench.backup_legacy(None)?;
    for project in &projects {
        project_locks.insert(project.id.clone(), lock_project(&project.path)?);
        workbench.backup_legacy(Some(project))?;
        let conn = db(project)?;
        let mut stmt = conn.prepare("SELECT document FROM desktop_tasks ORDER BY rowid")?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let mut task: Task = serde_json::from_str(&row?)?;
            task.project_id = project.id.clone();
            tasks.push(task);
        }
    }
    workbench.sync_legacy(&projects, &tasks)?;
    let capabilities = capabilities::Capabilities::open(&root)?;
    let service = Arc::new(Mutex::new(Service {
        root,
        executable: std::env::current_exe()?,
        projects,
        tasks,
        project_locks,
        workbench,
        capabilities,
        terminal_gates: HashMap::new(),
        terminal_inflight: std::collections::HashSet::new(),
        terminal_epoch: 0,
        collaboration: Arc::new(Mutex::new(None)),
    }));
    {
        let service = service.clone();
        std::thread::spawn(move || loop {
            if let Err(e)=supervisor_tick(&service){eprintln!("supervisor: {e:#}");}
            if let Err(e)=creation_setup::tick(&service){eprintln!("creation setup: {e:#}");}
            if let Err(e)=creation_launch::tick(&service){eprintln!("creation launch: {e:#}");}
            std::thread::sleep(Duration::from_millis(600));
        });
    }
    {
        let service=service.clone();
        std::thread::spawn(move||loop{
            if let Err(e)=orchestration::tick(&service){eprintln!("orchestration: {e:#}");}
            std::thread::sleep(Duration::from_millis(1000));
        });
    }
    for stream in listener.incoming() {
        let stream = stream?;
        let service = service.clone();
        std::thread::spawn(move || {
            ipc::handle(stream, |req| terminal_io::dispatch(&service, req))
        });
    }
    Ok(())
}

fn supervisor_tick(service:&Arc<Mutex<Service>>)->Result<()>{
    // Poll hosts outside the state lock. An input/control or lifecycle change
    // invalidates the entire observation batch before it can drive automation.
    let (epoch,descriptors)={let state=service.lock().unwrap();(state.terminal_epoch,state.tasks.iter().flat_map(|t|[t.agent.clone(),t.command.clone()]).flatten().collect::<Vec<_>>())};
    let observations=descriptors.iter().map(|d|(d.id.clone(),get_status(d).map_err(|e|e.to_string()))).collect();
    let ids={let mut state=service.lock().unwrap();
        if epoch!=state.terminal_epoch{return Ok(())}
        if let Err(error)=state.tick_automations(){eprintln!("automations: {error:#}");}
        state.tasks.iter().map(|t|t.id.clone()).collect::<Vec<_>>()
    };
    for id in ids {
        let key=format!("task:{id}");
        let gate={service.lock().unwrap().terminal_gates.entry(key.clone()).or_default().clone()};
        let Ok(_guard)=gate.try_lock() else{continue};
        let (mut task,context)={let mut state=service.lock().unwrap();
            if epoch!=state.terminal_epoch{break}
            let Some(task)=state.tasks.iter().find(|t|t.id==id).cloned() else{continue};
            if task.phase==Phase::Queued&&(state.active_tasks()>=3||!state.folder_available(&task)){continue}
            state.terminal_inflight.insert(key.clone());
            (task,state.execution())
        };
        let previous=task.clone();
        if task.phase==Phase::Queued {
            if let Err(error)=context.prepare(&mut task){task.phase=Phase::NeedsAttention;task.reason=format!("启动失败：{error:#}");}
        }else if task.phase.active(){
            if let Err(error)=context.advance(&mut task,&observations){task.phase=Phase::NeedsRecovery;task.reason=format!("需要恢复：{error:#}");}
        }else if let Some(desc)=&task.agent {
            if let Ok(status)=observed_status(&observations,desc){
                if task.phase==Phase::Review&&(status.state=="running"||status.turns>task.observed_turns){task.phase=Phase::Running;task.verified=false;task.reason="原生终端已继续任务；原验证结果失效".into();}
                task.status=Some(status);
            }
        }
        let status=task.status.take();let mut old=previous;old.status=None;
        let saved=if serde_json::to_vec(&task)?!=serde_json::to_vec(&old)?{context.persist(&task)}else{Ok(())};
        if let Err(error)=&saved{let stopped=context.stop(&task);task.phase=Phase::NeedsRecovery;task.reason=format!("状态保存失败：{error}; 停止结果：{stopped:?}");}
        task.status=status;
        {let mut state=service.lock().unwrap();state.terminal_inflight.remove(&key);
            if let Some(current)=state.tasks.iter_mut().find(|t|t.id==id){*current=task;}
        }
        saved?;
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = process::command("git")
        .current_dir(root)
        .args(args)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {}: {}",
        args.first().unwrap_or(&""),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn get_status(desc: &Descriptor) -> Result<Status> {
    let value = match ipc::request_with_timeout(&desc.endpoint, &json!({"cmd":"status"}),Duration::from_millis(250)) {
        Ok(value) => value,
        Err(error) => {
            if host::confirmed_dead(&desc.directory) {
                let saved: Status =
                    serde_json::from_slice(&std::fs::read(desc.directory.join("status.json"))?)?;
                if saved.exit.is_some() {
                    return Ok(saved);
                }
            }
            return Err(error);
        }
    };
    anyhow::ensure!(value["ok"] == true, "session status failed");
    Ok(serde_json::from_value(value["status"].clone())?)
}

fn observed_status(observations:&HashMap<String,std::result::Result<Status,String>>,desc:&Descriptor)->Result<Status>{
    observations.get(&desc.id).context("session changed during status collection")?.as_ref().cloned().map_err(|e|anyhow::anyhow!(e.clone()))
}

fn start_host(executable:&Path,desc:&Descriptor)->Result<()> {
    workbench_commands::validate_saved_launch(desc)?;
    let file=desc.directory.join("launch.json");
    ipc::atomic_json(&file,desc)?;
    ipc::spawn_hidden(process::command(executable.to_str().context("runtime path")?).arg("host").arg(&file))?;
    let until=Instant::now()+Duration::from_secs(8);
    loop {
        if ipc::request_with_timeout(&desc.endpoint,&json!({"cmd":"status"}),Duration::from_millis(250)).is_ok_and(|value|value["ok"]==true&&serde_json::from_value::<Status>(value["status"].clone()).is_ok()){return Ok(())}
        anyhow::ensure!(Instant::now()<until,"session did not start; inspect {}",desc.directory.display());
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn call_host(desc: &Descriptor, req: Value) -> Result<Value> {
    let response = terminal_io::ordered_host_request(desc,&req)?;
    anyhow::ensure!(
        response["ok"] == true,
        "{}",
        response["error"]
            .as_str()
            .unwrap_or("session operation failed")
    );
    Ok(response)
}

fn stop_host(desc: &Descriptor) -> Result<()> {
    match get_status(desc) {
        Ok(status) if status.exit.is_some() => Ok(()),
        Ok(_) => {
            call_host(desc, json!({"cmd":"kill"}))?;
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if get_status(desc)?.exit.is_some() {
                    return Ok(());
                }
                anyhow::ensure!(Instant::now() < until, "executor has not confirmed exit");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Err(_) if host::confirmed_dead(&desc.directory) => Ok(()),
        Err(e) => Err(e.context("cannot confirm old executor stopped")),
    }
}

fn file_tail(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let end = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(end.saturating_sub(32 * 1024)));
    let mut bytes = Vec::new();
    let _ = file.take(32 * 1024).read_to_end(&mut bytes);
    crate::adapter::strip_ansi(&String::from_utf8_lossy(&bytes))
}

fn renewed_deadline(task: &Task) -> Result<i64> {
    let seconds = parse_wall_clock(
        task.spec
            .policy
            .budget
            .wall_clock
            .as_deref()
            .unwrap_or("8h"),
    )
    .context("invalid time limit")?
    .as_secs();
    Ok(chrono::Utc::now().timestamp() + seconds as i64)
}

impl Service {
    fn active_tasks(&self)->usize{self.tasks.iter().filter(|task|task.phase.active()||self.terminal_inflight.contains(&format!("task:{}",task.id))).count()}
    fn validate_task_execution(&self,task:&Task,req:&Value)->Result<()> {
        let cmd=req["cmd"].as_str().unwrap_or("");
        if cmd!="task.finish"{anyhow::ensure!(self.folder_available(task),"该文件夹还有未结束的任务，请先结束或停止它");}
        if matches!(cmd,"task.resume"|"task.feedback")||(cmd=="task.verify"&&!task.checks.is_empty()) {
            anyhow::ensure!(self.active_tasks()<3,"three tasks are already running");
        }
        Ok(())
    }
    fn folder_available(&self, task: &Task) -> bool {
        let workspace=format!("main:{}",task.project_id);
        if task.direct && (self.terminal_inflight.contains(&format!("workspace:{workspace}")) || self.workbench.sessions(Some(&workspace)).map_or(true, |sessions| sessions.iter().any(|s| s.task_id.is_none() && (self.terminal_inflight.contains(&format!("session:{}",s.id)) || !host::confirmed_dead(&s.descriptor.directory))))) { return false; }
        !task.direct || (!self.project(&task.project_id).is_ok_and(|p| p.terminals.iter().any(|d| !host::confirmed_dead(&d.directory))) && !self.tasks.iter().any(|other| {
            other.id != task.id && other.project_id == task.project_id
                && (self.terminal_inflight.contains(&format!("task:{}",other.id)) || !matches!(other.phase, Phase::Queued | Phase::Cancelled | Phase::Delivered))
        }))
    }
    fn project(&self, id: &str) -> Result<Project> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .context("project not found")
    }
    fn persist(&self, task: &Task) -> Result<()> {
        save(&self.project(&task.project_id)?, task)
    }

    fn execution(&self)->task_execution::TaskExecution {
        task_execution::TaskExecution{root:self.root.clone(),executable:self.executable.clone(),projects:self.projects.clone()}
    }
    fn start(&self,desc:&Descriptor)->Result<()>{start_host(&self.executable,desc)}
    fn stop(&self,task:&Task)->Result<()>{self.execution().stop(task)}

    fn dispatch(&mut self, req: Value) -> Result<Value> {
        match req["cmd"].as_str().unwrap_or("") {
            "runtime.identity" => crate::remote_stdio::identity(&self.root),
            "cli.inspect" => Ok(cli_inspection::inspect()),
            "sandbox.capabilities" => Ok(json!({"ok":true,"sandbox":crate::sandbox::capabilities()})),
            "project.create" => self.create_project_folder(req),
            cmd if cmd.starts_with("ssh.") || cmd.starts_with("automation.") || cmd.starts_with("worktree.") => self.capability_command(req),
            cmd if cmd.starts_with("workbench.") => {
                self.workbench.sync_legacy(&self.projects, &self.tasks)?;
                self.workbench_command(req)
            }
            "protocol" => Ok(json!({"ok":true,"protocol":1,"submission_tracking":1,"capabilities":["workspaces","sessions","layout-v1","event-cursor-v1","files","git-local","automation-archive-v1","automation-history-v1","terminal-permissions-v1","terminal-accounts-v1"],"event_subscription":false})),
            "snapshot" => Ok(
                json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"projects":self.projects,"tasks":self.tasks,"max_parallel":3,"pid":std::process::id()}),
            ),
            "project.add" => {
                let path = req["path"].as_str().context("请选择项目文件夹")?.trim();
                anyhow::ensure!(!path.is_empty(), "请选择项目文件夹，示例路径不能代替实际路径");
                let input = dunce::canonicalize(path).context("无法打开项目文件夹，请检查路径")?;
                anyhow::ensure!(input.is_dir(), "请选择文件夹，而不是文件");
                // Probe Git only when metadata exists in this directory or an
                // ancestor. A broken/inaccessible repository is not silently
                // downgraded to in-place execution.
                let has_git = input.ancestors().any(|p| p.join(".git").exists());
                let root = if has_git {
                    dunce::canonicalize(git(&input, &["rev-parse", "--show-toplevel"])?.trim())?
                } else { input };
                if let Some(project) = self.projects.iter().find(|p| p.path == root) {
                    return Ok(json!({"ok":true,"project":project}));
                }
                anyhow::ensure!(!self.projects.iter().any(|p| (!has_git || p.direct) && (root.starts_with(&p.path) || p.path.starts_with(&root))), "该文件夹与已导入项目重叠，请使用已有项目");
                let project = Project {
                    id: uuid::Uuid::now_v7().to_string(),
                    name: root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: root,
                    setup: String::new(),
                    default_base: None,
                    setup_default: SetupPolicy::Skip,
                    direct: !has_git,
                    terminals: Vec::new(),
                };
                if has_git { let exclude = PathBuf::from(
                    git(&project.path, &["rev-parse", "--git-path", "info/exclude"])?.trim(),
                );
                let exclude = if exclude.is_absolute() {
                    exclude
                } else {
                    project.path.join(exclude)
                };
                let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
                if !existing.contains("# Velella desktop runtime") {
                    if let Some(parent) = exclude.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&exclude,format!("{existing}\n# Velella desktop runtime\n.velella/state.db*\n.velella/sessions/\n.velella/worktrees/\n.velella/*.bak\n.velella/runtime.lock\n"))?;
                } }
                let ownership = lock_project(&project.path)?;
                self.workbench.backup_legacy(Some(&project))?;
                db(&project)?;
                let conn = db(&project)?;
                let mut stmt = conn.prepare("SELECT document FROM desktop_tasks ORDER BY rowid")?;
                for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
                    let mut task: Task = serde_json::from_str(&row?)?;
                    task.project_id = project.id.clone();
                    self.tasks.push(task);
                }
                self.project_locks.insert(project.id.clone(), ownership);
                self.projects.push(project.clone());
                ipc::atomic_json(&self.root.join("projects.json"), &self.projects)?;
                Ok(json!({"ok":true,"project":project}))
            }
            "project.update" => {
                let mut projects = self.projects.clone();
                let p = projects.iter_mut().find(|p| Some(p.id.as_str()) == req["project_id"].as_str()).context("project not found")?;
                if let Some(setup) = req.get("setup") {
                    let setup = setup.as_str().context("setup must be a string")?;
                    anyhow::ensure!(setup.len() <= 65536 && !setup.contains('\0'), "setup is too large or contains NUL");
                    p.setup = setup.into();
                }
                if let Some(base) = req.get("default_base") {
                    p.default_base = if base.is_null() { None } else {
                        let base = base.as_str().context("default_base must be a string or null")?;
                        anyhow::ensure!(!base.trim().is_empty() && base.len() <= 1024 && !base.chars().any(char::is_control) && !base.trim().starts_with('-'), "invalid default_base");
                        Some(base.trim().into())
                    };
                }
                if let Some(policy) = req.get("setup_default") {
                    p.setup_default = serde_json::from_value(policy.clone()).context("setup_default must be run or skip")?;
                }
                let project = p.clone();
                ipc::atomic_json(&self.root.join("projects.json"), &projects)?;
                self.projects = projects;
                Ok(json!({"ok":true,"project":project}))
            }
            "agent.check" => {let agent=req["agent"].as_str().context("agent required")?;let home=account_launch::home(&req,agent)?;agent_check_for_home(agent,home.as_deref())},
            cmd if cmd.starts_with("workspace.") => self.workspace_command(req),
            "task.create" => self.create_task(req),
            "service.stop" => {
                for session in self.workbench.sessions(None)? { stop_host(&session.descriptor)?; }
                for project in &self.projects {
                    for terminal in &project.terminals { stop_host(terminal)?; }
                }
                for task in &self.tasks {
                    self.stop(task)?;
                }
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(300));
                    std::process::exit(0);
                });
                Ok(json!({"ok":true}))
            }
            _ => {
                let result = self.task_command(req);
                if result.is_err() {
                    // A write-ahead transition may have succeeded before an IPC
                    // operation failed. Reload it rather than acting on stale memory.
                    for project in &self.projects {
                        let conn = db(project)?;
                        let mut stmt = conn.prepare("SELECT document FROM desktop_tasks")?;
                        for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
                            let recovered: Task = serde_json::from_str(&row?)?;
                            if let Some(index) =
                                self.tasks.iter().position(|t| t.id == recovered.id)
                            {
                                self.tasks[index] = recovered;
                            }
                        }
                    }
                }
                result
            }
        }
    }

    fn workspace_command(&mut self, req: Value) -> Result<Value> {
        let index = self.projects.iter().position(|p| Some(p.id.as_str()) == req["project_id"].as_str()).context("project not found")?;
        let project = self.projects[index].clone();
        let cmd = req["cmd"].as_str().unwrap_or("");
        if matches!(cmd, "workspace.open" | "workspace.input") && project.direct {
            anyhow::ensure!(!self.tasks.iter().any(|t| t.project_id == project.id && !matches!(t.phase, Phase::Queued | Phase::Cancelled | Phase::Delivered)), "此文件夹正在处理任务，请先结束或停止任务再使用工作区终端");
        }
        if cmd == "workspace.list" {
            let terminals: Vec<_> = project.terminals.iter().map(|d| json!({"id":d.id,"status":get_status(d).ok()})).collect();
            return Ok(json!({"ok":true,"terminals":terminals}));
        }
        if cmd == "workspace.open" {
            if req["reuse"] == true {
                if let Some(desc) = project.terminals.iter().find(|d| get_status(d).is_ok_and(|s| s.exit.is_none())) {
                    return Ok(json!({"ok":true,"session_id":desc.id}));
                }
            }
            anyhow::ensure!(project.terminals.len() < 16, "终端标签已达上限，请先关闭不需要的标签");
            let id = uuid::Uuid::now_v7().to_string();
            let directory = project.path.join(".velella/sessions").join(&id);
            std::fs::create_dir_all(&directory)?;
            let mut env = HashMap::new();
            env.insert("TERM".into(), "xterm-256color".into());
            let desc = Descriptor {
                id, endpoint: ipc::endpoint(&self.root, &uuid::Uuid::now_v7().to_string()), directory,
                spawn: SpawnRequest { sandbox: Default::default(),
                    program: if cfg!(windows) {"powershell"} else {"sh"}.into(),
                    args: if cfg!(windows) {vec!["-NoLogo".into(), "-NoProfile".into()]} else {vec![]},
                    cwd: Some(project.path.to_string_lossy().into_owned()), env, adapter: Some("generic".into()),
                },
            };
            self.projects[index].terminals.push(desc.clone());
            if let Err(error) = ipc::atomic_json(&self.root.join("projects.json"), &self.projects) {
                self.projects[index].terminals.pop(); return Err(error);
            }
            self.start(&desc)?;
            return Ok(json!({"ok":true,"session_id":desc.id}));
        }
        let terminal_index = project.terminals.iter().position(|d| Some(d.id.as_str()) == req["session_id"].as_str()).context("workspace terminal not found")?;
        let desc = &project.terminals[terminal_index];
        match cmd {
            "workspace.output" => host::read_output(desc, req["since"].as_u64().unwrap_or(0)),
            "workspace.input" | "workspace.resize" | "workspace.control" => {
                let mut forwarded = req.clone(); forwarded["cmd"] = json!(cmd.trim_start_matches("workspace."));
                forwarded["automated"] = json!(false);
                call_host(desc, forwarded)
            }
            "workspace.close" => {
                stop_host(desc)?;
                let mut projects = self.projects.clone();
                projects[index].terminals.remove(terminal_index);
                ipc::atomic_json(&self.root.join("projects.json"), &projects)?;
                self.projects = projects;
                Ok(json!({"ok":true}))
            }
            _ => anyhow::bail!("unknown workspace operation"),
        }
    }

    fn create_task(&mut self, req: Value) -> Result<Value> {
        let id=creation::request_id(&req)?;
        let existing=self.workbench.creation(&id,Some(&req))?;
        let key=req["source_key"].as_str().map(str::to_owned);
        if let Some(key)=&key {
            let project=req["project_id"].as_str().context("project required")?;
            let sources=crate::task_sources::Store::open(&self.root)?;
            sources.item(key)?;
            if existing.is_none() {
                if let Some(workspace)=sources.linked_workspace(key,project)? {
                    if let Some(task)=self.tasks.iter().find(|t|format!("task:{}",t.id)==workspace){return Ok(json!({"ok":true,"task":task}));}
                    anyhow::bail!("此来源任务已有开发工作区，请从任务页打开");
                }
            }
        }
        let result=self.create_task_with_id(req, id)?;
        if let Some(key)=key {
            let task:Task=serde_json::from_value(result["task"].clone())?;
            let workspace=if task.direct{format!("main:{}",task.project_id)}else{format!("task:{}",task.id)};
            crate::task_sources::Store::open(&self.root)?.attach(&key,&workspace,&task.project_id)?;
        }
        Ok(result)
    }

    fn create_task_with_id(&mut self, req: Value, id: String) -> Result<Value> {
        if let Some(operation)=self.workbench.creation(&id,Some(&req))? {
            return self.persist_created_task(serde_json::from_value(operation.frozen["task"].clone())?);
        }
        let project = self.project(req["project_id"].as_str().context("project required")?)?;
        let raw_spec:Value = if let Some(yaml) = req["yaml"].as_str() {
            serde_yaml::from_str(yaml)?
        } else { req["spec"].clone() };
        let mut spec: TaskSpec = serde_json::from_value(raw_spec.clone())?;
        crate::native_launch::args(&spec.agent,&spec.launch)?;
        anyhow::ensure!(
            ["claude-code", "codex"].contains(&spec.agent.as_str()),
            "supported agents: claude-code, codex"
        );
        anyhow::ensure!(
            !spec.name.trim().is_empty() && spec.name.len() <= 160,
            "task name required (max 160 bytes)"
        );
        anyhow::ensure!(
            spec.instruction
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty()),
            "task instruction required"
        );
        anyhow::ensure!(
            spec.policy.permissions == Permissions::Ask,
            "desktop supports task-level native permissions only"
        );
        anyhow::ensure!(
            spec.policy.budget.tokens.is_none(),
            "hard token budgets are not supported by both CLIs"
        );
        let cap = parse_wall_clock(spec.policy.budget.wall_clock.as_deref().unwrap_or("8h"))
            .context("invalid time limit")?;
        anyhow::ensure!(
            cap.as_secs() > 0 && cap.as_secs() <= 7 * 86400,
            "time limit must be between 1 second and 7 days"
        );
        anyhow::ensure!(spec.verify.retries <= 1, "at most one repair retry");
        anyhow::ensure!(
            spec.env.is_none(),
            "configure environment preparation on the project"
        );
        let original_delivery = serde_json::to_value(spec.on_done)?;
        anyhow::ensure!(
            spec.on_done == OnDone::Review || req["confirm_review"] == true,
            "import requests delivery {original_delivery}; confirm local review before importing"
        );
        spec.on_done = OnDone::Review;
        if spec.policy.execution.is_sandboxed() {
            anyhow::ensure!(!project.direct, "sandboxed tasks require a Git worktree; direct folders are not supported");
            if let crate::sandbox::Policy::Isolated { home, .. } | crate::sandbox::Policy::Restricted { home, .. } = &spec.policy.execution {
                anyhow::ensure!(home.is_none(), "task HOME is managed by Velella; omit execution.home");
            }
            crate::sandbox::ensure_available(&spec.policy.execution)?;
        }

        let base_oid = if project.direct { String::new() } else {
            let base=creation_base::resolve(&project.path,raw_spec.get("base").and_then(Value::as_str),project.default_base.as_deref())?;
            spec.base=base.reference;
            base.oid
        };
        let parent_workspace_id=creation::parent(&req)?;
        self.workbench.sync_legacy(&self.projects,&self.tasks)?;
        self.workbench.check_parent(&if project.direct{format!("main:{}",project.id)}else{format!("task:{id}")},parent_workspace_id.as_deref())?;
        let setup_snapshot=creation::setup_snapshot(&project,&req,true)?;
        let checks = spec
            .done
            .iter()
            .map(|check| match check {
                velella_core::task::DoneCheck::Run(command) => Ok(Check {
                    command: command.clone(),
                    passed: None,
                    exit: None,
                    output: String::new(),
                }),
                _ => Err(anyhow::anyhow!(
                    "desktop currently accepts executable verification commands"
                )),
            })
            .collect::<Result<Vec<_>>>()?;
        let allowed_commands: Vec<String> =
            serde_json::from_value(req.get("allowed_commands").cloned().unwrap_or(json!([])))?;
        anyhow::ensure!(
            spec.agent == "claude-code" || allowed_commands.is_empty(),
            "Codex uses its workspace sandbox and native approval rules"
        );
        let task = Task {
            id: id.clone(),
            project_id: project.id.clone(),
            spec,
            phase: Phase::Queued,
            worktree: if project.direct { project.path.clone() } else { project.path.join(".velella/worktrees").join(&id) },
            branch: if project.direct { String::new() } else { format!("velella/{id}") },
            direct: project.direct,
            delivery: None,
            parent_workspace_id,
            setup_snapshot: Some(setup_snapshot),
            creation_id: Some(id.clone()),
            base_oid,
            agent: None,
            command: None,
            native_id: None,
            deadline: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            reason: "已加入队列".into(),
            verified: false,
            checks,
            retries: 0,
            observed_turns: 0,
            status: None,
            pr_url: None,
            allowed_commands,
            shell: None,
        };
        let operation=self.workbench.accept_creation(&id,&req,&json!({"task":task}))?;
        self.persist_created_task(serde_json::from_value(operation.frozen["task"].clone())?)
    }

    fn task_command(&mut self, req: Value) -> Result<Value> {
        let id = req["task_id"].as_str().context("task ID required")?;
        let index = self
            .tasks
            .iter()
            .position(|t| t.id == id)
            .context("task not found")?;
        let mut task = self.tasks[index].clone();
        let cmd = req["cmd"].as_str().unwrap_or("");
        let protocol_input=cmd=="terminal.input" && (req["input_kind"]=="protocol"||req["input_kind"]=="pointer");
        if cmd=="terminal.input" && req["input_kind"]=="user" && req["target"]!="shell" {
            anyhow::ensure!(task.phase!=Phase::Verifying,"verification is in progress; wait for it before continuing the Agent");
        }
        if !(cmd=="terminal.input"&&req["input_kind"]=="protocol") && matches!(cmd, "terminal.input" | "terminal.shell" | "file.write" | "task.resume" | "task.verify" | "task.feedback") {
            anyhow::ensure!(self.folder_available(&task), "该文件夹还有未结束的任务，请先结束或停止它");
        }
        let response = match cmd {
            "terminal.output" | "terminal.input" | "terminal.resize" | "terminal.control" => {
                let desc = if req["target"] == "command" {
                    task.command.as_ref()
                } else if req["target"] == "shell" {
                    task.shell.as_ref()
                } else {
                    task.agent.as_ref()
                }
                .context("terminal not started")?;
                let mut forwarded = req.clone();
                forwarded["cmd"] = json!(cmd.trim_start_matches("terminal."));
                if cmd == "terminal.input" {
                    forwarded["automated"] = json!(false);
                    if protocol_input {
                        let text=req["text"].as_str().unwrap_or("");
                        anyhow::ensure!(req["enter"]!=true&&if req["input_kind"]=="pointer"{crate::terminal_input::is_pointer_report(text)}else{crate::terminal_input::is_protocol_reply(text)},"invalid terminal non-driving input");
                        // Older live hosts know legacy input; mouse reports must
                        // never claim typing ownership, even across daemon upgrades.
                        if req["input_kind"]=="pointer"{forwarded["input_kind"]=json!("legacy");}
                    } else {task.verified = false;}
                    if req.get("input_kind").is_some(){anyhow::ensure!(get_status(desc)?.input_protocol==1,"session host does not support typed input; restore this session explicitly");}
                }
                if cmd == "terminal.output" {
                    host::read_output(desc, req["since"].as_u64().unwrap_or(0))?
                } else {
                    call_host(desc, forwarded)?
                }
            }
            "task.cancel" => {
                self.stop(&task)?;
                task.phase = Phase::Cancelled;
                task.reason = "已停止；工作目录与改动保留".into();
                json!({"ok":true})
            }
            "terminal.shell"|"task.resume"|"task.verify"|"task.feedback"=>{
                self.validate_task_execution(&task,&req)?;
                self.execution().command(&mut task,&req)?
            },
            "task.files" => {
                let names = if task.direct { folder_files(&task.worktree)?.join("\0") } else { git(
                    &task.worktree,
                    &[
                        "ls-files",
                        "-z",
                        "--cached",
                        "--others",
                        "--exclude-standard",
                    ],
                )? };
                let files: Vec<_> = names
                    .split('\0')
                    .filter(|s| !s.is_empty() && !s.starts_with(".velella/"))
                    .take(3000)
                    .collect();
                json!({"ok":true,"files":files})
            }
            "file.read" => {
                let relative = req["path"].as_str().context("path required")?;
                let path = contained_file(&task.worktree, relative)?;
                anyhow::ensure!(
                    path.metadata()?.len() <= 2 * 1024 * 1024,
                    "file is larger than 2 MiB"
                );
                let content = std::fs::read_to_string(&path).context("file is not UTF-8 text")?;
                let original = if task.direct { String::new() } else { git(
                    &task.worktree,
                    &["show", &format!("{}:{relative}", task.base_oid)],
                )
                .unwrap_or_default() };
                json!({"ok":true,"content":content,"original":original})
            }
            "file.write" => {
                if task.phase.active() {
                    let s = get_status(
                        task.agent
                            .as_ref()
                            .context("wait for environment preparation")?,
                    )?;
                    anyhow::ensure!(
                        s.human_control && s.state == "waiting_input",
                        "take control and interrupt the Agent before editing files"
                    );
                }
                let path = contained_file(
                    &task.worktree,
                    req["path"].as_str().context("path required")?,
                )?;
                let expected = req["previous"]
                    .as_str()
                    .context("previous content required")?;
                anyhow::ensure!(
                    std::fs::read_to_string(&path)? == expected,
                    "file changed since it was opened; reload before saving"
                );
                std::fs::write(path, req["content"].as_str().context("content required")?)?;
                task.verified = false;
                json!({"ok":true})
            }
            "task.diff" => {
                if task.direct {
                    return Ok(json!({"ok":true,"diff":"普通文件夹直接修改原文件，没有 Git 基线。请在文件视图检查结果。","status":"普通文件夹 · 原目录执行"}));
                }
                let diff = git(
                    &task.worktree,
                    &["diff", "--no-ext-diff", "--no-color", &task.base_oid, "--"],
                )?;
                let status = git(&task.worktree, &["status", "--short"])?;
                json!({"ok":true,"diff":diff,"status":status})
            }
            "task.events" => {
                let conn = db(&self.project(&task.project_id)?)?;
                let mut stmt=conn.prepare("SELECT seq,ts,phase,reason FROM desktop_events WHERE task_id=?1 AND seq>?2 ORDER BY seq LIMIT 500")?;
                let rows=stmt.query_map(params![task.id,req["since"].as_i64().unwrap_or(0)],|r|Ok(json!({"seq":r.get::<_,i64>(0)?,"ts":r.get::<_,String>(1)?,"phase":r.get::<_,String>(2)?,"reason":r.get::<_,String>(3)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
                json!({"ok":true,"events":rows})
            }
            "task.commit" | "task.pr" => {
                anyhow::ensure!(req["confirmed"]==true,"explicit review confirmation required");
                match self.deliver_task(index,&mut task,cmd=="task.pr") {
                    Ok(result)=>result,
                    Err(error)=>{
                        task.reason=format!("交付未完成：{error:#}");
                        self.persist(&task)?;
                        self.tasks[index]=task;
                        return Err(error);
                    }
                }
            }
            "task.delivery.reset" => {
                anyhow::ensure!(task.phase==Phase::Review&&req["confirmed"]==true,"请确认重新审查；已有提交和远端操作不会回滚");
                let progress=task.delivery.take().context("没有待恢复的交付步骤")?;
                task.verified=false;
                task.reason=format!("已重新开始本地审查；原提交 {} 和推送记录({}) 保留，不会回滚",progress.commit_oid.as_deref().unwrap_or("未完成"),progress.pushed);
                json!({"ok":true})
            }
            "task.finish" => {
                anyhow::ensure!(task.direct && task.phase == Phase::Review, "只能结束待审查的普通文件夹任务");
                anyhow::ensure!(req["confirmed"] == true, "请确认已检查任务结果");
                self.stop(&task)?;
                task.phase = Phase::Delivered;
                task.reason = "任务已结束；文件保留在原目录".into();
                json!({"ok":true})
            }
            _ => anyhow::bail!("unknown command: {cmd}"),
        };
        if serde_json::to_vec(&task)? != serde_json::to_vec(&self.tasks[index])? {
            self.persist(&task)?;
            self.tasks[index] = task;
        }
        Ok(response)
    }
}

fn folder_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        if depth > 32 { continue; }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            visited += 1;
            if visited > 20000 || files.len() >= 3000 { files.sort(); return Ok(files); }
            let name = entry.file_name();
            if matches!(name.to_str(), Some(".velella" | ".git" | "node_modules" | ".venv" | "target")) { continue; }
            let path = entry.path();
            if entry.file_type()?.is_symlink() { continue; }
            let resolved = dunce::canonicalize(&path)?;
            if !resolved.starts_with(root) { continue; }
            if entry.file_type()?.is_dir() { pending.push((path, depth + 1)); }
            else if entry.file_type()?.is_file() { files.push(path.strip_prefix(root)?.to_string_lossy().replace('\\', "/")); }
        }
    }
    files.sort();
    Ok(files)
}

fn hook_settings() -> Value { hook_settings_for("velella-runtime hook") }

fn hook_settings_for(command: &str) -> Value {
    let mut hooks = serde_json::Map::new();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "Notification",
        "Stop",
    ] {
        hooks.insert(
            event.into(),
            json!([{"hooks":[{"type":"command","command":command}]}]),
        );
    }
    Value::Object(hooks)
}

fn contained_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    anyhow::ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "invalid project-relative path"
    );
    let path = dunce::canonicalize(root.join(relative))?;
    let root = dunce::canonicalize(root)?;
    anyhow::ensure!(path.starts_with(&root), "file leaves the task worktree");
    Ok(path)
}

fn agent_check_for_home(agent: &str, home:Option<&str>) -> Result<Value> {
    let program = match agent {
        "claude-code" => "claude",
        "codex" => "codex",
        "minimax" => "mcode",
        _ => anyhow::bail!("unknown agent"),
    };
    let run = |args: &[&str]| crate::native_launch::account_diagnostic(program,args,home);
    let version = run(&["--version"])?;
    if program == "mcode" {
        return Ok(json!({"ok":true,"agent":agent,"available":version.0,"version":version.1.trim(),"authenticated":null,"auth_status":"use mcode login; authentication not probed","hooks":"session identity requires observation"}));
    }
    let help = run(&["--help"])?;
    let help_text = &help.1;
    let supports_model = help.0 && crate::native_launch::check_capabilities(agent,&velella_core::task::LaunchOptions{model:Some("requested-unverified".into()),effort:None},help_text).is_ok();
    let supports_effort = help.0 && crate::native_launch::check_capabilities(agent,&velella_core::task::LaunchOptions{model:None,effort:Some("high".into())},help_text).is_ok();
    let auth = run(if program == "claude" {
        &["auth", "status"]
    } else {
        &["login", "status"]
    })?;
    // Do not return auth payloads: some providers include account details.
    let authenticated = if program == "claude" {
        serde_json::from_str::<Value>(&auth.1)
            .ok()
            .and_then(|v| v["loggedIn"].as_bool())
    } else {
        Some(auth.0)
    };
    Ok(
        json!({"ok":true,"agent":agent,"available":version.0,"version":version.1.trim(),"authenticated":authenticated,"hooks":"requires first-session trust check","launch_capabilities":{"model":supports_model,"effort":supports_effort,"evidence":"target_cli_help","model_status":"requested_unverified"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_phases_do_not_consume_queue_slots() {
        for p in [
            Phase::Review,
            Phase::NeedsRecovery,
            Phase::Cancelled,
            Phase::Delivered,
        ] {
            assert!(!p.active());
        }
    }
    #[test]
    fn path_traversal_rejected() {
        let root = std::env::temp_dir();
        for s in ["../secret", "C:\\Windows\\win.ini", "a/../../b"] {
            assert!(contained_file(&root, s).is_err());
        }
    }
    #[test]
    fn hooks_never_auto_approve() {
        let value = hook_settings().to_string();
        assert!(!value.contains("allow"));
        assert!(!value.contains("bypass"));
    }
}
