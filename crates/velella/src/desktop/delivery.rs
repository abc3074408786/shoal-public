use super::*;
use std::io::Write;

/// A fixed reviewed snapshot, persisted before commit and after each side effect.
#[derive(Clone, Serialize, Deserialize)]
pub struct DeliveryProgress {
    #[serde(default)]
    pub pull_request: bool,
    pub base_head: String,
    pub tree: String,
    pub commit_oid: Option<String>,
    pub pushed: bool,
    pub pr_attempted: bool,
}

struct PrBody(PathBuf);
impl PrBody {
    fn new(text:&str)->Result<Self>{
        let path=std::env::temp_dir().join(format!("velella-pr-{}.txt",uuid::Uuid::now_v7()));
        let mut file=OpenOptions::new().write(true).create_new(true).open(&path)?;
        let body=Self(path);
        file.write_all(text.as_bytes())?;
        file.flush()?;
        Ok(body)
    }
}
impl Drop for PrBody {fn drop(&mut self){let _=std::fs::remove_file(&self.0);}}

impl Service {
    fn save_delivery(&mut self,index:usize,task:&Task)->Result<()> {
        self.persist(task)?;
        self.tasks[index]=task.clone();
        Ok(())
    }

    pub(super) fn deliver_task(&mut self,index:usize,task:&mut Task,pr:bool)->Result<Value>{
        anyhow::ensure!(!task.direct,"普通文件夹不支持 Git 交付");
        anyhow::ensure!(task.phase==Phase::Review,"only reviewed tasks may be delivered");
        anyhow::ensure!(git(&task.worktree,&["branch","--show-current"])?.trim()==task.branch,"工作区分支已改变，请先核对任务分支");
        if task.delivery.is_none(){
            self.stop(task)?;
            let base_head=git(&task.worktree,&["rev-parse","HEAD"])?.trim().to_owned();
            git(&task.worktree,&["add","-A"])?;
            let tree=git(&task.worktree,&["write-tree"])?.trim().to_owned();
            task.delivery=Some(DeliveryProgress{pull_request:pr,base_head,tree,commit_oid:None,pushed:false,pr_attempted:false});
            task.reason="已固定本次交付的文件快照，准备本地提交".into();
            self.save_delivery(index,task)?;
        }
        let progress=task.delivery.as_ref().unwrap().clone();
        anyhow::ensure!(progress.pull_request==pr,"已有另一种待完成的交付；请先继续该步骤，或明确重新审查");
        if progress.commit_oid.is_none(){
            let head=git(&task.worktree,&["rev-parse","HEAD"])?.trim().to_owned();
            let tree=git(&task.worktree,&["rev-parse","HEAD^{tree}"])?.trim().to_owned();
            if tree!=progress.tree {
                anyhow::ensure!(head==progress.base_head,"提交记录发生变化，请重新审查");
                anyhow::ensure!(git(&task.worktree,&["write-tree"])?.trim()==progress.tree,"暂存区发生变化，请重新审查");
                git(&task.worktree,&["commit","-m",&format!("velella: {}",task.spec.name)])?;
            }
            let committed=git(&task.worktree,&["rev-parse","HEAD"])?.trim().to_owned();
            anyhow::ensure!(git(&task.worktree,&["rev-parse","HEAD^{tree}"])?.trim()==progress.tree,"提交内容与审查快照不一致");
            task.delivery.as_mut().unwrap().commit_oid=Some(committed);
            task.reason="本地提交已保留，等待后续交付步骤".into();
            self.save_delivery(index,task)?;
        }
        let oid=task.delivery.as_ref().unwrap().commit_oid.as_ref().unwrap().clone();
        anyhow::ensure!(git(&task.worktree,&["rev-parse","HEAD"])?.trim()==oid,"HEAD 已改变；不会自动提交其他版本，请重新审查");
        anyhow::ensure!(git(&task.worktree,&["status","--porcelain"])?.trim().is_empty(),"工作区在交付过程中发生变化；请重新审查新改动，不会自动追加提交");
        if pr {
            let reference=format!("refs/heads/{}",task.branch);
            let remote=git(&task.worktree,&["ls-remote","--heads","origin",&reference])?;
            let remote_oid=remote.split_whitespace().next();
            if remote_oid!=Some(oid.as_str()){
                anyhow::ensure!(!task.delivery.as_ref().unwrap().pushed,"远端分支已变化；不会重新覆盖，请核对远端");
                git(&task.worktree,&["push","-u","origin",&format!("{oid}:{reference}")])?;
            }
            task.delivery.as_mut().unwrap().pushed=true;
            task.reason="任务提交已推送，正在核对 Draft PR".into();
            self.save_delivery(index,task)?;
            let listed=process::command("gh").current_dir(&task.worktree).args(["pr","list","--head",&task.branch,"--base",&task.spec.base,"--state","all","--limit","100","--json","url,headRefOid,state"]).output()?;
            anyhow::ensure!(listed.status.success(),"无法确认远端 PR 状态：{}",String::from_utf8_lossy(&listed.stderr).trim());
            let existing:Vec<Value>=serde_json::from_slice(&listed.stdout).context("invalid PR lookup response")?;
            if let Some(found)=existing.iter().find(|p|p["headRefOid"]==oid){
                task.pr_url=Some(found["url"].as_str().filter(|u|u.starts_with("https://")).context("PR URL missing")?.to_owned());
                anyhow::ensure!(found["state"]=="OPEN","关联 PR 状态为 {}；请核对远端，本次不标记为已交付，也不重复创建",found["state"].as_str().unwrap_or("未知"));
            }else{
                anyhow::ensure!(existing.is_empty(),"发现同分支的其他 PR 版本；请核对远端，不会重复创建");
                task.delivery.as_mut().unwrap().pr_attempted=true;
                task.reason="提交与推送已保留，正在创建 Draft PR".into();
                self.save_delivery(index,task)?;
                let body=PrBody::new(&format!("{}\n\nVerification: {}\n",task.spec.instruction.as_deref().unwrap_or(""),if task.verified{"passed"}else{"not verified"}))?;
                let output=process::command("gh").current_dir(&task.worktree).args(["pr","create","--draft","--title",&task.spec.name,"--base",&task.spec.base,"--head",&task.branch,"--body-file"]).arg(&body.0).output()?;
                anyhow::ensure!(output.status.success(),"PR 创建未确认；提交和推送已保留。重试将先查远端：{}",String::from_utf8_lossy(&output.stderr).trim());
                let url=String::from_utf8_lossy(&output.stdout).trim().to_owned();
                anyhow::ensure!(url.starts_with("https://")&&!url.contains(char::is_whitespace),"PR 创建返回了无法确认的链接；请重试核对远端");
                task.pr_url=Some(url);
            }
        }
        task.phase=Phase::Delivered;
        task.reason=if pr{"PR 已关联，提交版本已核对"}else{"已提交到任务分支"}.into();
        self.save_delivery(index,task)?;
        Ok(json!({"ok":true,"url":task.pr_url,"commit":oid}))
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn pr_body_is_unique_outside_worktree_and_owned_cleanup(){
        let first=PrBody::new("first").unwrap();let second=PrBody::new("second").unwrap();
        assert_ne!(first.0,second.0);assert_eq!(std::fs::read_to_string(&first.0).unwrap(),"first");
        let path=first.0.clone();drop(first);assert!(!path.exists());assert!(second.0.exists());
    }
}
