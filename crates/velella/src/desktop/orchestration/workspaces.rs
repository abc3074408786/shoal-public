//! Owned Git checkouts. The user's worktree and index are never used for writes.
use super::*;
use crate::workbench::Workspace;
pub(super) fn git_output(path:&Path,args:&[&str])->Result<String>{git(path,args)}
fn commit(path:&Path,message:&str)->Result<String>{
    git(path,&["-c","user.name=Velella","-c","user.email=velella@localhost","-c","commit.gpgsign=false","-c","core.hooksPath=NUL","commit","--allow-empty","-m",message])?;
    Ok(git(path,&["rev-parse","HEAD"])?.trim().into())
}
fn make(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,key:&str,base:&str)->Result<Workspace>{
    let source={service.lock().unwrap().workbench.workspace(text(doc,"workspace_id")?)?};
    let path=source.path.join(".velella/orchestration").join(text(doc,"id")?).join(key);
    let branch=format!("codex/orch/{}/{}",text(doc,"id")?,key);
    let id=doc["workspace_bindings"][key]["id"].as_str().map(str::to_owned).unwrap_or_else(||uuid::Uuid::now_v7().to_string());
    if doc["workspace_bindings"][key].is_null(){
        doc["workspace_bindings"][key]=json!({"id":id,"path":path,"branch":branch,"state":"creating"});h.orchestration_put(doc)?;
    }
    if !path.exists(){
        std::fs::create_dir_all(path.parent().context("worktree parent")?)?;
        git(&source.path,&["worktree","add","-b",&branch,path.to_str().context("worktree path")?,base])?;
    }else{
        anyhow::ensure!(git(&path,&["branch","--show-current"])?.trim()==branch,"保留工作树与运行不匹配");
        let actual=git(&path,&["rev-parse","--path-format=absolute","--git-common-dir"])?;
        let expected=git(&source.path,&["rev-parse","--path-format=absolute","--git-common-dir"])?;
        anyhow::ensure!(actual.trim()==expected.trim(),"工作树不属于此仓库");
    }
    let workspace=Workspace{id:id.clone(),project_id:source.project_id,name:format!("编排 · {key}"),path,kind:"worktree".into(),archived:false,favorite:false,position:0,parent_workspace_id:None};
    service.lock().unwrap().workbench.put_workspace(&workspace)?;
    if !doc["owned_workspaces"].as_array().context("owned workspaces missing")?.contains(&json!(id)){doc["owned_workspaces"].as_array_mut().unwrap().push(json!(id));}
    doc["workspace_bindings"][key]["state"]=json!("created");h.orchestration_put(doc)?;Ok(workspace)
}
fn snapshot_source(snapshot:&Value)->Result<&Value>{
    let source=&snapshot["source_candidate"];
    anyhow::ensure!(source.is_object()&&source["revision"].is_string()&&source["git_head"].is_string(),"基线快照缺少源码身份，不能安全恢复；请重新创建协作");
    Ok(source)
}
fn snapshot_revision(snapshot:&Value,include_deleted:bool)->Result<String>{
    use sha2::{Digest,Sha256};
    let snapshot_path=Path::new(text(snapshot,"snapshot")?);let mut manifest=Vec::new();
    for file in snapshot["files"].as_array().context("snapshot files missing")?{
        let relative=text(file,"path")?;
        if file["deleted"]==true {if include_deleted{manifest.push(json!({"path":relative,"deleted":true}));}continue}
        let bytes=std::fs::read(snapshot_path.join(relative))?;
        manifest.push(json!({"path":relative,"sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()}));
    }
    Ok(format!("sha256:{:x}",Sha256::digest(serde_json::to_vec(&manifest)?)))
}
fn worktree_has_no_unrecorded_files(path:&Path)->Result<bool>{
    Ok(git(path,&["status","--porcelain=v1","--untracked-files=all","--ignored=matching","--",".",":(exclude).velella"])?.trim().is_empty())
}
fn pristine_baseline_workspace(path:&Path,head:&str)->Result<()> {
    anyhow::ensure!(git(path,&["rev-parse","--verify","CHERRY_PICK_HEAD"]).is_err(),"基线工作树存在未完成合入，保留现场");
    anyhow::ensure!(worktree_has_no_unrecorded_files(path)?,"基线工作树包含未记录修改，保留现场");
    anyhow::ensure!(git(path,&["rev-parse","HEAD"])?.trim()==head,"基线工作树起点不匹配，保留现场");
    Ok(())
}
fn recover_committed_baseline(snapshot:&Value,source:&Value,workspace:&Workspace)->Result<String>{
    anyhow::ensure!(git(&workspace.path,&["rev-parse","--verify","CHERRY_PICK_HEAD"]).is_err(),"基线创建中断且存在未完成合入，保留现场");
    anyhow::ensure!(worktree_has_no_unrecorded_files(&workspace.path)?,"基线创建中断且工作树包含未记录修改，保留现场");
    let head=git(&workspace.path,&["rev-parse","HEAD"])?.trim().to_owned();let parent=git(&workspace.path,&["log","-1","--format=%P"])?.trim().to_owned();
    anyhow::ensure!(parent==text(source,"git_head")?,"中断后的基线提交不是捕获源码的直接后继，保留现场");
    anyhow::ensure!(git(&workspace.path,&["log","-1","--format=%B"])?.trim()=="Velella immutable input baseline","中断后的基线提交身份不明确，保留现场");
    let revision=snapshot_revision(snapshot,false)?;
    let candidate=crate::collaboration::source::candidate(&workspace.path)?;anyhow::ensure!(candidate["revision"]==revision,"中断后的基线内容与捕获快照不一致，保留现场");
    Ok(head)
}
pub(super) fn baseline(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value)->Result<()> {
    if doc["baseline_commit"].is_string(){return Ok(())}
    let (root,source)={let s=service.lock().unwrap();(s.root.clone(),s.workbench.workspace(text(doc,"workspace_id")?)?)};
    let before=crate::collaboration::source::candidate(&source.path)?;
    let snapshot=collaboration_commands::capture_baseline(&root,&source,text(doc,"id")?)?;
    let captured=snapshot_source(&snapshot)?;anyhow::ensure!(before==*captured&&before==crate::collaboration::source::candidate(&source.path)?,"捕获快照与当前源码身份不一致，保留快照并停止启动；待修改稳定后重新创建协作");
    anyhow::ensure!(captured["revision"]==snapshot_revision(&snapshot,true)?,"基线快照内容已变化，保留现场");
    let head=text(captured,"git_head")?.to_owned();
    let workspace=make(service,h,doc,"baseline",&head)?;
    if doc["baseline_capture_started"]==true {
        let oid=recover_committed_baseline(&snapshot,captured,&workspace)?;
        doc["baseline_commit"]=json!(oid);doc["baseline_candidate"]=before;doc["baseline_snapshot"]=snapshot;return h.orchestration_put(doc)
    }
    pristine_baseline_workspace(&workspace.path,&head)?;
    doc["baseline_capture_started"]=json!(true);h.orchestration_put(doc)?;
    let snapshot_path=Path::new(text(&snapshot,"snapshot")?);
    let included:std::collections::HashSet<&str>=snapshot["files"].as_array().unwrap().iter().filter(|f|f["deleted"]!=true).filter_map(|f|f["path"].as_str()).collect();
    for relative in git(&workspace.path,&["ls-files","-z"])?.split('\0').filter(|s|!s.is_empty()) {
        if !included.contains(relative) {let target=workspace.path.join(relative);if target.is_file(){std::fs::remove_file(target)?;}}
    }
    for file in snapshot["files"].as_array().context("snapshot files missing")? {
        let relative=text(file,"path")?;let target=workspace.path.join(relative);
        if file["deleted"]==true {if target.exists(){std::fs::remove_file(&target)?;}}else{std::fs::create_dir_all(target.parent().context("file parent")?)?;std::fs::copy(snapshot_path.join(relative),target)?;}
    }
    git(&workspace.path,&["add","-A","--",".",":(exclude).velella"])?;
    let oid=commit(&workspace.path,"Velella immutable input baseline")?;
    // Content hash is independent of path and Git HEAD. The snapshot directory
    // can sit below an unrelated Git repository, so hash only its declared files.
    let revision=snapshot_revision(&snapshot,false)?;
    anyhow::ensure!(json!(revision)==crate::collaboration::source::candidate(&workspace.path)?["revision"],"隔离基线与捕获源码不一致");
    doc["baseline_commit"]=json!(oid);doc["baseline_candidate"]=before;doc["baseline_snapshot"]=snapshot;h.orchestration_put(doc)
}
fn import_commit(h:&Hub,doc:&mut Value,path:&Path,key:&str,oid:&str)->Result<bool>{
    let marker=format!("import.{key}.{oid}");
    if doc["imports"][&marker]=="done" || doc["imports"][&marker]["state"]=="done" {return Ok(true)}
    let current=git(path,&["rev-parse","HEAD"])?.trim().to_owned();
    let pending=git(path,&["rev-parse","--verify","CHERRY_PICK_HEAD"]).ok().map(|s|s.trim().to_owned());
    if doc["imports"][&marker].is_null(){
        anyhow::ensure!(pending.is_none() && git(path,&["status","--porcelain","--",".",":(exclude).velella"])?.trim().is_empty(),"合入前工作树包含未记录修改，保留现场");
        doc["imports"][&marker]=json!({"state":"started","before":current,"source":oid});h.orchestration_put(doc)?;
    }
    let receipt=doc["imports"][&marker].clone();
    let before=text(&receipt,"before").context("旧合入记录缺少起始提交，须核对原现场")?;
    if pending.as_deref()==Some(oid) {return Ok(false)}
    anyhow::ensure!(pending.is_none(),"工作树存在其他合入操作，保留现场");
    if current!=before {
        let parent=git(path,&["rev-parse","HEAD^"])?;
        let message=git(path,&["log","-1","--format=%B"])?;
        anyhow::ensure!(parent.trim()==before && message.lines().any(|line|line==format!("(cherry picked from commit {oid})")) && git(path,&["status","--porcelain","--",".",":(exclude).velella"])?.trim().is_empty(),"中断后合入身份不明确，保留现场");
        doc["imports"][&marker]["state"]=json!("done");doc["imports"][&marker]["head"]=json!(current);h.orchestration_put(doc)?;return Ok(true)
    }
    anyhow::ensure!(receipt["state"]=="started","合入曾失败，请先处理已记录原因");
    anyhow::ensure!(git(path,&["status","--porcelain","--",".",":(exclude).velella"])?.trim().is_empty(),"中断后存在未记录修改，保留现场");
    match git(path,&["-c","user.name=Velella","-c","user.email=velella@localhost","-c","commit.gpgsign=false","-c","core.hooksPath=NUL","cherry-pick","-x","--allow-empty","--keep-redundant-commits",oid]) {
        Ok(_)=>{doc["imports"][&marker]["state"]=json!("done");doc["imports"][&marker]["head"]=json!(git(path,&["rev-parse","HEAD"])?.trim());h.orchestration_put(doc)?;Ok(true)},
        Err(error)=>{
            if git(path,&["rev-parse","--verify","CHERRY_PICK_HEAD"]).is_ok_and(|s|s.trim()==oid){doc["imports"][&marker]["state"]=json!("conflict");h.orchestration_put(doc)?;Ok(false)}
            else{doc["imports"][&marker]["state"]=json!("failed");doc["imports"][&marker]["error"]=json!(error.to_string());h.orchestration_put(doc)?;Err(error)}
        }
    }
}
pub(super) fn verify_imports(doc:&Value,key:&str,path:&Path)->Result<()> {
    anyhow::ensure!(git(path,&["rev-parse","--verify","CHERRY_PICK_HEAD"]).is_err() && git(path,&["diff","--name-only","--diff-filter=U"])?.trim().is_empty(),"集成合入尚未结束");
    let baseline=text(doc,"baseline_commit")?;
    git(path,&["merge-base","--is-ancestor",baseline,"HEAD"]).context("集成候选离开原始基线")?;
    let log=git(path,&["log","--format=%B",&format!("{baseline}..HEAD"),"--"])?;
    for oid in doc["workspace_bindings"][key]["imports"].as_array().into_iter().flatten() {
        let oid=oid.as_str().context("invalid import identity")?;
        anyhow::ensure!(log.lines().any(|line|line==format!("(cherry picked from commit {oid})")),"集成候选缺少依赖提交的合入回执：{oid}");
    }
    Ok(())
}
pub(super) fn verify_integration(doc:&Value,path:&Path)->Result<()> {verify_imports(doc,"integration",path)}
pub(super) fn node_workspace(service:&Arc<Mutex<Service>>,h:&Hub,doc:&mut Value,node:&model::v2::Node)->Result<Workspace>{
    let key=if node.id=="__verify"{"verify"}else if node.id=="__integrate"{"integration"}else{&node.id};
    let baseline=text(doc,"baseline_commit")?.to_owned();
    let verify_base=doc["bindings"]["__integrate"]["commit"].as_str().map(str::to_owned);
    let workspace=make(service,h,doc,key,if node.id=="__verify"{verify_base.as_deref().context("集成候选未就绪")?}else{&baseline})?;
    if doc["workspace_bindings"][key]["input_head"].is_string(){return Ok(workspace)}
    let plan=model::v2::validate(&doc["plan"])?;
    let mut needed=std::collections::HashSet::new();
    fn ancestors(id:&str,p:&model::v2::Plan,needed:&mut std::collections::HashSet<String>){if !needed.insert(id.into()){return}if let Some(n)=p.nodes.iter().find(|n|n.id==id){for dep in &n.depends_on{ancestors(dep,p,needed)}}}
    for dep in &node.depends_on {ancestors(dep,&plan,&mut needed);}
    let mut remaining:Vec<_>=plan.nodes.iter().filter(|n|needed.contains(&n.id)).collect();let mut done=std::collections::HashSet::new();let mut imports=Vec::<String>::new();
    while !remaining.is_empty(){
        let index=remaining.iter().position(|n|n.depends_on.iter().all(|d|done.contains(d))).context("依赖集成顺序无效")?;let dependency=remaining.remove(index);done.insert(dependency.id.clone());
        // Verification starts from the already-integrated candidate, so it must not
        // replay implementation commits.  Integration, however, starts from the
        // immutable baseline and is responsible for importing every accepted
        // dependency in dependency order.
        if node.id=="__verify"{continue}
        let binding=&doc["bindings"][&dependency.id];
        if let Some(commit)=binding["commit"].as_str(){
            let input=text(binding,"input_head")?;
            git(&workspace.path,&["merge-base","--is-ancestor",input,commit]).context("依赖候选不再继承派发时的输入提交")?;
            let range=format!("{input}..{commit}");
            // These are revisions, never paths. Explicitly terminate revisions
            // so Git does not stat a long OID range under a deep Windows cwd.
            anyhow::ensure!(git(&workspace.path,&["rev-list","--min-parents=2",&range,"--"])?.trim().is_empty(),"步骤增量包含合并提交，需要明确的集成方案");
            let commits=git(&workspace.path,&["rev-list","--reverse","--topo-order",&range,"--"])?;
            for c in commits.lines().filter(|s|!s.is_empty()) {
                if !imports.iter().any(|oid|oid==c){imports.push(c.into());}
            }
        }
    }
    doc["workspace_bindings"][key]["imports"]=json!(imports);h.orchestration_put(doc)?;
    let can_resolve_conflict=node.id=="__integrate" || plan.roles.iter().find(|role|role.id==node.role_id).is_some_and(|role|role.kind=="integrate");
    for (index,oid) in imports.iter().enumerate() {
        if !import_commit(h,doc,&workspace.path,key,oid)? {
            anyhow::ensure!(can_resolve_conflict,"依赖合入冲突，保留工作树 {}；该普通步骤不能改派，需由方案中的集成角色处理",workspace.path.display());
            doc["workspace_bindings"][key]["pending_imports"]=json!(&imports[index..]);
            // A designated integration role receives the conflicted checkout and
            // exact ordered remainder, rather than being blocked before launch.
            break
        }
    }
    let head=git(&workspace.path,&["rev-parse","HEAD"])?;doc["workspace_bindings"][key]["input_head"]=json!(head.trim());
    if node.id=="__integrate"{doc["integration_workspace"]=json!(workspace.id);doc["integration_branch"]=json!(format!("codex/orch/{}/integration",text(doc,"id")?));}
    h.orchestration_put(doc)?;Ok(workspace)
}
