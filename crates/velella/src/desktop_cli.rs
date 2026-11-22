//! Public CLI for the desktop daemon. The legacy TCP task CLI remains separate.
use anyhow::{Context, Result};
use base64::Engine;
use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand};
use serde_json::{json, Value};
use std::{ffi::OsString, path::PathBuf, time::Duration};

mod management;
mod services;
mod browser;
mod terminal_management;
mod remote;
mod skill_management;
mod terminal_search;
mod native_search;
mod submission;
mod codex_mcp;
use management::{SwarmCommand, TaskCommand};

#[derive(Parser)]
#[command(
    name = "velella-cli",
    version,
    about = "Manage Velella's projects, agents, tasks and orchestration",
    after_help = "Start with: velella-cli status --json\nGuides: velella-cli skills bundled\nRead: velella-cli skills get velella-cli"
)]
struct Cli {
    #[command(flatten)]
    args: Args,
}

#[derive(ClapArgs)]
pub struct Args {
    /// Desktop profile; defaults to VELELLA_DATA_DIR, then the desktop's default
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Emit one JSON object on stdout, including failures
    #[arg(long, global = true)]
    json: bool,
    /// IPC deadline in milliseconds (mutations are never retried automatically)
    #[arg(long, global = true, default_value_t = 45000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    timeout_ms: u64,
    /// Saved local SSH profile; requires an inspected server identity and root
    #[arg(long, global = true, requires_all = ["server_id", "remote_data_dir"])]
    ssh_profile: Option<String>,
    #[arg(long, global = true, requires = "ssh_profile")]
    server_id: Option<uuid::Uuid>,
    /// Exact remote root returned by host inspect-runtime, never a local path
    #[arg(long, global = true, requires = "ssh_profile")]
    remote_data_dir: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Machine-readable command discovery for Agents; works offline
    #[command(visible_alias = "agent-context")]
    Schema,
    /// Check the existing desktop daemon without launching or restarting it
    Status,
    /// Search local Codex/Claude conversation files, scoped to an exact folder
    Search(native_search::Options),
    /// Print the bundled Agent usage guide; works without a running daemon
    Guide,
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    #[command(visible_alias = "agent")]
    Terminal {
        #[command(subcommand)]
        command: TerminalCommand,
    },
    /// Create, inspect and control desktop tasks
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Manage the fish swarm using the existing orchestration runtime
    #[command(visible_alias = "orchestration")]
    Swarm {
        #[command(subcommand)]
        command: SwarmCommand,
    },
    /// Session messages, membership and work receipts over direct local IPC
    #[command(visible_alias = "collaboration")]
    Collab {
        /// Explicit credentials override environment and session discovery
        #[arg(long, global = true)]
        credentials: Option<PathBuf>,
        /// Stable mutation ID; reuse the same ID and content after an unknown response
        #[arg(long, global = true)]
        request_id: Option<String>,
        #[command(subcommand)]
        command: crate::collaboration::cli::Command,
    },
    /// Scheduled task management (saving or running requires explicit confirmation)
    Automation {
        #[command(subcommand)]
        command: services::AutomationCommand,
    },
    /// Saved SSH profiles and reviewed interactive SSH sessions
    Host {
        #[command(subcommand)]
        command: services::HostCommand,
    },
    /// Discover native skills and manage reviewed local packages with backups
    Skills {
        #[command(subcommand)]
        command: services::SkillsCommand,
    },
    /// Preview and confirm MCP source entries and explicit tool scopes
    Mcp {
        #[command(subcommand)]
        command: services::McpCommand,
    },
    /// Files inside an exact workspace, using the runtime's path and concurrency checks
    File {
        #[command(subcommand)]
        command: services::FileCommand,
    },
    /// Local Git status, diff, staging and explicit commits
    Git {
        #[command(subcommand)]
        command: services::GitCommand,
    },
    /// Velella's embedded browser; requires the desktop's browser bridge
    Browser {
        #[command(subcommand)]
        command: browser::BrowserCommand,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    List,
    Show {
        id: String,
    },
    /// Import an existing repository or ordinary folder
    Add {
        path: PathBuf,
    },
    /// Review or configure the project's base and preparation policy
    Configure {
        id: String,
        #[arg(long, conflicts_with = "reset_base")]
        default_base: Option<String>,
        #[arg(long)]
        reset_base: bool,
        #[arg(long)]
        setup_file: Option<PathBuf>,
        #[arg(long, value_parser = ["run", "skip"])]
        setup_default: Option<String>,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    /// Update only supplied metadata; --no-parent clears lineage
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(bool), action = clap::ArgAction::Set)]
        favorite: Option<bool>,
        #[arg(long, conflicts_with = "no_parent")]
        parent: Option<String>,
        #[arg(long)]
        no_parent: bool,
    },
    Archive {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    Restore { id: String },
    Layout { id: String },
    /// Save a complete layout JSON including its observed revision
    LayoutSave {
        id: String,
        #[arg(long)]
        file: PathBuf,
    },
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        include_archived: bool,
    },
    Show {
        id: String,
    },
    /// Create a Git worktree, skipping setup scripts and agent launches
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
        /// Explicit base ref; otherwise the runtime's configured default
        #[arg(long)]
        base: Option<String>,
        /// Reuse this UUID and identical options only to recover this creation
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
}

