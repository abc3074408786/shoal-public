//! Desktop session authorization adapter. Never enter a host pipe or wait for a
//! mailbox while holding Service. Existing sessions only join by confirmation.
use super::*;

pub(super) fn prepare_native_member(desc:&mut Descriptor,executable:&Path,data:&Path)->Result<()> {
    prepare_member_environment(desc,executable,data)?;
    match desc.spawn.program.as_str() {
        "claude"=>{
            let settings=desc.directory.join("claude-collaboration-settings.json");
            ipc::atomic_json(&settings,&json!({"hooks":hook_settings()}))?;
            desc.spawn.args.extend(["--settings".into(),settings.to_string_lossy().into_owned()]);
            desc.spawn.adapter=Some("claude-code".into());
        }
        "codex"=>{
            for event in ["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Notification","Stop","Interrupt"] {
                desc.spawn.args.extend(["-c".into(),format!("hooks.{event}=[{{hooks=[{{type=\"command\",command=\"velella-runtime hook\"}}]}}]")]);
            }
            desc.spawn.adapter=Some("codex".into());
        }
        "gemini"|"opencode"|"kimi"|"omp"=>{desc.spawn.adapter=Some("generic".into());},
        _=>anyhow::bail!("collaboration requires a supported native CLI"),
    }
    Ok(())
}

