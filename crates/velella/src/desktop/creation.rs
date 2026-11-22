use super::*;
use crate::workbench::{CreationOperation, CreationStepState, Workspace};
use sha2::{Digest, Sha256};

pub(super) fn request_id(req: &Value) -> Result<String> {
    req.get("request_id").map(|id| {
        Ok(uuid::Uuid::parse_str(id.as_str().context("request_id must be a UUID string")?)?.to_string())
    }).unwrap_or_else(|| Ok(uuid::Uuid::now_v7().to_string()))
}

pub(super) fn parent(req: &Value) -> Result<Option<String>> {
    match req.get("parent_workspace_id") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(value.as_str().context("parent_workspace_id must be a string or null")?.to_owned())),
    }
}

pub(super) fn setup_snapshot(project: &Project, req: &Value, managed: bool) -> Result<SetupSnapshot> {
    let decision = match req.get("setup").map(|value| value.as_str().context("setup must be run, skip or inherit")).transpose()? {
        Some("run") => SetupPolicy::Run,
        Some("skip") => SetupPolicy::Skip,
        Some("inherit") => project.setup_default,
        None if managed => SetupPolicy::Run,
        None => project.setup_default,
        Some(_) => anyhow::bail!("setup must be run, skip or inherit"),
    };
    let reviewed = req.get("reviewed_setup").and_then(Value::as_str) == Some(project.setup.as_str());
    if !managed {
        creation_setup::validate_execution(req, decision)?;
        if decision == SetupPolicy::Run && !project.setup.trim().is_empty() {
            anyhow::ensure!(reviewed, "请审阅当前准备脚本后再执行");
        }
    }
    Ok(SetupSnapshot {
        decision,
        script: project.setup.clone(),
        digest: format!("{:x}", Sha256::digest(project.setup.as_bytes())),
        reviewed_at: reviewed.then(|| chrono::Utc::now().to_rfc3339()),
    })
}

impl Service {
    pub(super) fn create_workbench(&mut self, req: Value) -> Result<Value> {
        let id = request_id(&req)?;
        if let Some(operation) = self.workbench.creation(&id, Some(&req))? {
            let mut response = self.finish_workbench_checkout(operation)?;
            response["recovered"] = json!(true);
            return Ok(response);
        }
        let project = self.project(req["project_id"].as_str().context("project required")?)?;
        let sources = crate::task_sources::Store::open(&self.root)?;
        let source_key = req["source_key"].as_str();
        if let Some(saved) = sources.saved_workspace_request(&id)? {
            let base = req["base"].as_str().unwrap_or("HEAD");
            let original = json!({"project":project.id,"name":req["name"].as_str().unwrap_or("").trim(),"base":base,"source":source_key});
            anyhow::ensure!(saved == original, "创建请求标识已属于其他工作区，请重新打开创建窗口");
            anyhow::ensure!(parent(&req)?.is_none() && req.get("setup").is_none(), "旧创建记录不包含父级或准备脚本审批，请使用新的创建请求");
            anyhow::ensure!(req.get("launch").is_none_or(|v| v == "none") && req.get("launch_options").is_none() && req.get("initial_prompt").is_none(), "旧创建记录不包含启动审批，请使用新的创建请求");
            let workspace = self.workbench.workspace(&id).context("旧创建记录尚未登记工作区，请检查保留的 Git 工作树；未自动重放")?;
            anyhow::ensure!(workspace.project_id == project.id, "创建请求与工作区项目不匹配");
            if let Some(key) = source_key { sources.attach(key, &id, &project.id)?; }
            return Ok(json!({"ok":true,"workspace":workspace,"recovered":true,"legacy_receipt":true}));
        }
        if let Some(key) = source_key {
            sources.item(key)?;
            if let Some(workspace) = sources.linked_workspace(key, &project.id)? {
                return Ok(json!({"ok":true,"workspace":self.workbench.workspace(&workspace)?,"existing":true}));
            }
        }
        anyhow::ensure!(!project.direct, "ordinary folders have one primary workspace");
        let name = req["name"].as_str().context("name required")?.trim();
        anyhow::ensure!(!name.is_empty() && name.len() <= 240, "workspace name required (max 240 bytes)");
        let setup = setup_snapshot(&project, &req, false)?;
        let launch_request = creation_launch::freeze(&req, &id)?;
        let explicit = req.get("base").map(|v| v.as_str().context("base must be a string")).transpose()?;
        let base = creation_base::resolve(&project.path, explicit, project.default_base.as_deref())?;
        let workspace = Workspace { id:id.clone(), project_id:project.id, name:name.into(), path:project.path.join(".velella/worktrees").join(&id), kind:"worktree".into(), archived:false, favorite:false, position:0, parent_workspace_id:parent(&req)? };
        self.workbench.check_parent(&id, workspace.parent_workspace_id.as_deref())?;
        let operation = self.workbench.accept_creation(&id, &req, &json!({"workspace":workspace,"project_path":project.path,"base":base,"setup":setup,"launch_request":launch_request,"branch":format!("codex/workspace-{id}")}))?;
        self.finish_workbench_checkout(operation)
    }