#[derive(ClapArgs)]
struct Target {
    /// Exact workspace ID from `workspace list`
    #[arg(long)]
    workspace: String,
    /// Exact session ID from `terminal list`
    #[arg(long)]
    terminal: String,
}

#[derive(Subcommand)]
enum TerminalCommand {
    /// Inspect local-operation approvals for a hybrid session
    HybridStatus { #[command(flatten)] target:Target },
    /// Approve or reject one exact local operation (never executes it twice)
    HybridDecide { #[command(flatten)] target:Target, #[arg(long)] operation_id:String, #[arg(long)] digest:String, #[arg(long)] approve:bool, #[arg(long,required=true)] confirm:bool },
    /// Read one saved feedback and execution receipt; never sends input
    Receipt {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        request_id: uuid::Uuid,
    },
    /// Show an existing native terminal in the local desktop; never starts a process
    Switch {
        #[command(flatten)]
        target: Target,
    },
    /// Search retained terminal recording bytes, not native model transcripts
    Search(terminal_search::SearchOptions),
    /// Provider installation/authentication diagnostics; no login is performed
    Check {
        #[arg(long, value_parser = ["codex", "claude-code", "minimax"])]
        provider: String,
    },
    Providers,
    /// Wait for an observed exit, safe idle state or output; never sends input
    Wait {
        #[command(flatten)]
        target: Target,
        #[arg(long = "for", value_parser = ["exit", "idle", "output"])]
        condition: String,
        #[arg(long)]
        contains: Option<String>,
        #[arg(long, default_value_t = 30000, value_parser = clap::value_parser!(u64).range(1..=300000))]
        deadline_ms: u64,
    },
    /// Reconstruct a logical VT screen from retained output, with explicit completeness
    Screen {
        #[command(flatten)]
        target: Target,
    },
    /// Rename the terminal tab in the workspace layout
    Rename {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        title: String,
    },
    /// Create a session and attach it as a terminal pane using layout revision checks
    Split {
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value = "vertical", value_parser = ["horizontal", "vertical"])]
        direction: String,
        #[arg(long, default_value = "terminal", value_parser = ["terminal", "codex", "claude-code"])]
        launch: String,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    /// Explicitly restore a dead generic terminal using existing recovery checks
    Recover {
        #[command(flatten)]
        target: Target,
        #[arg(long, required = true)]
        confirm: bool,
    },
    List {
        #[arg(long)]
        workspace: String,
    },
    Show(Target),
    Read {
        #[command(flatten)]
        target: Target,
        /// Absolute byte cursor; continue with the returned next_cursor
        #[arg(long, default_value_t = 0)]
        cursor: u64,
        #[arg(long, default_value_t = 131072, value_parser = clap::value_parser!(u32).range(1..=131072))]
        limit: u32,
    },
    /// Create a shell or a native Agent in an existing workspace
    Create {
        #[arg(long)]
        workspace: String,
        #[arg(long, default_value = "terminal", value_parser = ["terminal", "codex", "claude-code", "minimax", "hybrid-codex"])]
        launch: String,
        /// Stable launch UUID, echoed in the reply even if delivery is uncertain
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
        /// Opt in to synchronous native lifecycle hooks in this new session
        #[arg(long)]
        track_submission: bool,
        /// Saved SSH profile for --launch hybrid-codex (local workspace only)
        #[arg(long)]
        hybrid_host:Option<String>,
    },
    /// Deliver input once; success acknowledges PTY input, not task completion
    Send {
        #[command(flatten)]
        target: Target,
        #[arg(
            long,
            required_unless_present = "text_file",
            conflicts_with = "text_file"
        )]
        text: Option<String>,
        /// UTF-8 text file (up to 64 KiB); avoids shell quoting issues
        #[arg(long)]
        text_file: Option<PathBuf>,
        #[arg(long)]
        enter: bool,
        /// Codex/Claude feedback UUID; identical retries recover the saved receipt
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
        /// Reject input if the session has received any newer driving input
        #[arg(long)]
        expected_sequence: Option<u64>,
        /// Observe native execution for 0..60 seconds; never resends on timeout
        #[arg(long, requires = "enter", value_parser = clap::value_parser!(u64).range(0..=60))]
        wait_submit: Option<u64>,
    },
    /// Stop this session; a task-owned session stops its entire task
    Stop {
        #[command(flatten)]
        target: Target,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

const GUIDE: &str = include_str!("../../../docs/velella-cli.md");
const MANAGEMENT_GUIDE: &str = include_str!("../../../docs/velella-cli-management.md");
const COLLABORATION_GUIDE: &str = include_str!("../../../docs/velella-cli-collaboration.md");
const GUIDE_REVISION: &str = "2026-11-01.search-submission.1";

pub fn main_from(args: impl IntoIterator<Item = OsString>) -> i32 {
    let args: Vec<_> = args.into_iter().collect();
    let json = args.iter().any(|arg| arg == "--json");
    match Cli::try_parse_from(args) {
        Ok(cli) => run(cli.args),
        Err(error) if error.use_stderr() && json => {
            println!(
                "{}",
                json!({"ok":false,"error":{"code":"invalid_arguments","message":error.to_string()}})
            );
            2
        }
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            code
        }
    }
}

