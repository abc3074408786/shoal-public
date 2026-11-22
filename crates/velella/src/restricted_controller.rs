//! Private host controller configuration for restricted Codex terminals.
//! Only the selected model provider's data crosses into this profile. Tool
//! configuration, authentication files, shell hooks and host skills never do.
//! Native terminals can separately reuse an existing ChatGPT access token in
//! the trusted bridge's memory; neither auth files nor refresh tokens are copied.
use crate::{host::Descriptor, sandbox::{Network, Policy}};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}};

const ADAPTER: &str = include_str!("../../../scripts/fixtures/restricted-exec-server.py");
pub const ENVIRONMENT_ID: &str = "velella-restricted";
const CONFIG_LIMIT: u64 = 1024 * 1024;

pub struct Prepared {
    pub configuration: String,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub chatgpt_auth_source: Option<PathBuf>,
    pub resume_thread: Option<String>,
}

pub fn validate_launch(desc: &Descriptor) -> Result<()> {
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    { linux::validate_launch(desc) }
    #[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    { let _ = desc; anyhow::bail!("restricted terminal requires Linux x86_64 or aarch64") }
}

pub fn prepare(desc: &Descriptor) -> Result<Prepared> {
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    { linux::prepare(desc) }
    #[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    { let _ = desc; anyhow::bail!("restricted terminal requires Linux x86_64 or aarch64") }
}

pub fn run_exec_server(config_path: &Path) -> Result<()> {
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    { linux::run_exec_server(config_path) }
    #[cfg(not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    { let _ = config_path; anyhow::bail!("restricted exec-server requires Linux x86_64 or aarch64") }
}

/// Keep this profile independent of source config tables, project trust,
/// installed integrations, account stores and changing default feature flags.
pub(crate) const LOCKED_CONFIG: &str = r#"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
allow_login_shell = false
cli_auth_credentials_store = "file"
mcp_oauth_credentials_store = "file"
check_for_update_on_startup = false
[analytics]
enabled = false
[feedback]
enabled = false
[otel]
exporter = "none"
trace_exporter = "none"
metrics_exporter = "none"
[orchestrator.skills]
enabled = false
[features]
shell_snapshot = false
shell_snapshot_v2 = false
apps = false
plugins = false
recommended_plugins = false
plugin_sharing = false
hooks = false
browser_use = false
browser_use_external = false
browser_use_full_cdp_access = false
in_app_browser = false
in_app_local_automation = false
computer_use = false
code_mode_host = false
image_generation = false
multi_agent = false
multi_agent_v2 = false
view_image = false
workspace_dependencies = false
remote_plugin = false
memories = false
chronicle = false
code_mode = false
code_mode_only = false
code_mode_prewarm = false
js_repl = false
skill_search = false
skill_mcp_dependency_install = false
tool_call_mcp_elicitation = false
tool_suggest = false
sleep_tool = false
goals = false
auth_elicitation = false
skip_host_skill_discovery = true
unified_exec = true
enable_request_compression = false
responses_websockets = false
responses_websockets_v2 = false
"#;

fn text(value: &toml::Value, key: &str, max: usize) -> Result<String> {
    let text = value.as_str().with_context(|| format!("model configuration {key} must be a string"))?;
    anyhow::ensure!(!text.is_empty() && text.len() <= max && !text.contains('\0'),
                    "invalid model configuration {key}");
    Ok(text.to_owned())
}

