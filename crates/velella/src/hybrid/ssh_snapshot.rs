//! Conservative OpenSSH effective-configuration comparison for remote roles.
//! This only validates a snapshot; it does not yet bind a live connection.
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct DirectTarget {
    pub hostname: String,
    pub user: String,
    pub identities_only: String,
    pub identity_files: Vec<String>,
}
impl DirectTarget {
    /// Use only together with the caller's existing strict host-key and
    /// forwarding policy. `-F` prevents a later alias re-resolution.
    pub(crate) fn fixed_args(&self) -> Vec<String> {
        let mut args=vec!["-F".into(),if cfg!(windows){"NUL"}else{"/dev/null"}.into(),
            "-l".into(),self.user.clone(),"-o".into(),format!("IdentitiesOnly={}",self.identities_only)];
        for identity in &self.identity_files {args.push("-i".into());args.push(identity.clone());}
        args.push("--".into());args.push(self.hostname.clone());args
    }
}

fn parse(input: &str) -> Result<BTreeMap<String, Vec<String>>> {
    anyhow::ensure!(input.len() <= 256 * 1024, "SSH effective configuration exceeds limit");
    let mut values = BTreeMap::<String, Vec<String>>::new();
    for line in input.lines() {
        let (name, value) = line.split_once(' ').context("invalid SSH effective configuration")?;
        anyhow::ensure!(!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric())
            && !value.is_empty() && !value.chars().any(char::is_control), "invalid SSH effective configuration");
        values.entry(name.to_ascii_lowercase()).or_default().push(value.to_owned());
    }
    Ok(values)
}

/// Only differences that can eventually be supplied as explicit SSH arguments
/// are accepted. No config values are logged or returned in error messages.
pub(crate) fn direct_target(effective: &str, defaults: &str) -> Result<DirectTarget> {
    let actual = parse(effective)?;
    let baseline = parse(defaults)?;
    let allowed: BTreeSet<&str> = ["hostname", "user", "identityfile", "identitiesonly"].into_iter().collect();
    for name in actual.keys().chain(baseline.keys()) {
        anyhow::ensure!(allowed.contains(name.as_str()) || actual.get(name) == baseline.get(name),
            "SSH profile uses settings that cannot be safely frozen for a direct remote role");
    }
    let one = |name: &str| -> Result<String> {
        let items = actual.get(name).context("SSH target field missing")?;
        anyhow::ensure!(items.len() == 1, "SSH target field is ambiguous");
        Ok(items[0].clone())
    };
    let hostname = one("hostname")?;
    let user = one("user")?;
    let identities_only = one("identitiesonly")?;
    anyhow::ensure!(!hostname.starts_with('-') && hostname.len() <= 255 && hostname.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-[]".contains(&b)), "invalid resolved SSH hostname");
    anyhow::ensure!(!user.starts_with('-') && user.len() <= 128 && user.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)), "invalid resolved SSH user");
    anyhow::ensure!(matches!(identities_only.as_str(), "yes" | "no"), "invalid SSH identity policy");
    let identity_files = actual.get("identityfile").cloned().context("SSH identity references missing")?;
    anyhow::ensure!(!identity_files.is_empty() && identity_files.len() <= 32 && identity_files.iter().all(|p| p.len() <= 4096 && !p.starts_with('-') && !p.contains('%') && (p.starts_with("~/") || p.starts_with("~\\") || std::path::Path::new(p).is_absolute())), "unsupported SSH identity reference");
    Ok(DirectTarget { hostname, user, identities_only, identity_files })
}

fn identity_path(value:&str,home:&Path)->Result<PathBuf> {
    let path=if let Some(rest)=value.strip_prefix("~/").or_else(||value.strip_prefix("~\\")) {home.join(rest)}else{PathBuf::from(value)};
    anyhow::ensure!(path.is_absolute(),"SSH identity reference is not absolute");
    // Resolve only the path, never copy or read the identity's bytes.
    dunce::canonicalize(path).context("SSH identity reference is unavailable")
}