pub fn run(args: Args) -> i32 {
    let root = args.data_dir.clone().unwrap_or_else(crate::ipc::data_dir);
    let mut client = Client {
        root: root.clone(),
        remote: None,
        endpoint: crate::ipc::endpoint(&root, "daemon"),
        timeout: Duration::from_millis(args.timeout_ms),
    };
    let result=(|| {
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Search(_)) {
            return Err(failure("remote_not_supported", "native session search reads local history only; no connection or local fallback was attempted", json!({})));
        }
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Terminal { command:
            TerminalCommand::Receipt { .. } | TerminalCommand::Send { wait_submit:Some(_), .. } |
            TerminalCommand::Create { track_submission:true, .. } }) {
            return Err(failure("remote_not_supported", "submission tracking is local-only; no connection or local fallback was attempted", json!({})));
        }
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Terminal { command: TerminalCommand::Switch { .. } }) {
            return Err(failure("remote_not_supported", "terminal switch controls the local desktop only; no connection or local fallback was attempted", json!({})));
        }
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Collab { .. }) {
            anyhow::bail!("collaboration is local-session-only; omit --ssh-profile; no remote connection or local operation was attempted");
        }
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Skills { command: services::SkillsCommand::Manage(_) }) {
            anyhow::bail!("native skill management is local-only; omit --ssh-profile; nothing was changed");
        }
        if args.ssh_profile.is_some() && matches!(&args.command, Command::Mcp { command: services::McpCommand::Codex { .. } }) {
            anyhow::bail!("native Codex MCP configuration is local-only; nothing was changed");
        }
        if let Some(profile)=&args.ssh_profile {
            if !matches!(args.command,Command::Guide|Command::Schema|Command::Skills { command: services::SkillsCommand::Get { .. } | services::SkillsCommand::Bundled }) {
                client.remote=Some(remote::target(&client,profile,args.server_id.unwrap(),args.remote_data_dir.as_deref().unwrap())?);
            }
        }
        execute(&client,&args.command)
    })();
    match result {
        Ok(result) => {
            if args.json {
                println!("{}", json!({"ok":true,"result":result}));
            } else {
                print_result(&result);
            }
            0
        }
        Err(error) => {
            let detail = error
                .downcast_ref::<Failure>()
                .map(|e| e.detail.clone())
                .unwrap_or_else(|| json!({"code":"request_failed","message":format!("{error:#}")}));
            if args.json {
                println!("{}", json!({"ok":false,"error":detail,"data_dir":root}));
            } else {
                eprintln!(
                    "{}: {}",
                    detail["code"].as_str().unwrap_or("error"),
                    detail["message"].as_str().unwrap_or("request failed")
                );
            }
            1
        }
    }
}

