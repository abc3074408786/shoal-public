//! The actual Codex TUI runs in our existing PTY. Only its model controller is
//! outside the sandbox; its sole environment delegates tools to the fixed
//! restricted executor. No chat events are rendered as terminal output.
use crate::{restricted_controller, host::Descriptor, sandbox::Launch};
use anyhow::{Context, Result};
use std::{io::Read, path::{Path, PathBuf}, process::Stdio};

pub(crate) const ADAPTER: &str = "codex-restricted-terminal";

fn native_elf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic)).is_ok()
        && magic == *b"\x7fELF"
}

fn native_codex(desc: &Descriptor) -> Result<PathBuf> {
    let command = codex_command(desc)?;
    let entry = PathBuf::from(command.get_program());
    let mut candidates = vec![entry.clone()];
    // Resolve the installed official npm package without executing its JS
    // wrapper. Cancellation must suspend the actual TUI process, not Node.
    if entry.file_name().is_some_and(|name| name == "codex.js") {
        let package = entry.parent().and_then(Path::parent).context("Codex package root missing")?;
        let (name, target) = match std::env::consts::ARCH {
            "x86_64" => ("codex-linux-x64", "x86_64-unknown-linux-musl"),
            "aarch64" => ("codex-linux-arm64", "aarch64-unknown-linux-musl"),
            _ => anyhow::bail!("restricted Codex terminal requires Linux x86_64 or aarch64"),
        };
        let relative = Path::new("vendor").join(target).join("bin/codex");
        candidates.push(package.join("node_modules/@openai").join(name).join(&relative));
        if let Some(scope) = package.parent() { candidates.push(scope.join(name).join(&relative)); }
        candidates.push(package.join(relative));
    }
    let work = Path::new(desc.spawn.cwd.as_deref().context("terminal workspace missing")?);
    for candidate in candidates {
        if !native_elf(&candidate) { continue; }
        let candidate = candidate.canonicalize()?;
        anyhow::ensure!(!candidate.starts_with(work) && !candidate.starts_with(&desc.directory),
            "restricted terminal executable must stay outside writable workspace and session state");
        return Ok(candidate);
    }
    anyhow::bail!("受限 Codex 终端需要原生 Codex CLI 0.153.4；未找到 npm 包内的可执行文件")
}

fn codex_command(desc: &Descriptor) -> Result<std::process::Command> {
    anyhow::ensure!(desc.spawn.program == "codex" && desc.spawn.sandbox.is_restricted(), "restricted terminal requires Codex");
    let work = dunce::canonicalize(desc.spawn.cwd.as_ref().context("workspace missing")?)?;
    let path = desc.spawn.env.get("PATH").context("Codex discovery PATH missing")?;
    for directory in std::env::split_paths(path) {
        if !directory.is_absolute() { continue; }
        let candidate = directory.join("codex");
        if !candidate.is_file() { continue; }
        let executable = dunce::canonicalize(candidate)?;
        anyhow::ensure!(!executable.starts_with(&work) && !executable.starts_with(&desc.directory),
            "restricted terminal Codex executable must be outside task-writable directories");
        return Ok(crate::process::command(executable.to_str().context("Codex path must be UTF-8")?));
    }
    anyhow::bail!("Codex executable not found on the configured PATH")
}

fn fixed_overrides(work: &Path, controller: &Path) -> Result<Vec<String>> {
    let mut fixed: toml::Table = toml::from_str(restricted_controller::LOCKED_CONFIG)?;
    let mut projects = toml::Table::new();
    for root in [work, controller] {
        projects.insert(root.to_string_lossy().into_owned(), toml::Value::Table(
            toml::Table::from_iter([("trust_level".into(), toml::Value::String("untrusted".into()))])));
    }
    fixed.insert("projects".into(), toml::Value::Table(projects));
    // Root tables preserve literal path keys containing periods. Provider data
    // stays only in the private config file, never in argv or launch receipts.
    Ok(fixed.into_iter().flat_map(|(key, value)| ["-c".into(), format!("{key}={value}")]).collect())
}

pub(crate) fn prepare(desc: &Descriptor) -> Result<(Launch, std::sync::Arc<crate::terminal_bridge::Bridge>)> {
    anyhow::ensure!(desc.spawn.adapter.as_deref() == Some(ADAPTER), "invalid restricted terminal adapter");
    restricted_controller::validate_launch(desc)?;
    let executable = native_codex(desc)?;
    let prepared = restricted_controller::prepare(desc)?;
    let version = crate::process::command(executable.to_str().context("Codex path UTF-8")?)
        .arg("--version").env_clear().envs(&prepared.env).current_dir(&prepared.cwd)
        .stdin(Stdio::null()).output().context("check restricted-terminal Codex version")?;
    anyhow::ensure!(version.status.success() && String::from_utf8_lossy(&version.stdout).trim() == "codex-cli 0.153.4",
        "受限 Codex 终端目前只支持已验收的 Codex CLI 0.153.4，不会回退到本机执行");
    let work = Path::new(desc.spawn.cwd.as_deref().context("terminal workspace missing")?);
    let mut args = fixed_overrides(work, &prepared.cwd)?;
    let mut audit = crate::process::command(executable.to_str().context("Codex path UTF-8")?);
    audit.args(&args).env_clear().envs(&prepared.env).current_dir(&prepared.cwd);
    crate::terminal_config_audit::audit(audit, &desc.directory, work, &prepared.configuration)?;
    let bridge = crate::terminal_bridge::start(desc, &executable, &args, &prepared)?;
    args.extend(["--remote".into(), bridge.endpoint()]);
    args.extend(["--no-alt-screen".into(), "--cd".into(), work.to_string_lossy().into_owned()]);
    if let Some(thread) = prepared.resume_thread {
        args.extend(["resume".into(), thread]);
    }
    Ok((Launch {
        program: executable.to_string_lossy().into_owned(), args,
        cwd: Some(prepared.cwd.to_string_lossy().into_owned()), env: prepared.env, clear_env: true,
    }, bridge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_overrides_keep_literal_project_paths_and_only_fixed_data() {
        let work = Path::new("/tmp/project.with.dots");
        let controller = Path::new("/private/session/controller");
        let args = fixed_overrides(work, controller).unwrap();
        let mut values = toml::Table::new();
        for pair in args.chunks_exact(2) {
            assert_eq!(pair[0], "-c");
            let parsed: toml::Table = toml::from_str(&pair[1]).unwrap();
            values.extend(parsed);
        }
        assert_eq!(values["projects"][work.to_str().unwrap()]["trust_level"].as_str(), Some("untrusted"));
        assert_eq!(values["projects"][controller.to_str().unwrap()]["trust_level"].as_str(), Some("untrusted"));
        assert_eq!(values["features"]["hooks"].as_bool(), Some(false));
        assert_eq!(values["features"]["skip_host_skill_discovery"].as_bool(), Some(true));
        assert!(!values.contains_key("model_providers"));
        assert!(!args.join(" ").contains("API_KEY"));
    }
}
