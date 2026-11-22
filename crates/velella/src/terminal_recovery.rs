//! Recovery is a capability of one private terminal state directory. A stopped
//! host alone is insufficient: its bridge and both executors must have finished.
use anyhow::{Context, Result};
use crate::host::Descriptor;
use std::path::Path;

pub(crate) fn thread(desc: &Descriptor) -> Result<String> {
    #[cfg(target_os = "linux")]
    { linux::thread(desc) }
    #[cfg(not(target_os = "linux"))]
    { let _ = desc; anyhow::bail!("受限终端恢复目前仅支持 Linux") }
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{read_private, write_atomic, stopped};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use serde_json::{json, Value};
    use std::{
        fs::{self, OpenOptions},
        io::{Read, Write},
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };

    const LIMIT: u64 = 1024 * 1024;

    pub(crate) fn read_private(path: &Path) -> Result<Vec<u8>> {
        let file = OpenOptions::new().read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)?;
        let metadata = file.metadata()?;
        anyhow::ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.nlink() == 1 && metadata.mode() & 0o777 == 0o600
            && metadata.len() <= LIMIT, "恢复资料必须为当前用户的私有普通文件");
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 <= LIMIT, "恢复资料超过大小限制");
        Ok(bytes)
    }

    pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
        anyhow::ensure!(bytes.len() as u64 <= LIMIT, "恢复资料超过大小限制");
        match fs::symlink_metadata(path) {
            Ok(_) => { read_private(path)?; }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let parent = path.parent().context("private state parent missing")?;
        let metadata = fs::symlink_metadata(parent)?;
        anyhow::ensure!(metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o700 && parent.canonicalize()? == parent,
            "恢复资料目录必须由当前用户独占");
        let temporary = parent.join(format!(".recovery-{}", uuid::Uuid::now_v7()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result
    }

    pub(crate) fn stopped(directory: &Path, generation: &str) -> Result<()> {
        write_atomic(&directory.join("terminal-stopped.json"),
            &serde_json::to_vec(&json!({"schema":1, "generation":generation}))?)
    }

    pub(super) fn thread(desc: &Descriptor) -> Result<String> {
        anyhow::ensure!(desc.spawn.adapter.as_deref() == Some(crate::terminal_sandbox::ADAPTER)
            && desc.spawn.sandbox.is_restricted(), "此会话不支持受限终端恢复");
        crate::restricted_controller::validate_launch(desc)?;
        let receipt: Value = serde_json::from_slice(&read_private(&desc.directory.join("terminal-bridge.json"))?)?;
        let stopped: Value = serde_json::from_slice(&read_private(&desc.directory.join("terminal-stopped.json"))?)
            .context("上次终端未完成正常停止，暂不能恢复")?;
        let generation = receipt["generation"].as_str().context("旧版终端没有恢复记录，请新建终端")?;
        anyhow::ensure!(uuid::Uuid::parse_str(generation).is_ok()
            && stopped["schema"] == 1 && stopped["generation"] == generation,
            "上次终端尚未完成进程清理");
        let checkpoint: Value = serde_json::from_slice(&read_private(&desc.directory.join("terminal-resume.json"))?)?;
        anyhow::ensure!(checkpoint["schema"] == 1
            && checkpoint["cli_version"] == "0.153.4"
            && checkpoint["generation"] == generation
            && checkpoint["codex_home"].as_str()
                == desc.directory.join("controller/codex").to_str()
            && checkpoint["work"].as_str() == desc.spawn.cwd.as_deref()
            && checkpoint["history_ready"] == true
            && checkpoint["shell_history_dirty"] == false,
            "会话恢复记录与当前工作区不匹配或历史尚未保存");
        let thread = checkpoint["thread_id"].as_str().context("会话尚未建立，无法恢复")?;
        anyhow::ensure!(uuid::Uuid::parse_str(thread)?.to_string() == thread, "会话恢复标识无效");
        Ok(thread.to_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};

        #[test]
        fn recovery_requires_matching_completed_generation_and_original_worktree() {
            let root = std::env::temp_dir().join(format!("vl-resume-state-{}", uuid::Uuid::now_v7()));
            for directory in [&root, &root.join("work"), &root.join("state"), &root.join("state/tool-home")] {
                fs::create_dir(directory).unwrap();
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
            }
            let directory = root.join("state");
            let generation = uuid::Uuid::now_v7().to_string();
            let thread_id = uuid::Uuid::now_v7().to_string();
            let desc = Descriptor {
                id: uuid::Uuid::now_v7().to_string(), endpoint: "unused".into(), directory: directory.clone(),
                spawn: crate::sessions::SpawnRequest {
                    program: "codex".into(), args: vec![], env: Default::default(),
                    cwd: Some(root.join("work").to_string_lossy().into()),
                    adapter: Some(crate::terminal_sandbox::ADAPTER.into()),
                    sandbox: crate::sandbox::Policy::Restricted {
                        read_only: vec![], writable: vec![], network: crate::sandbox::Network::Denied,
                        home: Some(directory.join("tool-home")),
                    },
                },
            };
            write_atomic(&directory.join("terminal-bridge.json"),
                &serde_json::to_vec(&json!({"generation":generation})).unwrap()).unwrap();
            let mut checkpoint = json!({"schema":1,"thread_id":thread_id,"work":desc.spawn.cwd,
                "codex_home":directory.join("controller/codex"),"cli_version":"0.153.4",
                "generation":generation,"shell_history_dirty":false,"history_ready":true});
            let path = directory.join("terminal-resume.json");
            write_atomic(&path, &serde_json::to_vec(&checkpoint).unwrap()).unwrap();
            assert!(thread(&desc).is_err(), "host disappearance is not a cleanup receipt");
            stopped(&directory, &generation).unwrap();
            assert_eq!(thread(&desc).unwrap(), thread_id);
            for (key, bad) in [
                ("work", json!(root)), ("generation", json!(uuid::Uuid::now_v7().to_string())),
                ("shell_history_dirty", json!(true)), ("thread_id", json!("../other")),
                ("history_ready", json!(false)),
                ("cli_version", json!("unverified")), ("codex_home", json!("/tmp/other")),
            ] {
                let previous = checkpoint[key].clone();
                checkpoint[key] = bad;
                write_atomic(&path, &serde_json::to_vec(&checkpoint).unwrap()).unwrap();
                assert!(thread(&desc).is_err(), "{key} override accepted");
                checkpoint[key] = previous;
            }
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn atomic_state_rejects_links_and_non_private_files() {
            let root = std::env::temp_dir().join(format!("vl-recovery-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let file = root.join("state.json");
            write_atomic(&file, b"first").unwrap();
            write_atomic(&file, b"second").unwrap();
            assert_eq!(read_private(&file).unwrap(), b"second");
            let alias = root.join("alias");
            symlink(&file, &alias).unwrap();
            assert!(read_private(&alias).is_err());
            assert!(write_atomic(&alias, b"changed").is_err());
            fs::remove_file(&alias).unwrap();
            fs::hard_link(&file, &alias).unwrap();
            assert!(write_atomic(&file, b"changed").is_err());
            fs::remove_file(alias).unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_private(&file).is_err());
            assert!(write_atomic(&file, b"changed").is_err());
            assert_eq!(fs::read(file).unwrap(), b"second");
            fs::remove_dir_all(root).unwrap();
        }
    }
}
