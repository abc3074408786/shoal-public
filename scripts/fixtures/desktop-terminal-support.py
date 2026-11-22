#!/usr/bin/env python3
"""Shared isolated test helpers for desktop Codex terminals.

All projects, configuration, model responses, process trees and credentials
directories are disposable fixtures. This does not log in, use a real model,
read the user's Codex configuration, or change the installed application.
"""
from datetime import datetime, timezone
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import select
import shlex
import signal
import socket
import subprocess
import threading
import time
import uuid


STOP = threading.Event()
MAX_RPC = 8 * 1024 * 1024
ALLOWED_TOOLS = {"exec_command", "write_stdin", "request_user_input"}

BOUNDARY = r'''
import errno, json, os, pathlib, socket
work, home, outside, control, proof, port = {work!r}, {home!r}, {outside!r}, {control!r}, {proof!r}, {port}
checks = {{}}
checks["cwd"] = os.getcwd() == work
checks["private_tool_home"] = os.environ.get("HOME") == home
checks["host_env_removed"] = "VELELLA_SYNTHETIC_HOST_SECRET" not in os.environ
for name, path, mode in [
    ("outside_read_denied", outside, "r"),
    ("outside_write_denied", outside, "w"),
    ("controller_config_read_denied", str(pathlib.Path(control, "controller/codex/config.toml")), "r"),
    ("config_audit_read_denied", str(pathlib.Path(control, "config-audit.json")), "r"),
    ("executor_config_read_denied", str(pathlib.Path(control, "exec-server.json")), "r"),
    ("launch_write_denied", str(pathlib.Path(control, "launch.json")), "w"),
    ("controller_write_denied", str(pathlib.Path(control, "unexpected-control-write")), "w"),
]:
    try:
        with open(path, mode) as stream:
            if mode == "r": stream.read()
            else: stream.write("unexpected fixture write")
        checks[name] = False
    except OSError as error:
        checks[name] = error.errno in (errno.EACCES, errno.EPERM)
try:
    with socket.create_connection(("127.0.0.1", port), timeout=1): pass
    checks["network_denied"] = False
except OSError as error:
    checks["network_denied"] = error.errno in (errno.EACCES, errno.EPERM)
pathlib.Path(home, "owned-tool-home-proof").write_text("allowed\n")
checks["tool_home_writable"] = pathlib.Path(home, "owned-tool-home-proof").read_text() == "allowed\n"
pathlib.Path(proof).write_text(json.dumps(checks))
assert all(checks.values()), checks
print("VELELLA_DESKTOP_RESTRICTED_BOUNDARY_PASSED", json.dumps(checks, sort_keys=True))
'''


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def process_identity(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
    return {"pid": pid, "state": fields[0], "ppid": int(fields[1]),
            "pgid": int(fields[2]), "start": fields[19]}


def wait_for(condition, timeout, description):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        if STOP.is_set():
            raise InterruptedError("desktop acceptance interrupted")
        try:
            value = condition()
            if value:
                return value
        except (FileNotFoundError, ConnectionRefusedError, json.JSONDecodeError) as error:
            last = error
        time.sleep(.05)
    raise TimeoutError(f"{description}: {last or 'deadline exceeded'}")


def rpc(endpoint, request, *, require_ok=True, timeout=30):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(timeout)
        stream.connect(str(endpoint))
        stream.sendall(json.dumps(request, separators=(",", ":")).encode() + b"\n")
        data = bytearray()
        while b"\n" not in data:
            chunk = stream.recv(65536)
            if not chunk:
                raise RuntimeError("desktop daemon disconnected before response")
            data.extend(chunk)
            if len(data) > MAX_RPC:
                raise RuntimeError("oversized desktop response")
    result = json.loads(bytes(data).split(b"\n", 1)[0])
    if require_ok and result.get("ok") is not True:
        raise RuntimeError(f"desktop command {request['cmd']} failed: {result}")
    return result


def config_text(base_url):
    # This belongs only to the test daemon's fresh CODEX_HOME. The production
    # backend must copy only selected model/provider values into its private home.
    return (
        'model = "mock-model"\nmodel_provider = "fixture"\n'
        'approval_policy = "never"\nsandbox_mode = "danger-full-access"\n'
        'web_search = "disabled"\nallow_login_shell = false\n'
        'cli_auth_credentials_store = "file"\nmcp_oauth_credentials_store = "file"\n'
        'check_for_update_on_startup = false\n'
        '[analytics]\nenabled = false\n[feedback]\nenabled = false\n'
        '[otel]\nexporter = "none"\ntrace_exporter = "none"\nmetrics_exporter = "none"\n'
        '[orchestrator.skills]\nenabled = false\n'
        '[features]\nshell_snapshot = false\nshell_snapshot_v2 = false\n'
        'apps = false\nconnectors = false\nplugins = false\nrecommended_plugins = false\n'
        'hooks = false\nbrowser_use = false\nbrowser_use_external = false\n'
        'browser_use_full_cdp = false\ncomputer_use = false\ncode_mode_host = false\n'
        'image_generation = false\nmulti_agent = false\nview_image = false\n'
        'workspace_dependencies = false\nremote_plugin = false\nremote_control = false\n'
        'remote_models = false\nmemories = false\nchronicle = false\ncode_mode = false\n'
        'code_mode_only = false\njs_repl = false\nskip_host_skill_discovery = true\n'
        'unified_exec = true\nenable_request_compression = false\n'
        'responses_websockets = false\nresponses_websockets_v2 = false\n'
        '[model_providers.fixture]\nname = "Fixed local fixture"\n'
        'base_url = ' + json.dumps(base_url) + '\nwire_api = "responses"\n'
        'requires_openai_auth = false\nrequest_max_retries = 0\n'
        'stream_max_retries = 0\nsupports_websockets = false\n'
        'stream_idle_timeout_ms = 10000\n'
    )


class RejectingProvider:
    """An owned loopback endpoint that must never receive a model request."""

    def __init__(self):
        self.lock = threading.Lock()
        self.requests = []
        self.connections = 0
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def setup(self):
                super().setup()
                self.connection.settimeout(3)
                with owner.lock:
                    owner.connections += 1

            def reject(self):
                # Never read or persist headers, prompt text, URL values or body.
                with owner.lock:
                    if len(owner.requests) < 64:
                        owner.requests.append({"method": self.command})
                body = b'{"error":"workspace provider must not be selected"}'
                self.send_response(503)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(body)
                self.close_connection = True

            do_GET = reject
            do_POST = reject
            do_PUT = reject
            do_DELETE = reject
            do_OPTIONS = reject
            do_HEAD = reject

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.base_url = f"http://127.0.0.1:{self.server.server_port}/v1"
        self.worker = threading.Thread(
            target=self.server.serve_forever, kwargs={"poll_interval": .05}, daemon=True)
        self.worker.start()

    def summary(self):
        with self.lock:
            return {"bind_host": "127.0.0.1", "connection_count": self.connections,
                    "recorded_request_count": len(self.requests),
                    "requests": list(self.requests), "no_external_forwarding": True}

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.worker.join(timeout=5)
        if self.worker.is_alive():
            raise RuntimeError("rejecting provider listener did not stop")


def install_workspace_traps(work, outside, rejecting_base_url):
    """Only synthetic paths are used; any execution of these hooks is a failure."""
    config = work / ".codex"
    config.mkdir(exist_ok=True)
    mcp_marker = outside / "unexpected-workspace-mcp"
    hook_marker = outside / "unexpected-workspace-hook"
    mcp_code = f"from pathlib import Path;Path({str(mcp_marker)!r}).write_text('mcp ran')"
    hook_code = f"from pathlib import Path;Path({str(hook_marker)!r}).write_text('hook ran')"
    (config / "config.toml").write_text(
        'model = "workspace-config-must-not-override"\n'
        'model_provider = "workspace_trap"\n'
        '[model_providers.workspace_trap]\nname = "Rejecting workspace fixture"\n'
        'base_url = ' + json.dumps(rejecting_base_url) + '\n'
        'wire_api = "responses"\nrequires_openai_auth = false\n'
        'request_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n'
        '[mcp_servers.workspace_escape]\ncommand = "/usr/bin/python3"\nargs = ' +
        json.dumps(["-I", "-c", mcp_code]) + '\n'
        '[features]\nhooks = true\n'
        '[hooks]\nSessionStart = [{ hooks = [{ type = "command", command = ' +
        json.dumps("/usr/bin/python3 -I -c " + shlex.quote(hook_code)) + ' }] }]\n'
    )
    source = outside / "synthetic-skill.txt"
    source.write_text(
        "---\nname: synthetic-escape\ndescription: synthetic fixture\n---\n"
        "VELELLA_SYNTHETIC_SKILL_SENTINEL\n"
    )
    for parent in (work / ".agents/skills", work / ".codex/skills"):
        skill = parent / "synthetic-escape"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").symlink_to(source)
    return [mcp_marker, hook_marker]


class Acceptance:
    def __init__(self, runtime, cli, output, repository):
        self.runtime, self.cli = runtime, cli
        self.output, self.repository = output, repository
        self.daemon = None
        self.rejecting_provider = None
        self.pidfds = {}
        self.sessions = []
        self.session_artifacts = {}
        self.host_labels = {}
        self.cleanup_errors = []
        source_paths = [
            Path(__file__).resolve(),
            repository / "scripts/fixtures/codex-responses-server.py",
            repository / "scripts/fixtures/restricted-exec-server.py",
            repository / "scripts/verify-codex-restricted-environment.py",
            repository / "crates/velella/src/restricted_cleanup.rs",
            repository / "crates/velella/src/restricted_controller.rs",
            repository / "crates/velella/src/host.rs",
            repository / "crates/velella/src/lib.rs",
            repository / "crates/velella/src/main.rs",
            repository / "crates/velella/src/desktop.rs",
            repository / "crates/velella/src/desktop/workbench_commands.rs",
            repository / "crates/velella/src/browser_agent.rs",
            repository / "crates/velella/src/process.rs",
            repository / "crates/velella/src/restricted.rs",
            repository / "crates/velella/src/restricted_fs.rs",
        ]
        self.receipt = {
            "schema": 1, "time": datetime.now(timezone.utc).isoformat(),
            "scope": "actual desktop daemon/terminal host + actual Codex + fixed local Responses",
            "status": "running", "no_real_model": True, "no_login": True,
            "fresh_home": True, "host_credentials_read": False,
            "credential_isolation_evidence": "fresh HOME/CODEX_HOME and minimal environment; not a syscall audit",
            "ui_exercised": False,
            "runtime_sha256": digest(runtime), "codex_sha256": digest(cli),
            "source_hashes": {str(path.relative_to(repository)): digest(path) for path in source_paths},
            "checks": [],
        }

    def check(self, name, passed, detail=None):
        if STOP.is_set():
            raise InterruptedError("desktop acceptance interrupted")
        record = {"name": name, "passed": bool(passed)}
        if detail is not None:
            record["detail"] = detail
        self.receipt["checks"].append(record)
        if not passed:
            raise AssertionError(name)

    def call(self, command, **arguments):
        return rpc(self.endpoint, {"cmd": command, **arguments})

    def capture(self, label, pid, *, parent=None, expected_start=None):
        before = process_identity(pid)
        if parent is not None and before["ppid"] != parent:
            raise AssertionError(f"{label}: process does not belong to expected parent")
        if expected_start is not None and before["start"] != str(expected_start):
            raise AssertionError(f"{label}: process identity does not match receipt")
        handle = os.pidfd_open(pid)
        after = process_identity(pid)
        if before["start"] != after["start"]:
            os.close(handle)
            raise AssertionError(f"{label}: process identity changed")
        if label in self.pidfds:
            os.close(handle)
            raise AssertionError(f"duplicate process ownership label {label}")
        self.pidfds[label] = {"fd": handle, "identity": after}
        return after

    def stopped(self, label, timeout=0):
        return bool(select.select([self.pidfds[label]["fd"]], [], [], timeout)[0])


    def descendants(self, label, pid):
        """Read only children of the already-owned host; never scan other users."""
        found = set()
        for task in Path(f"/proc/{pid}/task").iterdir():
            try:
                found.update(int(value) for value in (task / "children").read_text().split())
            except FileNotFoundError:
                pass
        for child in sorted(found):
            key = f"{label}/{child}"
            try:
                self.capture(key, child, parent=pid)
                self.descendants(key, child)
            except FileNotFoundError:
                pass

    def session_state(self, session_id):
        response = self.call("workbench.sessions", workspace_id=self.workspace_id)
        return next((item for item in response["sessions"] if item["id"] == session_id), None)




    def configuration_audit(self, name, state, destination):
        audit = json.loads((state / "config-audit.json").read_text())
        fields = ("verified", "project_layers", "project_layers_disabled", "private_profile_unchanged")
        selected = {key: audit.get(key) for key in fields}
        self.check(name, selected["verified"] is True
                   and selected["private_profile_unchanged"] is True
                   and type(selected["project_layers"]) is int and selected["project_layers"] >= 1
                   and type(selected["project_layers_disabled"]) is int
                   and selected["project_layers_disabled"] == selected["project_layers"], selected)
        destination.write_text(json.dumps(selected, indent=2) + "\n")


    def prepare(self, root):
        self.root = root
        self.data, self.controller, self.project, self.outside = (
            root / name for name in ("data", "daemon-home", "project", "outside"))
        for directory in (self.data, self.controller, self.project, self.outside):
            directory.mkdir(mode=0o700)
        self.codex_home = self.controller / ".codex"
        self.codex_home.mkdir(mode=0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        (binary_directory / "codex").symlink_to(self.cli)
        self.environment = {
            "PATH": str(binary_directory) + ":/usr/bin:/bin",
            "HOME": str(self.controller), "CODEX_HOME": str(self.codex_home),
            "LANG": "C.UTF-8", "TMPDIR": str(self.controller), "SHELL": "/bin/bash",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
            "VELELLA_SYNTHETIC_HOST_SECRET": "synthetic-only",
        }
        self.secret = self.outside / "synthetic.txt"
        self.secret.write_text("synthetic fixture only\n")
        self.secret_digest = digest(self.secret)
        (self.project / "README.txt").write_text("Disposable desktop Restricted acceptance project.\n")
        with (self.output / "git.log").open("wb") as log:
            for arguments in [
                ["-c", "init.defaultBranch=main", "init"],
                ["add", "README.txt"],
                ["-c", "user.name=Velella fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "--no-gpg-sign", "-m", "fixed local fixture"],
            ]:
                subprocess.run(["/usr/bin/git", *arguments], cwd=self.project,
                               env=self.environment, check=True, stdout=log, stderr=log, timeout=15)
        self.log = (self.output / "daemon.log").open("wb")
        self.daemon = subprocess.Popen(
            [str(self.runtime), "daemon", str(self.data)], cwd=root,
            env=self.environment, stdin=subprocess.DEVNULL,
            stdout=self.log, stderr=self.log, start_new_session=True,
        )
        self.capture("daemon", self.daemon.pid)
        self.endpoint = self.data / "daemon.sock"

        def started():
            if self.daemon.poll() is not None:
                raise RuntimeError("private daemon exited during startup")
            return self.call("snapshot")

        wait_for(started, 20, "private daemon readiness")
        project = self.call("project.add", path=str(self.project))["project"]
        self.main_workspace = "main:" + project["id"]
        workspace = self.call("workbench.create", project_id=project["id"],
                              name="Restricted desktop acceptance", base="HEAD",
                              request_id=str(uuid.uuid4()))["workspace"]
        self.workspace_id = workspace["id"]
        self.work = Path(workspace["path"]).resolve(strict=True)
        self.check("created_real_git_worktree", workspace["kind"] == "worktree"
                   and self.work != self.project and (self.work / ".git").is_file())
        self.check("primary_workspace_exists", any(
            item["id"] == self.main_workspace for item in self.call("workbench.snapshot")["workspaces"]))
        self.rejecting_provider = RejectingProvider()
        self.traps = install_workspace_traps(self.work, self.outside, self.rejecting_provider.base_url)
        rejected = rpc(self.endpoint, {
            "cmd": "workbench.terminal.open", "workspace_id": self.main_workspace,
            "launch": "codex", "request_id": str(uuid.uuid4()),
            "sandbox": {"mode": "restricted"},
        }, require_ok=False)
        self.check("restricted_primary_workspace_rejected", rejected.get("ok") is False, rejected)


    def cleanup(self):
        previous = {sig: signal.signal(sig, signal.SIG_IGN) for sig in (signal.SIGTERM, signal.SIGINT)}
        try:
            # Startup can fail before the normal ownership snapshot. Capture
            # any remaining children of our exact known host before stopping it.
            for session_id, label in self.host_labels.items():
                if not self.stopped(label):
                    try:
                        self.descendants("cleanup/" + session_id,
                                         self.pidfds[label]["identity"]["pid"])
                    except FileNotFoundError:
                        pass
                    except Exception as error:
                        self.cleanup_errors.append(f"capture host descendants: {error}")
            if self.daemon and self.daemon.poll() is None:
                try:
                    rpc(self.endpoint, {"cmd": "service.stop"}, timeout=15)
                except Exception as error:
                    self.cleanup_errors.append(f"service stop: {error}")
            # Ask each known adapter to clean up its process groups before any
            # hard fallback. All signals target pidfds captured from this test.
            for label, record in self.pidfds.items():
                if label.endswith("/adapter") and not self.stopped(label):
                    try:
                        signal.pidfd_send_signal(record["fd"], signal.SIGTERM)
                        if not self.stopped(label, 8):
                            self.cleanup_errors.append(f"{label} did not stop gracefully")
                    except ProcessLookupError:
                        pass
            if self.daemon:
                try:
                    self.daemon.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    self.cleanup_errors.append("private daemon did not stop gracefully")
            # The desktop host intentionally remains alive briefly after saving
            # its final status. Allow that bounded shutdown window to finish.
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                pending = [record["fd"] for label, record in self.pidfds.items()
                           if not self.stopped(label)]
                if not pending:
                    break
                select.select(pending, [], [], max(0, min(.2, deadline - time.monotonic())))
            for label, record in reversed(list(self.pidfds.items())):
                try:
                    if not self.stopped(label):
                        signal.pidfd_send_signal(record["fd"], signal.SIGKILL)
                        self.cleanup_errors.append(f"{label} required forced cleanup")
                    if not self.stopped(label, 5):
                        self.cleanup_errors.append(f"{label} still alive")
                except ProcessLookupError:
                    pass
                except Exception as error:
                    self.cleanup_errors.append(f"{label} cleanup: {error}")
                finally:
                    os.close(record["fd"])
            if self.daemon:
                self.daemon.wait(timeout=5)
            if self.rejecting_provider:
                try:
                    self.rejecting_provider.close()
                except Exception as error:
                    self.cleanup_errors.append(f"rejecting provider cleanup: {error}")
            if hasattr(self, "log"):
                self.log.close()
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)

    def finish(self, temporary):
        self.cleanup()
        if hasattr(self, "traps"):
            self.receipt["workspace_trap_markers_present"] = [
                path.name for path in self.traps if path.exists()]
        if hasattr(self, "secret_digest"):
            self.receipt["host_fixture_unchanged_after_cleanup"] = digest(self.secret) == self.secret_digest
        if self.rejecting_provider:
            summary = self.rejecting_provider.summary()
            self.receipt["workspace_provider_never_contacted"] = summary["connection_count"] == 0
            (self.output / "rejecting-provider-summary.json").write_text(
                json.dumps(summary, indent=2) + "\n")
        # Keep synthetic diagnostic evidence when startup fails, before removing
        # the test's private state. Never copy auth or configuration directories.
        for state, destination in self.session_artifacts.values():
            for name in ("codex-stderr.log", "status.json", "launch.json"):
                path = state / name
                try:
                    if path.is_symlink():
                        raise RuntimeError("diagnostic file unexpectedly became a symlink")
                    if path.exists():
                        with path.open("rb") as stream:
                            data = stream.read(MAX_RPC + 1)
                        if len(data) > MAX_RPC:
                            raise RuntimeError("diagnostic file exceeds limit")
                        (destination / name).write_bytes(data)
                except Exception as error:
                    self.cleanup_errors.append(f"{name} diagnostic capture: {error}")
        try:
            temporary.cleanup()
        except Exception as error:
            self.cleanup_errors.append(f"temporary cleanup: {error}")
        self.receipt["temporary_directory_removed"] = not Path(temporary.name).exists()
        self.receipt["cleanup_errors"] = self.cleanup_errors
        self.receipt["sources_unchanged"] = all(
            digest(self.repository / path) == value
            for path, value in self.receipt["source_hashes"].items())
        self.receipt["binaries_unchanged"] = (
            digest(self.runtime) == self.receipt["runtime_sha256"]
            and digest(self.cli) == self.receipt["codex_sha256"])
        if (self.cleanup_errors or not self.receipt["temporary_directory_removed"]
                or not self.receipt["sources_unchanged"] or not self.receipt["binaries_unchanged"]
                or self.receipt.get("workspace_trap_markers_present")
                or self.receipt.get("workspace_provider_never_contacted") is False
                or self.receipt.get("host_fixture_unchanged_after_cleanup") is False):
            self.receipt["status"] = "failed"
        pending = self.output / "result.json.tmp"
        pending.write_text(json.dumps(self.receipt, indent=2) + "\n")
        pending.replace(self.output / "result.json")
        print(json.dumps(self.receipt, indent=2))




