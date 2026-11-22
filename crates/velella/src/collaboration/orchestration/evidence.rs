//! Versioned agent-provided evidence. File verification proves identity, not that
//! the runtime executed an agent's claimed command. Independent review is separate.
use super::*;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub path: String,
    pub sha256: String,
    pub kind: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub requirement_index: usize,
    pub method: String,
    pub result: String,
    pub detail: String,
    pub artifact_ids: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub requirement_index: usize,
    pub artifact_ids: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub version: u32,
    pub candidate: Value,
    pub checks: Vec<Check>,
    pub deliverables: Vec<Delivery>,
    pub artifacts: Vec<Artifact>,
    pub gaps: Vec<String>,
}

pub fn validate(contract:&Value, value:&Value, candidate:&Value)->Result<Evidence> {
    let evidence:Evidence=serde_json::from_value(value.clone()).context("v2 完成报告必须包含结构化 evidence")?;
    ensure!(evidence.version==1 && evidence.candidate==*candidate && candidate.is_object(),"证据必须绑定当前候选");
    ensure!(evidence.gaps.is_empty(),"存在未完成项，不能报告通过");
    ensure!(!evidence.artifacts.is_empty() && evidence.artifacts.len()<=128,"需要 1–128 个证据文件");
    let mut ids=HashSet::new();
    for a in &evidence.artifacts {
        ensure!(!a.id.is_empty() && a.id.len()<=80 && ids.insert(a.id.as_str()),"证据标识为空或重复");
        ensure!(matches!(a.kind.as_str(),"log"|"report"|"snapshot"),"证据类型无效");
        ensure!(a.sha256.len()==64 && a.sha256.bytes().all(|c|c.is_ascii_hexdigit()),"证据需要 SHA-256 指纹");
        let path=Path::new(&a.path);
        ensure!(a.path.len()<=512 && path.components().all(|c|matches!(c,Component::Normal(_))) && path.starts_with(".velella/evidence"),"证据文件必须位于分配工作树的 .velella/evidence 下");
    }
    let references=|refs:&[String]|!refs.is_empty() && refs.iter().all(|id|ids.contains(id.as_str()));
    let checks=contract["checks"].as_array().context("检查合同缺失")?;
    let mut seen=HashSet::new();
    ensure!(evidence.checks.len()==checks.len(),"必须逐项覆盖全部约定检查");
    for c in &evidence.checks {
        ensure!(c.requirement_index<checks.len() && seen.insert(c.requirement_index),"检查索引越界或重复");
        ensure!(c.result=="passed" && !c.method.trim().is_empty() && !c.detail.trim().is_empty() && c.method.len()<=4096 && c.detail.len()<=16384 && references(&c.artifact_ids),"检查必须实际通过并引用证据；未执行或失败不能验收");
    }
    let deliveries=contract["deliverables"].as_array().context("交付合同缺失")?;
    seen.clear();
    ensure!(evidence.deliverables.len()==deliveries.len(),"必须逐项覆盖全部交付物");
    for d in &evidence.deliverables {
        ensure!(d.requirement_index<deliveries.len() && seen.insert(d.requirement_index) && references(&d.artifact_ids),"交付物必须引用存在的证据文件");
    }
    Ok(evidence)
}

pub fn verify_files(work:&Value, value:&Value)->Result<()> {
    let evidence=validate(&work["evidence_contract"],value,&work["candidate"])?;
    // Keep the OS canonical representation on both sides. On Windows dunce
    // strips the extended prefix only for short paths, so a long child can
    // otherwise appear outside its own shorter root.
    let root=std::fs::canonicalize(field(work,"workspace_path")?)?;
    let mut total=0;
    for a in evidence.artifacts {
        let mut path=root.clone();
        for part in Path::new(&a.path).components() {
            path.push(part);
            ensure!(!std::fs::symlink_metadata(&path)?.file_type().is_symlink(),"证据路径不能包含链接");
        }
        ensure!(std::fs::canonicalize(&path)?.starts_with(&root),"证据路径离开工作树");
        let metadata=std::fs::metadata(&path)?;
        total+=metadata.len();
        ensure!(metadata.is_file() && total<=16*1024*1024,"证据文件必须是普通文件且总大小不超过 16 MiB");
        ensure!(format!("{:x}",Sha256::digest(std::fs::read(&path)?)).eq_ignore_ascii_case(&a.sha256),"证据文件已变化：{}",a.path);
    }
    Ok(())
}

