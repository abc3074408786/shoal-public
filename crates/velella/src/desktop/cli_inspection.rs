//! Locate the public CLI without executing it or changing the user's PATH.
use serde_json::{json, Value};
use std::path::PathBuf;

fn locate(paths: impl IntoIterator<Item = PathBuf>, fallback: Option<PathBuf>) -> Value {
    let name = if cfg!(windows) { "velella-cli.exe" } else { "velella-cli" };
    // Ignore relative/empty PATH entries: the daemon's working directory is not
    // the terminal's, and must not produce a misleading installed status.
    for directory in paths {
        if directory.is_absolute() {
            let path = directory.join(name);
            if path.is_file() {
                return json!({"ok":true,"found":true,"path":path,"source":"path"});
            }
        }
    }
    if let Some(path) = fallback.filter(|path| path.is_absolute() && path.is_file()) {
        return json!({"ok":true,"found":true,"path":path,"source":"installation"});
    }
    json!({"ok":true,"found":false,"path":null,"source":null})
}

pub(super) fn inspect() -> Value {
    let paths = std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect::<Vec<_>>()).unwrap_or_default();
    let fallback = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("Programs").join("Velella CLI").join("velella-cli.exe"))
    } else { None };
    locate(paths, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_path_from_standard_installation_without_executing() {
        let root = std::env::temp_dir().join(format!("velella-cli-inspect-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let name = if cfg!(windows) { "velella-cli.exe" } else { "velella-cli" };
        let file = root.join(name);
        // Deliberately not a valid program: inspection must never execute it.
        std::fs::write(&file, b"inspection fixture").unwrap();
        assert_eq!(locate([root.clone()], Some(file.clone()))["source"], "path");
        assert_eq!(locate([], Some(file.clone()))["source"], "installation");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(locate([root.clone()], Some(file))["found"], false);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn ignores_relative_paths_and_directories() {
        assert_eq!(locate([PathBuf::from("."), PathBuf::new()], Some(PathBuf::from(".")))["found"], false);
        assert_eq!(locate([], Some(std::env::temp_dir()))["found"], false);
    }
}
