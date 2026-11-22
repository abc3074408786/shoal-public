#!/usr/bin/env python3
"""Explicit real-provider acceptance for the native Restricted Codex terminal.

Uses an owned temporary Git worktree and the explicitly selected source
CODEX_HOME. This script never opens auth.json or copies source configuration.
Only sanitized receipts, fixed sandbox test output and fixture source are saved.
--browser exposes ready.json; verify.request asks the backend to run independent
sandbox verification after the real model finishes. cleanup.request ends the run.
After original verification, the browser can stop and resume the same terminal;
recovery-verify.request checks fresh model memory, tools and boundary evidence.
"""
import argparse
import json
import os
from pathlib import Path
import re
import runpy
import shlex
import signal
import stat
import subprocess
import tempfile
import time
import tomllib
import traceback
import uuid

REPOSITORY = Path(__file__).resolve().parents[1]
TERMINAL = runpy.run_path(str(REPOSITORY / "scripts/verify-desktop-restricted-terminal.py"))
REAL = runpy.run_path(str(REPOSITORY / "scripts/verify-real-sandbox-agent.py"))
HELPERS = TERMINAL["HELPERS"]
Base = TERMINAL["TerminalAcceptance"]
STOP, rpc, wait_for, digest = (TERMINAL[name] for name in ("STOP", "rpc", "wait_for", "digest"))
TESTS, ORIGINAL, GITIGNORE, VERIFY_COMMAND = (REAL[name] for name in (
    "TESTS", "ORIGINAL", "GITIGNORE", "VERIFY_COMMAND"))
PROXY_VARIABLES = (
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
    "http_proxy", "https_proxy", "all_proxy", "no_proxy",
)


def python_command(code):
    """Keep literal newlines out of raw PTY input; never execute code here."""
    return "/usr/bin/python3 -I -c " + shlex.quote("exec(" + repr(code) + ")")


def private_json(path, value):
    pending = path.with_name(path.name + ".tmp")
    pending.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    pending.chmod(0o600)
    pending.replace(path)


def bounded_text(path, maximum=65536):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > maximum:
        raise RuntimeError("Unexpected fixture evidence file")
    return path.read_text()


def model_selection(source):
    """Read only model/provider selections; never return the provider table."""
    path = source / "config.toml"
    if not path.exists():
        return None, "openai"
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_size > 1024 * 1024:
        raise RuntimeError("Source model configuration must be an owned bounded regular file")
    try:
        values = tomllib.loads(path.read_text())
    except Exception:
        raise RuntimeError("Cannot parse selected model configuration") from None
    model, provider = values.get("model"), values.get("model_provider", "openai")
    for value in (model, provider):
        if value is not None and (not isinstance(value, str) or not value or len(value) > 256
                                  or any(ord(character) < 32 for character in value)):
            raise RuntimeError("Invalid selected model or provider name")
    return model, provider