pub fn instructions()->&'static str {
    "v2 完成/审查报告必须带 evidence，CLI 用 --evidence <JSON文件>。结构：{version:1,candidate:<当前candidate原对象>,checks:[{requirement_index:0,method:<实际命令或核验方法>,result:passed,detail:<实际结果>,artifact_ids:[log]}],deliverables:[{requirement_index:0,artifact_ids:[log]}],artifacts:[{id:log,path:.velella/evidence/<唯一名称>.log,sha256:<64位十六进制>,kind:log|report|snapshot}],gaps:[]}。索引按 evidence_contract 的 checks/deliverables 从0开始，每项恰好覆盖一次。每份证据文件必须真实存在于当前分配工作树 .velella/evidence 下。独立审查须产生自己的核验记录。没执行、检查失败或仍有缺口应回报 failed；不得填写 passed。日志证明由 Agent 提供，系统只核对合同、候选和文件指纹。"
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values(root:&Path,relative:&str,content:&[u8])->(Value,Value){
        let candidate=json!({"revision":"fixture"});
        (json!({"workspace_path":root,"candidate":candidate,"evidence_contract":{"checks":[],"deliverables":[]}}),
         json!({"version":1,"candidate":candidate,"checks":[],"deliverables":[],"artifacts":[{"id":"log","path":relative,"sha256":format!("{:x}",Sha256::digest(content)),"kind":"log"}],"gaps":[]}))
    }
    #[test]
    fn long_evidence_path_remains_inside_its_short_workspace(){
        let root=std::env::temp_dir().join(format!("velella-evidence-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".velella/evidence")).unwrap();
        let canonical=std::fs::canonicalize(&root).unwrap();
        let relative=format!(".velella/evidence/{}.log","e".repeat(190));
        let file=canonical.join(&relative);
        assert!(file.to_string_lossy().len()>260);
        std::fs::write(&file,b"actual verification log").unwrap();
        let (work,evidence)=values(&root,&relative,b"actual verification log");
        let outcome=verify_files(&work,&evidence);
        std::fs::write(&file,b"changed after report").unwrap();
        assert!(verify_files(&work,&evidence).is_err(),"long paths must still check actual bytes");
        std::fs::remove_dir_all(&root).unwrap();
        outcome.unwrap();
    }
    #[test]
    fn evidence_directory_link_cannot_escape_to_a_sibling_workspace(){
        let temp=std::env::temp_dir().join(format!("velella-evidence-{}",uuid::Uuid::new_v4()));
        let root=temp.join("workspace");let outside=temp.join("workspace-other");
        std::fs::create_dir_all(root.join(".velella")).unwrap();std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("log.txt"),b"outside").unwrap();
        let link=root.join(".velella/evidence");
        #[cfg(windows)]
        {let output=std::process::Command::new("cmd").args(["/c","mklink","/J"]).arg(link.to_string_lossy().replace('/',"\\")).arg(&outside).output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));}
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside,&link).unwrap();
        let (work,evidence)=values(&root,".velella/evidence/log.txt",b"outside");
        let outcome=verify_files(&work,&evidence);
        #[cfg(windows)] std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)] std::fs::remove_file(&link).unwrap();
        std::fs::remove_dir_all(&temp).unwrap();
        assert!(outcome.is_err(),"sibling workspace is outside the assigned root");
    }
}
