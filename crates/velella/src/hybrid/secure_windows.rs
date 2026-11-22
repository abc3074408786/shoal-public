//! Windows file operations pinned against directory replacement and reparse points.
//! Transactional writes fail closed on filesystems that do not support TxF.
use super::tools::{self, LIMIT};
use super::Config;
use anyhow::{Context, Result};
use std::{fs::{self, File, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}, io::{FromRawHandle, OwnedHandle}}, path::{Component, Path, PathBuf}, ptr};
use windows_sys::Win32::{Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE}, Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, CommitTransaction, CreateFileTransactedW, CreateTransaction, GetFileInformationByHandle, RollbackTransaction, CREATE_NEW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING}};
use std::os::windows::io::AsRawHandle;

fn no_reparse(file: &File) -> Result<()> {
    anyhow::ensure!(file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0, "reparse points are not local tool targets");
    Ok(())
}
fn no_hard_links(file: &File) -> Result<()> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 { return Err(std::io::Error::last_os_error()).context("cannot check local file links"); }
    anyhow::ensure!(info.nNumberOfLinks == 1, "files with multiple hard links are not local tool targets"); Ok(())
}

// Pin every directory from the volume root, not just the project root: a
// parent renamed after validation would otherwise change the path's meaning.
fn pin(config: &Config, relative: &str, directory: bool) -> Result<(PathBuf, Vec<File>)> {
    anyhow::ensure!(relative.len() <= 4096 && !relative.contains(['\0', ':', '\\']), "invalid local relative path");
    let rel = Path::new(relative);
    anyhow::ensure!(!rel.is_absolute(), "absolute local paths are not allowed");
    for c in rel.components() {
        match c {
            Component::CurDir => (),
            Component::Normal(n) => {
                let n = n.to_string_lossy();
                anyhow::ensure!(!matches!(n.to_ascii_lowercase().as_str(), ".git" | ".velella") && !n.ends_with(['.', ' ']), "private metadata path denied");
            },
            _ => anyhow::bail!("local path traversal denied"),
        }
    }
    let target = config.project_root.join(rel);
    let last = if directory { target.as_path() } else { target.parent().context("file parent missing")? };
    let mut dirs: Vec<_> = last.ancestors().collect();
    dirs.reverse();
    let mut handles = Vec::with_capacity(dirs.len());
    for path in dirs {
        let file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(path)
            .with_context(|| format!("cannot protect directory {}", path.display()))?;
        no_reparse(&file)?;
        anyhow::ensure!(file.metadata()?.is_dir(), "path component is not a directory");
        handles.push(file);
    }
    anyhow::ensure!(dunce::canonicalize(&config.project_root)? == config.project_root, "authoritative project root changed; review before reconnecting");
    Ok((target, handles))
}

pub fn read(config: &Config, relative: &str) -> Result<Vec<u8>> {
    let (target, _dirs) = pin(config, relative, false)?;
    let file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(target)?;
    no_reparse(&file)?; no_hard_links(&file)?;
    anyhow::ensure!(file.metadata()?.is_file() && file.metadata()?.len() <= LIMIT as u64, "local file is not regular or exceeds 1 MiB");
    let mut bytes = Vec::new(); file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= LIMIT, "local file grew beyond limit"); Ok(bytes)
}

pub fn list(config: &Config, relative: &str) -> Result<Vec<serde_json::Value>> {
    let (target, _dirs) = pin(config, relative, true)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(target)?.take(513) {
        let e = entry?; let name = e.file_name().to_string_lossy().into_owned();
        if matches!(name.to_ascii_lowercase().as_str(), ".git" | ".velella") { continue; }
        entries.push(serde_json::json!({"name": name, "directory": e.file_type()?.is_dir(), "link": e.file_type()?.is_symlink()}));
    }
    anyhow::ensure!(entries.len() <= 512, "directory listing exceeds 512 entries"); Ok(entries)
}

struct Transaction { handle: OwnedHandle, committed: bool }
impl Drop for Transaction { fn drop(&mut self) { if !self.committed { unsafe { RollbackTransaction(self.handle.as_raw_handle() as _); } } } }