pub(super) fn prepare_hybrid_member(desc:&mut Descriptor,executable:&Path,data:&Path)->Result<()> {
    anyhow::ensure!(desc.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker"),"hybrid worker adapter required");
    prepare_member_environment(desc,executable,data)
}

fn prepare_member_environment(desc:&mut Descriptor,executable:&Path,data:&Path)->Result<()> {
    anyhow::ensure!(!desc.spawn.sandbox.is_sandboxed(),"collaboration currently requires native execution");
    let mut paths=vec![executable.parent().context("runtime executable directory")?.to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    desc.spawn.env.insert("PATH".into(),std::env::join_paths(paths)?.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_SESSION_ID".into(),desc.id.clone());
    desc.spawn.env.insert("VELELLA_DATA_DIR".into(),dunce::canonicalize(data)?.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_SESSION_ENDPOINT".into(),desc.endpoint.clone());
    let skill=desc.directory.join("skills/velella-collaboration/SKILL.md");
    ipc::atomic_write(&skill,include_bytes!("../../../../skills/velella-collaboration/SKILL.md"))?;
    desc.spawn.env.insert("VELELLA_COLLABORATION_SKILL".into(),skill.to_string_lossy().into_owned());
    Ok(())
}

fn session(
    service: &Arc<Mutex<Service>>,
    id: &str,
) -> Result<(crate::workbench::Session, crate::workbench::Workspace)> {
    let state = service.lock().unwrap();
    let session = state
        .workbench
        .sessions(None)?
        .into_iter()
        .find(|s| s.id == id)
        .context("desktop session not found")?;
    let workspace = state.workbench.workspace(&session.workspace_id)?;
    anyhow::ensure!(!workspace.archived, "workspace is archived");
    Ok((session, workspace))
}

fn identity(
    session: &crate::workbench::Session,
    workspace: &crate::workbench::Workspace,
) -> Result<Value> {
    let cwd = dunce::canonicalize(
        session
            .descriptor
            .spawn
            .cwd
            .as_deref()
            .context("session has no working directory")?,
    )?;
    let path = dunce::canonicalize(&workspace.path)?;
    anyhow::ensure!(
        cwd == path,
        "native session working directory does not match its workspace"
    );
    let mut key = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        key = key.replace('/', "\\").to_lowercase();
    }
    Ok(
        json!({"session_id":session.id,"host_id":session.descriptor.id,"endpoint":session.descriptor.endpoint,"workspace_id":workspace.id,"workspace_path":key}),
    )
}

fn live(desc: &Descriptor) -> Result<Status> {
    anyhow::ensure!(
        !host::confirmed_dead(&desc.directory),
        "session host has ended; explicitly recover before joining"
    );
    let response = ipc::request_with_timeout(
        &desc.endpoint,
        &json!({"cmd":"status"}),
        Duration::from_secs(2),
    )?;
    anyhow::ensure!(
        response["ok"] == true,
        "session host did not confirm its status"
    );
    let status: Status = serde_json::from_value(response["status"].clone())?;
    anyhow::ensure!(status.exit.is_none(), "session has exited");
    Ok(status)
}

fn automatic_notify_ready(status: &Status, directory: &std::path::Path) -> Result<u64> {
    anyhow::ensure!(status.hooks_ready||status.hybrid_worker_ready, "native hooks or hybrid worker are not ready");
    anyhow::ensure!(status.state == "waiting_input", "native turn is not idle");
    anyhow::ensure!(!status.human_control, "a human has terminal control");
    anyhow::ensure!(!status.permission_input, "native permission input is active");
    anyhow::ensure!(!directory.join("terminal-input-pending.json").exists(), "previous terminal input remains unresolved");
    status.input_sequence.context("native input sequence is unknown")
}

fn copy_baseline_file(source: &std::path::Path, root: &std::path::Path, snapshot: &std::path::Path, files: &mut Vec<Value>) -> Result<()> {
    let relative = source.strip_prefix(root).context("baseline file leaves workspace")?;
    let metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata)=>metadata,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{files.push(json!({"path":relative.to_string_lossy().replace('\\',"/"),"deleted":true}));return Ok(());}
        Err(error)=>return Err(error.into()),
    };
    anyhow::ensure!(metadata.file_type().is_file(), "baseline source must be a regular file: {}", source.display());
    anyhow::ensure!(dunce::canonicalize(source)?.starts_with(root),"baseline source leaves workspace");
    let target = snapshot.join(relative);
    std::fs::create_dir_all(target.parent().context("baseline file has no parent")?)?;
    std::fs::copy(source, &target)?;
    files.push(json!({"path":relative.to_string_lossy().replace('\\',"/"),"bytes":metadata.len()}));
    Ok(())
}

fn copy_plain_tree(current: &std::path::Path, root: &std::path::Path, snapshot: &std::path::Path, files: &mut Vec<Value>) -> Result<()> {
    for item in std::fs::read_dir(current)? {
        let item = item?;
        let name = item.file_name();
        if matches!(name.to_str(), Some(".git" | ".velella")) { continue; }
        let path = item.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        anyhow::ensure!(!metadata.file_type().is_symlink(), "baseline does not follow symlinks: {}", path.display());
        if metadata.is_dir() { copy_plain_tree(&path, root, snapshot, files)?; }
        else if metadata.is_file() { copy_baseline_file(&path, root, snapshot, files)?; }
    }
    Ok(())
}

pub(super) fn capture_baseline(data: &std::path::Path, workspace: &crate::workbench::Workspace, request_id: &str) -> Result<Value> {
    uuid::Uuid::parse_str(request_id).context("invalid baseline request ID")?;
    let root = dunce::canonicalize(&workspace.path).context("workspace folder is unavailable")?;
    std::fs::create_dir_all(data)?;
    let canonical_data=dunce::canonicalize(data)?;
    anyhow::ensure!(!canonical_data.starts_with(&root)||canonical_data.starts_with(root.join(".velella")),"collaboration data must be outside source or inside excluded .velella metadata");
    let destination = data.join("collaboration-baselines").join(request_id);
    let manifest = destination.join("baseline.json");
    if manifest.exists() { return Ok(serde_json::from_slice(&std::fs::read(manifest)?)?); }
    anyhow::ensure!(!destination.exists(), "a previous baseline capture is incomplete; use a new create request ID");
    let source_candidate=crate::collaboration::source::candidate(&root)?;
    let snapshot = destination.join("source");
    std::fs::create_dir_all(&snapshot)?;
    let git_worktree = crate::process::command("git").current_dir(&root).args(["rev-parse","--is-inside-work-tree"]).output().map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim()=="true").unwrap_or(false);
    let mut files = Vec::new();
    let kind = if git_worktree {
        let listed = git(&root,&["ls-files","-z","--cached","--others","--exclude-standard"])?;
        for relative in listed.split('\0').filter(|path| !path.is_empty()) {
            let path = std::path::Path::new(relative);
            anyhow::ensure!(!path.is_absolute() && path.components().all(|c| matches!(c,std::path::Component::Normal(_))), "invalid Git baseline path");
            if path.components().any(|c|matches!(c,std::path::Component::Normal(name) if name==".velella" || name==".git")){continue;}
            copy_baseline_file(&root.join(path),&root,&snapshot,&mut files)?;
        }
        "git"
    } else {
        copy_plain_tree(&root,&root,&snapshot,&mut files)?;
        "folder"
    };
    files.sort_by(|a,b| a["path"].as_str().cmp(&b["path"].as_str()));
    anyhow::ensure!(source_candidate==crate::collaboration::source::candidate(&root)?,"source changed during baseline capture");
    let baseline = json!({"state":"captured","kind":kind,"workspace_path":root,"snapshot":snapshot,"files":files,"source_candidate":source_candidate,"captured_at":chrono::Utc::now().to_rfc3339()});
    ipc::atomic_json(&manifest,&baseline)?;
    Ok(baseline)
}