    fn finish_workbench_checkout(&mut self, mut operation: CreationOperation) -> Result<Value> {
        anyhow::ensure!(creation_setup::is_ordinary(&operation), "not an ordinary workspace creation");
        let workspace: Workspace = serde_json::from_value(operation.frozen["workspace"].clone())?;
        let source = operation.request["source_key"].as_str().map(str::to_owned);
        if operation.steps[0].state == CreationStepState::Pending {
            self.workbench.check_parent(&workspace.id, workspace.parent_workspace_id.as_deref())?;
            operation.steps[0].state = CreationStepState::Running;
            operation = self.workbench.save_creation(&operation)?;
        }
        if operation.steps[0].state == CreationStepState::Running {
            let project = self.project(&workspace.project_id)?;
            let branch = operation.frozen["branch"].as_str().context("frozen branch missing")?;
            let oid = operation.frozen["base"]["oid"].as_str().context("frozen base missing")?;
            if workspace.path.exists() {
                anyhow::ensure!(git(&workspace.path, &["branch", "--show-current"])?.trim() == branch, "保留的工作目录与创建请求不匹配");
                let common = git(&workspace.path, &["rev-parse", "--git-common-dir"])?;
                let expected = git(&project.path, &["rev-parse", "--git-common-dir"])?;
                anyhow::ensure!(dunce::canonicalize(workspace.path.join(common.trim()))? == dunce::canonicalize(project.path.join(expected.trim()))?, "保留的工作目录不属于当前仓库");
                anyhow::ensure!(git(&workspace.path, &["rev-parse", "HEAD"])?.trim() == oid, "保留的工作树基准与创建记录不一致，请人工检查");
            } else {
                git(&project.path, &["worktree", "add", "-b", branch, workspace.path.to_str().context("workspace path")?, oid])?;
            }
            if !self.workbench.workspaces()?.iter().any(|w| w.id == workspace.id) {
                self.workbench.put_workspace(&workspace).with_context(|| format!("worktree retained at {}; registration failed", workspace.path.display()))?;
            }
            operation.steps[0].state = CreationStepState::Succeeded;
            operation = self.workbench.save_creation(&operation)?;
        }
        // The setup supervisor owns Run. Replays of a completed checkout never
        // touch Git or overwrite mutable workspace metadata from the snapshot.
        operation = creation_setup::settle_skips(&self.workbench, operation)?;
        if let Some(key) = source {
            crate::task_sources::Store::open(&self.root)?.attach(&key, &workspace.id, &workspace.project_id)?;
        }
        Ok(json!({"ok":true,"workspace":self.workbench.workspace(&workspace.id)?,"creation":operation}))
    }

    pub(super) fn persist_created_task(&mut self, task: Task) -> Result<Value> {
        if let Some(existing) = self.tasks.iter().find(|existing| existing.id == task.id) {
            return Ok(json!({"ok":true,"task":existing}));
        }
        let project = self.project(&task.project_id)?;
        let defs = project.path.join(".velella/tasks");
        std::fs::create_dir_all(&defs)?;
        std::fs::write(defs.join(format!("{}.yaml", task.id)), task.spec.to_yaml()?)?;
        self.persist(&task)?;
        self.tasks.push(task.clone());
        Ok(json!({"ok":true,"task":task}))
    }
}