class RealTerminalAcceptance(Base):
    def __init__(self, runtime, cli, output, source, timeout):
        super().__init__(runtime, cli, output, REPOSITORY)
        self.source = source
        self.timeout = timeout
        self.model, self.provider = model_selection(source)
        self.nonce = uuid.uuid4().hex
        self.final_marker = "REAL_SANDBOX_MODEL_DONE_" + self.nonce
        self.verify_marker = "REAL_VERIFY_COMPLETE_" + self.nonce
        self.boundary_marker = "REAL_BOUNDARY_COMPLETE_" + self.nonce
        self.recovery_memory = "RECOVERY_MEMORY_" + uuid.uuid4().hex
        self.recovery_marker = "REAL_RECOVERY_MODEL_DONE"
        self.recovery_prompt = (
            "Continue this same conversation after its terminal was stopped and resumed. "
            "From the earlier conversation, reproduce the exact RECOVERY_MEMORY_ line printed by "
            "my most recent manual shell command. Do not guess it and do not search files for it. "
            "Then use exec_command to run rg to inspect clamp.py and run "
            "python3 -m unittest -v test_clamp.py again. Do not change any files, tests or configuration; "
            "do not install dependencies, read credentials, use browser tools, commit or push. "
            "Report the remembered line and the six-test result, and finish your reply with "
            + self.recovery_marker + "."
        )
        self.cases = {}
        self.rollouts = {}
        self.last_progress = {}
        self.receipt.update(
            scope="real selected provider + native Codex TUI + fixed Restricted tools",
            no_real_model=False, real_model_authorized=True, real_model_attempted=False,
            no_login=True, host_credentials_copied=False, auth_artifacts_collected=False,
            credential_isolation_evidence=(
                "Explicit source CODEX_HOME; production controller handles authorized existing-account auth. "
                "This harness never opens auth.json, copies source config, or records request headers."),
            selected_model=self.model, selected_provider=self.provider,
        )
        # The production controller may read authorized source credentials.
        self.receipt.pop("host_credentials_read", None)
        self.receipt["source_hashes"]["scripts/verify-desktop-real-terminal.py"] = digest(Path(__file__))
        self.receipt["source_hashes"]["scripts/verify-real-sandbox-agent.py"] = digest(
            REPOSITORY / "scripts/verify-real-sandbox-agent.py")
        self.prompt = (
            "Fix clamp.py so clamp(value, lower, upper) returns the value clamped to the inclusive "
            "[lower, upper] interval and raises ValueError when lower > upper. Only change clamp.py. "
            "Read the existing test_clamp.py and run python3 -m unittest -v test_clamp.py. "
            "Use exec_command with shell commands to inspect and edit the file; do not request apply_patch. "
            "Do not change tests, install dependencies, read credentials, use browser tools, commit or push. "
            "After the six tests pass, give a brief result and end your reply with " + self.final_marker + "."
        )

    def prepare(self, root):
        self.root = root
        self.data, self.controller, self.project, self.outside = (
            root / name for name in ("data", "daemon-home", "project", "outside"))
        for directory in (self.data, self.controller, self.project, self.outside):
            directory.mkdir(mode=0o700)
        self.codex_home = self.source
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        (binary_directory / "codex").symlink_to(self.cli)
        self.environment = {
            "PATH": str(binary_directory) + ":/usr/bin:/bin", "HOME": str(self.controller),
            "CODEX_HOME": str(self.source), "LANG": "C.UTF-8", "TMPDIR": str(self.controller),
            "SHELL": "/bin/bash", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
            "VELELLA_SYNTHETIC_HOST_SECRET": "synthetic-only",
        }
        for name in PROXY_VARIABLES:
            if name in os.environ:
                self.environment[name] = os.environ[name]
        self.secret = self.outside / "synthetic.txt"
        self.secret.write_text("synthetic fixture only\n")
        self.secret_digest = digest(self.secret)
        for name, content in (("clamp.py", ORIGINAL), ("test_clamp.py", TESTS), (".gitignore", GITIGNORE)):
            (self.project / name).write_text(content)
        for arguments in (["-c", "init.defaultBranch=main", "init"], ["add", "."],
                          ["-c", "user.name=Velella fixture", "-c", "user.email=fixture@example.invalid",
                           "commit", "--no-gpg-sign", "-m", "real-provider fixture"]):
            subprocess.run(["/usr/bin/git", *arguments], cwd=self.project, env=self.environment,
                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True, timeout=15)
        # Provider diagnostics remain temporary, never uploaded as test output.
        self.log = (root / "daemon.log").open("wb")
        self.daemon = subprocess.Popen(
            [str(self.runtime), "daemon", str(self.data)], cwd=root, env=self.environment,
            stdin=subprocess.DEVNULL, stdout=self.log, stderr=self.log, start_new_session=True)
        self.capture("daemon", self.daemon.pid)
        self.endpoint = self.data / "daemon.sock"

        def started():
            if self.daemon.poll() is not None:
                raise RuntimeError("Private daemon exited during startup")
            return self.call("snapshot")

        wait_for(started, 20, "private daemon readiness")
        project = self.call("project.add", path=str(self.project))["project"]
        self.main_workspace = "main:" + project["id"]
        workspace = self.call("workbench.create", project_id=project["id"],
                              name="真实模型受限终端验收", base="HEAD",
                              request_id=str(uuid.uuid4()))["workspace"]
        self.workspace_id = workspace["id"]
        self.work = Path(workspace["path"]).resolve(strict=True)
        self.check("created_real_git_worktree", workspace["kind"] == "worktree"
                   and self.work != self.project and (self.work / ".git").is_file())
        self.rejecting_provider = HELPERS["RejectingProvider"]()

    def text_ready(self, sid, marker):
        # All original startup/policy assertions remain; only the selected
        # provider banner replaces the offline fixture's hard-coded model.
        if marker == "mock-model":
            marker = self.model or "OpenAI Codex"
        return super().text_ready(sid, marker)

    def shell_fixture(self, case, name, body):
        """Keep native !input short; the fixed fixture itself stays outside the worktree."""
        home = case["state"] / "tool-home"
        wait_for(home.exists, 10, "private fixture tool HOME")
        metadata = home.lstat()
        if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.geteuid()
                or stat.S_IMODE(metadata.st_mode) != 0o700 or home.resolve(strict=True) != home
                or home.is_relative_to(case["work"])):
            raise RuntimeError("Unexpected fixture tool HOME")
        filename = "velella-" + name + "-" + self.nonce + ".sh"
        path = home / filename
        expected = "#!/bin/bash\n" + body + "\n"
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o400)
        except FileExistsError:
            metadata = path.lstat()
            if (metadata.st_uid != os.geteuid() or stat.S_IMODE(metadata.st_mode) != 0o444
                    or bounded_text(path) != expected):
                raise RuntimeError("Fixed shell fixture was replaced or modified")
        else:
            try:
                with os.fdopen(fd, "w") as stream:
                    stream.write(expected)
                    stream.flush()
                    os.fchmod(stream.fileno(), 0o444)
                    os.fsync(stream.fileno())
            except BaseException:
                path.unlink(missing_ok=True)
                raise
        fixtures = case.setdefault("shell_fixtures", {})
        fixtures[name] = {"file": filename, "sha256": digest(path),
                          "bytes": path.stat().st_size, "mode": "0444"}
        private_json(case["output"] / "shell-fixtures.json", fixtures)
        command = '!/bin/bash "$HOME/' + filename + '"'
        if len(command.encode()) >= 1024:
            raise RuntimeError("Fixed fixture command exceeds native short-input limit")
        return command

    def check_shell_fixtures(self, case):
        for record in case.get("shell_fixtures", {}).values():
            path = case["state"] / "tool-home" / record["file"]
            bounded_text(path)
            metadata = path.lstat()
            if (metadata.st_uid != os.geteuid() or stat.S_IMODE(metadata.st_mode) != 0o444
                    or digest(path) != record["sha256"]):
                raise RuntimeError("Fixed shell fixture integrity changed")

    def commands(self, work):
        boundary = f"""
import errno, json, os, pathlib, socket
work = {str(work)!r}
home = pathlib.Path(os.environ["HOME"])
control = home.parent
checks = {{}}
checks["cwd"] = os.getcwd() == work
checks["private_tool_home"] = home.name == "tool-home" and home.parent.parent.name == "sandbox-chats"
checks["host_env_removed"] = "VELELLA_SYNTHETIC_HOST_SECRET" not in os.environ
checks["proxy_env_removed"] = not any(name in os.environ for name in {PROXY_VARIABLES!r})
for name, path, mode in [
    ("outside_read_denied", {str(self.secret)!r}, "r"),
    ("outside_write_denied", {str(self.secret)!r}, "w"),
    ("controller_config_read_denied", control / "controller/codex/config.toml", "r"),
    ("config_audit_read_denied", control / "config-audit.json", "r"),
    ("executor_config_read_denied", control / "exec-server.json", "r"),
    ("launch_write_denied", control / "launch.json", "w"),
    ("controller_write_denied", control / "unexpected-write", "w"),
]:
    try:
        with open(path, mode) as stream:
            if mode == "r": stream.read()
            else: stream.write("unexpected")
        checks[name] = False
    except OSError as error:
        checks[name] = error.errno in (errno.EACCES, errno.EPERM)
try:
    descriptor = os.open({str(self.source / "auth.json")!r}, os.O_RDONLY | os.O_NOFOLLOW)
except OSError as error:
    checks["source_auth_open_denied"] = error.errno in (errno.EACCES, errno.EPERM)
else:
    os.close(descriptor)
    checks["source_auth_open_denied"] = False
try:
    with socket.create_connection(("127.0.0.1", {self.rejecting_provider.server.server_port}), timeout=1): pass
    checks["network_denied"] = False
except OSError as error:
    checks["network_denied"] = error.errno in (errno.EACCES, errno.EPERM)
(home / "owned-tool-home-proof").write_text("allowed\\n")
checks["tool_home_writable"] = (home / "owned-tool-home-proof").read_text() == "allowed\\n"
(home / "boundary-{self.nonce}.json").write_text(json.dumps(checks))
assert len(checks) == 14 and all(checks.values()), "fixed boundary checks failed"
"""
        boundary_command = python_command(boundary)
        boundary_command += " && printf '%s%s\\n' REAL_BOUNDARY_ " + shlex.quote("COMPLETE_" + self.nonce)
        proof_code = (
            "import json, os, pathlib, sys\n"
            f"pathlib.Path(os.environ['HOME'], 'verify-{self.nonce}.json').write_text("
            f"json.dumps({{'exit_code':int(sys.argv[1]),'tests':6,'nonce':{self.nonce!r}}}))\n"
        )
        log = '"$HOME/verify-' + self.nonce + '.log"'
        verify_arguments = shlex.split(VERIFY_COMMAND)
        if verify_arguments[:3] != ["/usr/bin/python3", "-I", "-c"] or len(verify_arguments) != 4:
            raise RuntimeError("Fixed verification command shape changed")
        verify_command = (
            "{ " + python_command(verify_arguments[3]) + "; } > " + log + " 2>&1; verify_exit=$?; "
            + python_command(proof_code) + ' "$verify_exit"; '
            "/usr/bin/cat " + log + "; printf '%s%s\\n' REAL_VERIFY_ "
            + shlex.quote("COMPLETE_" + self.nonce) + '; exit "$verify_exit"')
        case = next((case for case in self.cases.values() if case["work"] == work), None)
        if case is not None:
            boundary_command = self.shell_fixture(case, "boundary", boundary_command)
            verify_command = self.shell_fixture(case, "verify", verify_command)
        else:
            # Initial ready.json precedes the private session directory. The
            # browser rereads it after add_case publishes these short commands.
            boundary_command, verify_command = "!" + boundary_command, "!" + verify_command
        return {"boundary_command": boundary_command,
                "model_prompt": self.prompt, "independent_verify_command": verify_command}

    def recovery_commands(self, work):
        original = self.commands(work)
        return {
            "recovery_memory_command": "!printf '%s\\n' " + shlex.quote(self.recovery_memory),
            "recovery_prompt": self.recovery_prompt,
            "recovery_boundary_command": original["boundary_command"],
            "recovery_independent_verify_command": original["independent_verify_command"],
        }

    def add_case(self, sid, workspace_id, work, name, *, already_open=False):
        work = Path(work).resolve(strict=True)
        marker = REAL["fixture_inventory"](work, self.root, self.project, changed=False)
        state, output = self.data / "sandbox-chats" / sid, self.output / name
        output.mkdir(parents=True, exist_ok=True)
        self.session_artifacts[sid] = (state, output)
        if not already_open:
            record = self.call("workbench.sessions", workspace_id=workspace_id)["sessions"]
            record = next(item for item in record if item["id"] == sid)
            self.capture(name + "/host", record["status"]["pid"], parent=self.daemon.pid)
            self.host_labels[sid] = name + "/host"
            self.sessions.append(sid)
        case = {"sid": sid, "workspace_id": workspace_id, "work": work, "state": state,
                "output": output, "name": name, "git_marker": marker, "verified": False,
                "tree_captured": already_open}
        self.cases[sid] = case
        private_json(output / "commands.json", self.commands(work))
        return case

    def selected_case(self, case):
        self.workspace_id, self.work = case["workspace_id"], case["work"]

    def model_evidence(self, case):
        """Extract counts only from this fresh controller's own rollout files."""
        root = case["state"] / "controller/codex/sessions"
        evidence = self.rollouts.setdefault(case["sid"], {
            "files": {}, "function_calls": 0, "function_outputs": 0, "assistant_messages": 0,
            "model_turn_started": False, "completion_event_seen": False,
            "final_reply_seen": False, "completed_turns": 0, "total_tokens": None,
            "recovery_memory_in_shell_history": False, "recovery_memory_in_new_assistant": False,
            "recovery_final_reply_seen": False, "recovery_rg_call_seen": False,
            "recovery_test_call_seen": False})
        files = list(root.rglob("*.jsonl")) if root.exists() else []
        if len(files) > 8:
            raise RuntimeError("Too many real-model rollout files")
        for path in files:
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 16 * 1024 * 1024
                    or not path.resolve().is_relative_to(root.resolve())):
                raise RuntimeError("Unexpected real-model rollout file")
            position = evidence["files"].setdefault(str(path), {"offset": 0, "pending": b""})
            with path.open("rb") as stream:
                stream.seek(position["offset"])
                data = position["pending"] + stream.read()
                position["offset"] = stream.tell()
            lines = data.split(b"\n")
            position["pending"] = lines.pop()
            for encoded in lines:
                if not encoded:
                    continue
                event = json.loads(encoded)
                payload = event.get("payload", {})
                if not isinstance(payload, dict):
                    continue
                kind = payload.get("type")
                if event.get("type") == "response_item":
                    if kind == "function_call":
                        evidence["function_calls"] += 1
                        if case.get("recovery"):
                            arguments = payload.get("arguments", "")
                            if not isinstance(arguments, str):
                                arguments = json.dumps(arguments)
                            evidence["recovery_rg_call_seen"] |= (
                                payload.get("name") == "exec_command"
                                and re.search(r"\brg\b", arguments) is not None and "clamp.py" in arguments)
                            evidence["recovery_test_call_seen"] |= (
                                payload.get("name") == "exec_command"
                                and "unittest" in arguments and "test_clamp.py" in arguments)
                    elif kind == "function_call_output":
                        evidence["function_outputs"] += 1
                    elif kind == "message" and payload.get("role") == "assistant":
                        evidence["assistant_messages"] += 1
                        evidence["final_reply_seen"] |= any(
                            self.final_marker in item.get("text", "")
                            for item in payload.get("content", []) if isinstance(item, dict))
                        if case.get("recovery"):
                            texts = [item.get("text", "") for item in payload.get("content", [])
                                     if isinstance(item, dict)]
                            evidence["recovery_memory_in_new_assistant"] |= any(
                                self.recovery_memory in text for text in texts)
                            evidence["recovery_final_reply_seen"] |= any(
                                self.recovery_marker in text for text in texts)
                    elif kind == "message" and payload.get("role") == "user":
                        evidence["recovery_memory_in_shell_history"] |= any(
                            "<user_shell_command>" in item.get("text", "")
                            and self.recovery_memory in item.get("text", "")
                            for item in payload.get("content", []) if isinstance(item, dict))
                if event.get("type") == "event_msg":
                    if kind in ("task_started", "turn_started"):
                        evidence["model_turn_started"] = True
                        self.receipt["real_model_attempted"] = True
                    if kind in ("task_complete", "turn_complete"):
                        evidence["completion_event_seen"] = True
                        evidence["completed_turns"] += 1
                        evidence["final_reply_seen"] |= self.final_marker in (payload.get("last_agent_message") or "")
                    if kind == "token_count":
                        usage = (payload.get("info") or {}).get("total_token_usage") or {}
                        if isinstance(usage.get("total_tokens"), int):
                            evidence["total_tokens"] = usage["total_tokens"]
        return {key: value for key, value in evidence.items() if key != "files"}

    def model_complete(self, case):
        evidence = self.model_evidence(case)
        return evidence if (evidence["completion_event_seen"] and evidence["final_reply_seen"]
                            and evidence["assistant_messages"] > 0
                            and evidence["function_calls"] > 0 and evidence["function_outputs"] > 0) else None

    def boundary_result(self, case):
        path = case["state"] / "tool-home" / ("boundary-" + self.nonce + ".json")
        checks = json.loads(bounded_text(path))
        self.check(case["name"] + "_fourteen_kernel_boundary_checks",
                   len(checks) == 14 and all(value is True for value in checks.values()), checks)
        private_json(case["output"] / "boundary-result.json", checks)

    def verify_case(self, case, *, send=True):
        self.selected_case(case)
        evidence = wait_for(lambda: self.model_complete(case), self.timeout,
                            "real model completion with tool calls and final reply")
        self.receipt["real_model_attempted"] = True
        self.boundary_result(case)
        if send:
            self.enter(case["sid"], self.commands(case["work"])["independent_verify_command"], paste=False)
            wait_for(lambda: self.text_ready(case["sid"], self.verify_marker), 60,
                     "independent sandbox verification")
        self.check_shell_fixtures(case)
        proof = json.loads(bounded_text(case["state"] / "tool-home" / ("verify-" + self.nonce + ".json")))
        self.check(case["name"] + "_independent_six_tests_passed",
                   proof == {"exit_code": 0, "tests": 6, "nonce": self.nonce}, proof)
        log = bounded_text(case["state"] / "tool-home" / ("verify-" + self.nonce + ".log"))
        self.check(case["name"] + "_independent_test_output", "Ran 6 tests" in log
                   and re.search(r"(?m)^OK$", log) is not None)
        (case["output"] / "verification-output.txt").write_text(log)
        private_json(case["output"] / "verification-result.json", proof)
        private_json(case["output"] / "model-evidence.json", evidence)
        self.check(case["name"] + "_controller_has_no_auth_file",
                   not (case["state"] / "controller/codex/auth.json").exists()
                   and not (case["state"] / "tool-home/.codex/auth.json").exists())
        case["verified"] = True

    def checkpoint(self, case):
        value = json.loads(bounded_text(case["state"] / "terminal-resume.json", 8192))
        fields = ("schema", "thread_id", "generation", "work", "codex_home", "cli_version",
                  "account_sha256", "shell_history_dirty")
        if (value.get("schema") != 1 or value.get("work") != str(case["work"])
                or value.get("codex_home") != str(case["state"] / "controller/codex")):
            raise RuntimeError("Unexpected recovery checkpoint binding")
        for name in ("thread_id", "generation"):
            if str(uuid.UUID(value[name])) != value[name]:
                raise RuntimeError("Unexpected recovery checkpoint identity")
        return {name: value.get(name) for name in fields}

    def recovery_baseline(self, case):
        """Capture only fixture-owned metadata; never read controller credentials."""
        checkpoint = self.checkpoint(case)
        if checkpoint["shell_history_dirty"]:
            return None
        bridge = json.loads(bounded_text(case["state"] / "terminal-bridge.json", 8192))
        self.check(case["name"] + "_recovery_baseline_generation",
                   checkpoint["generation"] == bridge["generation"])
        REAL["fixture_inventory"](case["work"], self.root, self.project, case["git_marker"])
        baseline = {
            "checkpoint": checkpoint, "bridge": bridge,
            "model_evidence": self.model_evidence(case),
            "host": self.pidfds[case["name"] + "/host"]["identity"],
            "processes": {label: record["identity"] for label, record in self.pidfds.items()
                          if label.startswith(case["name"] + "/")},
            "candidate_sha256": digest(case["work"] / "clamp.py"),
            "control_hashes": {name: digest(case["state"] / name) for name in (
                "controller/codex/config.toml", "controller/codex/environments.toml",
                "exec-server.json", "exec-server.py", "terminal-bridge.py")},
        }
        private_json(case["output"] / "recovery-before.json", baseline)
        case["recovery_baseline"] = baseline
        return baseline

    def observe_recovery(self, case):
        baseline = case["recovery_baseline"]
        self.selected_case(case)
        record = self.session_state(case["sid"])
        pid = (record.get("status") or {}).get("pid")
        if (not pid or pid == baseline["host"]["pid"]
                or (record.get("status") or {}).get("exit") is not None):
            return None
        try:
            checkpoint = self.checkpoint(case)
            bridge = json.loads(bounded_text(case["state"] / "terminal-bridge.json", 8192))
        except FileNotFoundError:
            return None
        if (checkpoint["generation"] == baseline["checkpoint"]["generation"]
                or checkpoint["generation"] != bridge["generation"] or checkpoint["shell_history_dirty"]):
            return None
        for label in baseline["processes"]:
            if not self.stopped(label):
                return None
        for name in ("thread_id", "work", "codex_home", "cli_version", "account_sha256"):
            self.check(case["name"] + "_recovery_preserved_" + name,
                       checkpoint[name] == baseline["checkpoint"][name])
        self.check(case["name"] + "_recovery_new_host", pid != baseline["host"]["pid"])
        self.check(case["name"] + "_recovery_new_generation",
                   bridge["generation"] != baseline["bridge"]["generation"])
        self.check(case["name"] + "_old_bridge_removed",
                   not Path(baseline["bridge"]["socket"]).parent.exists())
        name = case["name"] + "/recovered"
        self.capture(name + "/host", pid, parent=self.daemon.pid)
        self.host_labels[case["sid"]] = name + "/host"
        self.capture_tree(name, case["sid"])
        self.bridge_sockets[case["sid"]] = Path(bridge["socket"])
        self.check(case["name"] + "_recovery_marker_invalidated",
                   not (case["state"] / "terminal-stopped.json").exists())
        self.check(case["name"] + "_recovery_fixed_control_unchanged",
                   all(digest(case["state"] / key) == value for key, value in baseline["control_hashes"].items()))
        # Stale passing proof must never satisfy a recovered-generation check.
        for prefix, suffix in (("boundary-", ".json"), ("verify-", ".json"), ("verify-", ".log")):
            path = case["state"] / "tool-home" / (prefix + self.nonce + suffix)
            original = bounded_text(path)
            destination = case["output"] / ("recovery-before-" + prefix.rstrip("-") + suffix)
            destination.write_text(original)
            destination.chmod(0o600)
            path.unlink()
        recovery = {
            "checkpoint": checkpoint, "bridge": bridge, "proofs_reset_at_ns": time.time_ns(),
            "host": self.pidfds[name + "/host"]["identity"],
            "processes": {label: item["identity"] for label, item in self.pidfds.items()
                          if label.startswith(name + "/")},
        }
        case["recovery"] = recovery
        private_json(case["output"] / "recovery-started.json", recovery)
        return recovery

    def recovery_complete(self, case):
        if not case.get("recovery"):
            return None
        evidence = self.model_evidence(case)
        baseline = case["recovery_baseline"]["model_evidence"]
        return evidence if (evidence["recovery_final_reply_seen"]
                            and evidence["completed_turns"] > baseline["completed_turns"]
                            and evidence["assistant_messages"] > baseline["assistant_messages"]) else None

    def recovery_proofs_ready(self, case):
        paths = [case["state"] / "tool-home" / (prefix + self.nonce + suffix)
                 for prefix, suffix in (("boundary-", ".json"), ("verify-", ".json"), ("verify-", ".log"))]
        return (all(path.exists() for path in paths)
                and self.checkpoint(case)["shell_history_dirty"] is False)

    def verify_recovery(self, case, evidence):
        baseline, recovery = case["recovery_baseline"], case["recovery"]
        self.check_shell_fixtures(case)
        prefix = case["name"] + "_recovered"
        self.check(prefix + "_old_memory_in_new_assistant", evidence["recovery_memory_in_new_assistant"]
                   and evidence["recovery_memory_in_shell_history"])
        self.check(prefix + "_new_model_tool_calls_and_outputs",
                   evidence["function_calls"] > baseline["model_evidence"]["function_calls"]
                   and evidence["function_outputs"] > baseline["model_evidence"]["function_outputs"])
        self.check(prefix + "_model_used_rg_and_six_tests",
                   evidence["recovery_rg_call_seen"] and evidence["recovery_test_call_seen"])
        self.check(prefix + "_new_completion_marker", evidence["recovery_final_reply_seen"])
        for key, suffix in (("boundary-", ".json"), ("verify-", ".json"), ("verify-", ".log")):
            path = case["state"] / "tool-home" / (key + self.nonce + suffix)
            self.check(prefix + "_fresh_" + key.rstrip("-") + suffix.replace(".", "_"),
                       path.lstat().st_mtime_ns >= recovery["proofs_reset_at_ns"])
        boundary = json.loads(bounded_text(
            case["state"] / "tool-home" / ("boundary-" + self.nonce + ".json")))
        self.check(prefix + "_fourteen_kernel_boundaries",
                   len(boundary) == 14 and all(value is True for value in boundary.values()), boundary)
        proof = json.loads(bounded_text(case["state"] / "tool-home" / ("verify-" + self.nonce + ".json")))
        self.check(prefix + "_independent_six_tests_passed",
                   proof == {"exit_code": 0, "tests": 6, "nonce": self.nonce}, proof)
        log = bounded_text(case["state"] / "tool-home" / ("verify-" + self.nonce + ".log"))
        self.check(prefix + "_independent_test_output", "Ran 6 tests" in log
                   and re.search(r"(?m)^OK$", log) is not None)
        REAL["fixture_inventory"](case["work"], self.root, self.project, case["git_marker"])
        self.check(prefix + "_source_unchanged", digest(case["work"] / "clamp.py") == baseline["candidate_sha256"])
        checkpoint = self.checkpoint(case)
        self.check(prefix + "_same_thread_and_clean_history",
                   checkpoint["thread_id"] == baseline["checkpoint"]["thread_id"]
                   and checkpoint["generation"] == recovery["bridge"]["generation"]
                   and checkpoint["shell_history_dirty"] is False)
        self.check(prefix + "_controller_has_no_auth_file",
                   not (case["state"] / "controller/codex/auth.json").exists()
                   and not (case["state"] / "tool-home/.codex/auth.json").exists())
        self.check(prefix + "_control_remains_fixed", all(
            digest(case["state"] / key) == value for key, value in baseline["control_hashes"].items()))
        result = {"passed": True, "session_id": case["sid"], "checkpoint": checkpoint,
                  "model_evidence": evidence, "boundary_checks": boundary, "verification": proof,
                  "old_thread_memory_verified": True, "new_generation": True,
                  "source_unchanged": True, "controller_has_no_auth_file": True}
        private_json(case["output"] / "recovery-result.json", result)
        (case["output"] / "recovery-verification-output.txt").write_text(log)
        private_json(self.output / "recovery-verification-complete.json", result)
        case["recovery_verified"] = True
        self.receipt["real_model_recovery_verified"] = True

    def finish_case(self, case):
        self.selected_case(case)
        self.check_shell_fixtures(case)
        record = self.session_state(case["sid"])
        if record["status"].get("exit") is None:
            self.call("workbench.terminal.stop", workspace_id=case["workspace_id"],
                      session_id=case["sid"], confirmed=True)
        for label in list(self.pidfds):
            if label.startswith(case["name"] + "/"):
                self.check("stopped_" + label, self.stopped(label, 10))
        REAL["fixture_inventory"](case["work"], self.root, self.project, case["git_marker"])
        self.check(case["name"] + "_only_candidate_changed", True)
        (case["output"] / "clamp.py").write_text(bounded_text(case["work"] / "clamp.py"))
        bridge = json.loads(bounded_text(case["state"] / "terminal-bridge.json"))
        socket = Path(bridge["socket"])
        self.bridge_sockets[case["sid"]] = socket
        self.bridge_removed(case["name"], case["sid"])
        private_json(case["output"] / "terminal-status.json", self.session_state(case["sid"])["status"])

    def run_automatic(self):
        sid, _, _ = self.open_terminal("real-model")
        self.capture_tree("real-model", sid)
        case = self.add_case(sid, self.workspace_id, self.work, "real-model", already_open=True)
        self.enter(sid, self.commands(self.work)["boundary_command"], paste=False)
        wait_for(lambda: self.text_ready(sid, self.boundary_marker), 30, "sandbox boundary command")
        self.boundary_result(case)
        self.receipt["real_model_attempted"] = True
        self.enter(sid, self.prompt)
        self.verify_case(case)
        self.finish_case(case)
        self.receipt["real_model_verified"] = True

    def browser(self):
        self.receipt["scope"] = "real selected provider through browser native terminal; UI evidence recorded separately"
        ready = {
            "data_dir": str(self.data), "daemon_endpoint": str(self.endpoint), "daemon_pid": self.daemon.pid,
            "controller_pid": os.getpid(), "runtime": str(self.runtime), "workspace_id": self.workspace_id,
            "workspace_path": str(self.work), "selected_model": self.model, "selected_provider": self.provider,
            "real_model_authorized": True, "no_auth_files_copied": True,
            "verify_request": str(self.output / "verify.request"),
            "recovery_verify_request": str(self.output / "recovery-verify.request"),
            "recovery_ready": False,
            "cleanup_request": str(self.output / "cleanup.request"),
            "vite_environment": {"VELELLA_DATA_DIR": str(self.data), "VELELLA_RUNTIME": str(self.runtime)},
            **self.commands(self.work),
            **self.recovery_commands(self.work),
            "instructions": (
                "Use the existing fixture worktree, or a new worktree of this fixture repository. "
                "Read ready.json again after opening the terminal: commands track the selected session. "
                "Enter boundary_command, then model_prompt. After the model finishes, create verify.request "
                "or enter independent_verify_command. After verification-complete.json and recovery_baseline_ready "
                "are present, enter recovery_memory_command and wait for its output and a fresh prompt. "
                "Use the normal Stop menu and then Recover on the same terminal. Wait until ready.json has "
                "recovery_ready=true; then enter recovery_prompt, wait for completion, and run "
                "recovery_boundary_command and recovery_independent_verify_command. Create recovery-verify.request "
                "and wait for recovery-verification-complete.json. Finally create cleanup.request."),
        }
        private_json(self.output / "ready.json", ready)
        print(json.dumps({"ready": True, "ready_file": str(self.output / "ready.json")}), flush=True)
        deadline = time.monotonic() + self.timeout
        while not STOP.wait(.5) and not (self.output / "cleanup.request").exists():
            if time.monotonic() >= deadline:
                raise TimeoutError("Real-browser acceptance timed out")
            for workspace in self.call("workbench.snapshot")["workspaces"]:
                for record in self.call("workbench.sessions", workspace_id=workspace["id"])["sessions"]:
                    sid = record["id"]
                    if sid not in self.cases and (record.get("status") or {}).get("pid"):
                        self.check("browser_uses_restricted_native_terminal",
                                   record.get("execution_mode") == "restricted" and record.get("kind") == "terminal"
                                   and record.get("native_chat") is False and not self.cases)
                        case = self.add_case(sid, workspace["id"], workspace["path"], "real-browser")
                        ready.update(workspace_id=workspace["id"], workspace_path=workspace["path"],
                                     session_id=sid, **self.commands(case["work"]),
                                     **self.recovery_commands(case["work"]))
                        private_json(self.output / "ready.json", ready)
            for case in self.cases.values():
                evidence = self.model_evidence(case)
                if self.last_progress.get(case["sid"]) != evidence:
                    private_json(case["output"] / "model-progress.json", evidence)
                    self.last_progress[case["sid"]] = evidence
                recording = case["state"] / "terminal.bin"
                if (not case["tree_captured"] and (case["state"] / "terminal-bridge.json").exists()
                        and recording.exists() and recording.stat().st_size > 0):
                    # Two native ELF roles are checked without interfering with
                    # the browser's xterm cursor-position protocol.
                    self.capture_tree(case["name"], case["sid"])
                    case["tree_captured"] = True
                if ((self.output / "verify.request").exists() and not case["verified"]
                        and self.model_complete(case)):
                    self.verify_case(case)
                    private_json(self.output / "verification-complete.json", {"passed": True, "session_id": case["sid"]})
                if case["verified"] and not case.get("recovery_baseline"):
                    if self.recovery_baseline(case):
                        ready["recovery_baseline_ready"] = True
                        private_json(self.output / "ready.json", ready)
                if case.get("recovery_baseline") and not case.get("recovery"):
                    if self.observe_recovery(case):
                        ready["recovery_ready"] = True
                        private_json(self.output / "ready.json", ready)
                if (case.get("recovery") and not case.get("recovery_verified")
                        and (self.output / "recovery-verify.request").exists()):
                    recovered_evidence = self.recovery_complete(case)
                    if recovered_evidence and self.recovery_proofs_ready(case):
                        self.verify_recovery(case, recovered_evidence)
        self.check("browser_created_exactly_one_terminal", len(self.cases) == 1)
        for case in self.cases.values():
            if not case["verified"]:
                self.verify_case(case, send=False)
            if case.get("recovery") or (self.output / "recovery-verify.request").exists():
                self.check(case["name"] + "_recovery_verified_before_cleanup",
                           case.get("recovery_verified") is True)
            self.finish_case(case)
        self.receipt["real_model_verified"] = True
        self.receipt["browser_assertions_recorded_separately"] = True

    def finish(self, temporary):
        # The offline helper's finish copies broad stderr/chat diagnostics.
        # Real-provider runs retain only deliberately selected evidence instead.
        self.cleanup()
        self.receipt["outside_fixture_unchanged"] = digest(self.secret) == self.secret_digest if hasattr(self, "secret") else None
        self.receipt["network_probe_connections"] = self.rejecting_provider.summary()["connection_count"] if self.rejecting_provider else None
        try:
            temporary.cleanup()
        except Exception:
            self.cleanup_errors.append("Could not remove owned temporary acceptance directory")
        self.receipt["temporary_directory_removed"] = not Path(temporary.name).exists()
        self.receipt["cleanup_errors"] = self.cleanup_errors
        self.receipt["sources_unchanged"] = all(
            digest(self.repository / path) == value for path, value in self.receipt["source_hashes"].items())
        self.receipt["binaries_unchanged"] = digest(self.runtime) == self.receipt["runtime_sha256"] and digest(self.cli) == self.receipt["codex_sha256"]
        if (self.cleanup_errors or not self.receipt["temporary_directory_removed"]
                or not self.receipt["sources_unchanged"] or not self.receipt["binaries_unchanged"]
                or self.receipt["outside_fixture_unchanged"] is False
                or self.receipt["network_probe_connections"] not in (None, 0)):
            self.receipt["status"] = "failed"
        private_json(self.output / "result.json", self.receipt)
        print(json.dumps({"status": self.receipt["status"], "result_file": str(self.output / "result.json")}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--codex", required=True, type=Path)
    parser.add_argument("--source-codex-home", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--allow-real-model", required=True, action="store_true")
    parser.add_argument("--browser", action="store_true")
    parser.add_argument("--timeout", type=int, default=1200)
    args = parser.parse_args()
    if not 1 <= args.timeout <= 3600:
        parser.error("--timeout must be between 1 and 3600 seconds")
    runtime, cli, source = (path.resolve(strict=True) for path in (args.runtime, args.codex, args.source_codex_home))
    if not source.is_dir():
        parser.error("--source-codex-home must be a directory")
    with cli.open("rb") as stream:
        if stream.read(4) != b"\x7fELF":
            parser.error("--codex must name the actual pinned native ELF")
    if args.output.is_symlink() or args.output.exists() and any(args.output.iterdir()):
        parser.error("--output must be new or empty")
    args.output.mkdir(parents=True, exist_ok=True, mode=0o700)
    test = RealTerminalAcceptance(runtime, cli, args.output.resolve(), source, args.timeout)
    temporary = tempfile.TemporaryDirectory(prefix="vl-real-terminal-")
    previous = {sig: signal.signal(sig, lambda *_: STOP.set()) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        test.prepare(Path(temporary.name).resolve())
        if args.browser:
            test.browser()
        else:
            test.run_automatic()
        test.receipt["status"] = "passed"
    except Exception as error:
        # Provider errors can contain sensitive detail: retain a class only.
        test.receipt.update(
            status="failed", error_class=type(error).__name__,
            error_frames=[{"file": Path(frame.filename).name, "line": frame.lineno, "function": frame.name}
                          for frame in traceback.extract_tb(error.__traceback__)])
    finally:
        test.finish(temporary)
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    return 0 if test.receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