/// Re-evaluate the fixed arguments with `ssh -G -F ...` before using them.
/// The original host label may differ; every effective option must otherwise
/// match, including host-key checking and forwarding. Identity paths are
/// compared after expansion because OpenSSH may print them differently.
pub(crate) fn verify_reconstruction(original:&str,rebuilt:&str,home:&Path)->Result<()> {
    let a=parse(original)?;let b=parse(rebuilt)?;
    for name in a.keys().chain(b.keys()) {
        if name=="host"||name=="identityfile" {continue}
        anyhow::ensure!(a.get(name)==b.get(name),"fixed SSH arguments do not preserve the effective configuration");
    }
    let before=a.get("identityfile").context("original SSH identities missing")?;
    let after=b.get("identityfile").context("fixed SSH identities missing")?;
    anyhow::ensure!(before.len()==after.len(),"fixed SSH identity references changed");
    for (left,right) in before.iter().zip(after) {
        anyhow::ensure!(identity_path(left,home)?==identity_path(right,home)?,"fixed SSH identity reference changed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DEFAULT: &str = "host agent\nhostname agent\nuser local\nidentitiesonly no\nidentityfile ~/.ssh/default\nstricthostkeychecking true\n";
    const ALIAS: &str = "host agent\nhostname remote.example\nuser remote\nidentitiesonly yes\nidentityfile ~/.ssh/role\nstricthostkeychecking true\n";
    #[test] fn direct_alias_preserves_resolved_fields() {
        let target = direct_target(ALIAS, DEFAULT).unwrap();
        assert_eq!(target.hostname, "remote.example");
        assert_eq!(target.user, "remote");
        assert_eq!(target.identity_files, ["~/.ssh/role"]);
        assert_eq!(target.identities_only, "yes");
        let args=target.fixed_args();
        assert_eq!(&args[args.len()-2..], ["--", "remote.example"]);
        assert!(args.windows(2).any(|v|v==["-l","remote"]));
        assert!(args.windows(2).any(|v|v==["-i","~/.ssh/role"]));
    }
    #[test] fn unknown_settings_and_ambiguous_fields_fail_closed() {
        assert!(direct_target(&format!("{ALIAS}proxycommand arbitrary\n"), DEFAULT).is_err());
        assert!(direct_target(&ALIAS.replace("stricthostkeychecking true", "stricthostkeychecking false"), DEFAULT).is_err());
        assert!(direct_target(&format!("{ALIAS}hostname other\n"), DEFAULT).is_err());
        assert!(direct_target(&ALIAS.replace("~/.ssh/role", "%h"), DEFAULT).is_err());
        assert!(direct_target(&ALIAS.replace("~/.ssh/role", "relative-key"), DEFAULT).is_err());
    }
    #[test] fn reconstructed_policy_and_identity_must_match() {
        let home=std::env::temp_dir().join(format!("ssh-snapshot-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".ssh")).unwrap();std::fs::write(home.join(".ssh/role"),b"fixture").unwrap();
        let rebuilt=ALIAS.replace("host agent", "host remote.example").replace("~/.ssh/role", &home.join(".ssh/role").to_string_lossy());
        assert!(verify_reconstruction(ALIAS,&rebuilt,&home).is_ok());
        assert!(verify_reconstruction(ALIAS,&rebuilt.replace("stricthostkeychecking true","stricthostkeychecking false"),&home).is_err());
        assert!(verify_reconstruction(ALIAS,&format!("{rebuilt}proxycommand unexpected\n"),&home).is_err());
        std::fs::write(home.join(".ssh/other"),b"fixture").unwrap();
        assert!(verify_reconstruction(ALIAS,&rebuilt.replace(&*home.join(".ssh/role").to_string_lossy(),&home.join(".ssh/other").to_string_lossy()),&home).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "explicit read-only check against the locally configured agent alias"]
    fn current_agent_alias_reconstructs_without_loading_config() {
        fn query(extra:&[String])->String {
            let mut command=std::process::Command::new("ssh");
            command.args(["-G","-T","-o","BatchMode=yes","-o","StrictHostKeyChecking=yes","-o","ForwardAgent=no","-o","ForwardX11=no","-o","PermitLocalCommand=no","-o","RemoteCommand=none","-o","ControlMaster=no","-o","ControlPath=none","-o","ClearAllForwardings=yes","-p","22"]);
            let result=command.args(extra).output().unwrap();assert!(result.status.success(),"ssh -G failed");String::from_utf8(result.stdout).unwrap()
        }
        let actual=query(&["agent".into()]);
        let defaults=query(&["-F".into(),"NUL".into(),"agent".into()]);
        let target=direct_target(&actual,&defaults).unwrap();
        let fixed=query(&target.fixed_args());
        let home=std::env::var_os("USERPROFILE").unwrap();
        verify_reconstruction(&actual,&fixed,Path::new(&home)).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn fixed_arguments_ignore_later_alias_changes() {
        let root=std::env::temp_dir().join(format!("ssh-alias-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
        let key=root.join("fixture-key");std::fs::write(&key,b"fixture only").unwrap();
        let file=root.join("config");
        let write_config=|host:&str|std::fs::write(&file,format!("Host fixture\n  HostName {host}\n  User fixtureuser\n  IdentitiesOnly yes\n  IdentityFile \"{}\"\n",key.display())).unwrap();
        fn query(extra:&[String])->String {
            let result=std::process::Command::new("ssh").args(["-G","-T","-o","BatchMode=yes","-o","StrictHostKeyChecking=yes","-o","ForwardAgent=no","-o","ForwardX11=no","-o","PermitLocalCommand=no","-o","RemoteCommand=none","-o","ControlMaster=no","-o","ControlPath=none","-o","ClearAllForwardings=yes","-p","22"]).args(extra).output().unwrap();
            assert!(result.status.success(),"ssh -G fixture failed");String::from_utf8(result.stdout).unwrap()
        }
        write_config("first.example");
        let actual=query(&["-F".into(),file.to_string_lossy().into_owned(),"fixture".into()]);
        let defaults=query(&["-F".into(),"NUL".into(),"fixture".into()]);
        let target=direct_target(&actual,&defaults).unwrap();
        let home=std::env::var_os("USERPROFILE").unwrap();
        verify_reconstruction(&actual,&query(&target.fixed_args()),Path::new(&home)).unwrap();
        write_config("second.example");
        let changed=query(&["-F".into(),file.to_string_lossy().into_owned(),"fixture".into()]);
        assert_ne!(parse(&actual).unwrap().get("hostname"),parse(&changed).unwrap().get("hostname"));
        verify_reconstruction(&actual,&query(&target.fixed_args()),Path::new(&home)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
