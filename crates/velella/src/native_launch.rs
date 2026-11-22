//! Shared, invocation-local native CLI options. These requests never change
//! permissions, sandboxing, hooks, or role policy, and do not verify models.

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use velella_core::task::LaunchOptions;

const HELP_TIMEOUT: Duration = Duration::from_secs(5);
const HELP_MAX_BYTES: usize = 64 * 1024;

/// Explicit Skill Center launches on other CLIs use a declared prompt option.
/// No prompt text enters a shell. These launches are never replayed on recovery.
pub(crate) fn skill_prompt_args(agent:&str,text:&str)->Result<Vec<String>> {
    anyhow::ensure!(!text.trim().is_empty()&&text.len()<=16000&&text.chars().all(|c|!c.is_control()||matches!(c,'\n'|'\t'|'\r')),"skill prompt exceeds native argv bounds or contains controls");
    let (program,flag)=match agent {"gemini"=>("gemini","--prompt-interactive"),"opencode"=>("opencode","--prompt"),"kimi"=>("kimi","--prompt"),"omp"=>("omp","--prompt"),"zcode"=>("zcode-free","--prompt"),_=>anyhow::bail!("unsupported skill prompt Agent")};
    let help=permission_probe(program,false,false,None)?;
    if agent=="omp" && help.lines().any(|line|line.trim_start().starts_with("MESSAGES ")) {
        anyhow::ensure!(!text.starts_with('-'),"positional prompt cannot start with a flag");
        return Ok(vec![text.into()]);
    }
    anyhow::ensure!(declares_flag(&help,flag),"this CLI has no verified interactive prompt flag; use native skill discovery or adapt the launcher before claiming compatibility");
    Ok(vec![format!("{flag}={text}")])
}

/// Validate options against a fresh, successful help invocation on inherited PATH.
/// Codex resume probes `codex resume --help`; fresh Codex and all Claude launches
/// probe top-level help. Empty options never spawn. This proves flag declarations,
/// not model availability, authentication, or model/effort compatibility.
pub fn validated_args(agent: &str, launch: &LaunchOptions, resume: bool) -> Result<Vec<String>> {
    validated_args_for_program(agent, launch, provider_program(agent), resume, None)
}

/// As `validated_args`, using the launcher's actual program and optional PATH.
/// Nonempty options fail closed for custom programs (including shell wrappers):
/// another executable's help cannot validate their invocation. Only bare `codex`
/// and `claude` provider programs are supported. No launch values enter a shell.
pub fn validated_args_for_program(
    agent: &str,
    launch: &LaunchOptions,
    program: &str,
    resume: bool,
    path: Option<&OsStr>,
) -> Result<Vec<String>> {
    validate_with_probe(agent, launch, || {
        anyhow::ensure!(program == provider_program(agent),
            "cannot validate launch options for custom adapter program `{program}`; expected `{}`", provider_program(agent));
        let mut command = help_command(agent, resume);
        if let Some(path) = path {
            command.env("PATH", path);
        }
        probe_help(&mut command, HELP_TIMEOUT, HELP_MAX_BYTES)
            .with_context(|| format!("target `{program}` CLI help probe failed"))
    })
}

fn provider_program(agent: &str) -> &str {
    match agent {
        "codex" => "codex",
        "claude-code" => "claude",
        _ => "", // Nonempty unsupported requests are rejected by args first.
    }
}

/// Invocation-local permission probes. All shell text is fixed, never caller input.
pub(crate) fn permission_probe(program: &str, resume: bool, version: bool, path: Option<&OsStr>) -> Result<String> {
    let invocation = match (program, resume, version) {
        ("codex", true, false) => "codex resume --help",
        ("codex", false, false) => "codex --help",
        ("claude", _, false) => "claude --help",
        ("mcode", _, false) => "mcode --help",
        ("gemini", _, false) => "gemini --help",
        ("opencode", _, false) => "opencode --help",
        ("opencode", _, true) => "opencode --version",
        ("omp", _, false) => "omp --help",
        ("kimi", _, false) => "kimi --help",
        ("zcode-free", _, false) => "zcode-free --help",
        _ => anyhow::bail!("unsupported permission probe program"),
    };
    #[cfg(windows)]
    let mut command = { let mut cmd = crate::process::command("cmd.exe"); cmd.args(["/D", "/S", "/C", invocation]); cmd };
    #[cfg(not(windows))]
    let mut command = { let mut cmd = crate::process::command(program); cmd.args(invocation.split_whitespace().skip(1)); cmd };
    if let Some(path) = path { command.env("PATH", path); }
    probe_help(&mut command, HELP_TIMEOUT, HELP_MAX_BYTES)
        .with_context(|| format!("无法验证 `{program}` 权限模式：CLI 帮助检查失败"))
}

