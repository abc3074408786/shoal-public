//! Existing runtime management APIs; no direct writes to the daemon's stores.
use super::*;

#[derive(Subcommand)]
pub(super) enum AutomationCommand {
    List {
        /// List removed schedules, available for disabled restoration
        #[arg(long)]
        archived: bool,
    },
    Show {
        id: String,
        #[arg(long)]
        archived: bool,
    },
    /// Create a schedule, disabled unless --enabled is explicitly supplied
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=525600))]
        interval_minutes: u32,
        /// RFC3339 with timezone, or Unix seconds
        #[arg(long)]
        start_at: String,
        #[arg(long)]
        enabled: bool,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Save a reviewed full automation JSON; set enabled:true to resume
    Save {
        id: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, required = true)]
        confirm: bool,
    },
    Pause {
        id: String,
    },
    Run {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Remove a schedule reversibly, retaining its run history
    Remove {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Restore a removed schedule in disabled state; save to re-enable
    Restore {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Paginated history on new runtimes; legacy runtimes return latest global 200
    Runs {
        #[arg(long)]
        automation: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: Option<u32>,
        #[arg(long)]
        cursor: Option<String>,
    },
}

#[derive(Subcommand)]
pub(super) enum HostCommand {
    List,
    Show {
        id: String,
    },
    /// Save a reviewed SSH profile JSON; identity is a key-file reference
    Save {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, required = true)]
        confirm: bool,
    },
    Inspect {
        id: String,
    },
    /// Expand local OpenSSH settings without making a network connection
    Diagnose {
        id: String,
    },
    /// Inspect the remote daemon identity over strict noninteractive SSH; no RPC is sent
    InspectRuntime {
        id: String,
        #[arg(long)]
        remote_root: Option<String>,
    },
    /// Open an interactive SSH terminal, checking the exact reviewed profile
    Connect {
        id: String,
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        reviewed_file: PathBuf,
        #[arg(long, required = true)]
        confirm: bool,
    },
    Remove {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum SkillsCommand {
    /// Discover guides bundled with this CLI binary; offline, no installation
    Bundled,
    /// Print a version-matched built-in guide; offline, no installation
    Get { name: String },
    #[command(visible_alias = "installed")]
    List,
    /// Read a discovered SKILL.md; duplicate names need an exact --path
    Show {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    #[command(flatten)]
    Manage(super::skill_management::SkillsOperation),
}

#[derive(Subcommand)]
pub(super) enum McpCommand {
    /// Reviewed local Codex stdio MCP config, with exact-file rollback
    Codex {
        #[command(subcommand)]
        command: super::codex_mcp::CodexMcpCommand,
    },
    Preview {
        path: PathBuf,
    },
    Confirm {
        path: PathBuf,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        entry: String,
        #[arg(long = "tool", required = true)]
        tools: Vec<String>,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum FileCommand {
    List {
        #[arg(long)]
        workspace: String,
    },
    Read {
        #[arg(long)]
        workspace: String,
        path: String,
    },
    /// Search code files, not Agent conversation history
    Search {
        #[arg(long)]
        workspace: String,
        query: String,
    },
    Create {
        #[arg(long)]
        workspace: String,
        path: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Compare the current content with --previous-file before writing
    Write {
        #[arg(long)]
        workspace: String,
        path: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        previous_file: PathBuf,
    },
    Rename {
        #[arg(long)]
        workspace: String,
        path: String,
        new_path: String,
    },
    Remove {
        #[arg(long)]
        workspace: String,
        path: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum GitCommand {
    Status {
        #[arg(long)]
        workspace: String,
    },
    Diff {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        staged: bool,
    },
    Stage {
        #[arg(long)]
        workspace: String,
        path: String,
    },
    Unstage {
        #[arg(long)]
        workspace: String,
        path: String,
    },
    Commit {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        message: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

pub(super) fn call(client: &Client, request: Value) -> Result<Value> {
    let mut response = client.request(request)?;
    response
        .as_object_mut()
        .context("runtime response must be an object")?
        .remove("ok");
    Ok(response)
}

pub(super) fn text_file(path: &std::path::Path, max: u64) -> Result<String> {
    Ok(exact_text_file(path, max)?
        .trim_start_matches('\u{feff}')
        .to_owned())
}

fn exact_text_file(path: &std::path::Path, max: u64) -> Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= max, "input file exceeds {max} bytes");
    String::from_utf8(bytes).context("input file must be UTF-8")
}

pub(super) fn object_file(path: &std::path::Path) -> Result<Value> {
    let value: Value = serde_json::from_str(&text_file(path, 2 * 1024 * 1024)?)
        .context("file must contain JSON")?;
    anyhow::ensure!(value.is_object(), "file must contain a JSON object");
    Ok(value)
}

fn named(text: &str, name: &str) -> Result<()> {
    anyhow::ensure!(!text.trim().is_empty(), "{name} must not be empty");
    Ok(())
}

pub(super) fn automation(client: &Client, command: &AutomationCommand) -> Result<Value> {
    match command {
        AutomationCommand::List { archived } | AutomationCommand::Show { archived, .. } => {
            if *archived { require_automation_capability(client, "automation-archive-v1")?; }
            let value = client.request(json!({"cmd":if *archived {"automation.archived"} else {"automation.list"}}))?;
            match command {
                AutomationCommand::Show { id, .. } => {
                    Ok(json!({"automation":find(array(&value,"automations")?,id,"automation")?}))
                }
                _ => Ok(json!({"automations":array(&value,"automations")?})),
            }
        }
        AutomationCommand::Runs { automation, limit, cursor } => {
            if automation_capability(client, "automation-history-v1")? {
                call(client, json!({"cmd":"automation.runs","automation_id":automation,"limit":limit.unwrap_or(100),"cursor":cursor}))
            } else {
                anyhow::ensure!(limit.is_none() && cursor.is_none(), "当前 runtime 不支持完整历史分页；需要升级 runtime，未执行分页请求");
                let value = client.request(json!({"cmd":"automation.list"}))?;
                Ok(json!({"history_limit":200,"history_scope":"legacy_global_latest_200","pagination_supported":false,"runs":array(&value,"runs")?.iter().filter(|row|automation.as_ref().is_none_or(|id|row["automation_id"]==*id)).collect::<Vec<_>>()}))
            }
        }
        AutomationCommand::Remove { id, .. } | AutomationCommand::Restore { id, .. } => {
            require_automation_capability(client, "automation-archive-v1")?;
            call(client, json!({"cmd":if matches!(command,AutomationCommand::Remove{..}) {"automation.remove"} else {"automation.restore"},"id":id,"confirmed":true}))
        }
        AutomationCommand::Create {
            project,
            file,
            interval_minutes,
            start_at,
            enabled,
            ..
        } => {
            let spec: Value = serde_yaml::from_str(&text_file(file, 65536)?)?;
            let _: velella_core::task::TaskSpec = serde_json::from_value(spec.clone())?;
            let due = start_at
                .parse::<i64>()
                .or_else(|_| {
                    chrono::DateTime::parse_from_rfc3339(start_at).map(|date| date.timestamp())
                })
                .context("--start-at must be Unix seconds or RFC3339 with a timezone")?;
            call(
                client,
                json!({"cmd":"automation.save","confirmed":true,"automation":{"project_id":project,"spec":spec,"interval_minutes":interval_minutes,"next_run":due,"enabled":enabled}}),
            )
        }
        AutomationCommand::Save { id, file, .. } => {
            let mut value = object_file(file)?;
            anyhow::ensure!(
                value.get("id").is_none_or(|old| old == id),
                "automation ID in the file differs from the target"
            );
            value["id"] = json!(id);
            call(
                client,
                json!({"cmd":"automation.save","confirmed":true,"automation":value}),
            )
        }
        AutomationCommand::Pause { id } => call(client, json!({"cmd":"automation.pause","id":id})),
        AutomationCommand::Run { id, .. } => call(
            client,
            json!({"cmd":"automation.run","id":id,"confirmed":true}),
        ),
    }
}

fn automation_capability(client: &Client, capability: &str) -> Result<bool> {
    let protocol = client.request(json!({"cmd":"protocol"}))?;
    Ok(array(&protocol, "capabilities")?.iter().any(|v| v == capability))
}

fn require_automation_capability(client: &Client, capability: &str) -> Result<()> {
    anyhow::ensure!(automation_capability(client, capability)?, "当前 runtime 不支持自动化移除/恢复；需要升级 runtime，未修改自动化");
    Ok(())
}

pub(super) fn host(client: &Client, command: &HostCommand) -> Result<Value> {
    match command {
        HostCommand::List => call(client, json!({"cmd":"ssh.list"})),
        HostCommand::Show { id } => {
            let value = client.request(json!({"cmd":"ssh.list"}))?;
            Ok(json!({"profile":find(array(&value,"profiles")?,id,"SSH profile")?}))
        }
        HostCommand::Save { file, .. } => call(
            client,
            json!({"cmd":"ssh.save","profile":object_file(file)?}),
        ),
        HostCommand::Inspect { id } => call(client, json!({"cmd":"ssh.inspect","id":id})),
        HostCommand::Diagnose { id } => call(client, json!({"cmd":"ssh.diagnose","id":id})),
        HostCommand::InspectRuntime { id, remote_root } => {
            anyhow::ensure!(
                client.remote.is_none(),
                "host inspection uses the local SSH profile store"
            );
            let profiles = client.request(json!({"cmd":"ssh.list"}))?;
            let profile = find(array(&profiles, "profiles")?, id, "SSH profile")?;
            Ok(
                json!({"identity":remote::inspect(&profile,remote_root.as_deref(),client.timeout)?,"connection_confirmed":false}),
            )
        }
        HostCommand::Connect {
            id,
            workspace,
            reviewed_file,
            ..
        } => call(
            client,
            json!({"cmd":"ssh.connect","id":id,"workspace_id":workspace,"confirmed":true,"reviewed_profile":object_file(reviewed_file)?}),
        ),
        HostCommand::Remove { id, .. } => {
            call(client, json!({"cmd":"ssh.delete","id":id,"confirmed":true}))
        }
    }
}

pub(super) fn skills(client: &Client, command: &SkillsCommand) -> Result<Value> {
    match command {
        SkillsCommand::Bundled => return Ok(bundled_skills()),
        SkillsCommand::Get { name } => {
            let guide = match name.as_str() {
                "velella-cli" => format!("{GUIDE}\n\n{MANAGEMENT_GUIDE}"),
                "velella-collaboration" => COLLABORATION_GUIDE.to_owned(),
                _ => return Err(failure("unknown_bundled_skill", "unknown bundled guide; use skills bundled", json!({"name":name}))),
            };
            return Ok(json!({"name":name,"guide":guide,"cli_version":env!("CARGO_PKG_VERSION"),"guide_revision":GUIDE_REVISION,"source":"bundled-cli"}));
        }
        _ => {}
    }
    if let SkillsCommand::Manage(operation) = command {
        anyhow::ensure!(client.remote.is_none(), "native skill management is local-only");
        return super::skill_management::execute(operation);
    }
    let value = client.request(json!({"cmd":"orchestration.capabilities"}))?;
    let rows = array(&value, "skills")?;
    match command {
        SkillsCommand::List => Ok(json!({"skills":rows})),
        SkillsCommand::Show { name, path } => {
            let matches: Vec<_> = rows
                .iter()
                .filter(|row| row["name"] == *name)
                .filter(|row| path.as_ref().is_none_or(|path| row["path"] == json!(path)))
                .collect();
            anyhow::ensure!(
                matches.len() == 1,
                "skill is missing or ambiguous; use an exact --path from skills list"
            );
            let row = matches[0];
            let path = std::path::Path::new(row["path"].as_str().context("skill path missing")?);
            Ok(
                json!({"skill":row,"content":text_file(path,1024*1024)?,"content_is_untrusted":true}),
            )
        }
        SkillsCommand::Manage(_) | SkillsCommand::Bundled | SkillsCommand::Get { .. } => unreachable!(),
    }
}

pub(super) fn mcp(client: &Client, command: &McpCommand) -> Result<Value> {
    match command {
        McpCommand::Codex { command } => {
            anyhow::ensure!(client.remote.is_none(), "native Codex MCP configuration is local-only");
            super::codex_mcp::execute(command)
        },
        McpCommand::Preview { path } => call(
            client,
            json!({"cmd":"orchestration.mcp.preview","path":dunce::canonicalize(path)?}),
        ),
        McpCommand::Confirm {
            path,
            digest,
            entry,
            tools,
            request_id,
            ..
        } => {
            let id = request_id.unwrap_or_else(uuid::Uuid::now_v7);
            let mut value = call(
                client,
                json!({"cmd":"orchestration.mcp.confirm","path":dunce::canonicalize(path)?,"digest":digest,"entry_id":entry,"tools":tools,"request_id":id,"confirmed":true}),
            )?;
            value["request_id"] = json!(id);
            Ok(value)
        }
    }
}

pub(super) fn project(client: &Client, command: &Command) -> Result<Value> {
    let Command::Project {
        command:
            ProjectCommand::Configure {
                id,
                default_base,
                reset_base,
                setup_file,
                setup_default,
                ..
            },
    } = command
    else {
        unreachable!()
    };
    let mut request = json!({"cmd":"project.update","project_id":id});
    if let Some(base) = default_base {
        named(base, "base")?;
        request["default_base"] = json!(base);
    }
    if *reset_base {
        request["default_base"] = Value::Null;
    }
    if let Some(file) = setup_file {
        request["setup"] = json!(text_file(file, 65536)?);
    }
    if let Some(policy) = setup_default {
        request["setup_default"] = json!(policy);
    }
    anyhow::ensure!(
        request.as_object().unwrap().len() > 2,
        "supply a base or preparation setting"
    );
    let value = call(client, request)?;
    Ok(json!({"project":project_detail(&value["project"])}))
}

pub(super) fn workspace(client: &Client, command: &WorkspaceCommand) -> Result<Value> {
    let request = match command {
        WorkspaceCommand::Update {
            id,
            name,
            favorite,
            parent,
            no_parent,
        } => {
            let mut value = json!({"cmd":"workbench.update","workspace_id":id});
            if let Some(name) = name {
                named(name, "workspace name")?;
                value["name"] = json!(name);
            }
            if let Some(favorite) = favorite {
                value["favorite"] = json!(favorite);
            }
            if let Some(parent) = parent {
                value["parent_workspace_id"] = json!(parent);
            }
            if *no_parent {
                value["parent_workspace_id"] = Value::Null;
            }
            anyhow::ensure!(
                value.as_object().unwrap().len() > 2,
                "supply metadata to update"
            );
            value
        }
        WorkspaceCommand::Archive { id, .. } => {
            json!({"cmd":"workbench.update","workspace_id":id,"archived":true,"confirmed":true})
        }
        WorkspaceCommand::Restore { id } => {
            json!({"cmd":"workbench.update","workspace_id":id,"archived":false})
        }
        WorkspaceCommand::Layout { id } => json!({"cmd":"workbench.layout","workspace_id":id}),
        WorkspaceCommand::LayoutSave { id, file } => {
            json!({"cmd":"workbench.layout.save","workspace_id":id,"layout":object_file(file)?})
        }
        _ => unreachable!(),
    };
    call(client, request)
}

pub(super) fn file(client: &Client, command: &FileCommand) -> Result<Value> {
    let request = match command {
        FileCommand::List { workspace } => {
            json!({"cmd":"workbench.files","workspace_id":workspace})
        }
        FileCommand::Read { workspace, path } => {
            json!({"cmd":"workbench.file.read","workspace_id":workspace,"path":path})
        }
        FileCommand::Search { workspace, query } => {
            json!({"cmd":"workbench.search","workspace_id":workspace,"query":query})
        }
        FileCommand::Create {
            workspace,
            path,
            file,
        } => {
            json!({"cmd":"workbench.file.create","workspace_id":workspace,"path":path,"content":exact_text_file(file,2*1024*1024)?})
        }
        FileCommand::Write {
            workspace,
            path,
            file,
            previous_file,
        } => {
            json!({"cmd":"workbench.file.write","workspace_id":workspace,"path":path,"content":exact_text_file(file,2*1024*1024)?,"previous":exact_text_file(previous_file,2*1024*1024)?})
        }
        FileCommand::Rename {
            workspace,
            path,
            new_path,
        } => {
            json!({"cmd":"workbench.file.rename","workspace_id":workspace,"path":path,"new_path":new_path})
        }
        FileCommand::Remove {
            workspace, path, ..
        } => {
            json!({"cmd":"workbench.file.delete","workspace_id":workspace,"path":path,"confirmed":true})
        }
    };
    call(client, request)
}

pub(super) fn git(client: &Client, command: &GitCommand) -> Result<Value> {
    match command {
        GitCommand::Status { workspace } | GitCommand::Diff { workspace, .. } => {
            let value = call(
                client,
                json!({"cmd":"workbench.git","workspace_id":workspace}),
            )?;
            match command {
                GitCommand::Diff { staged, .. } => Ok(
                    json!({"workspace_id":workspace,"git":value["git"],"diff":value[if *staged{"staged"}else{"diff"}]}),
                ),
                _ => Ok(
                    json!({"workspace_id":workspace,"git":value["git"],"status":value["status"],"branch":value["branch"]}),
                ),
            }
        }
        GitCommand::Stage { workspace, path } => call(
            client,
            json!({"cmd":"workbench.git.stage","workspace_id":workspace,"path":path}),
        ),
        GitCommand::Unstage { workspace, path } => call(
            client,
            json!({"cmd":"workbench.git.unstage","workspace_id":workspace,"path":path}),
        ),
        GitCommand::Commit {
            workspace, message, ..
        } => {
            named(message, "commit message")?;
            call(
                client,
                json!({"cmd":"workbench.git.commit","workspace_id":workspace,"message":message,"confirmed":true}),
            )
        }
    }
}

fn bundled_skills() -> Value {
    json!({"source":"bundled-cli","cli_version":env!("CARGO_PKG_VERSION"),"guide_revision":GUIDE_REVISION,"skills":[
        {"name":"velella-cli","description":"Projects, terminals, tasks, orchestration and management","command":"velella-cli skills get velella-cli"},
        {"name":"velella-collaboration","description":"Session identity, mailbox, assignment and review receipts","command":"velella-cli skills get velella-collaboration"}
    ]})
}

pub(super) fn schema() -> Value {
    fn describe(command: &clap::Command) -> Value {
        json!({"name":command.get_name(),"about":command.get_about().map(ToString::to_string),
            "arguments":command.get_arguments().map(|arg|json!({"name":arg.get_id().as_str(),"long":arg.get_long(),"required":arg.is_required_set(),"help":arg.get_help().map(ToString::to_string)})).collect::<Vec<_>>(),
            "commands":command.get_subcommands().map(describe).collect::<Vec<_>>()})
    }
    let mut command = Cli::command();
    command.build();
    json!({"schema_version":1,"cli_version":env!("CARGO_PKG_VERSION"),"bundled_skills":bundled_skills(),"transports":["local-owner-only-ipc","ssh-stdio-v1"],"local_only_commands":["collab","search","terminal receipt","terminal send --wait-submit","terminal create --track-submission"],"command":describe(&command)})
}