fn provider_environment_name(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    anyhow::ensure!(!bytes.is_empty() && bytes.len() <= 128
        && bytes[0].is_ascii_uppercase()
        && bytes.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_'),
        "provider environment variable name is invalid");
    // These names control processes, paths, code loading, filesystem helpers,
    // network routing or the application itself; they cannot double as keys.
    let blocked = ["HOME", "PATH", "SHELL", "ENV", "BASH_ENV", "ZDOTDIR", "IFS",
        "TMP", "TEMP", "TMPDIR", "LANG", "LANGUAGE", "TERM", "COLORTERM", "PWD",
        "OLDPWD", "USER", "LOGNAME", "LOCPATH", "HOSTALIASES", "RES_OPTIONS",
        "SSLKEYLOGFILE", "RUBYOPT", "RUBYLIB", "PERL5OPT", "PERL5LIB"];
    let prefixes = ["LD_", "DYLD_", "PYTHON", "NODE_", "BASH_", "ZSH_", "GIT_",
        "SSH_", "CODEX_", "VELELLA_", "XDG_", "LC_", "GLIBC_", "MALLOC_", "GCONV_",
        "RUST", "CARGO_", "SSL_", "CURL_", "REQUESTS_", "AWS_", "GOOGLE_", "TCL",
        "JAVA", "JDK", "QT_", "GTK_", "GIO_", "GI_", "DBUS_", "XAUTH", "DISPLAY"];
    anyhow::ensure!(!blocked.contains(&name) && !prefixes.iter().any(|p| name.starts_with(p))
        && !name.ends_with("_PROXY") && !name.ends_with("_OPTIONS")
        && (!name.starts_with("OPENAI_") || name == "OPENAI_API_KEY"),
        "provider variable {name} is a reserved process setting");
    Ok(())
}

