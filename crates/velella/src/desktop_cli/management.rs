//! Typed management commands. State transitions and permissions stay in the daemon.
use super::*;

#[derive(Subcommand)]
pub(super) enum TaskCommand {
    List {
        #[arg(long)]
        project: Option<String>,
    },
    Show {
        id: String,
    },
    /// Print a task YAML template without connecting to the daemon
    Template,
    /// Queue a task from a UTF-8 YAML/JSON specification (starts an Agent)
    Create {
        #[arg(long)]
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    /// Read task state transitions; continue from the last returned event seq
    Events {
        id: String,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i64).range(0..))]
        since: i64,
    },
    Diff {
        id: String,
    },
    /// Read output from the Agent, verification command or auxiliary shell
    Read {
        id: String,
        #[arg(long, default_value = "agent", value_parser = ["agent", "command", "shell"])]
        target: String,
        #[arg(long, default_value_t = 0)]
        cursor: u64,
        #[arg(long, default_value_t = 131072, value_parser = clap::value_parser!(u32).range(1..=131072))]
        limit: u32,
    },
    /// Explicitly recover a task using the runtime's existing recovery checks
    Resume {
        id: String,
    },
    /// Start the task's configured checks; completion is observed with task show
    Verify {
        id: String,
    },
    /// Send review feedback once; inspect the receipt before retrying
    Feedback {
        id: String,
        #[arg(
            long,
            required_unless_present = "text_file",
            conflicts_with = "text_file"
        )]
        text: Option<String>,
        #[arg(long)]
        text_file: Option<PathBuf>,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    FeedbackStatus {
        id: String,
        #[arg(long)]
        request_id: uuid::Uuid,
    },
    /// Stop the task's sessions, retaining its workspace and changes
    #[command(visible_alias = "stop")]
    Cancel {
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(ClapArgs)]
pub(super) struct Change {
    id: String,
    /// Stable UUID for this operation; reuse only with identical arguments
    #[arg(long)]
    request_id: Option<uuid::Uuid>,
}