#[derive(Debug)]
struct Failure {
    detail: Value,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}
impl std::error::Error for Failure {}
fn failure(code: &str, message: impl ToString, extra: Value) -> anyhow::Error {
    let mut detail = json!({"code":code,"message":message.to_string()});
    if let Some(fields) = extra.as_object() {
        detail.as_object_mut().unwrap().extend(fields.clone());
    }
    Failure { detail }.into()
}

struct Client {
    root: PathBuf,
    remote: Option<remote::Target>,
    endpoint: String,
    timeout: Duration,
}
impl Client {
    fn request(&self, request: Value) -> Result<Value> {
        let mutation = !matches!(request["cmd"].as_str().unwrap_or(""),
            "snapshot" | "protocol" | "workbench.snapshot" | "workbench.sessions" |
            "workbench.terminal.output" | "terminal.output" | "workbench.layout" | "workbench.hybrid.status" |
            "workbench.events" | "task.events" | "task.diff" | "task.feedback.status" |
            "workbench.feedback.status" | "workbench.feedback.observe" | "workbench.files" | "workbench.search" |
            "workbench.file.read" | "workbench.git" | "automation.list" |
            "ssh.list" | "ssh.inspect" | "ssh.diagnose" | "agent.check" |
            "orchestration.list" | "orchestration.snapshot" | "orchestration.preflight" |
            "orchestration.capabilities" | "orchestration.mcp.preview");
        let extra = json!({"operation":request["cmd"],"request_id":request["request_id"],"workspace_id":request["workspace_id"],"session_id":request["session_id"],"task_id":request["task_id"],"id":request["id"],"path":request["path"]});
        let transfer=if let Some(target)=&self.remote {remote::request(target,&request,self.timeout)}
            else {crate::ipc::request_with_timeout(&self.endpoint,&request,self.timeout)};
        let value=transfer.map_err(|error| {
            if error.downcast_ref::<Failure>().is_some(){error}else{
                failure(if mutation{"delivery_uncertain"}else{"runtime_unavailable"},format!("{error:#}. No request was automatically retried."),extra.clone())
            }
        })?;
        if value["ok"] != true {
            return Err(failure(
                "request_rejected",
                value["error"]
                    .as_str()
                    .unwrap_or("runtime rejected the request"),
                extra,
            ));
        }
        Ok(value)
    }