fn validate_with_probe(
    agent: &str,
    launch: &LaunchOptions,
    probe: impl FnOnce() -> Result<String>,
) -> Result<Vec<String>> {
    let argv = args(agent, launch)?;
    if !launch.is_empty() {
        check_capabilities(agent, launch, &probe()?)?;
    }
    Ok(argv)
}

fn help_command(agent: &str, resume: bool) -> Command {
    #[cfg(windows)]
    {
        // npm .cmd shims require cmd. These are fixed literals, never a program,
        // model, effort, session ID, or other caller-provided shell fragment.
        let invocation = match (agent, resume) {
            ("codex", true) => "codex resume --help",
            ("codex", false) => "codex --help",
            ("claude-code", _) => "claude --help",
            _ => unreachable!("options validated before command construction"),
        };
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/S", "/C", invocation]);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new(provider_program(agent));
        if agent == "codex" && resume {
            command.arg("resume");
        }
        command.arg("--help");
        command
    }
}

/// No reader threads or unbounded output files: a descendant retaining a pipe
/// cannot keep this call alive beyond its deadline. Only our probe child is killed
/// and reaped; never use process-name kills, process groups, or service controls.
fn probe_help(command: &mut Command, timeout: Duration, max_bytes: usize) -> Result<String> {
    let output=bounded_probe(command,timeout,max_bytes)?;
    anyhow::ensure!(output.0.success(),"CLI help probe exited unsuccessfully ({})",output.0);
    Ok(format!("{}\n{}",output.1,output.2))
}

/// Fixed provider diagnostics share the bounded reader, but auth exit 1 remains
/// a signed-out result rather than leaking provider output in an error.
pub(crate) fn account_diagnostic(program:&str,args:&[&str],home:Option<&str>)->Result<(bool,String)> {
    anyhow::ensure!(matches!(program,"codex"|"claude"|"mcode"),"unsupported account provider");
    anyhow::ensure!(program!="mcode"||home.is_none(),"MiniMax diagnostics inherit the native data directory");
    #[cfg(windows)]
    let mut command={let mut command=crate::process::command("cmd");command.args(["/d","/c",program]);command};
    #[cfg(not(windows))]
    let mut command=crate::process::command(program);
    command.args(args);
    if let Some(home)=home{command.env(if program=="codex"{"CODEX_HOME"}else{"CLAUDE_CONFIG_DIR"},home);}
    let output=bounded_probe(&mut command,HELP_TIMEOUT,HELP_MAX_BYTES)?;
    Ok((output.0.success(),output.1))
}

fn bounded_probe(command:&mut Command,timeout:Duration,max_bytes:usize)->Result<(std::process::ExitStatus,String,String)> {
    let deadline = Instant::now() + timeout;
    command.stdin(Stdio::null());
    #[cfg(unix)]
    let (mut stdout, mut stderr) = {
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;
        // Socket-backed stdio supplies portable nonblocking reads without adding
        // platform dependencies. The child ends remain blocking.
        let (stdout, stdout_writer) = UnixStream::pair()?;
        let (stderr, stderr_writer) = UnixStream::pair()?;
        stdout.set_nonblocking(true)?;
        stderr.set_nonblocking(true)?;
        command.stdout(Stdio::from(OwnedFd::from(stdout_writer)));
        command.stderr(Stdio::from(OwnedFd::from(stderr_writer)));
        (stdout, stderr)
    };
    #[cfg(windows)]
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let spawned = command.spawn();
    // Command owns the Unix writer ends too: release them so EOF is observable.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = ProbeChild(spawned.context("start help probe")?);
    #[cfg(windows)]
    let (mut stdout, mut stderr) = (
        child.0.stdout.take().context("help stdout pipe missing")?,
        child.0.stderr.take().context("help stderr pipe missing")?,
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_eof, mut err_eof) = (false, false);
    let mut total = 0;
    loop {
        anyhow::ensure!(Instant::now() < deadline, "CLI help probe timed out");
        let before = total;
        if !out_eof {
            out_eof = read_help_chunk(&mut stdout, &mut out, &mut total, max_bytes)?;
        }
        if !err_eof {
            err_eof = read_help_chunk(&mut stderr, &mut err, &mut total, max_bytes)?;
        }
        if let Some(status) = child.0.try_wait().context("poll help probe")? {
            if out_eof && err_eof {
                // Keep independent streams separated so partial lines cannot
                // combine into a flag declaration that neither stream emitted.
                return Ok((status,String::from_utf8_lossy(&out).into_owned(),String::from_utf8_lossy(&err).into_owned()));
            }
        }
        if total == before {
            std::thread::sleep(Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())));
        }
    }
}