#[derive(Subcommand)]
pub(super) enum SwarmCommand {
    List {
        #[arg(long)]
        workspace: Option<String>,
    },
    Show {
        id: String,
    },
    Members {
        id: String,
    },
    Work {
        id: String,
    },
    Events {
        id: String,
    },
    /// Check supported native providers without starting one
    Capabilities,
    Preflight {
        #[arg(long)]
        workspace: String,
    },
    /// Create a v2 swarm and start its planning Agent; execution needs plan confirm
    Create {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        acceptance: String,
        #[arg(long, default_value = "codex", value_parser = ["codex", "claude-code", "omp", "opencode"])]
        agent: String,
        /// Optional complete v2 plan as UTF-8 JSON
        #[arg(long)]
        plan_file: Option<PathBuf>,
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
    },
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// Pause new dispatches; active work may finish
    Pause(Change),
    Resume(Change),
    /// Stop this swarm's sessions, retaining results
    Stop {
        #[command(flatten)]
        change: Change,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Allow one repair after a first failure, with a root cause and new evidence
    Retry {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        node: String,
        #[arg(long)]
        cause_key: String,
        #[arg(long)]
        new_evidence: String,
    },
    /// Record a human decision after the runtime's technical acceptance checks
    Gate {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        node: String,
        #[arg(long)]
        revision: u64,
        #[arg(long, value_parser = ["approve", "reject"])]
        decision: String,
        #[arg(long)]
        summary: String,
    },
    /// Accept the current verified candidate; the runtime rejects candidate drift
    Accept {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        summary: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum PlanCommand {
    /// Save an edited plan, checking the revision from swarm show
    Save {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        file: PathBuf,
    },
    /// Ask the planning Agent to revise the current draft
    Refine {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        instruction: String,
    },
    /// Approve the exact current revision and begin dispatching work
    Confirm {
        #[command(flatten)]
        change: Change,
        #[arg(long)]
        revision: u64,
        #[arg(long, required = true)]
        confirm: bool,
    },
}

fn nonempty(text: &str, name: &str) -> Result<()> {
    anyhow::ensure!(!text.trim().is_empty(), "{name} must not be empty");
    Ok(())
}

fn json_file(path: &std::path::Path) -> Result<Value> {
    let value: Value = serde_json::from_str(&input_text(None, Some(path))?)
        .context("plan file must contain a JSON object")?;
    anyhow::ensure!(value.is_object(), "plan file must contain a JSON object");
    Ok(value)
}

fn response(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("ok");
    value
}

fn submit(client: &Client, mut request: Value, id: Option<uuid::Uuid>) -> Result<Value> {
    let id = id.unwrap_or_else(uuid::Uuid::now_v7);
    request["request_id"] = json!(id);
    let mut value = response(client.request(request)?);
    value["request_id"] = json!(id);
    Ok(value)
}

fn task_spec(path: &std::path::Path) -> Result<Value> {
    let raw: Value = serde_yaml::from_str(&input_text(None, Some(path))?)
        .context("task file must contain YAML or JSON")?;
    let spec: velella_core::task::TaskSpec =
        serde_json::from_value(raw.clone()).context("invalid task specification")?;
    anyhow::ensure!(
        spec.on_done == velella_core::task::OnDone::Review,
        "CLI tasks must use on_done: review; review results before delivery"
    );
    nonempty(&spec.name, "task name")?;
    nonempty(
        spec.instruction.as_deref().unwrap_or(""),
        "task instruction",
    )?;
    // Preserve absent fields: the daemon resolves the project's actual default base.
    Ok(raw)
}

pub(super) fn task(client: &Client, command: &TaskCommand) -> Result<Value> {
    match command {
        TaskCommand::Template => Ok(
            json!({"text": "name: my-task\nagent: codex\ninstruction: Describe the result and acceptance criteria here.\npolicy:\n  permissions: ask\n  budget:\n    wall_clock: 2h\ndone: []\nverify:\n  retries: 0\non_done: review\n"}),
        ),
        TaskCommand::List { project } => {
            let snapshot = client.snapshot()?;
            let rows: Vec<_> = array(&snapshot, "tasks")?.iter().map(task_row).collect();
            Ok(
                json!({"tasks":rows.iter().filter(|row| project.as_ref().is_none_or(|id| row["project_id"] == *id)).collect::<Vec<_>>()}),
            )
        }
        TaskCommand::Show { id } => {
            let snapshot = client.snapshot()?;
            let raw = find(array(&snapshot, "tasks")?, id, "task")?;
            let mut row = task_row(&raw);
            for key in [
                "spec",
                "checks",
                "verified",
                "direct",
                "created_at",
                "observed_turns",
                "pr_url",
            ] {
                row[key] = raw[key].clone();
            }
            Ok(json!({"task":row}))
        }
        TaskCommand::Create {
            project,
            file,
            request_id,
        } => {
            let spec = task_spec(file)?;
            let mut value = submit(
                client,
                json!({"cmd":"task.create","project_id":project,"spec":spec,"setup":"skip"}),
                *request_id,
            )?;
            value["task"] = task_row(&value["task"]);
            Ok(value)
        }
        TaskCommand::Events { id, since } => Ok(response(
            client.request(json!({"cmd":"task.events","task_id":id,"since":since}))?,
        )),
        TaskCommand::Diff { id } => Ok(response(
            client.request(json!({"cmd":"task.diff","task_id":id}))?,
        )),
        TaskCommand::Read {
            id,
            target,
            cursor,
            limit,
        } => output_page(
            client.request(
                json!({"cmd":"terminal.output","task_id":id,"target":target,"since":cursor}),
            )?,
            *limit as usize,
        ),
        TaskCommand::Resume { id }
        | TaskCommand::Verify { id }
        | TaskCommand::Cancel { id, .. } => {
            let cmd = match command {
                TaskCommand::Resume { .. } => "task.resume",
                TaskCommand::Verify { .. } => "task.verify",
                _ => "task.cancel",
            };
            client.request(json!({"cmd":cmd,"task_id":id}))?;
            Ok(
                json!({"task_id":id,"operation":cmd,"acknowledged":true,"completion_verified":false}),
            )
        }
        TaskCommand::Feedback {
            id,
            text,
            text_file,
            request_id,
        } => {
            let text = input_text(text.as_deref(), text_file.as_deref())?.replace("\r\n", "\n");
            nonempty(&text, "feedback")?;
            let mut value = submit(
                client,
                json!({"cmd":"task.feedback","task_id":id,"text":text}),
                *request_id,
            )?;
            if value["receipt"]["state"] != "submitted" {
                let code = if value["receipt"]["state"] == "rejected" {
                    "request_rejected"
                } else {
                    "delivery_uncertain"
                };
                return Err(failure(code, "Feedback was not confirmed. Inspect this receipt and task; do not resend with a new UUID.", value));
            }
            value["turn_started_confirmed"] = json!(false);
            Ok(value)
        }
        TaskCommand::FeedbackStatus { id, request_id } => Ok(response(client.request(
            json!({"cmd":"task.feedback.status","task_id":id,"request_id":request_id}),
        )?)),
    }
}

fn change_request(change: &Change, cmd: &str) -> Value {
    json!({"cmd":cmd,"id":change.id})
}

pub(super) fn swarm(client: &Client, command: &SwarmCommand) -> Result<Value> {
    let (request, rid) = match command {
        SwarmCommand::List { workspace } => {
            let value = client.request(json!({"cmd":"orchestration.list"}))?;
            return Ok(
                json!({"runs":array(&value,"runs")?.iter().filter(|row|workspace.as_ref().is_none_or(|id|row["workspace_id"]==*id)).collect::<Vec<_>>()}),
            );
        }
        SwarmCommand::Show { id }
        | SwarmCommand::Members { id }
        | SwarmCommand::Work { id }
        | SwarmCommand::Events { id } => {
            let value = client.request(json!({"cmd":"orchestration.snapshot","id":id}))?;
            return Ok(match command {
                SwarmCommand::Members { .. } => {
                    json!({"id":id,"members":value["run"]["status"]["members"]})
                }
                SwarmCommand::Work { .. } => json!({"id":id,"work":value["run"]["status"]["work"]}),
                SwarmCommand::Events { .. } => json!({"id":id,"events":value["run"]["events"]}),
                _ => response(value),
            });
        }
        SwarmCommand::Capabilities => {
            return Ok(response(
                client.request(json!({"cmd":"orchestration.capabilities"}))?,
            ))
        }
        SwarmCommand::Preflight { workspace } => {
            return Ok(response(client.request(
                json!({"cmd":"orchestration.preflight","workspace_id":workspace}),
            )?))
        }
        SwarmCommand::Create {
            workspace,
            goal,
            acceptance,
            agent,
            plan_file,
            request_id,
        } => {
            nonempty(goal, "goal")?;
            nonempty(acceptance, "acceptance")?;
            let mut request = json!({"cmd":"orchestration.create","schema_version":2,"workspace_id":workspace,"goal":goal,"acceptance":acceptance,"agent":agent});
            if let Some(path) = plan_file {
                request["plan"] = json_file(path)?;
            }
            (request, *request_id)
        }
        SwarmCommand::Pause(change)
        | SwarmCommand::Resume(change)
        | SwarmCommand::Stop { change, .. } => {
            let cmd = match command {
                SwarmCommand::Pause(_) => "orchestration.pause",
                SwarmCommand::Resume(_) => "orchestration.resume",
                _ => "orchestration.stop",
            };
            (change_request(change, cmd), change.request_id)
        }
        SwarmCommand::Retry {
            change,
            node,
            cause_key,
            new_evidence,
        } => {
            nonempty(cause_key, "cause key")?;
            nonempty(new_evidence, "new evidence")?;
            let mut request = change_request(change, "orchestration.retry");
            request["node_id"] = json!(node);
            request["cause_key"] = json!(cause_key);
            request["new_evidence"] = json!(new_evidence);
            (request, change.request_id)
        }
        SwarmCommand::Gate {
            change,
            node,
            revision,
            decision,
            summary,
        } => {
            nonempty(summary, "decision summary")?;
            let mut request = change_request(change, "orchestration.gate.resolve");
            request["node_id"] = json!(node);
            request["revision"] = json!(revision);
            request["approved"] = json!(decision == "approve");
            request["summary"] = json!(summary);
            (request, change.request_id)
        }
        SwarmCommand::Accept {
            change, summary, ..
        } => {
            nonempty(summary, "acceptance summary")?;
            let mut request = change_request(change, "orchestration.accept");
            request["summary"] = json!(summary);
            (request, change.request_id)
        }
        SwarmCommand::Plan { command } => {
            let (change, revision, cmd) = match command {
                PlanCommand::Save {
                    change, revision, ..
                } => (change, revision, "orchestration.plan.save"),
                PlanCommand::Refine {
                    change, revision, ..
                } => (change, revision, "orchestration.plan.refine"),
                PlanCommand::Confirm {
                    change, revision, ..
                } => (change, revision, "orchestration.plan.confirm"),
            };
            let mut request = change_request(change, cmd);
            request["revision"] = json!(revision);
            match command {
                PlanCommand::Save { file, .. } => request["plan"] = json_file(file)?,
                PlanCommand::Refine { instruction, .. } => {
                    nonempty(instruction, "refinement instruction")?;
                    request["instruction"] = json!(instruction);
                }
                _ => (),
            }
            (request, change.request_id)
        }
    };
    submit(client, request, rid)
}