    fn snapshot(&self) -> Result<Value> {
        self.request(json!({"cmd":"workbench.snapshot"}))
    }
    fn sessions(&self, workspace: &str) -> Result<Vec<Value>> {
        let value = self.request(json!({"cmd":"workbench.sessions","workspace_id":workspace}))?;
        Ok(array(&value, "sessions")?.to_vec())
    }
    fn session(&self, target: &Target) -> Result<Value> {
        find(
            self.sessions(&target.workspace)?.as_slice(),
            &target.terminal,
            "terminal",
        )
    }
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .with_context(|| format!("runtime response has no {key} array"))
}
fn find(rows: &[Value], id: &str, kind: &str) -> Result<Value> {
    rows.iter()
        .find(|row| row["id"] == id)
        .cloned()
        .ok_or_else(|| failure("not_found", format!("{kind} ID not found: {id}"), json!({})))
}
fn project_row(value: &Value) -> Value {
    json!({"id":value["id"],"name":value["name"],"path":value["path"],"direct":value["direct"],"default_base":value["default_base"]})
}
fn project_detail(value:&Value)->Value {
    let mut row=project_row(value);
    for key in ["setup","setup_default"] {row[key]=value[key].clone();}
    row
}
fn task_row(value: &Value) -> Value {
    json!({"id":value["id"],"project_id":value["project_id"],"title":value["spec"]["name"],"phase":value["phase"],"worktree":value["worktree"],"branch":value["branch"],"reason":value["reason"]})
}

fn execute(client: &Client, command: &Command) -> Result<Value> {
    match command {
        Command::Schema => Ok(services::schema()),
        Command::Search(options) => native_search::execute(client, options),
        Command::Collab { credentials, request_id, command } => crate::collaboration::cli::execute(
            crate::collaboration::cli::Args {
                data_dir: Some(client.root.clone()),
                credentials: credentials.clone(),
                request_id: request_id.clone(),
                command: command.clone(),
            }, client.timeout),
        Command::Automation { command } => services::automation(client, command),
        Command::Host { command } => services::host(client, command),
        Command::Skills { command } => services::skills(client, command),
        Command::Mcp { command } => services::mcp(client, command),
        Command::File { command } => services::file(client, command),
        Command::Git { command } => services::git(client, command),
        Command::Browser { command } => browser::execute(client, command),
        Command::Project { command: ProjectCommand::Configure { .. } } => services::project(client, command),
        Command::Guide => Ok(json!({"guide":format!("{GUIDE}\n\n{MANAGEMENT_GUIDE}\n\n{COLLABORATION_GUIDE}"),"cli_version":env!("CARGO_PKG_VERSION"),"guide_revision":GUIDE_REVISION,"source":"bundled-cli"})),
        Command::Status => {
            let value = client.request(json!({"cmd":"snapshot"}))?;
            Ok(
                json!({"runtime":"ready","version":value["version"],"pid":value["pid"],"projects":array(&value,"projects")?.len(),"tasks":array(&value,"tasks")?.len(),"transport":if client.remote.is_some(){"ssh-stdio-v1"}else{"local-owner-only-ipc"},"remote_identity":value["remote_identity"]}),
            )
        }
        Command::Project {
            command: ProjectCommand::Add { path },
        } => {
            let path = if client.remote.is_some(){path.clone()}else{dunce::canonicalize(path).context("project folder is unavailable")?};
            let value = client.request(json!({"cmd":"project.add","path":path}))?;
            Ok(json!({"project":project_row(&value["project"])}))
        }
        Command::Project { command } => {
            let snapshot = client.snapshot()?;
            let rows: Vec<_> = array(&snapshot, "projects")?
                .iter()
                .map(project_row)
                .collect();
            match command {
                ProjectCommand::List => Ok(json!({"projects":rows})),
                ProjectCommand::Show { id } => Ok(json!({"project":project_detail(&find(array(&snapshot,"projects")?,id,"project")?)})),
                _ => unreachable!(),
            }
        }
        Command::Workspace {
            command:
                WorkspaceCommand::Create {
                    project,
                    name,
                    base,
                    request_id,
                },
        } => {
            let id = request_id.unwrap_or_else(uuid::Uuid::now_v7);
            let mut request = json!({"cmd":"workbench.create","project_id":project,"name":name,"request_id":id,"setup":"skip","launch":"none"});
            if let Some(base) = base {
                request["base"] = json!(base);
            }
            let mut response = client.request(request)?;
            response.as_object_mut().unwrap().remove("ok");
            response["request_id"] = json!(id);
            Ok(response)
        }
        Command::Workspace { command } => {
            let snapshot = client.snapshot()?;
            let rows = array(&snapshot, "workspaces")?;
            match command {
                WorkspaceCommand::List {
                    project,
                    include_archived,
                } => Ok(
                    json!({"workspaces":rows.iter().filter(|row| (*include_archived || row["archived"] != true) && project.as_ref().is_none_or(|id| row["project_id"] == *id)).collect::<Vec<_>>()}),
                ),
                WorkspaceCommand::Show { id } => {
                    Ok(json!({"workspace":find(rows,id,"workspace")?}))
                }
                _ => services::workspace(client, command),
            }
        }
        Command::Task { command } => management::task(client, command),
        Command::Swarm { command } => management::swarm(client, command),
        Command::Terminal { command } => terminal(client, command),
    }
}

