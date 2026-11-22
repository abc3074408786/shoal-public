//! Fixed, system-owned tool aliases for Restricted processes.
//!
//! Discover only named package locations, never the host PATH. Aliases live
//! beside the private tool HOME and receive a separate read-only Landlock rule;
//! the source binaries already reside under the existing system read rules.
use anyhow::{Context, Result};
use std::{
    fs, io,
    os::unix::fs::{symlink, DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

const SYSTEM_ROOTS: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64"];
const TOOLS: &[(&str, &[&str])] = &[
    (
        "rg",
        &[
            "/usr/local/bin/rg",
            "/usr/bin/rg",
            "/bin/rg",
            "/usr/lib/chatgpt/resources/rg",
        ],
    ),
    (
        "node",
        &[
            "/usr/local/bin/node",
            "/usr/bin/node",
            "/bin/node",
            "/usr/lib/chatgpt/resources/cua_node/bin/node",
        ],
    ),
    ("git", &["/usr/local/bin/git", "/usr/bin/git", "/bin/git"]),
    (
        "python3",
        &["/usr/local/bin/python3", "/usr/bin/python3", "/bin/python3"],
    ),
];

fn system_path(path: &Path) -> bool {
    SYSTEM_ROOTS.iter().any(|root| path.starts_with(root))
}

/// Both the binary and its ancestry must remain root-owned and not writable by
/// ordinary users. A user-controlled PATH or symlink cannot add a tool source.
fn trusted_executable(candidate: &Path) -> Option<PathBuf> {
    if !candidate.is_absolute() || !system_path(candidate) {
        return None;
    }
    let canonical = candidate.canonicalize().ok()?;
    if !system_path(&canonical) {
        return None;
    }
    let binary = fs::metadata(&canonical).ok()?;
    if !binary.is_file()
        || binary.uid() != 0
        || binary.mode() & 0o022 != 0
        || binary.mode() & 0o111 == 0
    {
        return None;
    }
    for directory in canonical.ancestors().skip(1) {
        let metadata = fs::metadata(directory).ok()?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return None;
        }
    }
    Some(canonical)
}

fn discover() -> Vec<(&'static str, PathBuf)> {
    TOOLS
        .iter()
        .filter_map(|(name, candidates)| {
            candidates
                .iter()
                .find_map(|candidate| trusted_executable(Path::new(candidate)))
                .map(|source| (*name, source))
        })
        .collect()
}

fn directory(home: &Path) -> Result<PathBuf> {
    let name = home
        .file_name()
        .and_then(|name| name.to_str())
        .context("tool HOME name must be UTF-8")?;
    Ok(home.with_file_name(format!("{name}.velella-tools-v1")))
}

fn materialize(
    home: &Path,
    writable: &[&Path],
    tools: &[(&str, PathBuf)],
) -> Result<Option<PathBuf>> {
    if tools.is_empty() {
        return Ok(None);
    }
    let directory = directory(home)?;
    anyhow::ensure!(
        !writable
            .iter()
            .any(|root| directory.starts_with(root) || root.starts_with(&directory)),
        "trusted tool directory must stay outside every writable root"
    );
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).context("create private trusted tool directory"),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    anyhow::ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::getuid() }
            && metadata.mode() & 0o777 == 0o700
            && directory.canonicalize()? == directory,
        "trusted tool directory must be an owned private canonical directory"
    );
    // Never repair or execute a substituted entry. In particular, do not
    // overwrite a regular file or follow an unexpected directory symlink.
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let expected = tools
            .iter()
            .find(|(tool, _)| name == *tool)
            .context("unexpected entry in trusted tool directory")?;
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::getuid() }
                && fs::read_link(entry.path())? == expected.1,
            "trusted tool alias was replaced"
        );
    }
    for (name, source) in tools {
        let alias = directory.join(name);
        match symlink(source, &alias) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("create trusted tool alias"),
        }
        let metadata = fs::symlink_metadata(&alias)?;
        anyhow::ensure!(
            metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::getuid() }
                && fs::read_link(&alias)? == *source,
            "trusted tool alias changed during preparation"
        );
    }
    Ok(Some(directory))
}

/// Called before applying Landlock. The caller grants READ only to the returned
/// directory and reconstructs PATH; the aliases do not alter global installs.
pub(super) fn prepare(home: &Path, writable: &[&Path]) -> Result<Option<PathBuf>> {
    materialize(home, writable, &discover())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("vl-trusted-tools-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("home")).unwrap();
            fs::create_dir(path.join("work")).unwrap();
            Self(path)
        }
        fn home(&self) -> PathBuf {
            self.0.join("home")
        }
        fn source() -> PathBuf {
            trusted_executable(Path::new("/usr/bin/true")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn restricted_tools_reject_user_sources_even_when_executable() {
        let fixture = Fixture::new();
        let source = fixture.0.join("rg");
        fs::write(&source, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(trusted_executable(&source).is_none());
        fs::remove_file(&source).unwrap();
        symlink(Fixture::source(), &source).unwrap();
        assert!(trusted_executable(&source).is_none());
    }

    #[test]
    fn restricted_tools_aliases_are_exact_and_idempotent() {
        let fixture = Fixture::new();
        let home = fixture.home();
        let tools = [("rg", Fixture::source())];
        let directory = materialize(&home, &[&home, &fixture.0.join("work")], &tools)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read_link(directory.join("rg")).unwrap(), tools[0].1);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        assert_eq!(
            materialize(&home, &[&home], &tools).unwrap(),
            Some(directory)
        );
    }

    #[test]
    fn restricted_tools_reject_writable_overlap_and_directory_symlink() {
        let fixture = Fixture::new();
        let home = fixture.home();
        let tools = [("rg", Fixture::source())];
        assert!(materialize(&home, &[&fixture.0], &tools).is_err());
        assert!(!directory(&home).unwrap().exists());
        symlink(fixture.0.join("work"), directory(&home).unwrap()).unwrap();
        assert!(materialize(&home, &[&home], &tools).is_err());
        assert_eq!(fs::read_dir(fixture.0.join("work")).unwrap().count(), 0);
    }

    #[test]
    fn restricted_tools_do_not_repair_substituted_aliases_or_extra_files() {
        let fixture = Fixture::new();
        let home = fixture.home();
        let tools = [("rg", Fixture::source())];
        let directory = materialize(&home, &[&home], &tools).unwrap().unwrap();
        fs::remove_file(directory.join("rg")).unwrap();
        fs::write(directory.join("rg"), b"must not execute").unwrap();
        assert!(materialize(&home, &[&home], &tools).is_err());
        assert_eq!(fs::read(directory.join("rg")).unwrap(), b"must not execute");
        fs::remove_file(directory.join("rg")).unwrap();
        symlink(&tools[0].1, directory.join("rg")).unwrap();
        fs::write(directory.join("unexpected"), b"must remain unchanged").unwrap();
        assert!(materialize(&home, &[&home], &tools).is_err());
    }
}
