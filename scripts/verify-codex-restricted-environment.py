#!/usr/bin/env python3
"""Exercise actual Codex tool routing with a local fixed Responses fixture.

Uses a fresh HOME, never logs in, never loads host credentials, and never calls
a real model. Codex's experimental environments.toml transport connects its host
controller to an exec-server confined by the actual restricted-exec runtime.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import queue
import runpy
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time

STOP = threading.Event()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def identity(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
    return {"pid": pid, "start": int(fields[19])}


def executor(config_path):
    """Supervise the pipe-based executor without reaping its group leader early."""
    config = json.loads(Path(config_path).read_text())
    child = None
    stopping = threading.Event()

    def interrupted(*_args):
        stopping.set()

    signal.signal(signal.SIGTERM, interrupted)
    try:
        command = ([sys.executable, "-I", config["adapter"], "--config", config["adapter_config"]]
                   if config.get("adapter") else
                   [config["runtime"], "restricted-exec", json.dumps(config["spec"])])
        child = subprocess.Popen(
            command,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"},
            cwd=config["spec"]["cwd"], start_new_session=True,
        )
        Path(config["pid_file"]).write_text(json.dumps(
            {"supervisor": identity(os.getpid()), "executor": identity(child.pid)}))
        wire_lock = threading.Lock()

        def forward(source, destination, direction):
            try:
                for line in iter(lambda: source.readline(8 * 1024 * 1024 + 1), b""):
                    if len(line) > 8 * 1024 * 1024:
                        raise ValueError("oversized executor record")
                    with wire_lock:
                        with open(config["wire_file"], "ab") as log:
                            log.write(json.dumps({"direction": direction,
                                                  "message": json.loads(line)}).encode() + b"\n")
                    destination.write(line)
                    destination.flush()
            finally:
                if direction == "request":
                    destination.close()

        for source, destination, direction in (
            (sys.stdin.buffer, child.stdin, "request"),
            (child.stdout, sys.stdout.buffer, "response"),
        ):
            threading.Thread(target=forward, args=(source, destination, direction), daemon=True).start()

        def errors():
            for line in child.stderr:
                with open(config["stderr_file"], "ab") as log:
                    log.write(line)
                sys.stderr.buffer.write(line)
                sys.stderr.buffer.flush()

        threading.Thread(target=errors, daemon=True).start()
        while not os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
            if stopping.is_set():
                break
            time.sleep(.03)
    except InterruptedError:
        pass
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        if child is not None:
            if config.get("adapter"):
                # The trusted adapter owns per-operation groups: let it stop them
                # before terminating its own pinned group leader.
                # Popen.send_signal() internally polls/reaps; keep this leader
                # reserved until group cleanup is finished instead.
                try:
                    os.kill(child.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                deadline = time.monotonic() + 5
                while not os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
                    if time.monotonic() >= deadline:
                        break
                    time.sleep(.03)
            # waitid above does not reap: this group ID cannot have been reused.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
    return 0


class Client:
    def __init__(self, cli, cwd, env, output):
        self.process = subprocess.Popen(
            [str(cli), "app-server"], cwd=cwd, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True,
        )
        self.messages = queue.Queue()
        self.sequence = 0
        self.events = []
        self.output = output
        self.stderr = bytearray()

        def read():
            try:
                for line in iter(lambda: self.process.stdout.readline(8 * 1024 * 1024 + 1), b""):
                    if len(line) > 8 * 1024 * 1024:
                        raise ValueError("oversized app-server record")
                    self.messages.put(json.loads(line))
            except Exception as error:
                self.messages.put(error)
            finally:
                self.messages.put(EOFError("app-server closed"))

        def errors():
            while block := self.process.stderr.read(8192):
                if len(self.stderr) < 512 * 1024:
                    self.stderr.extend(block[:512 * 1024 - len(self.stderr)])

        self.readers = [threading.Thread(target=read, daemon=True),
                        threading.Thread(target=errors, daemon=True)]
        for reader in self.readers:
            reader.start()

    def send(self, value):
        self.process.stdin.write(json.dumps(value).encode() + b"\n")
        self.process.stdin.flush()

    def receive(self, timeout):
        deadline = time.monotonic() + timeout
        while True:
            if STOP.is_set():
                raise InterruptedError("offline acceptance interrupted")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("app-server response")
            try:
                value = self.messages.get(timeout=min(.1, remaining))
                break
            except queue.Empty:
                continue
        if isinstance(value, Exception):
            raise value
        # Unexpected tool/approval callbacks cannot execute host actions.
        if "method" in value and "id" in value:
            self.send({"id": value["id"], "error": {
                "code": -32601, "message": "Offline acceptance rejects client tool callbacks"}})
        self.events.append(value)
        return value

    def request(self, method, params, timeout=30):
        self.sequence += 1
        request_id = self.sequence
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = self.receive(max(.01, deadline - time.monotonic()))
            if value.get("id") == request_id and "method" not in value:
                return value
        raise TimeoutError(method)

    def completed_turn(self, thread_id, turn_id, timeout=60):
        deadline = time.monotonic() + timeout
        while True:
            for value in self.events:
                if (value.get("method") == "turn/completed"
                        and value.get("params", {}).get("threadId") == thread_id
                        and value["params"]["turn"]["id"] == turn_id):
                    return value["params"]["turn"]
            if time.monotonic() >= deadline:
                raise TimeoutError("turn/completed")
            self.receive(max(.01, deadline - time.monotonic()))

    def close(self):
        try:
            self.process.stdin.close()
            deadline = time.monotonic() + 5
            while not os.waitid(os.P_PID, self.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT):
                if time.monotonic() >= deadline:
                    break
                time.sleep(.03)
        finally:
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
                time.sleep(.1)
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait(timeout=5)
            for reader in self.readers:
                reader.join(timeout=1)
            (self.output / "app-server-stderr.log").write_bytes(self.stderr)
            # All input/output is synthetic, from an isolated fresh HOME.
            (self.output / "app-server-events.json").write_text(json.dumps(self.events, indent=2))


CHECK = r'''
import errno, json, os, pathlib, socket
work, outside, readonly, port = {work!r}, {outside!r}, {readonly!r}, {port}
checks = {{}}
checks["cwd"] = os.getcwd() == work
checks["host_env_removed"] = "VELELLA_SYNTHETIC_HOST_SECRET" not in os.environ
for name, path, mode in [("outside_read_denied", outside, "r"),
                         ("outside_write_denied", outside, "w"),
                         ("readonly_write_denied", readonly, "w")]:
    try:
        with open(path, mode) as f:
            if mode == "r": f.read()
            else: f.write("unexpected fixture write")
        checks[name] = False
    except OSError as e:
        checks[name] = e.errno in (errno.EACCES, errno.EPERM)
try:
    with socket.create_connection(("127.0.0.1", port), timeout=1): pass
    checks["network_denied"] = False
except OSError as e:
    checks["network_denied"] = e.errno in (errno.EACCES, errno.EPERM)
pathlib.Path(work, "tool-proof.json").write_text(json.dumps(checks))
assert all(checks.values()), checks
print("VELELLA_RESTRICTED_TOOL_CHECKS_PASSED", json.dumps(checks, sort_keys=True))
'''

LIFECYCLE = r'''
import json, os, pathlib, signal, time
root = pathlib.Path.cwd()
signal.signal(signal.SIGHUP, signal.SIG_IGN)
name = "parent"
child = os.fork()
if child == 0:
    name = "child"
    if os.fork() == 0:
        name = "grandchild"
(root / (name + ".json")).write_text(json.dumps({"pid": os.getpid(), "pgid": os.getpgrp()}))
if name == "parent":
    deadline = time.monotonic() + 3
    while not all((root / (n + ".json")).exists() for n in ("parent", "child", "grandchild")):
        assert time.monotonic() < deadline
        time.sleep(.02)
    print("VELELLA_RESTRICTED_DESCENDANTS_READY", flush=True)
while True:
    time.sleep(1)
'''


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=repo / "target/debug/velella-runtime")
    parser.add_argument("--codex", required=True, type=Path, help="Actual native Codex executable")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--executor-kind", choices=("native", "adapter"), default="adapter")
    args = parser.parse_args()
    if args.output.is_symlink() or args.output.exists() and any(args.output.iterdir()):
        parser.error("output must be new or empty; previous evidence is never overwritten")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    cli, runtime = args.codex.resolve(), args.runtime.resolve()
    fixture_path = repo / "scripts/fixtures/codex-responses-server.py"
    fixture = runpy.run_path(str(fixture_path))["Server"]
    receipt = {"schema": 1, "time": datetime.now(timezone.utc).isoformat(),
               "scope": "actual Codex + restricted tools + fixed local Responses fixture",
               "executor_kind": args.executor_kind,
               "status": "running", "no_real_model": True, "no_login": True,
               "fresh_home": True, "host_credentials_read": False,
               "credential_isolation_evidence": "fresh HOME/CODEX_HOME, file credential stores, clean environment; not a syscall audit",
               "experimental_protocol": True, "desktop_product_integration": False,
               "runtime_sha256": digest(runtime), "codex_sha256": digest(cli),
               "source_hashes": {str(p.relative_to(repo)): digest(p) for p in (
                   Path(__file__).resolve(), fixture_path, repo / "crates/velella/src/restricted.rs",
                   repo / "crates/velella/src/restricted_fs.rs")},
               "checks": []}
    adapter = repo / "scripts/fixtures/restricted-exec-server.py"
    if args.executor_kind == "adapter":
        receipt["source_hashes"][str(adapter.relative_to(repo))] = digest(adapter)
    client = None
    temporary = tempfile.TemporaryDirectory(prefix="vl-codex-restricted-environment-")
    root = Path(temporary.name).resolve()
    pid_handles = {}
    pid_file = root / "controller/executor-pids.json"
    cleanup_errors = []

    def capture_handles(required=False):
        if not pid_file.exists():
            if required:
                raise AssertionError("executor PID receipt missing")
            return
        for name, value in json.loads(pid_file.read_text()).items():
            if name in pid_handles:
                continue
            try:
                if identity(value["pid"]) != value:
                    raise AssertionError("executor process identity changed")
                handle = os.pidfd_open(value["pid"])
                if identity(value["pid"]) != value:
                    os.close(handle)
                    raise AssertionError("executor process identity changed")
                pid_handles[name] = handle
            except FileNotFoundError:
                if required:
                    raise AssertionError("executor exited before PID capture")
        if required and set(pid_handles) != {"supervisor", "executor"}:
            raise AssertionError("executor process ownership incomplete")

    def interrupted(*_args):
        STOP.set()

    previous_signals = {s: signal.signal(s, interrupted) for s in (signal.SIGTERM, signal.SIGINT)}

    def check(name, passed, detail=None):
        if STOP.is_set():
            raise InterruptedError("offline acceptance interrupted")
        record = {"name": name, "passed": bool(passed)}
        if detail is not None:
            record["detail"] = detail
        receipt["checks"].append(record)
        if not passed:
            raise AssertionError(name)

    try:
        controller, home, work, outside, readonly = (root / n for n in (
            "controller", "executor-home", "work", "outside", "readonly"))
        for directory in (controller, home, work, outside, readonly):
            directory.mkdir(mode=0o700)
        lifecycle_work = work / "lifecycle"
        lifecycle_work.mkdir()
        codex_home = controller / ".codex"
        codex_home.mkdir()
        secret, ro = outside / "synthetic.txt", readonly / "fixture.txt"
        secret.write_text("synthetic fixture only\n")
        ro.write_text("readonly fixture\n")
        before = [digest(secret), digest(ro)]
        spec = {"policy": {"mode": "restricted", "network": "denied", "home": str(home),
                           "read_only": [str(cli.parent), str(readonly)], "writable": []},
                "program": str(cli), "args": ["exec-server", "--listen", "stdio"],
                "cwd": str(work), "env": {}}
        config_path = controller / "executor.json"
        config = {"runtime": str(runtime), "spec": spec, "pid_file": str(pid_file),
                  "wire_file": str(output / "exec-server-wire.jsonl"),
                  "stderr_file": str(output / "exec-server-stderr.log")}
        if args.executor_kind == "adapter":
            adapter_config = controller / "adapter.json"
            adapter_config.write_text(json.dumps({
                "runtime": str(runtime), "work": str(work), "home": str(home),
                "read_only": [str(cli.parent), str(readonly)],
            }))
            adapter_config.chmod(0o600)
            config.update(adapter=str(adapter), adapter_config=str(adapter_config))
        config_path.write_text(json.dumps(config))
        config_path.chmod(0o600)
        wrapper = controller / "executor"
        wrapper.write_text("#!/bin/sh\nexec /usr/bin/python3 -I " +
                           shlex.quote(str(Path(__file__).resolve())) + " --executor " +
                           shlex.quote(str(config_path)) + "\n")
        wrapper.chmod(0o700)
        (codex_home / "environments.toml").write_text(
            'include_local = false\ndefault = "restricted-fixture"\n'
            '[[environments]]\nid = "restricted-fixture"\nprogram = ' +
            json.dumps(str(wrapper)) + '\ncwd = ' + json.dumps(str(work)) + '\n')
        # Placeholder arguments are replaced only with owned fixture paths/port.
        fallback_marker = controller / "unexpected-host-fallback"
        resumed_marker = work / "unexpected-command-after-disconnect"
        disconnected_command = (
            "from pathlib import Path; "
            f"Path({str(resumed_marker)!r}).write_text('unexpected command'); "
            f"Path({str(fallback_marker)!r}).write_text('unexpected host fallback')"
        )
        steps = [{"kind": "tool", "name": "exec_command", "arguments": {},
                  "expect_output_contains": "VELELLA_RESTRICTED_TOOL_CHECKS_PASSED"},
                 {"kind": "tool", "name": "exec_command", "arguments": {
                     "cmd": "/usr/bin/python3 -I -c " + shlex.quote(LIFECYCLE),
                     "workdir": str(lifecycle_work), "yield_time_ms": 1000,
                     "max_output_tokens": 500,
                  }, "expect_output_contains": "VELELLA_RESTRICTED_DESCENDANTS_READY"},
                 {"kind": "final", "text": "Fixed local acceptance completed."},
                 {"kind": "tool", "name": "exec_command", "arguments": {
                     "cmd": "/usr/bin/python3 -I -c " + shlex.quote(disconnected_command),
                     "workdir": str(work), "max_output_tokens": 1000,
                  }, "expect_output_contains": "exec-server"},
                 {"kind": "final", "text": "The fixed tool correctly reported executor failure."}]
        with fixture(steps, output / "responses", timeout=120) as server:
            code = CHECK.format(work=str(work), outside=str(secret), readonly=str(ro), port=server.port)
            server.steps[0]["arguments"] = {
                "cmd": "/usr/bin/python3 -I -c " + shlex.quote(code),
                "workdir": str(work), "max_output_tokens": 1200,
            }
            (codex_home / "config.toml").write_text(
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
                'workspace_dependencies = false\n'
                'remote_plugin = false\nremote_control = false\nremote_models = false\n'
                'memories = false\nchronicle = false\ncode_mode = false\ncode_mode_only = false\n'
                'js_repl = false\nskip_host_skill_discovery = true\nunified_exec = true\n'
                'enable_request_compression = false\nresponses_websockets = false\n'
                'responses_websockets_v2 = false\n'
                '[model_providers.fixture]\nname = "Fixed local fixture"\n'
                'base_url = ' + json.dumps(server.base_url) + '\nwire_api = "responses"\n'
                'requires_openai_auth = false\nrequest_max_retries = 0\n'
                'stream_max_retries = 0\nsupports_websockets = false\n'
                'stream_idle_timeout_ms = 10000\n')
            environment = {"PATH": "/usr/bin:/bin", "HOME": str(controller),
                           "CODEX_HOME": str(codex_home), "LANG": "C.UTF-8",
                           "TMPDIR": str(controller), "SHELL": "/bin/bash",
                           "VELELLA_SYNTHETIC_HOST_SECRET": "synthetic-only"}
            client = Client(cli, controller, environment, output)
            initialized = client.request("initialize", {
                "clientInfo": {"name": "velella_offline_fixture", "version": "1.0"},
                "capabilities": {"experimentalApi": True}})
            check("app_server_initialize", "result" in initialized, initialized.get("error"))
            client.send({"method": "initialized"})
            info = client.request("environment/info", {"environmentId": "restricted-fixture"})
            check("restricted_environment_ready",
                  info.get("result", {}).get("cwd") == work.as_uri(), info)
            capture_handles(required=True)
            local = client.request("environment/info", {"environmentId": "local"})
            check("host_local_environment_disabled",
                  local.get("error", {}).get("code") == -32600 and
                  local["error"]["message"] == "unknown environment id `local`", local)
            selected = [{"environmentId": "restricted-fixture", "cwd": str(work),
                         "runtimeWorkspaceRoots": [str(work)]}]
            thread = client.request("thread/start", {
                "model": "mock-model", "modelProvider": "fixture", "cwd": str(work),
                "sandbox": "danger-full-access", "approvalPolicy": "never",
                "environments": selected, "ephemeral": True})
            check("thread_started", "result" in thread, thread.get("error"))
            thread_id = thread["result"]["thread"]["id"]
            turn = client.request("turn/start", {
                "threadId": thread_id,
                "input": [{"type": "text", "text": "Run the fixed offline acceptance tool."}],
                "environments": selected, "approvalPolicy": "never",
                "sandboxPolicy": {"type": "externalSandbox", "networkAccess": "restricted"}})
            check("turn_started", "result" in turn, turn.get("error"))
            completed = client.completed_turn(thread_id, turn["result"]["turn"]["id"], timeout=90)
            check("turn_completed", completed.get("status") == "completed", completed)
            first_summary = server.summary()
            check("first_turn_fixed_tool_outputs_returned",
                  first_summary["completed_steps"] == 3 and
                  first_summary["request_count"] == 3 and not first_summary["errors"]
                  and server.pending_call is None)
            proof = json.loads((work / "tool-proof.json").read_text())
            expected = {"cwd", "host_env_removed", "outside_read_denied",
                        "outside_write_denied", "readonly_write_denied", "network_denied"}
            check("actual_restricted_tool_checks",
                  set(proof) == expected and all(value is True for value in proof.values()), proof)
            check("host_fixture_unchanged", before == [digest(secret), digest(ro)])
            check("fresh_homes_no_auth", not any(
                p.exists() for p in (codex_home / "auth.json", home / ".codex/auth.json")))
            lifecycle = {}
            for name in ("parent", "child", "grandchild"):
                value = json.loads((lifecycle_work / (name + ".json")).read_text())
                owned = identity(value["pid"])
                handle = os.pidfd_open(value["pid"])
                pid_handles["lifecycle_" + name] = handle
                check("lifecycle_" + name + "_running",
                      identity(value["pid"]) == owned and
                      os.getpgid(value["pid"]) == value["pgid"] and
                      not select.select([handle], [], [], 0)[0])
                lifecycle[name] = value
            check("descendants_share_owned_group",
                  len({value["pgid"] for value in lifecycle.values()}) == 1)
            # Prevent reconnection, then remove the owned execution service.
            # The controller can still request model output. Its next actual
            # tool call must report executor failure, without running on host.
            wrapper.write_text("#!/bin/sh\nexit 73\n")
            signal.pidfd_send_signal(pid_handles["executor"], signal.SIGTERM)
            check("executor_stopped", bool(select.select(
                [pid_handles["executor"]], [], [], 5)[0]))
            for name in lifecycle:
                check("disconnect_terminated_" + name, bool(select.select(
                    [pid_handles["lifecycle_" + name]], [], [], 5)[0]))
            disconnected = client.request("environment/status", {"environmentId": "restricted-fixture"})
            receipt["disconnect_status"] = disconnected
            check("environment_reports_disconnected",
                  disconnected.get("result", {}).get("status") == "disconnected")
            failed_turn = client.request("turn/start", {
                "threadId": thread_id,
                "input": [{"type": "text", "text": "Try the fixed tool once against the deliberately unavailable environment."}],
                "environments": selected, "approvalPolicy": "never",
                "sandboxPolicy": {"type": "externalSandbox", "networkAccess": "restricted"}})
            check("disconnected_tool_test_turn_started", "result" in failed_turn, failed_turn.get("error"))
            failure = client.completed_turn(
                thread_id, failed_turn["result"]["turn"]["id"], timeout=30)
            check("executor_failure_returned_to_model", failure.get("status") == "completed", failure)
            server.assert_complete()
            check("all_fixed_tool_outputs_verified", all(server.assertions.values()), server.assertions)
            allowed_tools = {"exec_command", "write_stdin", "request_user_input"}
            check("only_reviewed_tools_advertised", all(
                "exec_command" in record["tool_names"] and
                set(record["tool_names"]) <= allowed_tools
                for record in server.summary()["requests"]))
            check("no_command_or_host_fallback_after_disconnect",
                  not resumed_marker.exists() and not fallback_marker.exists())
            check("host_fixture_unchanged_after_disconnect", before == [digest(secret), digest(ro)])
            server.assert_complete()
        receipt["status"] = "passed"
    except Exception as error:
        receipt.update(status="failed", error=f"{type(error).__name__}: {error}")
    finally:
        for sig in previous_signals:
            signal.signal(sig, signal.SIG_IGN)
        try:
            capture_handles()
        except Exception as error:
            cleanup_errors.append(f"capture process ownership: {error}")
        # Stop the supervisor first, allowing it to kill/reap its confined group.
        if "supervisor" in pid_handles:
            try:
                signal.pidfd_send_signal(pid_handles["supervisor"], signal.SIGTERM)
                if not select.select([pid_handles["supervisor"]], [], [], 5)[0]:
                    cleanup_errors.append("executor supervisor did not stop")
            except ProcessLookupError:
                pass
        if client:
            try:
                client.close()
            except Exception as error:
                cleanup_errors.append(f"controller cleanup: {error}")
        for name, handle in pid_handles.items():
            try:
                if not select.select([handle], [], [], 0)[0]:
                    signal.pidfd_send_signal(handle, signal.SIGKILL)
                if not select.select([handle], [], [], 5)[0]:
                    cleanup_errors.append(f"{name} still alive")
            except ProcessLookupError:
                pass
            except Exception as error:
                cleanup_errors.append(f"{name} cleanup: {error}")
            finally:
                os.close(handle)
        try:
            temporary.cleanup()
        except Exception as error:
            cleanup_errors.append(f"temporary directory cleanup: {error}")
        receipt["temporary_directory_removed"] = not root.exists()
        try:
            receipt["sources_unchanged"] = all(
                digest(repo / path) == value for path, value in receipt["source_hashes"].items())
            receipt["binaries_unchanged"] = (
                digest(runtime) == receipt["runtime_sha256"] and
                digest(cli) == receipt["codex_sha256"])
        except Exception as error:
            cleanup_errors.append(f"source identity: {error}")
            receipt["sources_unchanged"] = False
            receipt["binaries_unchanged"] = False
        receipt["cleanup_errors"] = cleanup_errors
        if (cleanup_errors or not receipt["temporary_directory_removed"]
                or not receipt["sources_unchanged"] or not receipt["binaries_unchanged"]):
            receipt["status"] = "failed"
        pending = output / "result.json.tmp"
        pending.write_text(json.dumps(receipt, indent=2) + "\n")
        pending.replace(output / "result.json")
        for sig, previous in previous_signals.items():
            signal.signal(sig, previous)
        print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--executor":
        raise SystemExit(executor(sys.argv[2]))
    raise SystemExit(main())
