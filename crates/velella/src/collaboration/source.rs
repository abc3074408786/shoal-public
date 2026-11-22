//! Content identity for a review candidate, including dirty and untracked source.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

fn excluded(path:&Path)->bool {
    path.components().any(|c| matches!(c,Component::Normal(name) if name==".git" || name==".velella"))
}

fn walk(root:&Path,current:&Path,paths:&mut Vec<PathBuf>)->Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry=entry?;
        let path=entry.path();
        if excluded(path.strip_prefix(root)?) {continue;}
        let metadata=std::fs::symlink_metadata(&path)?;
        anyhow::ensure!(!metadata.file_type().is_symlink(),"candidate does not follow symlinks: {}",path.display());
        if metadata.is_dir(){walk(root,&path,paths)?;}else{paths.push(path);}
    }
    Ok(())
}

pub(crate) fn candidate(workspace:&Path)->Result<Value> {
    let root=dunce::canonicalize(workspace).context("candidate workspace unavailable")?;
    let probe=crate::process::command("git").current_dir(&root).args(["rev-parse","--is-inside-work-tree"]).output();
    let is_git=probe.is_ok_and(|r|r.status.success()&&String::from_utf8_lossy(&r.stdout).trim()=="true");
    let mut paths=Vec::new();
    let head=if is_git {
        let listed=crate::process::command("git").current_dir(&root).args(["ls-files","-z","--cached","--others","--exclude-standard"]).output()?;
        anyhow::ensure!(listed.status.success(),"cannot enumerate candidate source");
        for relative in String::from_utf8(listed.stdout)?.split('\0').filter(|s|!s.is_empty()) {
            let path=Path::new(relative);
            anyhow::ensure!(!path.is_absolute()&&path.components().all(|c|matches!(c,Component::Normal(_))),"invalid candidate path");
            if !excluded(path){paths.push(root.join(path));}
        }
        let output=crate::process::command("git").current_dir(&root).args(["rev-parse","--verify","HEAD"]).output()?;
        if output.status.success(){Some(String::from_utf8(output.stdout)?.trim().to_owned())}else{None}
    }else{walk(&root,&root,&mut paths)?;None};
    paths.sort();paths.dedup();
    let mut manifest=Vec::new();
    for path in paths {
        let relative=path.strip_prefix(&root)?.to_string_lossy().replace('\\',"/");
        let metadata=match std::fs::symlink_metadata(&path){
            Ok(m)=>m,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{manifest.push(json!({"path":relative,"deleted":true}));continue;}
            Err(e)=>return Err(e.into()),
        };
        anyhow::ensure!(metadata.is_file()&&!metadata.file_type().is_symlink(),"candidate source must be a regular file: {}",path.display());
        anyhow::ensure!(dunce::canonicalize(&path)?.starts_with(&root),"candidate source leaves workspace");
        let content=std::fs::read(&path)?;
        manifest.push(json!({"path":relative,"sha256":format!("{:x}",Sha256::digest(&content)),"bytes":content.len()}));
    }
    let digest=format!("sha256:{:x}",Sha256::digest(serde_json::to_vec(&manifest)?));
    let mut path=root.to_string_lossy().into_owned();
    if cfg!(windows){path=path.replace('/',"\\").to_lowercase();}
    Ok(json!({"workspace_path":path,"revision":digest,"git_head":head}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_tracks_content_but_not_runtime_metadata() {
        let root=std::env::temp_dir().join(format!("velella-candidate-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join(".velella")).unwrap();
        std::fs::write(root.join("code.txt"),"before").unwrap();
        let first=candidate(&root).unwrap();
        std::fs::write(root.join(".velella/session.json"),"runtime").unwrap();
        assert_eq!(first,candidate(&root).unwrap());
        std::fs::write(root.join("code.txt"),"after").unwrap();
        assert_ne!(first,candidate(&root).unwrap());
        assert!(!root.join(".git").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