pub fn write(config: &Config, relative: &str, expected: &str, bytes: &[u8]) -> Result<()> {
    let (target, _dirs) = pin(config, relative, false)?;
    let tx = unsafe { CreateTransaction(ptr::null_mut(), ptr::null_mut(), 0, 0, 0, 0, ptr::null()) };
    if tx == INVALID_HANDLE_VALUE { return Err(std::io::Error::last_os_error()).context("local filesystem transaction unavailable"); }
    let mut tx = Transaction { handle: unsafe { OwnedHandle::from_raw_handle(tx as _) }, committed: false };
    let mut name: Vec<u16> = target.as_os_str().encode_wide().collect(); name.push(0);
    let disposition = if expected == "absent" { CREATE_NEW } else { OPEN_EXISTING };
    let handle = unsafe { CreateFileTransactedW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, ptr::null(), disposition, FILE_FLAG_OPEN_REPARSE_POINT, ptr::null_mut(), tx.handle.as_raw_handle() as _, ptr::null(), ptr::null()) };
    if handle == INVALID_HANDLE_VALUE { return Err(std::io::Error::last_os_error()).context("cannot safely open local file in transaction"); }
    let mut file = unsafe { File::from_raw_handle(handle as _) };
    no_reparse(&file)?; no_hard_links(&file)?;
    let meta = file.metadata()?;
    anyhow::ensure!(meta.is_file() && meta.len() <= LIMIT as u64, "existing file exceeds write limit or is not regular");
    if expected != "absent" {
        let mut old = Vec::new(); (&mut file).take((LIMIT + 1) as u64).read_to_end(&mut old)?;
        anyhow::ensure!(old.len() <= LIMIT && tools::digest(&old) == expected, "local file changed; read and request approval again");
    }
    file.seek(SeekFrom::Start(0))?; file.write_all(bytes)?; file.set_len(bytes.len() as u64)?; file.sync_all()?; drop(file);
    if unsafe { CommitTransaction(tx.handle.as_raw_handle() as _) } == 0 { return Err(std::io::Error::last_os_error()).context("local file transaction did not commit"); }
    tx.committed = true; Ok(())
}