fn terminal(client: &Client, command: &TerminalCommand) -> Result<Value> {
    match command {
        TerminalCommand::HybridStatus{target}=>{anyhow::ensure!(client.remote.is_none(),"hybrid approvals are local-only");client.request(json!({"cmd":"workbench.hybrid.status","workspace_id":target.workspace,"session_id":target.terminal}))},
        TerminalCommand::HybridDecide{target,operation_id,digest,approve,confirm}=>{anyhow::ensure!(client.remote.is_none()&&*confirm,"hybrid decisions require local scope and confirmation");client.request(json!({"cmd":"workbench.hybrid.decide","workspace_id":target.workspace,"session_id":target.terminal,"operation_id":operation_id,"digest":digest,"approved":approve,"confirmed":true}))},
        TerminalCommand::Receipt { target, request_id } => submission::observe(client,target,*request_id),
        TerminalCommand::Search(options) => terminal_search::execute(client, options),
        TerminalCommand::Check { provider } => services::call(client, json!({"cmd":"agent.check","agent":provider})),
        TerminalCommand::Providers => services::call(client, json!({"cmd":"orchestration.capabilities"})),
        TerminalCommand::List { workspace } => {
            Ok(json!({"workspace_id":workspace,"sessions":client.sessions(workspace)?}))
        }
        TerminalCommand::Show(target) => {
            Ok(json!({"workspace_id":target.workspace,"session":client.session(target)?}))
        }
        TerminalCommand::Read {
            target,
            cursor,
            limit,
        } => {
            let value = client.request(json!({"cmd":"workbench.terminal.output","workspace_id":target.workspace,"session_id":target.terminal,"since":cursor}))?;
            output_page(value, *limit as usize)
        }
        TerminalCommand::Create {
            workspace,
            launch,
            request_id,
            track_submission,
            hybrid_host,
        } => {
            let id = request_id.unwrap_or_else(uuid::Uuid::now_v7);
            anyhow::ensure!(!track_submission||matches!(launch.as_str(),"codex"|"claude-code"),"submission tracking requires Codex or Claude");
            let mut request=json!({"cmd":"workbench.terminal.open","workspace_id":workspace,"launch":launch,"request_id":id});
            if launch=="hybrid-codex"{anyhow::ensure!(client.remote.is_none(),"hybrid Codex requires a local project");request["ssh_profile_id"]=json!(hybrid_host.as_deref().context("--hybrid-host is required")?);}else{anyhow::ensure!(hybrid_host.is_none(),"--hybrid-host requires --launch hybrid-codex");}
            if *track_submission {
                // An old runtime must refuse before launch, rather than silently
                // ignoring a new option and creating an untracked session.
                let protocol=client.request(json!({"cmd":"protocol"}))?;
                anyhow::ensure!(protocol["submission_tracking"]==1,"runtime does not support submission tracking; no session was created");
                request["track_submission"]=json!(true);
            }
            let value = client.request(request)?;
            Ok(json!({"session_id":value["session_id"],"workspace_id":workspace,"request_id":id}))
        }
        TerminalCommand::Stop { target, confirm } => {
            anyhow::ensure!(confirm, "--confirm is required to stop a process");
            client.request(json!({"cmd":"workbench.terminal.stop","workspace_id":target.workspace,"session_id":target.terminal,"confirmed":true}))?;
            Ok(json!({"stop_acknowledged":true,"session_id":target.terminal}))
        }
        TerminalCommand::Wait { .. } | TerminalCommand::Screen { .. } | TerminalCommand::Rename { .. } |
        TerminalCommand::Split { .. } | TerminalCommand::Recover { .. } | TerminalCommand::Switch { .. } => terminal_management::execute(client, command),
        TerminalCommand::Send {
            target,
            text,
            text_file,
            enter,
            request_id,
            expected_sequence,
            wait_submit,
        } => {
            let text = input_text(text.as_deref(), text_file.as_deref())?;
            let session = client.session(target)?;
            let program = session["program"].as_str().unwrap_or("");
            if matches!(program, "codex" | "claude") {
                return submission::send(client,target,&session,&text,*enter,*request_id,*expected_sequence,*wait_submit);
            }
            anyhow::ensure!(wait_submit.is_none(),"--wait-submit requires a tracked native Codex or Claude session; input was not sent");
            anyhow::ensure!(
                matches!(
                    program,
                    "powershell" | "pwsh" | "cmd" | "bash" | "zsh" | "sh" | "fish"
                ),
                "guarded prompt delivery supports Codex/Claude or plain shells only"
            );
            anyhow::ensure!(request_id.is_none(), "plain shell input has no replay receipt; use --expected-sequence and inspect output after uncertainty");
            let status = &session["status"];
            let pid = status["pid"]
                .as_u64()
                .context("terminal host is unavailable; input was not sent")?;
            anyhow::ensure!(
                status["exit"].is_null() && status["input_protocol"] == 1,
                "terminal has exited or cannot guard input"
            );
            let sequence = status["input_sequence"]
                .as_u64()
                .context("terminal has no input sequence; input was not sent")?;
            anyhow::ensure!(
                expected_sequence.is_none_or(|value| value == sequence),
                "terminal has newer input; input was not sent"
            );
            let value = client.request(json!({"cmd":"workbench.terminal.input","workspace_id":target.workspace,"session_id":target.terminal,"text":text,"enter":enter,"input_kind":"user","expected_host_pid":pid,"expected_input_sequence":sequence}))?;
            Ok(
                json!({"input_accepted":true,"input_sequence":value["input_sequence"],"session_id":target.terminal,"completion_verified":false,"turn_started_confirmed":false}),
            )
        }
    }
}

