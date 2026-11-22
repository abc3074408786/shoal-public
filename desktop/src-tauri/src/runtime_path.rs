//! A stable desktop entry can use a versioned runtime while old PTY hosts stay alive.
use std::path::{Component,Path,PathBuf};

pub const RUNTIME_NAME: &str = if cfg!(windows) { "velella-runtime.exe" } else { "velella-runtime" };

pub fn resolve(install: &Path, explicit: Option<PathBuf>, development: &Path) -> Result<PathBuf,String> {
    if let Some(path)=explicit {return Ok(path);}
    let manifest=install.join("active-runtime.json");
    if manifest.exists() {
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&manifest).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        let relative=Path::new(value["runtime"].as_str().ok_or("active runtime path missing")?);
        if relative.is_absolute() || !relative.components().all(|p|matches!(p,Component::Normal(_))) || relative.file_name().is_none_or(|n|n!=RUNTIME_NAME) {
            return Err(format!("active runtime must be an installation-relative {RUNTIME_NAME}"));
        }
        let root=std::fs::canonicalize(install).map_err(|e|e.to_string())?;
        let target=std::fs::canonicalize(root.join(relative)).map_err(|e|e.to_string())?;
        if !target.starts_with(&root) || !target.is_file() {return Err("active runtime leaves installation".into());}
        return Ok(target);
    }
    let installed=install.join(RUNTIME_NAME);
    Ok(if installed.is_file(){installed}else{development.to_path_buf()})
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn missing_install_uses_development_runtime(){
        let root=std::env::temp_dir().join(format!("velella-missing-runtime-{}",std::process::id()));
        let development=root.join("debug").join(RUNTIME_NAME);
        assert_eq!(resolve(&root,None,&development).unwrap(),development);
    }

    #[cfg(unix)]
    #[test]fn versioned_symlink_cannot_escape_installation(){
        let root=std::env::temp_dir().join(format!("velella-runtime-symlink-{}",std::process::id()));
        let install=root.join("install");
        std::fs::create_dir_all(install.join("versions")).unwrap();
        let outside=root.join(RUNTIME_NAME);
        std::fs::write(&outside,b"outside").unwrap();
        std::os::unix::fs::symlink(&outside,install.join("versions").join(RUNTIME_NAME)).unwrap();
        std::fs::write(install.join("active-runtime.json"),serde_json::json!({"runtime":format!("versions/{RUNTIME_NAME}")}).to_string()).unwrap();
        assert!(resolve(&install,None,Path::new("dev")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn versioned_runtime_fails_closed_and_legacy_stays_compatible(){
        let root=std::env::temp_dir().join(format!("velella-runtime-path-{}",std::process::id()));
        std::fs::create_dir_all(root.join("versions/test")).unwrap();
        let legacy=root.join(RUNTIME_NAME);std::fs::write(&legacy,b"old").unwrap();
        let versioned=root.join("versions/test").join(RUNTIME_NAME);std::fs::write(&versioned,b"new").unwrap();
        assert_eq!(resolve(&root,None,Path::new("dev")).unwrap(),legacy);
        let manifest=root.join("active-runtime.json");
        std::fs::write(&manifest,serde_json::json!({"runtime":format!("versions/test/{RUNTIME_NAME}")}).to_string()).unwrap();
        assert_eq!(resolve(&root,None,Path::new("dev")).unwrap(),std::fs::canonicalize(versioned).unwrap());
        for path in [format!("../{RUNTIME_NAME}"),format!("C:/elsewhere/{RUNTIME_NAME}"),format!("versions/missing/{RUNTIME_NAME}"),"versions/test/not-a-runtime".into()] {
            std::fs::write(&manifest,serde_json::json!({"runtime":path}).to_string()).unwrap();
            assert!(resolve(&root,None,Path::new("dev")).is_err());
        }
        assert_eq!(resolve(&root,Some(legacy.clone()),Path::new("dev")).unwrap(),legacy);
        std::fs::remove_dir_all(root).unwrap();
    }
}