/// Return a newly constructed config and the *names* of explicitly selected
/// provider variables. Never read or return authentication files here.
fn selected_model_config(source: &str) -> Result<(String, HashSet<String>)> {
    let source: toml::Table = toml::from_str(source)
        .map_err(|_| anyhow::anyhow!("selected model configuration is not valid TOML"))?;
    let mut result: toml::Table = toml::from_str(LOCKED_CONFIG)?;
    for key in ["model", "model_provider", "model_reasoning_effort"] {
        if let Some(value) = source.get(key) {
            let copied = text(value, key, 256)?;
            if key == "model_reasoning_effort" {
                anyhow::ensure!(["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
                    .contains(&copied.as_str()), "unsupported model reasoning effort");
            }
            result.insert(key.into(), toml::Value::String(copied));
        }
    }
    let provider_id = result.get("model_provider").and_then(toml::Value::as_str)
        .unwrap_or("openai").to_owned();
    let mut environment_names = HashSet::new();
    if let Some(provider) = source.get("model_providers")
        .and_then(toml::Value::as_table).and_then(|providers| providers.get(&provider_id)) {
        let provider = provider.as_table().context("selected model provider must be a table")?;
        // Dropping command-backed auth silently could trigger a different
        // credential chain. Reject it with a clear, non-secret diagnostic.
        for forbidden in ["auth", "aws", "gateway_oauth", "experimental_bearer_token"] {
            anyhow::ensure!(!provider.contains_key(forbidden),
                "restricted terminal does not import provider {forbidden}; use an explicit API-key environment variable");
        }
        let mut copied = toml::Table::new();
        for key in ["name", "base_url", "env_key", "env_key_instructions", "wire_api"] {
            if let Some(value) = provider.get(key) {
                let value = text(value, key, 8192)?;
                if key == "wire_api" {
                    anyhow::ensure!(value == "responses", "restricted terminal requires a Responses provider");
                }
                if key == "env_key" {
                    provider_environment_name(&value)?;
                    environment_names.insert(value.clone());
                }
                copied.insert(key.into(), toml::Value::String(value));
            }
        }
        for key in ["request_max_retries", "stream_max_retries", "stream_idle_timeout_ms"] {
            if let Some(value) = provider.get(key) {
                let number = value.as_integer().with_context(|| format!("{key} must be an integer"))?;
                let maximum = if key == "stream_idle_timeout_ms" { 3_600_000 } else { 100 };
                anyhow::ensure!((0..=maximum).contains(&number), "invalid provider {key}");
                copied.insert(key.into(), value.clone());
            }
        }
        for key in ["supports_websockets", "requires_openai_auth"] {
            if let Some(value) = provider.get(key) {
                anyhow::ensure!(value.as_bool().is_some(), "provider {key} must be boolean");
                copied.insert(key.into(), value.clone());
            }
        }
        for key in ["http_headers", "env_http_headers", "query_params"] {
            if let Some(value) = provider.get(key) {
                let table = value.as_table().with_context(|| format!("{key} must be a table"))?;
                anyhow::ensure!(table.len() <= 64, "too many provider {key}");
                let mut map = toml::Table::new();
                for (name, value) in table {
                    anyhow::ensure!(!name.is_empty() && name.len() <= 256
                        && !name.chars().any(char::is_control), "invalid provider {key} entry name");
                    let value = text(value, key, 8192)?;
                    anyhow::ensure!(!value.contains(['\r', '\n']), "invalid provider {key} entry");
                    if key == "env_http_headers" {
                        provider_environment_name(&value)?;
                        environment_names.insert(value.clone());
                    }
                    map.insert(name.clone(), toml::Value::String(value));
                }
                copied.insert(key.into(), toml::Value::Table(map));
            }
        }
        let mut providers = toml::Table::new();
        providers.insert(provider_id, toml::Value::Table(copied));
        result.insert("model_providers".into(), toml::Value::Table(providers));
    } else {
        anyhow::ensure!(["openai", "ollama", "lmstudio"].contains(&provider_id.as_str()),
            "selected model provider is missing from config.toml");
    }
    Ok((toml::to_string(&result)?, environment_names))
}

// Existing ChatGPT credentials must never follow a custom provider endpoint,
// headers or key selection, even when that provider uses the name "openai".
fn uses_builtin_chatgpt_provider(source: &str) -> Result<bool> {
    let source: toml::Table = toml::from_str(source)
        .map_err(|_| anyhow::anyhow!("selected model configuration is not valid TOML"))?;
    Ok(source.get("model_provider").and_then(toml::Value::as_str).unwrap_or("openai") == "openai"
        && source.get("model_providers").and_then(toml::Value::as_table)
            .and_then(|providers| providers.get("openai")).is_none())
}

/// Preserve the host's model-service route without adding it to tool processes.
/// These values come only from the trusted host, never workspace configuration.
fn controller_network_environment(
    read: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
                 "http_proxy", "https_proxy", "all_proxy", "no_proxy"] {
        if let Some(value) = read(name) {
            let value = value.into_string()
                .map_err(|_| anyhow::anyhow!("model network setting {name} must be UTF-8"))?;
            anyhow::ensure!(value.len() <= 65536 && !value.chars().any(char::is_control),
                "invalid model network setting {name}");
            env.insert(name.into(), value);
        }
    }
    Ok(env)
}

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod linux {
    use super::*;
    use std::{fs::{self, DirBuilder, File, OpenOptions}, io::{Read, Write},
        os::unix::{fs::{DirBuilderExt, MetadataExt, OpenOptionsExt}, process::CommandExt},
        process::Command};

    fn uid() -> u32 { unsafe { libc::geteuid() } }

    fn canonical_directory(path: &Path) -> Result<PathBuf> {
        anyhow::ensure!(path.is_absolute(), "restricted terminal paths must be absolute");
        let canonical = path.canonicalize().context("restricted terminal directory is unavailable")?;
        anyhow::ensure!(canonical == path && canonical.is_dir(),
            "restricted terminal directory must be canonical and must not contain symlink aliases");
        Ok(canonical)
    }

    fn private_directory(path: &Path) -> Result<PathBuf> {
        let canonical = canonical_directory(path)?;
        let meta = fs::symlink_metadata(path)?;
        anyhow::ensure!(meta.uid() == uid() && meta.mode() & 0o777 == 0o700,
            "restricted terminal state must be owned by the current user with mode 0700");
        Ok(canonical)
    }

    fn create_private_directory(path: &Path) -> Result<PathBuf> {
        DirBuilder::new().mode(0o700).create(path)
            .context("create new private restricted terminal directory")?;
        private_directory(path)
    }

    fn private_file(path: &Path) -> Result<File> {
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path).context("open private restricted terminal file")?;
        let meta = file.metadata()?;
        anyhow::ensure!(meta.is_file() && meta.uid() == uid() && meta.nlink() == 1
            && meta.mode() & 0o777 == 0o600 && meta.len() <= CONFIG_LIMIT,
            "restricted terminal file must be a bounded, private, unlinked regular file");
        Ok(file)
    }

    fn private_read(path: &Path) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        private_file(path)?.take(CONFIG_LIMIT + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 <= CONFIG_LIMIT, "restricted terminal file exceeds limit");
        Ok(bytes)
    }

    fn private_write_new(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW).open(path)
            .context("create new private restricted terminal file")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    fn outside(path: &Path, work: &Path, home: &Path) -> bool {
        !path.starts_with(work) && !path.starts_with(home)
    }

    pub(super) fn validate_launch(desc: &Descriptor) -> Result<()> {
        anyhow::ensure!(desc.spawn.program == "codex"
            && desc.spawn.adapter.as_deref() == Some(crate::terminal_sandbox::ADAPTER),
            "restricted controller requires a Codex terminal");
        anyhow::ensure!(desc.spawn.args.is_empty(), "restricted terminal rejects custom command-line configuration");
        anyhow::ensure!(desc.spawn.env.keys().all(|key| matches!(key.as_str(), "TERM" | "PATH")),
            "restricted terminal launch environment contains unsupported overrides");
        let Policy::Restricted { read_only, writable, network, home: Some(home) } = &desc.spawn.sandbox
            else { anyhow::bail!("restricted terminal requires a fixed restricted policy and private HOME"); };
        anyhow::ensure!(*network == Network::Denied && read_only.is_empty() && writable.is_empty(),
            "restricted terminal requires denied tool networking and no extra filesystem roots");
        let work = canonical_directory(Path::new(desc.spawn.cwd.as_deref().context("terminal workspace missing")?))?;
        let state = private_directory(&desc.directory)?;
        anyhow::ensure!(*home == state.join("tool-home"), "restricted terminal HOME must belong to its session");
        let parent = state.parent().context("restricted terminal state parent missing")?;
        anyhow::ensure!(outside(&state, &work, home) && !parent.starts_with(&work)
            && !work.starts_with(&state), "restricted terminal control state must stay outside its writable workspace");
        if home.try_exists()? { private_directory(home)?; }
        let runtime = std::env::current_exe()?.canonicalize()?;
        anyhow::ensure!(outside(&runtime, &work, home),
            "restricted terminal runtime must stay outside writable directories");
        let python = fs::metadata("/usr/bin/python3").context("restricted terminal requires /usr/bin/python3")?;
        anyhow::ensure!(python.is_file() && python.mode() & 0o111 != 0,
            "restricted terminal requires executable /usr/bin/python3");
        // Check broad/system work roots even before the child HOME is created.
        // This substitute is used only for validation, never for execution.
        let validation_policy = Policy::Restricted {
            read_only: vec![], writable: vec![], network: Network::Denied,
            home: Some(if home.is_dir() { home.clone() } else { state }),
        };
        crate::restricted::validate(&validation_policy, work.to_str().context("workspace UTF-8")?)?;
        Ok(())
    }

    fn source_home() -> Result<PathBuf> {
        Ok(match std::env::var_os("CODEX_HOME") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(std::env::var_os("HOME").context("model configuration HOME is unavailable")?).join(".codex"),
        })
    }

    fn source_configuration(work: &Path) -> Result<String> {
        let source_home = source_home()?;
        let path = source_home.join("config.toml");
        if !path.try_exists()? { return Ok(String::new()); }
        anyhow::ensure!(!source_home.canonicalize()?.starts_with(work),
            "model configuration must stay outside the writable workspace");
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path).context("read selected model configuration")?;
        let meta = file.metadata()?;
        anyhow::ensure!(meta.is_file() && meta.uid() == uid() && meta.nlink() == 1
            && meta.len() <= CONFIG_LIMIT, "model config must be an owned bounded regular file");
        let mut source = String::new();
        file.take(CONFIG_LIMIT + 1).read_to_string(&mut source)?;
        anyhow::ensure!(source.len() as u64 <= CONFIG_LIMIT, "model configuration exceeds limit");
        Ok(source)
    }

    fn chatgpt_auth_source(source: &Path, work: &Path, home: &Path) -> Result<Option<PathBuf>> {
        let path = source.join("auth.json");
        if !path.try_exists()? { return Ok(None); }
        let source = source.canonicalize().context("ChatGPT login directory is unavailable")?;
        anyhow::ensure!(outside(&source, work, home),
            "ChatGPT login must stay outside writable tool directories");
        let directory = fs::metadata(&source)?;
        anyhow::ensure!(directory.is_dir() && directory.uid() == uid()
            && directory.mode() & 0o022 == 0,
            "ChatGPT login directory must be owned and not writable by other users");
        let path = source.join("auth.json");
        // Inspect metadata only. The trusted bridge reads the access token
        // after initializing app-server and never copies the credential file.
        let file = private_file(&path).context("ChatGPT login file must be private (0600)")?;
        anyhow::ensure!(file.metadata()?.len() <= 65536, "ChatGPT login file exceeds limit");
        Ok(Some(path))
    }

    pub(super) fn prepare(desc: &Descriptor) -> Result<Prepared> {
        validate_launch(desc)?;
        let work = Path::new(desc.spawn.cwd.as_deref().context("terminal workspace missing")?);
        let home = desc.directory.join("tool-home");
        if !home.try_exists()? { create_private_directory(&home)?; }
        crate::restricted::validate(&desc.spawn.sandbox, work.to_str().context("workspace UTF-8")?)?;
        crate::sandbox::ensure_available(&desc.spawn.sandbox)?;
        let source = source_configuration(work)?;
        let (configuration, variables) = selected_model_config(&source)?;
        let chatgpt_auth_source = if desc.spawn.adapter.as_deref() == Some("codex-restricted-terminal")
            && uses_builtin_chatgpt_provider(&source)? {
            chatgpt_auth_source(&source_home()?, work, &home)?
        } else { None };
        let mut configuration: toml::Table = toml::from_str(&configuration)?;
        // app-server otherwise auto-trusts writable/External projects at
        // thread/start, then reloads their executable configuration. Explicit
        // untrusted state prevents that path; it does not change tool roots.
        let mut project = toml::Table::new();
        project.insert("trust_level".into(), toml::Value::String("untrusted".into()));
        let mut projects = toml::Table::new();
        projects.insert(work.to_string_lossy().into_owned(), toml::Value::Table(project));
        if desc.spawn.adapter.as_deref() == Some("codex-restricted-terminal") {
            projects.insert(desc.directory.join("controller").to_string_lossy().into_owned(),
                toml::Value::Table(toml::Table::from_iter([
                    ("trust_level".into(), toml::Value::String("untrusted".into())),
                ])));
        }
        configuration.insert("projects".into(), toml::Value::Table(projects));
        let configuration = toml::to_string(&configuration)?;
        let controller = desc.directory.join("controller");
        let resume_thread = if controller.try_exists()? {
            Some(crate::terminal_recovery::thread(desc)
                .context("无法恢复此受限会话；不会切换为本机执行")?)
        } else { None };
        let directory = |path: &Path| if resume_thread.is_some() {
            private_directory(path)
        } else { create_private_directory(path) };
        directory(&controller)?;
        let controller_home = directory(&controller.join("home"))?;
        let codex_home = directory(&controller.join("codex"))?;
        let temporary = directory(&controller.join("tmp"))?;
        // Authentication is always the explicit in-memory path. A stray store
        // must not become a second identity during a resumed model request.
        anyhow::ensure!(std::fs::symlink_metadata(codex_home.join("auth.json"))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
            "受限模型控制目录中存在未授权的登录文件");
        let fixed_file = |path: &Path, bytes: &[u8]| -> Result<()> {
            if resume_thread.is_some() {
                anyhow::ensure!(private_read(path)? == bytes,
                    "恢复配置或执行器已经变化，请新建受限终端");
                Ok(())
            } else { private_write_new(path, bytes) }
        };
        let script = desc.directory.join("exec-server.py");
        if script.try_exists()? {
            anyhow::ensure!(private_read(&script)? == ADAPTER.as_bytes(), "restricted adapter content has changed");
        } else { private_write_new(&script, ADAPTER.as_bytes())?; }
        let runtime = std::env::current_exe()?.canonicalize()?;
        let config_path = desc.directory.join("exec-server.json");
        fixed_file(&config_path, &serde_json::to_vec(&json!({
            "runtime":runtime,"work":work,"home":home,"read_only":[],
        }))?)?;
        fixed_file(&codex_home.join("config.toml"), configuration.as_bytes())?;
        let environments = format!("include_local = false\ndefault = {id}\n[[environments]]\nid = {id}\nprogram = {runtime}\nargs = {args}\ncwd = {cwd}\n",
            id = json!(ENVIRONMENT_ID), runtime = json!(runtime),
            args = json!(["restricted-exec-server", config_path]), cwd = json!(work));
        fixed_file(&codex_home.join("environments.toml"), environments.as_bytes())?;
        let mut env = HashMap::from([
            ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
            ("LANG".into(), "C.UTF-8".into()), ("TERM".into(), "xterm-256color".into()),
            ("SHELL".into(), "/bin/bash".into()),
            ("HOME".into(), controller_home.to_string_lossy().into_owned()),
            ("CODEX_HOME".into(), codex_home.to_string_lossy().into_owned()),
            ("TMPDIR".into(), temporary.to_string_lossy().into_owned()),
        ]);
        env.extend(controller_network_environment(|name| std::env::var_os(name))?);
        for name in variables {
            if let Some(value) = std::env::var_os(&name) {
                let value = value.into_string().map_err(|_| anyhow::anyhow!("provider variable {name} must be UTF-8"))?;
                anyhow::ensure!(value.len() <= 65536 && !value.contains('\0'),
                    "provider variable {name} is invalid");
                env.insert(name, value);
            }
        }
        Ok(Prepared { configuration, cwd: controller, env, chatgpt_auth_source, resume_thread })
    }

    pub(super) fn run_exec_server(config_path: &Path) -> Result<()> {
        anyhow::ensure!(config_path.file_name().is_some_and(|name| name == "exec-server.json")
            && config_path.canonicalize()? == config_path, "invalid restricted exec-server configuration path");
        let state = private_directory(config_path.parent().context("executor state missing")?)?;
        let config: Value = serde_json::from_slice(&private_read(config_path)?)?;
        let config_object = config.as_object().context("executor configuration must be an object")?;
        anyhow::ensure!(config_object.len() == 4 && ["runtime", "work", "home", "read_only"]
            .iter().all(|key| config_object.contains_key(*key)), "unexpected executor configuration");
        let runtime = Path::new(config["runtime"].as_str().context("executor runtime missing")?);
        let work = canonical_directory(Path::new(config["work"].as_str().context("executor work missing")?))?;
        let home = private_directory(Path::new(config["home"].as_str().context("executor HOME missing")?))?;
        anyhow::ensure!(home == state.join("tool-home") && outside(&state, &work, &home)
            && !work.starts_with(&state)
            && outside(runtime, &work, &home) && runtime == std::env::current_exe()?.canonicalize()?
            && config["read_only"].as_array().is_some_and(|roots| roots.is_empty()),
            "restricted exec-server policy differs from its fixed session");
        crate::restricted::validate(&Policy::Restricted {
            read_only: vec![], writable: vec![], network: Network::Denied, home: Some(home),
        }, work.to_str().context("executor workspace UTF-8")?)?;
        let script = state.join("exec-server.py");
        anyhow::ensure!(private_read(&script)? == ADAPTER.as_bytes(), "restricted adapter content has changed");
        let process_stat = fs::read_to_string("/proc/self/stat")?;
        let start = process_stat.rsplit_once(") ").context("process identity unavailable")?.1
            .split_whitespace().nth(19).context("process start time unavailable")?;
        anyhow::ensure!(start.bytes().all(|byte| byte.is_ascii_digit()), "invalid process identity");
        private_write_new(&state.join("exec-server.pid.json"),
            &serde_json::to_vec(&json!({"pid":std::process::id(),"start":start}))?)?;
        let error = Command::new("/usr/bin/python3").arg("-I").arg(script)
            .arg("--config").arg(config_path).current_dir(work).env_clear()
            .env("PATH", "/usr/bin:/bin").env("LANG", "C.UTF-8").exec();
        Err(error).context("start restricted exec-server adapter")
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::sessions::SpawnRequest;
        use std::os::unix::fs::symlink;

        struct Fixture(PathBuf);
        impl Fixture {
            fn new() -> Self {
                let root = std::env::temp_dir().join(format!("vl-terminal-policy-{}", uuid::Uuid::now_v7()));
                create_private_directory(&root).unwrap();
                Self(root)
            }
            fn descriptor(&self) -> Descriptor {
                let work = create_private_directory(&self.0.join("work")).unwrap();
                let control = create_private_directory(&self.0.join("control")).unwrap();
                let state = create_private_directory(&control.join("session")).unwrap();
                let home = create_private_directory(&state.join("tool-home")).unwrap();
                Descriptor { id: uuid::Uuid::now_v7().to_string(), endpoint: "unused".into(),
                    directory: state, spawn: SpawnRequest {
                        sandbox: Policy::Restricted { read_only: vec![], writable: vec![],
                            network: Network::Denied, home: Some(home) },
                        program: "codex".into(), args: vec![], cwd: Some(work.to_string_lossy().into_owned()),
                        env: HashMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
                        adapter: Some(crate::terminal_sandbox::ADAPTER.into()),
                    }}
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
        }

        #[test]
        fn launch_rejects_overrides_and_control_state_inside_work() {
            let fixture = Fixture::new();
            let desc = fixture.descriptor();
            validate_launch(&desc).unwrap();
            let mut bad = desc.clone();
            bad.spawn.args = vec!["-c".into(), "features.hooks=true".into()];
            assert!(validate_launch(&bad).is_err());
            bad = desc.clone();
            bad.spawn.env.insert("CODEX_HOME".into(), "/tmp".into());
            assert!(validate_launch(&bad).is_err());
            bad = desc.clone();
            bad.spawn.sandbox = Policy::Native {};
            assert!(validate_launch(&bad).is_err());
            bad = desc.clone();
            let Policy::Restricted { read_only, .. } = &mut bad.spawn.sandbox else { unreachable!() };
            read_only.push(fixture.0.join("control"));
            assert!(validate_launch(&bad).is_err());
            bad = desc.clone();
            bad.spawn.cwd = Some(fixture.0.to_string_lossy().into_owned());
            assert!(validate_launch(&bad).is_err());
            let alias = fixture.0.join("state-link");
            symlink(&desc.directory, &alias).unwrap();
            bad = desc;
            bad.directory = alias;
            assert!(validate_launch(&bad).is_err());
        }

        #[test]
        fn private_state_writes_never_follow_or_replace_links() {
            let fixture = Fixture::new();
            let original = fixture.0.join("original");
            private_write_new(&original, b"unchanged").unwrap();
            let symlink_path = fixture.0.join("symlink");
            symlink(&original, &symlink_path).unwrap();
            assert!(private_write_new(&symlink_path, b"changed").is_err());
            assert!(private_read(&symlink_path).is_err());
            let hardlink = fixture.0.join("hardlink");
            fs::hard_link(&original, &hardlink).unwrap();
            assert!(private_write_new(&hardlink, b"changed").is_err());
            assert!(private_read(&hardlink).is_err());
            assert_eq!(fs::read(original).unwrap(), b"unchanged");
        }

        #[test]
        fn chatgpt_source_stays_private_outside_tool_roots_without_copying() {
            let fixture = Fixture::new();
            let desc = fixture.descriptor();
            let work = Path::new(desc.spawn.cwd.as_deref().unwrap());
            let home = desc.directory.join("tool-home");
            let source = create_private_directory(&fixture.0.join("source")).unwrap();
            assert!(chatgpt_auth_source(&source, work, &home).unwrap().is_none());
            let auth = source.join("auth.json");
            private_write_new(&auth, b"synthetic metadata-only credential").unwrap();
            assert_eq!(chatgpt_auth_source(&source, work, &home).unwrap(), Some(auth.clone()));
            assert!(!desc.directory.join("controller/codex/auth.json").exists());
            private_write_new(&work.join("auth.json"), b"untrusted").unwrap();
            assert!(chatgpt_auth_source(work, work, &home).is_err());
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&auth, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(chatgpt_auth_source(&source, work, &home).is_err());
            fs::set_permissions(&auth, fs::Permissions::from_mode(0o600)).unwrap();
            fs::rename(&auth, source.join("original")).unwrap();
            symlink(source.join("original"), &auth).unwrap();
            assert!(chatgpt_auth_source(&source, work, &home).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_provider_does_not_import_tools_commands_or_other_providers() {
        let source = r#"
model = "fixture-model"
model_provider = "fixture"
model_reasoning_effort = "low"
notify = ["/bin/false"]
profile = "danger"
[features]
hooks = true
plugins = true
skip_host_skill_discovery = false
[mcp_servers.attack]
command = "/bin/false"
[profiles.danger]
model = "wrong"
[model_providers.unselected.auth]
command = "/bin/false"
[model_providers.fixture]
name = "fixture"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
env_key = "FIXTURE_API_KEY"
requires_openai_auth = false
request_max_retries = 0
[model_providers.fixture.env_http_headers]
X-Test = "FIXTURE_HEADER_TOKEN"
"#;
        let (config, env) = selected_model_config(source).unwrap();
        let value: toml::Table = toml::from_str(&config).unwrap();
        assert_eq!(value["model"].as_str(), Some("fixture-model"));
        assert_eq!(value["features"]["hooks"].as_bool(), Some(false));
        assert_eq!(value["features"]["skip_host_skill_discovery"].as_bool(), Some(true));
        assert!(!value.contains_key("mcp_servers") && !value.contains_key("notify") && !value.contains_key("profiles"));
        assert_eq!(value["model_providers"].as_table().unwrap().len(), 1);
        assert_eq!(env, HashSet::from(["FIXTURE_API_KEY".into(), "FIXTURE_HEADER_TOKEN".into()]));
    }

    #[test]
    fn provider_cannot_import_executable_auth_or_process_control_variables() {
        for field in ["auth", "aws", "gateway_oauth", "experimental_bearer_token"] {
            let source = format!("model_provider='fixture'\n[model_providers.fixture]\nname='fixture'\n{field}='rejected'\n");
            assert!(selected_model_config(&source).is_err(), "{field}");
        }
        for name in ["LD_PRELOAD", "PYTHONPATH", "HOME", "PATH", "CODEX_HOME",
            "BASH_ENV", "NODE_OPTIONS", "GIT_CONFIG_COUNT", "OPENAI_BASE_URL", "HTTP_PROXY"] {
            let source = format!("model_provider='fixture'\n[model_providers.fixture]\nname='fixture'\nenv_key='{name}'\n");
            assert!(selected_model_config(&source).is_err(), "{name}");
        }
        assert!(provider_environment_name("OPENAI_API_KEY").is_ok());
        assert!(provider_environment_name("ACCESS_TOKEN").is_ok());
        assert!(selected_model_config("model_provider='missing'").is_err());
    }

    #[test]
    fn host_chatgpt_login_cannot_follow_custom_provider_configuration() {
        assert!(uses_builtin_chatgpt_provider("model='test'").unwrap());
        assert!(uses_builtin_chatgpt_provider("model_provider='openai'").unwrap());
        for config in [
            "model_provider='fixture'",
            "[model_providers.openai]\nbase_url='http://127.0.0.1:9'",
            "[model_providers.openai]\nname='openai'",
            "[model_providers.openai]\nenv_key='OTHER_KEY'",
        ] {
            assert!(!uses_builtin_chatgpt_provider(config).unwrap());
        }
    }

    #[test]
    fn model_proxy_settings_are_bounded_and_only_from_the_host_allowlist() {
        let source = HashMap::from([
            ("HTTPS_PROXY", "http://127.0.0.1:9999"),
            ("no_proxy", "localhost,127.0.0.1"),
            ("LD_PRELOAD", "/untrusted.so"),
            ("OPENAI_API_KEY", "synthetic-secret"),
        ]);
        let result = controller_network_environment(|name| source.get(name).map(Into::into)).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["HTTPS_PROXY"], source["HTTPS_PROXY"]);
        assert_eq!(result["no_proxy"], source["no_proxy"]);
        for invalid in ["http://proxy\ninjected", "http://proxy\0suffix"] {
            let error = controller_network_environment(|name|
                (name == "HTTPS_PROXY").then(|| invalid.into())).unwrap_err();
            assert!(!error.to_string().contains(invalid));
        }
    }
}
