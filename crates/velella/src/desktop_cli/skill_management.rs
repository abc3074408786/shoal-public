//! Local native skill packages. No daemon database writes or script execution.
use super::*;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::Path};
mod center;

#[derive(Clone, Copy, clap::ValueEnum)]
pub(super) enum SkillTarget { Agents, Codex }
impl SkillTarget {
    fn directory(self) -> &'static str { match self { Self::Agents => ".agents", Self::Codex => ".codex" } }
}

#[derive(Subcommand)]
pub(super) enum SkillsOperation {
    /// Skill Center JSON API; local-only, uses the same package checks as native management
    Center { #[arg(long)] library: PathBuf, #[arg(long)] file: PathBuf },
    /// Inspect a local package without executing it or contacting the daemon
    Preview { source: PathBuf },
    /// Inspect an exact local installed package before removal
    Inspect {
        name: String,
        #[arg(long, value_enum, default_value = "agents")]
        target: SkillTarget,
    },
    /// Install a reviewed package into a new name; never overwrite
    Install {
        source: PathBuf,
        #[arg(long)] name: String,
        #[arg(long, value_enum, default_value = "agents")] target: SkillTarget,
        #[arg(long)] digest: String,
        #[arg(long, required = true)] confirm: bool,
    },
    /// Move a reviewed package into a recoverable backup
    Remove {
        name: String,
        #[arg(long, value_enum, default_value = "agents")] target: SkillTarget,
        #[arg(long)] digest: String,
        #[arg(long, required = true)] confirm: bool,
    },
    /// List removal receipts in this user's local native skill store
    Backups {
        #[arg(long, value_enum, default_value = "agents")] target: SkillTarget,
    },
    /// Restore a backup only if the original name is free
    Restore {
        id: uuid::Uuid,
        #[arg(long, value_enum, default_value = "agents")] target: SkillTarget,
        #[arg(long)] digest: String,
        #[arg(long, required = true)] confirm: bool,
    },
}

struct Entry { relative: PathBuf, bytes: Vec<u8>, permissions: fs::Permissions }
struct Package { root: PathBuf, files: Vec<Entry>, digest: String, bytes: usize }

// Junctions and other Windows reparse points are links too, even when Rust's
// is_symlink() is false. Check ancestors before canonicalizing, including sources.
fn plain(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt;
      anyhow::ensure!(meta.file_attributes() & 0x400 == 0, "reparse point rejected: {}", path.display()); }
    anyhow::ensure!(!meta.file_type().is_symlink(), "link rejected: {}", path.display());
    Ok(())
}
fn ancestors(path: &Path) -> Result<()> {
    anyhow::ensure!(path.is_absolute(), "absolute path required");
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(_) => plain(part)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn absolute(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    ancestors(&path)?;
    Ok(dunce::canonicalize(path)?)
}
fn package(source: &Path) -> Result<Package> {
    let root = absolute(source)?;
    anyhow::ensure!(root.is_dir(), "source must be a package directory containing SKILL.md");
    let skill = root.join("SKILL.md");
    plain(&skill)?;
    anyhow::ensure!(fs::metadata(&skill)?.is_file() && fs::metadata(&skill)?.len() <= 256*1024,
        "SKILL.md must be a regular file at most 256 KiB");
    fn visit(root: &Path, path: &Path, files: &mut Vec<Entry>, bytes: &mut usize, visited: &mut usize, depth: usize) -> Result<()> {
        *visited += 1;
        anyhow::ensure!(*visited <= 4096 && depth <= 64, "package directory bounds exceeded");
        plain(path)?;
        let meta = fs::symlink_metadata(path)?;
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                if entry.file_name() != ".git" { visit(root, &entry.path(), files, bytes, visited, depth+1)?; }
            }
        } else {
            anyhow::ensure!(meta.is_file() && files.len() < 2048, "package must contain at most 2048 regular files");
            let mut data = Vec::new();
            fs::File::open(path)?.take((32*1024*1024 - *bytes + 1) as u64).read_to_end(&mut data)?;
            *bytes += data.len();
            anyhow::ensure!(*bytes <= 32*1024*1024, "package exceeds 32 MiB");
            files.push(Entry { relative: path.strip_prefix(root)?.to_owned(), bytes: data, permissions: meta.permissions() });
        }
        Ok(())
    }
    let (mut files, mut bytes, mut visited) = (Vec::new(), 0, 0);
    visit(&root, &root, &mut files, &mut bytes, &mut visited, 0)?;
    files.sort_by(|a,b| a.relative.cmp(&b.relative));
    let manifest = files.iter().map(|entry| json!({"path":entry.relative.to_string_lossy().replace('\\',"/"),
        "sha256":format!("{:x}",Sha256::digest(&entry.bytes))})).collect::<Vec<_>>();
    let digest = format!("sha256:{:x}",Sha256::digest(serde_json::to_vec(&manifest)?));
    Ok(Package { root, files, digest, bytes })
}
fn receipt(package: &Package) -> Value {
    json!({"path":package.root,"digest":package.digest,"files":package.files.len(),"bytes":package.bytes,
        "content_is_untrusted":true,"scripts_executed":false,"scope":"local-native-skills"})
}
fn check_name(name: &str) -> Result<()> {
    anyhow::ensure!(!name.is_empty() && name.len() <= 64 && name.as_bytes()[0].is_ascii_lowercase()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
        "name must start with a-z and contain only a-z, 0-9, - or _ (max 64)");
    let reserved = ["con","prn","aux","nul","com1","com2","com3","com4","com5","com6","com7","com8","com9",
        "lpt1","lpt2","lpt3","lpt4","lpt5","lpt6","lpt7","lpt8","lpt9"];
    anyhow::ensure!(!reserved.contains(&name), "reserved native skill name");
    Ok(())
}
fn check_digest(package: &Package, digest: &str) -> Result<()> {
    anyhow::ensure!(package.digest == digest, "package changed; preview/inspect again before confirming"); Ok(())
}
struct Store { base: PathBuf, skills: PathBuf, backups: PathBuf }
impl Store {
    fn new(home: &Path, target: SkillTarget) -> Result<Self> {
        let home = absolute(home)?;
        let base = home.join(target.directory());
        ancestors(&base)?;
        Ok(Self { skills: base.join("skills"), backups: base.join("velella-skill-backups"), base })
    }
    fn lock(&self) -> Result<fs::File> {
        ancestors(&self.base)?;
        fs::create_dir_all(&self.base)?;
        let path = self.base.join("velella-skills.lock");
        ancestors(&path)?;
        let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
        lock.try_lock_exclusive().context("another Velella skill operation is in progress; no automatic retry")?;
        Ok(lock)
    }
    fn destination(&self, name: &str) -> Result<PathBuf> {
        check_name(name)?;
        let path = self.skills.join(name);
        ancestors(&path)?;
        Ok(path)
    }
    fn vacant(path: &Path) -> Result<()> {
        anyhow::ensure!(fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "destination already exists or is inaccessible; refusing to overwrite: {}",path.display()); Ok(())
    }
    fn backup_path(&self, id: uuid::Uuid) -> Result<PathBuf> {
        let path = self.backups.join(id.to_string()); ancestors(&path)?; Ok(path)
    }
    fn read_receipt(&self, id: uuid::Uuid) -> Result<Value> {
        let path = self.backup_path(id)?.join("receipt.json");
        ancestors(&path)?;
        anyhow::ensure!(fs::metadata(&path)?.is_file(),"backup receipt must be a regular file");
        let mut bytes = Vec::new(); fs::File::open(path)?.take(8193).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 8192,"backup receipt exceeds bounds");
        let value: Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(value["version"] == 1 && value["id"] == id.to_string(), "invalid backup receipt");
        check_name(value["name"].as_str().context("backup name missing")?)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("velella-skill-test-{}",uuid::Uuid::now_v7()));
            fs::create_dir(&path).unwrap(); Self(path)
        }
        fn source(&self) -> PathBuf {
            let source = self.0.join("source"); fs::create_dir(&source).unwrap();
            fs::write(source.join("SKILL.md"),b"---\r\nname: sample\r\ndescription: fixture\r\n---\r\n").unwrap();
            fs::create_dir(source.join("scripts")).unwrap();
            fs::write(source.join("scripts/data.bin"),[0,255,128,10]).unwrap(); source
        }
        fn run(&self, command: SkillsOperation) -> Result<Value> { execute_at(&self.0,&command) }
        fn install(&self, source: &Path, digest: &str) -> Result<Value> {
            self.run(SkillsOperation::Install {source:source.to_owned(),name:"sample".into(),target:SkillTarget::Agents,digest:digest.into(),confirm:true})
        }
        fn remove(&self, digest: &str) -> Result<Value> {
            self.run(SkillsOperation::Remove {name:"sample".into(),target:SkillTarget::Agents,digest:digest.into(),confirm:true})
        }
        fn restore(&self, id: uuid::Uuid, digest: &str) -> Result<Value> {
            self.run(SkillsOperation::Restore {id,target:SkillTarget::Agents,digest:digest.into(),confirm:true})
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn package_lifecycle_preserves_native_digest_and_resources() {
        let fixture=Fixture::new(); let source=fixture.source();
        let digest=package(&source).unwrap().digest;
        assert_eq!(digest,crate::collaboration::providers::skill_digest(&source).unwrap());
        let result=fixture.install(&source,&digest).unwrap();
        assert_eq!(result["installed"]["digest"],digest);
        let installed=fixture.0.join(".agents/skills/sample");
        assert_eq!(fs::read(installed.join("scripts/data.bin")).unwrap(),[0,255,128,10]);
        let removed=fixture.remove(&digest).unwrap();
        assert!(!installed.exists());
        let id=uuid::Uuid::parse_str(removed["backup"]["id"].as_str().unwrap()).unwrap();
        let listed=fixture.run(SkillsOperation::Backups{target:SkillTarget::Agents}).unwrap();
        assert_eq!(listed["backups"][0]["package_present"],true);
        fixture.restore(id,&digest).unwrap();
        assert_eq!(package(&installed).unwrap().digest,digest);
        assert!(fixture.restore(id,&digest).is_err());
    }

    #[test]
    fn stale_digest_and_collisions_leave_installed_package_untouched() {
        let fixture=Fixture::new(); let source=fixture.source(); let digest=package(&source).unwrap().digest;
        fs::write(source.join("SKILL.md"),b"changed").unwrap();
        assert!(fixture.install(&source,&digest).is_err());
        assert!(!fixture.0.join(".agents/skills/sample").exists());
        let digest=package(&source).unwrap().digest; fixture.install(&source,&digest).unwrap();
        assert!(fixture.install(&source,&digest).is_err());
        assert!(fixture.remove("sha256:stale").is_err());
        assert_eq!(package(&fixture.0.join(".agents/skills/sample")).unwrap().digest,digest);
    }

    #[test]
    fn restore_checks_content_and_never_overwrites_a_new_install() {
        let fixture=Fixture::new(); let source=fixture.source(); let digest=package(&source).unwrap().digest;
        fixture.install(&source,&digest).unwrap(); let removed=fixture.remove(&digest).unwrap();
        let id=uuid::Uuid::parse_str(removed["backup"]["id"].as_str().unwrap()).unwrap();
        fixture.install(&source,&digest).unwrap(); assert!(fixture.restore(id,&digest).is_err());
        fixture.remove(&digest).unwrap();
        let backup=Path::new(removed["backup_path"].as_str().unwrap()).join("package");
        fs::write(backup.join("SKILL.md"),b"tampered").unwrap();
        assert!(fixture.restore(id,&digest).is_err());
        assert!(backup.is_dir()); assert!(!fixture.0.join(".agents/skills/sample").exists());
    }

    #[test]
    fn names_receipts_and_store_locks_bound_mutations() {
        let fixture=Fixture::new(); let source=fixture.source(); let digest=package(&source).unwrap().digest;
        for name in ["../escape","sample/child","nul","lpt1","UPPER",""] { assert!(check_name(name).is_err(),"{name}"); }
        let store=Store::new(&fixture.0,SkillTarget::Agents).unwrap();
        let lock=store.lock().unwrap(); assert!(fixture.install(&source,&digest).is_err()); drop(lock);
        fixture.install(&source,&digest).unwrap(); let removed=fixture.remove(&digest).unwrap();
        let id=uuid::Uuid::parse_str(removed["backup"]["id"].as_str().unwrap()).unwrap();
        let mut value=removed["backup"].clone(); value["name"]=json!("../escape");
        fs::write(store.backup_path(id).unwrap().join("receipt.json"),serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(fixture.restore(id,&digest).is_err());
    }

    #[test]
    fn package_limits_and_git_exclusion_match_native_discovery() {
        let fixture=Fixture::new(); let source=fixture.source(); let digest=package(&source).unwrap().digest;
        fs::create_dir(source.join(".git")).unwrap(); fs::write(source.join(".git/config"),b"ignored").unwrap();
        assert_eq!(package(&source).unwrap().digest,digest);
        fs::write(source.join("SKILL.md"),vec![b'x';256*1024+1]).unwrap(); assert!(package(&source).is_err());
        fs::write(source.join("SKILL.md"),b"valid").unwrap();
        fs::write(source.join("large"),vec![0;32*1024*1024]).unwrap(); assert!(package(&source).is_err());
    }

    #[test]
    fn native_skill_management_parser_requires_review_and_confirmation() {
        for command in ["install","remove","restore"] {
            let id=if command=="restore" {"00000000-0000-0000-0000-000000000001"} else {"sample"};
            assert!(Cli::try_parse_from(["velella-cli","skills",command,id]).is_err());
        }
        assert!(Cli::try_parse_from(["velella-cli","skills","install","source","--name","sample","--digest","sha256:reviewed","--confirm"]).is_ok());
    }

    #[test]
    fn links_in_packages_and_managed_roots_are_rejected() {
        let fixture=Fixture::new(); let source=fixture.source();
        let link=source.join("linked");
        #[cfg(windows)]
        { // Junctions require no Developer Mode privilege, unlike symlinks.
            let status=crate::process::command("cmd.exe").args(["/c","mklink","/J"]).arg(&link).arg(&fixture.0).output().unwrap();
            assert!(status.status.success(),"junction fixture failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&fixture.0,&link).unwrap();
        assert!(package(&source).is_err());
        #[cfg(windows)] fs::remove_dir(&link).unwrap();
        #[cfg(unix)] fs::remove_file(&link).unwrap();
        let store=fixture.0.join(".agents");
        #[cfg(windows)]
        { let status=crate::process::command("cmd.exe").args(["/c","mklink","/J"]).arg(&store).arg(&source).output().unwrap(); assert!(status.status.success()); }
        #[cfg(unix)] std::os::unix::fs::symlink(&source,&store).unwrap();
        assert!(fixture.install(&source,&package(&source).unwrap().digest).is_err());
        #[cfg(windows)] fs::remove_dir(&store).unwrap();
        #[cfg(unix)] fs::remove_file(&store).unwrap();
    }
}

pub(super) fn execute(operation: &SkillsOperation) -> Result<Value> {
    if let SkillsOperation::Center { library, file } = operation {
        ancestors(&std::path::absolute(file)?)?;
        let mut bytes=Vec::new();fs::File::open(file)?.take(1024*1024+1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len()<=1024*1024,"Skill Center request exceeds 1 MiB");
        let request: Value = serde_json::from_slice(&bytes)?;
        return center::execute(library, &request);
    }
    if let SkillsOperation::Preview { source } = operation { return Ok(receipt(&package(source)?)); }
    let home = std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).context("user home unavailable")?;
    execute_at(Path::new(&home), operation)
}
fn execute_at(home: &Path, operation: &SkillsOperation) -> Result<Value> {
    let target = match operation {
        SkillsOperation::Center { .. } => anyhow::bail!("center requires explicit library"),
        SkillsOperation::Preview { source } => return Ok(receipt(&package(source)?)),
        SkillsOperation::Inspect { target, .. } | SkillsOperation::Install { target, .. }
        | SkillsOperation::Remove { target, .. } | SkillsOperation::Backups { target }
        | SkillsOperation::Restore { target, .. } => *target,
    };
    let store = Store::new(home, target)?;
    match operation {
        SkillsOperation::Inspect { name, .. } => Ok(receipt(&package(&store.destination(name)?)?)),
        SkillsOperation::Install { source, name, digest, .. } => {
            let _lock = store.lock()?;
            let destination = store.destination(name)?; Store::vacant(&destination)?;
            let original = package(source)?; check_digest(&original, digest)?;
            ancestors(&store.skills)?; fs::create_dir_all(&store.skills)?;
            // Same filesystem as skills; an interrupted stage is outside discovery.
            let stage = store.base.join(format!("velella-skill-stage-{}",uuid::Uuid::now_v7()));
            fs::create_dir(&stage)?;
            for entry in &original.files {
                let path = stage.join(&entry.relative); fs::create_dir_all(path.parent().unwrap())?;
                let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
                file.write_all(&entry.bytes)?; file.sync_all()?;
                fs::set_permissions(path, entry.permissions.clone())?;
            }
            check_digest(&package(&stage)?, digest)?;
            check_digest(&package(source)?, digest)?;
            ancestors(&destination)?; Store::vacant(&destination)?;
            fs::rename(&stage, &destination).with_context(||format!("install not published; staging package retained at {}",stage.display()))?;
            let installed = package(&destination)?; check_digest(&installed,digest)?;
            Ok(json!({"installed":receipt(&installed),"native_discovery":"available_on_next_scan","existing_sessions":"unchanged"}))
        }
        SkillsOperation::Remove { name, digest, .. } => {
            let _lock = store.lock()?;
            let source = store.destination(name)?;
            let original = package(&source)?; check_digest(&original,digest)?;
            let id = uuid::Uuid::now_v7(); let backup = store.backup_path(id)?;
            fs::create_dir_all(&backup)?;
            let value = json!({"version":1,"id":id,"name":name,"digest":digest,"created_at":chrono::Utc::now().to_rfc3339()});
            let mut file = fs::OpenOptions::new().create_new(true).write(true).open(backup.join("receipt.json"))?;
            file.write_all(&serde_json::to_vec_pretty(&value)?)?; file.sync_all()?;
            check_digest(&package(&source)?,digest)?;
            fs::rename(&source,backup.join("package"))?;
            check_digest(&package(&backup.join("package"))?,digest).map_err(|e|failure("backup_changed",e,json!({"backup_id":id,"backup_path":backup})))?;
            Ok(json!({"removed":true,"backup":value,"backup_path":backup,"existing_sessions":"unchanged"}))
        }
        SkillsOperation::Backups { .. } => {
            ancestors(&store.backups)?;
            if !store.backups.exists() { return Ok(json!({"backups":[],"scope":"local-native-skills"})); }
            let mut rows = Vec::new();
            for entry in fs::read_dir(&store.backups)?.take(1001) {
                anyhow::ensure!(rows.len() < 1000,"backup listing exceeds 1000 entries");
                let entry = entry?;
                let id = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).context("unexpected backup directory name")?;
                let mut value = store.read_receipt(id)?;
                let path = store.backup_path(id)?.join("package"); ancestors(&path)?;
                value["package_present"] = json!(path.is_dir());
                value["digest_verified"] = json!(false); rows.push(value);
            }
            rows.sort_by_key(|row|row["id"].as_str().unwrap_or_default().to_owned());
            Ok(json!({"backups":rows,"scope":"local-native-skills"}))
        }
        SkillsOperation::Restore { id, digest, .. } => {
            let _lock = store.lock()?;
            let value = store.read_receipt(*id)?;
            anyhow::ensure!(value["digest"] == *digest,"digest does not match backup receipt");
            let destination = store.destination(value["name"].as_str().unwrap())?;
            Store::vacant(&destination)?;
            let source = store.backup_path(*id)?.join("package");
            check_digest(&package(&source)?,digest)?;
            fs::create_dir_all(&store.skills)?;
            ancestors(&destination)?; Store::vacant(&destination)?;
            fs::rename(&source,&destination)?;
            let restored = package(&destination)?; check_digest(&restored,digest)?;
            Ok(json!({"restored":receipt(&restored),"backup_id":id,"existing_sessions":"unchanged"}))
        }
        SkillsOperation::Preview { .. } => unreachable!(),
        SkillsOperation::Center { .. } => unreachable!(),
    }
}