pub(super) fn dispatch(service: &Arc<Mutex<Service>>, req: Value) -> Result<Value> {
    let gate=super::orchestration::agent_gate(service,&req)?;
    let _guard=gate.as_ref().map(|g|g.lock().unwrap());
    dispatch_inner(service,req,false)
}
pub(super) fn managed(service: &Arc<Mutex<Service>>, req: Value) -> Result<Value> {
    dispatch_inner(service,req,true)
}
fn dispatch_inner(service: &Arc<Mutex<Service>>, mut req: Value, trusted:bool) -> Result<Value> {
    anyhow::ensure!(req["cmd"]!="collab.input.accepted","notification acknowledgement is internal to the desktop host adapter");
    if req["cmd"]=="collab.start" {
        uuid::Uuid::parse_str(req["request_id"].as_str().context("request_id required")?)?;
        req["cmd"]=json!("workbench.terminal.open");
        req["collaboration"]=json!(true);
        let response=terminal_io::dispatch(service,req)?;
        return confirmed_launch(service,response);
    }
    let (root, slot) = {
        let state = service.lock().unwrap();
        (state.root.clone(), state.collaboration.clone())
    };
    let hub = {
        let mut current = slot.lock().unwrap();
        if current.is_none() {
            *current = Some(Arc::new(crate::collaboration::Hub::open(&root)?));
        }
        current.as_ref().unwrap().clone()
    };
    if !trusted { hub.orchestration_guard(&req)?; }
    let cmd = req["cmd"].as_str().unwrap_or("").to_owned();
    let wire = req.clone();
    if let Some(response) = hub.replay(&wire)? {
        return Ok(response);
    }
    if cmd == "collab.launch" {
        // Launch is a coordinator-controlled adapter operation. The workbench
        // request ID is the same collaboration request ID, so a lost response
        // cannot create a second terminal on retry.
        let expected=hub.coordinator_identity(&req)?;
        let workspace=req["workspace_id"].as_str().context("workspace_id required")?;
        let owned=trusted && hub.orchestration_records()?.iter().any(|d|d["collab_id"]==req["run_id"] && d["owned_workspaces"].as_array().is_some_and(|ws|ws.contains(&json!(workspace))));
        anyhow::ensure!(workspace==expected["workspace_id"].as_str().unwrap_or("") || owned,"collaboration launch must use an owned workspace");
        let launch=req["launch"].as_str().context("launch required")?;
        crate::collaboration::providers::program(launch)?;
        let request_id=req["request_id"].as_str().context("request_id required")?;
        let remote=if req["role_config"].is_null(){None}else{serde_json::from_value::<crate::collaboration::orchestration::v2::RoleDefinition>(req["role_config"].clone())?.remote_codex};
        let request=if let Some(remote)=remote {
            anyhow::ensure!(trusted&&owned&&launch=="codex","remote collaboration requires an orchestrator-owned workspace");
            json!({"cmd":"workbench.terminal.open","workspace_id":workspace,"launch":"hybrid-codex","ssh_profile_id":remote.ssh_profile_id,"request_id":request_id,"collaboration":true,"role_config":req["role_config"]})
        }else{json!({"cmd":"workbench.terminal.open","workspace_id":workspace,"launch":launch,"request_id":request_id,"collaboration":true,"role_config":req["role_config"]})};
        let response=terminal_io::dispatch(service,request)?;
        return confirmed_launch(service,response);
    }
    if matches!(
        cmd.as_str(),
        "collab.create" | "collab.invite" | "collab.join"
    ) {
        // Resolve from desktop metadata. A caller cannot supply its own endpoint
        // or path and use collaboration as an arbitrary local pipe proxy.
        let (member, workspace) = session(
            service,
            req["session_id"].as_str().context("session_id required")?,
        )?;
        anyhow::ensure!(
            matches!(member.descriptor.spawn.program.as_str(), "codex" | "claude" | "gemini" | "opencode" | "kimi") || member.descriptor.spawn.env.get("VELELLA_PROVIDER").is_some_and(|provider|crate::collaboration::providers::program(provider).is_ok()),
            "join a supported native CLI terminal"
        );
        let _status = live(&member.descriptor)?;
        req["identity"] = identity(&member, &workspace)?;
        if cmd == "collab.create" {
            let request_id=req["request_id"].as_str().context("request_id required")?;
            req["baseline"] = capture_baseline(&root,&workspace,request_id)?;
            let links=crate::task_sources::Store::open(&root)?.links()?.into_iter().filter(|link| link["workspace_id"]==workspace.id).collect::<Vec<_>>();
            req["source"] = json!({"workspace_id":workspace.id,"project_id":workspace.project_id,"links":links});
        }
    } else if cmd=="collab.handoff" {
        // The coordinator still authenticates from its current native session.
        // The replacement is resolved independently from the desktop registry;
        // callers cannot smuggle an endpoint or working directory into the
        // durable handoff record.
        let expected = hub.actor_identity(&req)?;
        let (actor, actor_workspace) = session(service, expected["session_id"].as_str().context("member session missing")?)?;
        anyhow::ensure!(identity(&actor,&actor_workspace)?==expected,"native session identity changed; explicit reconciliation is required");
        let (replacement, workspace)=session(service,req["session_id"].as_str().context("handoff session_id required")?)?;
        anyhow::ensure!(matches!(replacement.descriptor.spawn.program.as_str(),"codex"|"claude"|"gemini"|"opencode"|"kimi") || replacement.descriptor.spawn.env.get("VELELLA_PROVIDER").is_some_and(|provider|crate::collaboration::providers::program(provider).is_ok()),"handoff requires a supported native CLI terminal");
        let _status=live(&replacement.descriptor)?;
        req["identity"]=identity(&replacement,&workspace)?;
    } else {
        let expected = hub.actor_identity(&req)?;
        if let Some(native)=req["native_session_id"].as_str() {anyhow::ensure!(native==expected["session_id"].as_str().unwrap_or(""),"native session no longer owns this collaboration member; open the replacement handoff session");}
        let (member, workspace) = session(
            service,
            expected["session_id"]
                .as_str()
                .context("member session missing")?,
        )?;
        anyhow::ensure!(
            identity(&member, &workspace)? == expected,
            "native session identity changed; explicit reconciliation is required"
        );
        if cmd == "collab.reconcile" {
            let status = live(&member.descriptor)?;
            return Ok(
                json!({"ok":true,"identity":expected,"status":status,"automatic_notify_ready":automatic_notify_ready(&status,&member.descriptor.directory).is_ok()}),
            );
        }
    }
    if !matches!(cmd.as_str(),"collab.create"|"collab.invite"|"collab.join") {
        if let Some(mut work)=hub.candidate_work(&req)? {
            if cmd=="collab.candidate" {
                return Ok(json!({"ok":true,"candidate":crate::collaboration::source::candidate(Path::new(work["workspace_path"].as_str().context("work workspace missing")?))?}));
            }
            if work["write"]==true || work["evidence_contract"].is_object() {
                let expected=if cmd=="collab.report"{&req["candidate"]}else{&work["candidate"]};
                let actual=crate::collaboration::source::candidate(Path::new(work["workspace_path"].as_str().context("work workspace missing")?))?;
                anyhow::ensure!(&actual==expected,"candidate source changed or fingerprint is invalid; capture and review the current candidate");
            }
            if work["evidence_contract"].is_object() {
                if cmd=="collab.report"{work["candidate"]=req["candidate"].clone();}
                let mut evidence=hub.orchestration_evidence(&work,&cmd)?;
                if matches!(cmd.as_str(),"collab.report"|"collab.review.report"){evidence.push(req["evidence"].clone());}
                for value in evidence {crate::collaboration::orchestration::evidence::verify_files(&work,&value)?;}
            }
        }
    }
    let mut response = hub.execute_resolved(&wire, &req)?;
    if cmd != "collab.notify" {
        return Ok(response);
    }
    // The durable intent commits before we touch a native host. Replaying its
    // request returns above, so an unknown input outcome is never resent.
    let recipient = hub.coordinator_recipient(
        &req,
        response["notification"]["recipient"].as_str().context("notification recipient missing")?,
    )?;
    let (target, workspace) = session(
        service,
        recipient["identity"]["session_id"].as_str().context("recipient session missing")?,
    )?;
    if identity(&target, &workspace)? != recipient["identity"] {
        response["input"] = json!({"state":"deferred","reason":"recipient native identity changed"});
        return Ok(response);
    }
    let status = match live(&target.descriptor) {
        Ok(status) => status,
        Err(error) => {
            response["input"] = json!({"state":"deferred","reason":error.to_string()});
            return Ok(response);
        }
    };
    let sequence = match automatic_notify_ready(&status, &target.descriptor.directory) {
        Ok(sequence) => sequence,
        Err(error) => {
            response["input"] = json!({"state":"deferred","reason":error.to_string()});
            return Ok(response);
        }
    };
    let guidance = if target.descriptor.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker") {
        "Velella collaboration notification. Use the session-bound collaboration MCP tool to read your durable mailbox and status before acting. Local writes and commands require individual approval."
    } else {
        "Velella collaboration notification. Read $VELELLA_COLLABORATION_SKILL and your durable mailbox before acting."
    };
    let text = format!("{}\n\n{}", guidance, response["notification"]["text"].as_str().unwrap_or(""));
    match terminal_io::ordered_host_request(&target.descriptor, &json!({
        "cmd":"input","text":text,"enter":true,"automated":true,
        "input_kind":"legacy","expected_input_sequence":sequence
    })) {
        Err(error) => response["input"] = json!({"state":"uncertain","reason":error.to_string()}),
        Ok(ack) if ack["ok"]!=true => {
            response["input"]=json!({"state":"rejected","reason":ack["error"]});
        }
        Ok(_) => {
            let notification_id = response["notification"]["id"].as_str().context("notification id missing")?;
            let accepted = json!({
                "cmd":"collab.input.accepted","run_id":req["run_id"],"member_id":req["member_id"],"token":req["token"],
                "notification_id":notification_id,"request_id":format!("notification:{notification_id}:input-accepted")
            });
            match hub.execute_resolved(&accepted, &accepted) {
                Ok(accepted) => { response["notification"] = accepted["notification"].clone(); response["input"] = json!({"state":"accepted"}); }
                Err(error) => response["input"] = json!({"state":"uncertain","reason":format!("input accepted but durable acknowledgement failed: {error}")}),
            }
        }
    }
    Ok(response)
}