/// The sole metadata exception: create a new generated evidence file. Keep
/// each newly created directory pinned before opening its child.
pub fn write_evidence(config:&Config,id:&uuid::Uuid,bytes:&[u8])->Result<String>{
    let (_,mut handles)=pin(config,".",true)?;
    let mut root=config.project_root.clone();
    for name in [".velella","evidence"] {
        root.push(name);
        match fs::create_dir(&root){Ok(())=>{},Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(error)=>return Err(error.into())}
        let handle=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE).custom_flags(FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT).open(&root)?;
        no_reparse(&handle)?;anyhow::ensure!(handle.metadata()?.is_dir(),"evidence parent is not a directory");handles.push(handle);
    }
    let mut scoped=config.clone();scoped.project_root=root;
    let name=format!("{id}.log");write(&scoped,&name,"absent",bytes)?;
    Ok(format!(".velella/evidence/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]fn evidence_is_create_only_and_cannot_follow_metadata_junctions(){
        let c=fixture();let id=uuid::Uuid::new_v4();write_evidence(&c,&id,b"first").unwrap();assert!(write_evidence(&c,&id,b"replace").is_err());
        assert_eq!(fs::read(c.project_root.join(format!(".velella/evidence/{id}.log"))).unwrap(),b"first");
        let other=fixture();let outside=other.directory.join("outside");fs::create_dir(&outside).unwrap();
        let status=crate::process::command("cmd.exe").args(["/c","mklink","/J"]).arg(other.project_root.join(".velella")).arg(&outside).status().unwrap();assert!(status.success());
        assert!(write_evidence(&other,&uuid::Uuid::new_v4(),b"escape").is_err());assert!(!outside.join("evidence").exists());
        fs::remove_dir_all(c.directory).unwrap();fs::remove_dir_all(other.directory).unwrap();
    }
    fn fixture() -> Config {
        let d = std::env::temp_dir().join(format!("velella-safe-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&d).unwrap();
        let d = dunce::canonicalize(d).unwrap();
        let root = d.join("project"); fs::create_dir(&root).unwrap();
        Config { session_id: uuid::Uuid::new_v4().to_string(), workspace_id: "fixture".into(), project_root: root, directory: d, profile: json!({"host":"test"}), profile_fingerprint: "test".into(), collaboration_data: None, collaboration_writes:false }
    }
    #[test]
    fn pinned_directories_cannot_be_replaced() {
        let c = fixture(); fs::create_dir(c.project_root.join("sub")).unwrap();
        let (_, held) = pin(&c, "sub/file", false).unwrap();
        assert!(fs::rename(c.project_root.join("sub"), c.project_root.join("moved")).is_err());
        drop(held); fs::rename(c.project_root.join("sub"), c.project_root.join("moved")).unwrap();
        fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn transaction_rejects_stale_digest_and_updates_existing_file() {
        let c = fixture(); let file = c.project_root.join("a.txt"); fs::write(&file, b"external").unwrap();
        assert!(write(&c, "a.txt", &tools::digest(b"old"), b"new").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"external");
        write(&c, "a.txt", &tools::digest(b"external"), b"new").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"new");
        assert!(write(&c, "a.txt", "absent", b"bad").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"new");
        fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn pinned_read_denies_junction_escape() {
        let c = fixture(); let outside = c.directory.join("outside"); fs::create_dir(&outside).unwrap(); fs::write(outside.join("secret"), b"secret").unwrap();
        let link = c.project_root.join("link");
        assert!(crate::process::command("cmd.exe").args(["/d", "/c", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap().status.success());
        assert!(read(&c, "link/secret").is_err());
        assert!(write(&c, "link/new", "absent", b"bad").is_err());
        fs::remove_dir(&link).unwrap(); fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn hard_links_to_external_files_are_rejected() {
        let c = fixture(); let outside = c.directory.join("outside"); fs::write(&outside, b"secret").unwrap();
        fs::hard_link(&outside, c.project_root.join("link")).unwrap();
        assert!(read(&c, "link").is_err());
        assert!(write(&c, "link", &tools::digest(b"secret"), b"bad").is_err());
        assert_eq!(fs::read(outside).unwrap(), b"secret"); fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn competing_open_writer_fails_closed() {
        let c = fixture(); let path = c.project_root.join("a"); fs::write(&path, b"original").unwrap();
        let held = OpenOptions::new().write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).open(&path).unwrap();
        assert!(write(&c, "a", &tools::digest(b"original"), b"new").is_err());
        drop(held); assert_eq!(fs::read(path).unwrap(), b"original"); fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn simultaneous_writes_with_same_digest_cannot_both_commit() {
        let c = fixture(); let path = c.project_root.join("race");
        for _ in 0..20 {
        fs::write(&path, b"original").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for value in [b"first".as_slice(), b"second".as_slice()] {
            let c = c.clone(); let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait(); write(&c, "race", &tools::digest(b"original"), value).is_ok()
            }));
        }
        barrier.wait();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|v| **v).count(), 1, "exactly one competing write must commit: {results:?}");
        let actual = fs::read(&path).unwrap(); assert!(actual == b"first" || actual == b"second");
        }
        fs::remove_dir_all(c.directory).unwrap();
    }
    #[test]
    fn simultaneous_creates_do_not_replace_each_other() {
        let c=fixture(); let path=c.project_root.join("new");
        for _ in 0..20 {
            let barrier=std::sync::Arc::new(std::sync::Barrier::new(3)); let mut tasks=Vec::new();
            for value in [b"one".as_slice(),b"two".as_slice()] {let c=c.clone();let barrier=barrier.clone();tasks.push(std::thread::spawn(move||{barrier.wait();write(&c,"new","absent",value).is_ok()}));}
            barrier.wait();let outcomes:Vec<_>=tasks.into_iter().map(|t|t.join().unwrap()).collect();
            assert_eq!(outcomes.iter().filter(|v|**v).count(),1,"exactly one create must commit: {outcomes:?}");
            let actual=fs::read(&path).unwrap();assert!(actual==b"one"||actual==b"two");fs::remove_file(&path).unwrap();
        }
        fs::remove_dir_all(c.directory).unwrap();
    }
}