fn input_text(text: Option<&str>, path: Option<&std::path::Path>) -> Result<String> {
    let text = if let Some(path) = path {
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)?
            .take(65540)
            .read_to_string(&mut text)?;
        text.trim_start_matches('\u{feff}').to_owned()
    } else {
        text.context("--text or --text-file is required")?
            .to_owned()
    };
    anyhow::ensure!(
        text.len() <= 65536
            && text
                .chars()
                .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')),
        "input exceeds 64 KiB or contains unsupported control characters"
    );
    Ok(text)
}

fn output_page(value: Value, limit: usize) -> Result<Value> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(value["data"].as_str().context("output data missing")?)?;
    let data = &data[..data.len().min(limit)];
    let from = value["from"].as_u64().context("output cursor missing")?;
    let total = value["total"].as_u64().context("output total missing")?;
    let next = from + data.len() as u64;
    Ok(
        json!({"text":String::from_utf8_lossy(data),"data":base64::engine::general_purpose::STANDARD.encode(data),"from":from,"next_cursor":next,"total":total,"has_more":next < total,"cursor_unit":"bytes"}),
    )
}

fn print_result(value: &Value) {
    if let Some(guide) = value["guide"].as_str() {
        println!("{guide}");
    } else if let Some(text) = value["text"].as_str() {
        print!("{text}");
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}

#[cfg(test)]
#[path = "desktop_cli_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "desktop_cli_expansion_tests.rs"]
mod expansion_tests;

#[cfg(test)]
#[path = "desktop_cli_search_tests.rs"]
mod search_tests;

#[cfg(test)]
#[path = "desktop_cli_collaboration_tests.rs"]
mod collaboration_tests;

#[cfg(test)]
#[path = "desktop_cli_switch_tests.rs"]
mod switch_tests;