fn confirmed_launch(service:&Arc<Mutex<Service>>,mut response:Value)->Result<Value> {
    let (member,_)=session(service,response["session_id"].as_str().context("launch did not return a session")?)?;
    let status=live(&member.descriptor).context("launch is recorded but the host is unavailable; inspect or explicitly recover this session, do not launch another")?;
    response["state"]=json!("host_running");
    response["hooks_ready"]=json!(status.hooks_ready);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_baseline_captures_dirty_and_nonignored_files_without_writing_workspace() {
        let base=std::env::temp_dir().join(format!("velella-collab-baseline-{}",uuid::Uuid::now_v7()));
        let source=base.join("source"); let data=base.join("data");
        std::fs::create_dir_all(&source).unwrap();
        git(&source,&["init"]).unwrap();
        std::fs::write(source.join("tracked.txt"),"initial").unwrap();
        std::fs::write(source.join(".gitignore"),"ignored.txt\n").unwrap();
        git(&source,&["add","tracked.txt",".gitignore"]).unwrap();
        std::fs::write(source.join("tracked.txt"),"dirty").unwrap();
        std::fs::write(source.join("untracked.txt"),"untracked").unwrap();
        std::fs::write(source.join("ignored.txt"),"ignored").unwrap();
        std::fs::write(source.join("deleted.txt"),"deleted").unwrap();
        git(&source,&["add","deleted.txt"]).unwrap();
        std::fs::remove_file(source.join("deleted.txt")).unwrap();
        std::fs::create_dir_all(source.join(".velella")).unwrap();
        std::fs::write(source.join(".velella/credentials.json"),"private runtime state").unwrap();
        let workspace=crate::workbench::Workspace{id:uuid::Uuid::now_v7().to_string(),project_id:"p".into(),name:"w".into(),path:source.clone(),kind:"worktree".into(),archived:false,favorite:false,position:0,parent_workspace_id:None};
        let request=uuid::Uuid::now_v7().to_string();
        let baseline=capture_baseline(&data,&workspace,&request).unwrap();
        let snapshot=std::path::PathBuf::from(baseline["snapshot"].as_str().unwrap());
        assert_eq!(std::fs::read_to_string(snapshot.join("tracked.txt")).unwrap(),"dirty");
        assert_eq!(std::fs::read_to_string(snapshot.join("untracked.txt")).unwrap(),"untracked");
        assert!(!snapshot.join("ignored.txt").exists());
        assert!(!snapshot.join(".velella").exists());
        assert!(baseline["files"].as_array().unwrap().iter().any(|file|file["path"]=="deleted.txt"&&file["deleted"]==true));
        assert_eq!(std::fs::read_to_string(source.join("tracked.txt")).unwrap(),"dirty");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn folder_baseline_stays_a_folder_and_excludes_runtime_metadata() {
        let base=std::env::temp_dir().join(format!("velella-folder-baseline-{}",uuid::Uuid::now_v7()));
        let source=base.join("source"); let data=base.join("data");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::create_dir_all(source.join(".velella")).unwrap();
        std::fs::write(source.join("nested").join("source.txt"),"plain").unwrap();
        std::fs::write(source.join(".velella").join("state.db"),"runtime").unwrap();
        let workspace=crate::workbench::Workspace{id:uuid::Uuid::now_v7().to_string(),project_id:"p".into(),name:"plain".into(),path:source.clone(),kind:"folder".into(),archived:false,favorite:false,position:0,parent_workspace_id:None};
        let baseline=capture_baseline(&data,&workspace,&uuid::Uuid::now_v7().to_string()).unwrap();
        let snapshot=std::path::PathBuf::from(baseline["snapshot"].as_str().unwrap());
        assert_eq!(baseline["kind"],"folder");
        assert_eq!(std::fs::read_to_string(snapshot.join("nested").join("source.txt")).unwrap(),"plain");
        assert!(!snapshot.join(".velella").exists());
        assert!(!source.join(".git").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