struct ProbeChild(Child);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // A successful kill permits a wait without waiting for natural exit.
            if self.0.kill().is_ok() {
                let _ = self.0.wait();
            }
        }
    }
}

trait ProbeRead {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
}
#[cfg(unix)]
impl ProbeRead for std::os::unix::net::UnixStream {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.read(buffer)
    }
}
#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle> ProbeRead for T {
    fn read_ready(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut available = 0;
        // We are the sole reader; reading no more than the peeked bytes cannot
        // block. Broken pipe is EOF; a connected empty pipe is WouldBlock.
        let ok = unsafe {
            windows_sys::Win32::System::Pipes::PeekNamedPipe(
                self.as_raw_handle(), std::ptr::null_mut(), 0,
                std::ptr::null_mut(), &mut available, std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::BrokenPipe { Ok(0) } else { Err(error) };
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let len = buffer.len().min(available as usize);
        self.read(&mut buffer[..len])
    }
}

fn read_help_chunk(
    reader: &mut impl ProbeRead,
    output: &mut Vec<u8>,
    total: &mut usize,
    max_bytes: usize,
) -> Result<bool> {
    let mut buffer = [0; 4096];
    match reader.read_ready(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(n) => {
            anyhow::ensure!(n <= max_bytes.saturating_sub(*total),
                "CLI help output exceeded {max_bytes} bytes (truncated); capability evidence rejected");
            *total += n;
            output.extend_from_slice(&buffer[..n]);
            Ok(false)
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => Ok(false),
        Err(error) => Err(error).context("read CLI help"),
    }
}

/// Build independent argv entries, to append before any positional prompt.
/// Empty options inherit native defaults, including for other adapters.
/// Model names are trimmed but otherwise opaque; availability is CLI-owned.
pub fn args(agent: &str, launch: &LaunchOptions) -> Result<Vec<String>> {
    if launch.is_empty() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        matches!(agent, "codex" | "claude-code"),
        "launch options are not supported for agent `{agent}`"
    );
    let mut args = Vec::new();
    if let Some(model) = &launch.model {
        anyhow::ensure!(model.len() <= 200, "launch.model must be at most 200 UTF-8 bytes");
        anyhow::ensure!(
            !model.chars().any(char::is_control),
            "launch.model must not contain control characters"
        );
        let model = model.trim();
        anyhow::ensure!(!model.is_empty(), "launch.model must not be empty");
        anyhow::ensure!(!model.starts_with('-'), "launch.model must not start with '-'");
        args.extend(["--model".into(), model.to_owned()]);
    }
    if let Some(effort) = &launch.effort {
        if agent == "codex" {
            anyhow::ensure!(
                matches!(effort.as_str(), "none" | "minimal" | "low" | "medium" | "high" | "xhigh"),
                "Codex launch.effort must be one of none/minimal/low/medium/high/xhigh"
            );
            args.extend([
                "-c".into(),
                format!("model_reasoning_effort={}", toml::Value::String(effort.clone())),
            ]);
        } else {
            anyhow::ensure!(
                matches!(effort.as_str(), "low" | "medium" | "high" | "xhigh" | "max"),
                "Claude launch.effort must be one of low/medium/high/xhigh/max"
            );
            args.extend(["--effort".into(), effort.clone()]);
        }
    }
    Ok(args)
}

/// Check only the requested flags against plain help from the target CLI.
/// The caller must supply a successful help probe from the actual executable
/// and invocation mode (e.g. `codex resume --help` for resume), not another
/// installation or a static support list. This helper does not spawn processes.
/// Flag declarations do NOT verify a model, account access, effort semantics,
/// or a model/effort combination. Empty options need no capability evidence.
pub fn check_capabilities(agent: &str, launch: &LaunchOptions, help: &str) -> Result<()> {
    args(agent, launch)?;
    if launch.model.is_some() {
        anyhow::ensure!(
            declares_flag(help, "--model"),
            "target `{agent}` CLI help does not advertise --model; model availability remains unverified"
        );
    }
    if launch.effort.is_some() {
        let flag = if agent == "codex" { "-c" } else { "--effort" };
        anyhow::ensure!(
            declares_flag(help, flag),
            "target `{agent}` CLI help does not advertise {flag} for launch.effort"
        );
    }
    Ok(())
}

pub(crate) fn declares_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        let line = line.trim_start();
        if !line.starts_with('-') {
            return false;
        }
        // Inspect option declarations, not descriptions mentioning other flags.
        let declaration = line.split("  ").next().unwrap_or(line);
        declaration
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|token| !token.is_empty())
            .take_while(|token| token.starts_with('-'))
            .any(|token| token.split(['=', '<', '[']).next() == Some(flag))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(model: Option<&str>, effort: Option<&str>) -> LaunchOptions {
        LaunchOptions { model: model.map(str::to_owned), effort: effort.map(str::to_owned) }
    }

    fn fixture_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "native_launch::tests::help_probe_fixture", "--nocapture"]);
        command.env("VELELLA_HELP_PROBE_FIXTURE", mode);
        command
    }

    // A subprocess of this test executable, never an installed Agent or auth CLI.
    #[test]
    fn help_probe_fixture() {
        use std::io::Write;
        let Ok(mode) = std::env::var("VELELLA_HELP_PROBE_FIXTURE") else { return };
        let help = b"\n  --model <MODEL>\n  -c <CONFIG>\n  --effort <LEVEL>\n";
        match mode.as_str() {
            "success" => std::io::stdout().write_all(help).unwrap(),
            "stderr" => std::io::stderr().write_all(help).unwrap(),
            "missing" => std::io::stdout().write_all(b"\n  --help  Mentions --model and --effort\n").unwrap(),
            "nonzero" => {
                std::io::stdout().write_all(help).unwrap();
                std::process::exit(7);
            }
            "timeout" => std::thread::sleep(Duration::from_secs(30)),
            "stdout_limit" | "stderr_limit" | "combined_limit" => {
                // Even valid declarations before truncation are not evidence.
                std::io::stdout().write_all(help).unwrap();
                let bytes = [b'x'; 2048];
                if mode != "stderr_limit" { std::io::stdout().write_all(&bytes).unwrap(); }
                if mode != "stdout_limit" { std::io::stderr().write_all(&bytes).unwrap(); }
            }
            _ => panic!("unknown help fixture"),
        }
        std::process::exit(0);
    }

    #[test]
    fn help_probe_requires_successful_complete_flag_declarations() {
        let launch = options(Some("custom/model;$(literal)"), Some("high"));
        for agent in ["codex", "claude-code"] {
            for mode in ["success", "stderr"] {
                assert_eq!(validate_with_probe(agent, &launch, || {
                    probe_help(&mut fixture_command(mode), Duration::from_secs(3), 8192)
                }).unwrap(), args(agent, &launch).unwrap());
            }
            for (mode, expected) in [("missing", "does not advertise --model"), ("nonzero", "exited unsuccessfully")] {
                let error = validate_with_probe(agent, &launch, || {
                    probe_help(&mut fixture_command(mode), Duration::from_secs(3), 8192)
                }).unwrap_err();
                assert!(error.to_string().contains(expected), "{error:#}");
            }
        }
    }

    #[test]
    fn help_probe_bounds_stdout_stderr_and_combined_output() {
        for mode in ["stdout_limit", "stderr_limit", "combined_limit"] {
            let limit = if mode == "combined_limit" { 3000 } else { 1024 };
            let error = probe_help(&mut fixture_command(mode), Duration::from_secs(3), limit).unwrap_err();
            assert!(error.to_string().contains("truncated"), "{error:#}");
        }
    }

    #[test]
    fn help_probe_times_out_and_cleans_up_its_child() {
        let start = Instant::now();
        let error = probe_help(&mut fixture_command("timeout"), Duration::from_millis(100), 8192).unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error:#}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn empty_or_invalid_options_never_invoke_probe() {
        for agent in ["codex", "claude-code", "custom"] {
            assert!(validate_with_probe(agent, &LaunchOptions::default(), || panic!("must not probe")).unwrap().is_empty());
            assert!(validated_args(agent, &LaunchOptions::default(), true).unwrap().is_empty());
            assert!(validated_args_for_program(agent, &LaunchOptions::default(), "custom-program", false, None).unwrap().is_empty());
            assert!(validate_with_probe(agent, &options(Some("--invalid"), None), || panic!("must not probe")).is_err());
        }
        for (agent, program) in [("codex", "custom-codex"), ("codex", "/custom/codex"), ("claude-code", "codex"), ("codex", "cmd.exe")] {
            let error = validated_args_for_program(agent, &options(Some("custom/model"), None), program, false, None).unwrap_err();
            assert!(error.to_string().contains("custom adapter program"));
        }
    }

    #[test]
    fn help_invocations_use_only_known_provider_and_mode_arguments() {
        for (agent, resume) in [("codex", false), ("codex", true), ("claude-code", false), ("claude-code", true)] {
            let command = help_command(agent, resume);
            let argv: Vec<_> = command.get_args().map(|arg| arg.to_str().unwrap()).collect();
            #[cfg(windows)]
            {
                assert_eq!(command.get_program(), "cmd.exe");
                let fixed = match (agent, resume) {
                    ("codex", false) => "codex --help",
                    ("codex", true) => "codex resume --help",
                    _ => "claude --help",
                };
                assert_eq!(argv, ["/D", "/S", "/C", fixed]);
            }
            #[cfg(unix)]
            {
                assert_eq!(command.get_program(), provider_program(agent));
                assert_eq!(argv, if agent == "codex" && resume { vec!["resume", "--help"] } else { vec!["--help"] });
            }
        }
    }

    #[test]
    fn empty_launch_inherits_defaults_for_every_adapter() {
        for agent in ["codex", "claude-code", "gemini", "opencode", "omp", "kimi", "custom"] {
            assert!(args(agent, &LaunchOptions::default()).unwrap().is_empty());
            assert!(check_capabilities(agent, &LaunchOptions::default(), "").is_ok());
        }
    }

    #[test]
    fn codex_efforts_are_single_toml_assignments() {
        for effort in ["none", "minimal", "low", "medium", "high", "xhigh"] {
            let argv = args("codex", &options(Some("custom/model"), Some(effort))).unwrap();
            assert_eq!(argv, ["--model", "custom/model", "-c", &format!("model_reasoning_effort=\"{effort}\"")]);
            let config: toml::Value = toml::from_str(&argv[3]).unwrap();
            assert_eq!(config.as_table().unwrap().len(), 1);
            assert_eq!(config["model_reasoning_effort"].as_str(), Some(effort));
        }
        assert_eq!(args("codex", &options(None, Some("high"))).unwrap(), ["-c", "model_reasoning_effort=\"high\""]);
    }

    #[test]
    fn claude_efforts_are_independent_arguments() {
        for effort in ["low", "medium", "high", "xhigh", "max"] {
            assert_eq!(
                args("claude-code", &options(Some("custom/model"), Some(effort))).unwrap(),
                ["--model", "custom/model", "--effort", effort]
            );
            assert_eq!(args("claude-code", &options(None, Some(effort))).unwrap(), ["--effort", effort]);
        }
    }

    #[test]
    fn models_are_trimmed_opaque_single_arguments_without_policy_flags() {
        let model = "vendor/model:v2 + variant;$(literal)\"\\suffix";
        for agent in ["codex", "claude-code"] {
            assert_eq!(args(agent, &options(Some(&format!("  {model}  ")), None)).unwrap(), ["--model", model]);
            assert_eq!(args(agent, &options(Some("arbitrary-future-model"), None)).unwrap(), ["--model", "arbitrary-future-model"]);
            for model in ["m".repeat(200), "é".repeat(100)] {
                assert_eq!(args(agent, &options(Some(&model), None)).unwrap(), ["--model", model.as_str()]);
            }
        }
    }

    #[test]
    fn invalid_models_are_rejected_for_both_agents() {
        for agent in ["codex", "claude-code"] {
            for model in ["", "   ", "\u{2003}", "-model", "  --help", "a\nb", "a\rb", "a\0b", "a\tb", "a\u{7f}b", "a\u{85}b", "\tmodel"] {
                assert!(args(agent, &options(Some(model), None)).is_err(), "{agent}: {model:?}");
            }
            for model in ["m".repeat(201), "é".repeat(101)] {
                assert!(args(agent, &options(Some(&model), None)).is_err());
            }
        }
    }

    #[test]
    fn efforts_are_exact_and_provider_specific() {
        for agent in ["codex", "claude-code"] {
            for effort in ["", " ", "HIGH", " high ", "ultra", "--help", "high\n", "high\"\npermissions=\"all"] {
                assert!(args(agent, &options(None, Some(effort))).is_err(), "{agent}: {effort:?}");
            }
        }
        assert!(args("codex", &options(None, Some("max"))).is_err());
        for effort in ["none", "minimal"] {
            assert!(args("claude-code", &options(None, Some(effort))).is_err());
        }
    }

    #[test]
    fn unsupported_agents_cannot_silently_ignore_options() {
        for agent in ["gemini", "opencode", "omp", "kimi", "generic", "claude", "Codex", "custom", ""] {
            for launch in [options(Some("custom/model"), None), options(None, Some("high")), options(Some("custom/model"), Some("high"))] {
                assert!(args(agent, &launch).is_err(), "{agent}");
                assert!(check_capabilities(agent, &launch, "  --model <MODEL>\n  --effort <LEVEL>\n  -c <CONFIG>").is_err());
            }
        }
    }

    #[test]
    fn capability_checks_require_only_requested_flag_declarations() {
        let codex = "Options:\n  -m, --model <MODEL>  Model request\n  -c, --config <KEY=VALUE>  Config override\n";
        let claude = "Options:\n  --model <model>  Model request\n  --effort <level>  Effort request\n";
        for (agent, help) in [("codex", codex), ("claude-code", claude)] {
            assert!(check_capabilities(agent, &options(Some("not-a-verified-model"), Some("high")), help).is_ok());
            assert!(check_capabilities(agent, &options(Some("custom/model"), None), "  --model=MODEL  Request").is_ok());
            assert!(check_capabilities(agent, &options(Some("custom/model"), Some("high")), "  --model <MODEL>").is_err());
            assert!(check_capabilities(agent, &options(Some("custom/model"), None), "").is_err());
            assert!(check_capabilities(agent, &options(None, Some("invalid")), help).is_err());
            assert!(check_capabilities(agent, &options(Some("--help"), None), help).is_err());
        }
        assert!(check_capabilities("codex", &options(None, Some("high")), "  -c, --config <CONFIG>").is_ok());
        assert!(check_capabilities("codex", &options(None, Some("high")), "  --config <CONFIG>").is_err());
        assert!(check_capabilities("claude-code", &options(None, Some("max")), "  --effort <LEVEL>").is_ok());
    }

    #[test]
    fn capability_checks_do_not_match_similar_flags_or_prose() {
        for help in ["Use --model to choose a model", "  --model-cache <PATH>", "  --models <NAME>", "  --help  See --model for models", "  --other=--model"] {
            assert!(check_capabilities("codex", &options(Some("custom/model"), None), help).is_err(), "{help}");
        }
        assert!(check_capabilities("codex", &options(None, Some("high")), "  --config-only <CONFIG>").is_err());
        assert!(check_capabilities("claude-code", &options(None, Some("high")), "  --effort-default <LEVEL>").is_err());
    }
}
