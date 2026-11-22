#!/usr/bin/env python3
"""Exercise actual desktop PTY -> native Codex TUI -> fixed Restricted tools.

Uses only owned temporary projects, fresh HOME/CODEX_HOME, and fixed loopback
Responses. No login, host credentials, real model, or production configuration.
--browser holds an isolated backend until OUTPUT/cleanup.request is created.
--shell-only exercises native ! commands without making any model request.
"""
import argparse
import base64
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import runpy
import shlex
import signal
import stat
import tempfile
import threading
import time
import uuid

REPOSITORY = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(REPOSITORY / "scripts/fixtures/desktop-terminal-support.py"))
FIXTURE = runpy.run_path(str(REPOSITORY / "scripts/fixtures/codex-responses-server.py"))
LIFECYCLE = runpy.run_path(str(REPOSITORY / "scripts/verify-codex-restricted-environment.py"))["LIFECYCLE"]
Acceptance = HELPERS["Acceptance"]
rpc, wait_for, digest = (HELPERS[name] for name in ("rpc", "wait_for", "digest"))
STOP = HELPERS["STOP"]
TITLE_SCHEMA = {
    "type": "object", "properties": {"title": {"type": "string", "minLength": 1, "maxLength": 36}},
    "required": ["title"], "additionalProperties": False,
}


class TitleAwareServer:
    """Add the native TUI's fixed metadata reply to the existing Responses fixture.

Only the exact title schema is handled separately. All other requests are
forwarded to our own fixed loopback fixture, whose address is never caller supplied.
"""

    def __init__(self, steps, output, *, timeout=120, expect_shell_context=None):
        self.inner = FIXTURE["Server"](steps, Path(output) / "main", timeout=timeout)
        self.steps, self.output = self.inner.steps, Path(output)
        self.lock = threading.Lock()
        self.requests = self.titles = 0
        self.errors = []
        self.expect_shell_context = expect_shell_context
        self.shell_context_verified = None

    def __enter__(self):
        self.inner.__enter__()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            def setup(self):
                super().setup()
                self.connection.settimeout(10)

            def reply(self, status, content, content_type="application/json"):
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(content)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(content)
                self.close_connection = True

            def do_POST(self):
                try:
                    with owner.lock:
                        owner.requests += 1
                        if owner.requests > len(owner.steps) + 3:
                            raise ValueError("bounded native TUI fixture request count exceeded")
                    if self.path != "/v1/responses":
                        raise ValueError("unexpected fixture path")
                    if any(self.headers.get(key) for key in ("Authorization", "Proxy-Authorization", "Cookie")):
                        raise ValueError("credential-bearing request rejected without recording headers")
                    if self.headers.get("Transfer-Encoding") or self.headers.get("Content-Encoding", "identity") != "identity":
                        raise ValueError("fixture requires uncompressed Content-Length JSON")
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length <= 4 * 1024 * 1024:
                        raise ValueError("request length outside fixed bound")
                    encoded = self.rfile.read(length)
                    body = json.loads(encoded)
                    if len(encoded) != length or not isinstance(body, dict) or body.get("stream") is not True:
                        raise ValueError("incomplete or non-streaming JSON request")
                    if body.get("model") != "mock-model":
                        raise ValueError("unexpected model; workspace configuration may have overridden the fixture")
                    text = body.get("text", {})
                    format_value = text.get("format", {}) if isinstance(text, dict) else {}
                    if format_value.get("type") == "json_schema" and format_value.get("schema") == TITLE_SCHEMA:
                        if body.get("tools", []) != []:
                            raise ValueError("metadata request unexpectedly advertised tools")
                        with owner.lock:
                            owner.titles += 1
                            if owner.titles != 1:
                                raise ValueError("only one fixed title request is allowed")
                        item = {
                            "type": "message", "id": "fixed-native-title", "role": "assistant", "status": "completed",
                            "content": [{"type": "output_text", "text": '{"title":"Verify restricted native TUI"}', "annotations": []}],
                        }
                        return self.reply(200, FIXTURE["_sse"]("title", item), "text/event-stream")
                    if owner.expect_shell_context is not None and owner.shell_context_verified is None:
                        command, marker = owner.expect_shell_context
                        messages = [
                            content.get("text", "")
                            for item in body.get("input", [])
                            if isinstance(item, dict) and item.get("role") == "user"
                            for content in item.get("content", [])
                            if isinstance(content, dict) and content.get("type") == "input_text"
                        ]
                        matched = any("<user_shell_command>" in text and command in text
                                      and "Exit code: 0" in text and marker in text for text in messages)
                        with owner.lock:
                            owner.shell_context_verified = matched
                        if not matched:
                            raise ValueError("first model request did not contain completed native shell command and output")
                    connection = http.client.HTTPConnection("127.0.0.1", owner.inner.port, timeout=15)
                    try:
                        connection.request("POST", "/v1/responses", body=encoded,
                                           headers={"Content-Type": "application/json"})
                        response = connection.getresponse()
                        payload = response.read(4 * 1024 * 1024 + 1)
                        if len(payload) > 4 * 1024 * 1024:
                            raise ValueError("fixed fixture response outside size bound")
                        self.reply(response.status, payload, response.getheader("Content-Type", "application/json"))
                    finally:
                        connection.close()
                except Exception as error:
                    # This fixture owns every prompt and records only authored reasons.
                    reason = str(error) if type(error) is ValueError else type(error).__name__
                    with owner.lock:
                        owner.errors.append(reason)
                    self.reply(400, b'{"error":"native TUI fixed fixture rejected request"}')

            def reject(self):
                with owner.lock:
                    owner.errors.append("unexpected HTTP method")
                self.reply(405, b'{"error":"unexpected HTTP method"}')

            do_GET = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = reject

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.port = self.httpd.server_port
        self.base_url = f"http://127.0.0.1:{self.port}/v1"
        self.worker = threading.Thread(target=self.httpd.serve_forever, kwargs={"poll_interval": .05}, daemon=True)
        self.worker.start()
        return self

    def summary(self):
        inner = self.inner.summary()
        with self.lock:
            return {
                "scope": "fixed native TUI Responses; forwards only to owned loopback fixture",
                "no_external_forwarding": True, "request_count": self.requests,
                "title_requests": self.titles, "errors": list(self.errors),
                "shell_context_verified": self.shell_context_verified,
                "main": inner, "complete": inner["complete"] and not self.errors,
            }

    def __exit__(self, *args):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.worker.join(timeout=5)
        self.inner.__exit__(*args)
        (self.output / "responses-summary.json").write_text(json.dumps(self.summary(), indent=2) + "\n")


def plain_terminal(raw):
    text = raw.decode(errors="replace")
    text = re.sub(r"\x1b\][^\x07]*(?:\x07|\x1b\\)", "", text)
    return re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)


class TerminalAcceptance(Acceptance):
    def __init__(self, *args):
        super().__init__(*args)
        self.receipt["scope"] = "actual desktop daemon/PTY host + native Codex TUI + fixed local Responses"
        self.receipt["ui_exercised"] = False
        self.streams = {}
        self.bridge_sockets = {}
        self.cleaned = False
        for relative in (
            "scripts/verify-desktop-restricted-terminal.py",
            "crates/velella/src/terminal_sandbox.rs",
            "crates/velella/src/terminal_config_audit.rs",
            "crates/velella/src/terminal_bridge.rs",
            "crates/velella/src/terminal_recovery.rs",
            "crates/velella/src/restricted_tools.rs",
            "crates/velella/src/desktop/terminal_io.rs",
            "desktop/src/TerminalView.tsx",
            "scripts/fixtures/restricted-terminal-bridge.py",
            "crates/velella/src/sessions.rs",
        ):
            path = self.repository / relative
            if path.exists():
                self.receipt["source_hashes"][relative] = digest(path)

    def cleanup(self):
        if not self.cleaned:
            super().cleanup()
            for state, output in self.session_artifacts.values():
                for name in ("terminal-bridge.json", "terminal-bridge-diagnostics.json"):
                    path = state / name
                    if path.exists():
                        if path.is_symlink() or path.stat().st_size > 1024 * 1024:
                            self.cleanup_errors.append(name + " unsafe diagnostic capture")
                        else:
                            (output / name).write_bytes(path.read_bytes())
            for socket in self.bridge_sockets.values():
                if socket.parent.exists():
                    self.cleanup_errors.append("owned terminal bridge socket directory survived cleanup")
                    # Only exact private paths already validated while live.
                    # Preserve the failed check while removing our fixture.
                    try:
                        if socket.is_socket():
                            socket.unlink()
                        socket.parent.rmdir()
                    except OSError as error:
                        self.cleanup_errors.append("owned bridge socket cleanup: " + type(error).__name__)
            self.cleaned = True

    def pump(self, session_id):
        stream = self.streams.setdefault(session_id, {"offset": 0, "raw": bytearray(), "geometry": []})
        response = self.call("workbench.terminal.output", workspace_id=self.workspace_id,
                             session_id=session_id, since=stream["offset"])
        chunk = base64.b64decode(response["data"], validate=True)
        stream["offset"] = response["end"]
        stream["raw"].extend(chunk)
        stream["geometry"].extend(response.get("geometry", []))
        if b"\x1b[6n" in chunk:
            self.call("workbench.terminal.input", workspace_id=self.workspace_id, session_id=session_id,
                      text="\x1b[1;1R", enter=False, input_kind="protocol")
        return plain_terminal(stream["raw"])

    def text_ready(self, session_id, marker):
        text = self.pump(session_id)
        return text if marker in text else None

    def enter(self, session_id, text, *, paste=True):
        payload = "\x1b[200~" + text + "\x1b[201~" if paste else text
        self.call("workbench.terminal.input", workspace_id=self.workspace_id, session_id=session_id,
                  text=payload, enter=False, input_kind="user")
        time.sleep(.6)  # Native TUI paste classification must settle before Enter.
        self.call("workbench.terminal.input", workspace_id=self.workspace_id, session_id=session_id,
                  text="", enter=True, input_kind="user")

    def open_terminal(self, name):
        sid = str(uuid.uuid4())
        output = self.output / name
        output.mkdir()
        state = self.data / "sandbox-chats" / sid
        self.session_artifacts[sid] = (state, output)
        request = {"cmd": "workbench.terminal.open", "workspace_id": self.workspace_id,
                   "launch": "codex", "request_id": sid, "sandbox": {"mode": "restricted"}}
        opened = rpc(self.endpoint, request)
        self.sessions.append(sid)
        self.check(name + "_terminal_opened", opened.get("session_id") == sid)
        record = self.session_state(sid)
        self.capture(name + "/host", record["status"]["pid"], parent=self.daemon.pid)
        self.host_labels[sid] = name + "/host"
        self.check(name + "_native_terminal_metadata",
                   record.get("execution_mode") == "restricted" and record.get("native_chat") is False
                   and record.get("kind") == "terminal", record)
        self.call("workbench.terminal.resize", workspace_id=self.workspace_id, session_id=sid, rows=40, cols=140)
        wait_for(lambda: self.text_ready(sid, "OpenAI Codex"), 30, "native TUI startup")
        wait_for(lambda: self.text_ready(sid, "mock-model"), 15, "fixed provider displayed")
        descriptor = json.loads((state / "launch.json").read_text())
        self.check(name + "_descriptor_keeps_fixed_policy",
                   descriptor["spawn"]["adapter"] == "codex-restricted-terminal"
                   and descriptor["spawn"]["sandbox"]["mode"] == "restricted"
                   and descriptor["spawn"]["sandbox"]["network"] == "denied"
                   and descriptor["spawn"]["sandbox"]["writable"] == []
                   and descriptor["spawn"]["sandbox"]["read_only"] == [])
        self.check(name + "_control_outside_work", not state.is_relative_to(self.work))
        self.configuration_audit(name + "_configuration_audited", state, output / "config-audit.json")
        same = rpc(self.endpoint, request)
        self.check(name + "_launch_idempotent", same.get("session_id") == sid)
        downgrade = rpc(self.endpoint, {**request, "sandbox": {"mode": "native"}}, require_ok=False)
        self.check(name + "_native_downgrade_rejected", downgrade.get("ok") is False)
        self.check(name + "_real_pty_recording", (state / "terminal.bin").is_file()
                   and b"\x1b[" in self.streams[sid]["raw"])
        bridge = json.loads((state / "terminal-bridge.json").read_text())
        socket = Path(bridge["socket"])
        self.check(name + "_bridge_socket_is_private",
                   socket.is_absolute() and socket.name == "rpc.sock"
                   and socket.parent.parent == Path("/tmp")
                   and re.fullmatch(r"vl-tui-[0-9a-f-]{36}", socket.parent.name) is not None
                   and not socket.parent.is_symlink() and stat.S_IMODE(socket.parent.stat().st_mode) == 0o700
                   and socket.is_socket() and stat.S_IMODE(socket.stat().st_mode) == 0o600)
        self.bridge_sockets[sid] = socket
        return sid, state, output

    def bridge_removed(self, name, sid):
        socket = self.bridge_sockets[sid]
        wait_for(lambda: not socket.parent.exists(), 10, name + " private bridge socket directory removed")
        self.check(name + "_bridge_socket_and_directory_removed", not socket.exists() and not socket.parent.exists())

    def capture_tree(self, name, sid):
        host_pid = self.pidfds[name + "/host"]["identity"]["pid"]
        self.descendants(name + "/tree", host_pid)
        native, tui, servers = [], [], []
        for label, record in self.pidfds.items():
            if label.startswith(name + "/tree"):
                try:
                    if Path(f"/proc/{record['identity']['pid']}/exe").resolve(strict=True) == self.cli:
                        argv = Path(f"/proc/{record['identity']['pid']}/cmdline").read_bytes().split(b"\0")
                        entry = {**record["identity"], "role": "unknown"}
                        if b"--remote" in argv or any(value.startswith(b"--remote=") for value in argv):
                            entry["role"] = "remote-native-tui"
                            tui.append(entry)
                        elif b"app-server" in argv:
                            entry["role"] = "native-app-server"
                            servers.append(entry)
                        native.append(entry)
                except FileNotFoundError:
                    pass
        self.check(name + "_actual_native_codex_elf",
                   len(tui) == 1 and len(servers) == 1 and len(native) == 2, native)

    def save_terminal(self, sid, output):
        self.pump(sid)
        stream = self.streams[sid]
        (output / "terminal.raw").write_bytes(stream["raw"])
        (output / "terminal-stripped.txt").write_text(plain_terminal(stream["raw"]))
        (output / "geometry.json").write_text(json.dumps(stream["geometry"], indent=2) + "\n")

    def main_scenario(self):
        case = self.output / "main-responses"
        final = "LOCAL FIXTURE: native terminal and restricted tools verified; no real model."
        lifecycle = self.work / "native-terminal-lifecycle"
        lifecycle.mkdir()
        steps = [
            {"kind": "tool", "name": "exec_command", "arguments": {},
             "expect_output_contains": "VELELLA_DESKTOP_RESTRICTED_BOUNDARY_PASSED"},
            {"kind": "tool", "name": "exec_command", "arguments": {
                "cmd": "/usr/bin/python3 -I -c " + shlex.quote(LIFECYCLE),
                "workdir": str(lifecycle), "yield_time_ms": 1000, "max_output_tokens": 500},
             "expect_output_contains": "VELELLA_RESTRICTED_DESCENDANTS_READY"},
            {"kind": "final", "text": final},
        ]
        shell_context = "VELELLA_NATIVE_SHELL_CONTEXT_VERIFIED"
        shell_command = "printf 'VELELLA_NATIVE_SHELL_%s\\n' CONTEXT_VERIFIED"
        with TitleAwareServer(steps, case, expect_shell_context=(shell_command, shell_context)) as fixture:
            (self.codex_home / "config.toml").write_text(HELPERS["config_text"](fixture.base_url))
            sid, state, output = self.open_terminal("main")
            profile = digest(state / "controller/codex/config.toml")
            proof = self.work / "native-terminal-proof.json"
            code = HELPERS["BOUNDARY"].format(
                work=str(self.work), home=str(state / "tool-home"), outside=str(self.secret),
                control=str(state), proof=str(proof), port=fixture.port)
            fixture.steps[0]["arguments"] = {
                "cmd": "/usr/bin/python3 -I -c " + shlex.quote(code),
                "workdir": str(self.work), "max_output_tokens": 1600,
            }
            self.shell_enter(sid, shell_command)
            self.shell_marker(sid, shell_context)
            self.check("native_shell_before_model_does_not_request_model",
                       fixture.summary()["request_count"] == 0 and self.shell_alive(sid))
            self.enter(sid, "Run the fixed local Restricted terminal acceptance.")
            wait_for(lambda: self.text_ready(sid, "LOCAL FIXTURE: native terminal"), 60, "native TUI fixed reply")
            wait_for(lambda: fixture.summary()["complete"], 10, "fixed tool outputs returned")
            checks = json.loads(proof.read_text())
            self.check("native_tools_all_boundary_checks", len(checks) == 12 and all(checks.values()), checks)
            self.check("native_worktree_provider_not_contacted", self.rejecting_provider.summary()["connection_count"] == 0)
            self.check("native_workspace_hooks_mcp_not_executed", not any(path.exists() for path in self.traps))
            self.check("native_private_profile_unchanged", digest(state / "controller/codex/config.toml") == profile)
            self.check("native_no_auth_files", not any(path.exists() for path in (
                self.codex_home / "auth.json", state / "controller/codex/auth.json", state / "tool-home/.codex/auth.json")))
            self.call("workbench.terminal.resize", workspace_id=self.workspace_id, session_id=sid, rows=32, cols=100)
            wait_for(lambda: any(item["rows"] == 32 and item["cols"] == 100
                                 for item in (self.pump(sid) and self.streams[sid]["geometry"])),
                     5, "PTY resize geometry persisted")
            self.check("native_terminal_resize_recorded", True)
            identities = {}
            for role in ("parent", "child", "grandchild"):
                identities[role] = self.capture("main/lifecycle/" + role, json.loads((lifecycle / (role + ".json")).read_text())["pid"])
            self.check("native_live_descendants_share_owned_group",
                       len({item["pgid"] for item in identities.values()}) == 1)
            self.capture_tree("main", sid)
            summary = fixture.summary()
            self.check("native_fixed_responses_and_optional_title",
                       summary["main"]["request_count"] == 3 and 0 <= summary["title_requests"] <= 1
                       and summary["request_count"] == 3 + summary["title_requests"]
                       and not summary["errors"], summary)
            self.check("native_shell_output_reaches_real_upstream_model_context",
                       summary["shell_context_verified"] is True)
            self.check("native_tools_only_expected_inventory",
                       all(set(item.get("tool_names", [])) <= HELPERS["ALLOWED_TOOLS"]
                           for item in summary["main"]["requests"]))
            started = time.monotonic()
            self.call("workbench.terminal.stop", workspace_id=self.workspace_id, session_id=sid, confirmed=True)
            elapsed = round(time.monotonic() - started, 3)
            self.check("native_desktop_stop_bounded", elapsed < 15, {"seconds": elapsed})
            for label in list(self.pidfds):
                if label.startswith("main/"):
                    self.check("stopped_" + label, self.stopped(label, 5))
            self.bridge_removed("main", sid)
            self.save_terminal(sid, output)
            after = rpc(self.endpoint, {"cmd": "workbench.terminal.recover", "workspace_id": self.workspace_id,
                                       "session_id": sid, "sandbox": {"mode": "native"}}, require_ok=False)
            self.check("native_recovery_refuses_downgrade", after.get("ok") is False)
            self.receipt["main_provider"] = fixture.summary()

    def shell_alive(self, sid):
        record = self.session_state(sid)
        return record is not None and record["status"].get("exit") is None

    def shell_enter(self, sid, command):
        self.enter(sid, "!" + command, paste=False)

    def shell_marker(self, sid, marker, timeout=20):
        # Markers never appear literally in the entered command, so an echo of
        # the native input field cannot satisfy the output assertion.
        return wait_for(lambda: self.text_ready(sid, marker), timeout, marker)

    def shell_script(self, relative, content):
        destination = self.work / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(content)
        return "/usr/bin/python3 -I " + shlex.quote(str(destination))

    def shell_scenario(self):
        with TitleAwareServer([{"kind": "final", "text": "UNEXPECTED MODEL REQUEST"}],
                              self.output / "shell-responses") as fixture:
            (self.codex_home / "config.toml").write_text(HELPERS["config_text"](fixture.base_url))
            sid, state, output = self.open_terminal("shell")
            self.capture_tree("shell", sid)
            started = time.monotonic()
            profile = digest(state / "controller/codex/config.toml")
            proof = self.work / "native-shell-proof.json"
            boundary = HELPERS["BOUNDARY"].format(
                work=str(self.work), home=str(state / "tool-home"), outside=str(self.secret),
                control=str(state), proof=str(proof), port=fixture.port)
            command = self.shell_script("shell-cases/boundary.py", boundary)
            self.shell_enter(sid, command)
            self.shell_marker(sid, "VELELLA_DESKTOP_RESTRICTED_BOUNDARY_PASSED")
            checks = json.loads(proof.read_text())
            self.check("shell_all_12_boundary_checks", len(checks) == 12 and all(checks.values()), checks)
            self.check("shell_success_keeps_terminal_alive", self.shell_alive(sid))

            chinese = "中文终端输出验证通过"
            chinese_output = self.work / "shell-cases/chinese.txt"
            command = self.shell_script("shell-cases/chinese.py",
                "from pathlib import Path\n"
                f"value = bytes.fromhex({chinese.encode().hex()!r}).decode()\n"
                f"Path({str(chinese_output)!r}).write_text(value)\nprint(value, flush=True)\n")
            self.shell_enter(sid, command)
            self.shell_marker(sid, chinese)
            self.check("shell_chinese_output", chinese_output.read_text() == chinese)

            pipeline = self.work / "shell-cases/pipeline.txt"
            self.shell_enter(sid, "printf 'native\\nshell\\n' | /usr/bin/tr a-z A-Z > "
                             + shlex.quote(str(pipeline))
                             + "; /usr/bin/cat " + shlex.quote(str(pipeline))
                             + "; printf 'PIPELINE_%s\\n' VERIFIED")
            self.shell_marker(sid, "PIPELINE_VERIFIED")
            self.check("shell_pipe_and_redirection", pipeline.read_text() == "NATIVE\nSHELL\n")

            command = self.shell_script("shell-cases/nonzero.py",
                "import sys\n"
                "print('EXPECTED_' + 'SHELL_STDOUT', flush=True)\n"
                "print('EXPECTED_' + 'SHELL_STDERR', file=sys.stderr, flush=True)\n"
                "raise SystemExit(7)\n")
            self.shell_enter(sid, command)
            self.shell_marker(sid, "EXPECTED_SHELL_STDOUT")
            self.shell_marker(sid, "EXPECTED_SHELL_STDERR")
            self.check("shell_nonzero_keeps_terminal_alive", self.shell_alive(sid))
            self.shell_enter(sid, "printf 'AFTER_FAILURE_%s\\n' ALIVE")
            self.shell_marker(sid, "AFTER_FAILURE_ALIVE")
            self.check("shell_command_after_failure", self.shell_alive(sid))

            marker = self.outside / "unexpected-native-shell"
            self.shell_enter(sid, "/usr/bin/touch " + shlex.quote(str(marker))
                             + "; printf 'AFTER_DENIAL_%s\\n' ALIVE")
            self.shell_marker(sid, "AFTER_DENIAL_ALIVE")
            self.check("shell_outside_write_denied_and_session_alive",
                       not marker.exists() and self.shell_alive(sid))
            self.check("shell_private_profile_unchanged", digest(state / "controller/codex/config.toml") == profile)
            self.check("shell_poisoned_workspace_not_executed", not any(path.exists() for path in self.traps)
                       and self.rejecting_provider.summary()["connection_count"] == 0)

            cancelled = self.work / "shell-cases/cancelled"
            cancelled.mkdir()
            command = self.shell_script("shell-cases/cancelled.py", LIFECYCLE)
            self.shell_enter(sid, "cd " + shlex.quote(str(cancelled)) + "; " + command)
            wait_for(lambda: all((cancelled / (role + ".json")).exists()
                                 for role in ("parent", "child", "grandchild")), 15,
                     "cancellable shell descendants launched")
            for role in ("parent", "child", "grandchild"):
                self.capture("shell/cancelled/" + role,
                    json.loads((cancelled / (role + ".json")).read_text())["pid"])
            cancel_started = time.monotonic()
            self.call("workbench.terminal.input", workspace_id=self.workspace_id, session_id=sid,
                      text="\x1b", enter=False, input_kind="user")
            for role in ("parent", "child", "grandchild"):
                self.check("shell_escape_stopped_" + role, self.stopped("shell/cancelled/" + role, 10))
            cancelled_seconds = round(time.monotonic() - cancel_started, 3)
            self.check("shell_escape_cancel_bounded", cancelled_seconds < 15,
                       {"seconds": cancelled_seconds})
            self.shell_enter(sid, "printf 'AFTER_CANCEL_%s\\n' ALIVE")
            self.shell_marker(sid, "AFTER_CANCEL_ALIVE")
            self.check("shell_command_after_cancel", self.shell_alive(sid))

            # Foreground shell owns background descendants while it waits; the
            # public desktop stop must tear down all three generations.
            lifecycle = self.work / "shell-cases/lifecycle"
            lifecycle.mkdir()
            command = self.shell_script("shell-cases/lifecycle.py", LIFECYCLE)
            self.shell_enter(sid, "cd " + shlex.quote(str(lifecycle)) + "; " + command + " & wait")
            wait_for(lambda: all((lifecycle / (role + ".json")).exists()
                                 for role in ("parent", "child", "grandchild")), 15,
                     "shell descendants launched")
            identities = {}
            for role in ("parent", "child", "grandchild"):
                identities[role] = self.capture("shell/lifecycle/" + role,
                    json.loads((lifecycle / (role + ".json")).read_text())["pid"])
            self.check("shell_live_descendants_share_owned_group",
                       len({item["pgid"] for item in identities.values()}) == 1
                       and all(not self.stopped("shell/lifecycle/" + role) for role in identities))
            stop_started = time.monotonic()
            self.call("workbench.terminal.stop", workspace_id=self.workspace_id, session_id=sid, confirmed=True)
            elapsed = round(time.monotonic() - stop_started, 3)
            self.check("shell_desktop_stop_bounded", elapsed < 15, {"seconds": elapsed})
            self.save_terminal(sid, output)
            for label in list(self.pidfds):
                if label.startswith("shell/"):
                    self.check("shell_stopped_" + label, self.stopped(label, 5))
            self.bridge_removed("shell", sid)
            summary = fixture.summary()
            self.check("shell_no_model_or_title_request", summary["request_count"] == 0
                       and summary["main"]["request_count"] == 0 and summary["title_requests"] == 0
                       and not summary["errors"], summary)
            self.receipt["shell_provider"] = summary
            self.receipt["shell_scenario"] = {
                "native_bang_commands": True, "actual_sandbox_tools": True,
                "model_requests": 0, "seconds": round(time.monotonic() - started, 3),
                "escape_cancel_seconds": cancelled_seconds, "stop_seconds": elapsed,
                "timeout_coverage": "Short explicit timeout covered by separate proxy protocol tests; native TUI default not timed here.",
                "not_a_performance_benchmark": True,
            }

    def host_crash_scenario(self):
        with TitleAwareServer([{"kind": "final", "text": "UNEXPECTED MODEL REQUEST"}],
                              self.output / "host-crash-responses") as fixture:
            (self.codex_home / "config.toml").write_text(HELPERS["config_text"](fixture.base_url))
            sid, state, output = self.open_terminal("host-crash")
            self.capture_tree("host-crash", sid)
            lifecycle = self.work / "host-crash-lifecycle"
            lifecycle.mkdir()
            command = self.shell_script("host-crash-lifecycle/fixture.py", LIFECYCLE)
            self.shell_enter(sid, "cd " + shlex.quote(str(lifecycle)) + "; " + command + " & wait")
            wait_for(lambda: all((lifecycle / (role + ".json")).exists()
                                 for role in ("parent", "child", "grandchild")), 15,
                     "host-crash shell descendants launched")
            for role in ("parent", "child", "grandchild"):
                self.capture("host-crash/lifecycle/" + role,
                    json.loads((lifecycle / (role + ".json")).read_text())["pid"])
            self.check("host_crash_shell_is_running", self.shell_alive(sid)
                       and all(not self.stopped("host-crash/lifecycle/" + role)
                               for role in ("parent", "child", "grandchild")))
            started = time.monotonic()
            signal.pidfd_send_signal(self.pidfds["host-crash/host"]["fd"], signal.SIGKILL)
            for label in list(self.pidfds):
                if label.startswith("host-crash/"):
                    self.check("host_crash_stopped_" + label, self.stopped(label, 10))
            self.bridge_removed("host_crash", sid)
            elapsed = round(time.monotonic() - started, 3)
            self.check("host_crash_lease_cleanup_bounded", elapsed < 20, {"seconds": elapsed})
            summary = fixture.summary()
            self.check("host_crash_no_model_request", summary["request_count"] == 0
                       and summary["title_requests"] == 0 and not summary["errors"], summary)
            self.receipt["host_crash_provider"] = summary
            self.receipt["host_crash_scenario"] = {
                "trigger": "SIGKILL through pidfd for the owned disposable terminal host",
                "lease_eof_cleanup_verified": True, "model_requests": 0,
                "cleanup_seconds": elapsed, "not_a_performance_benchmark": True,
            }

    def browser(self):
        self.receipt["scope"] = "isolated backend for separate real-browser native terminal validation"
        final = "本地模拟回复：真实 Codex 终端已接入自研受限工具，未调用真实模型。"
        with TitleAwareServer([{"kind": "final", "text": final}], self.output / "responses", timeout=300) as fixture:
            fixture.inner.timer.cancel()
            (self.codex_home / "config.toml").write_text(HELPERS["config_text"](fixture.base_url))
            folder = self.root / "ordinary-folder"
            folder.mkdir()
            direct = self.call("project.add", path=str(folder))["project"]
            ready = {"data_dir": str(self.data), "daemon_endpoint": str(self.endpoint),
                     "daemon_pid": self.daemon.pid, "controller_pid": os.getpid(),
                     "runtime": str(self.runtime), "workspace_id": self.workspace_id,
                     "workspace_path": str(self.work), "direct_project": direct,
                     "fixed_reply": final, "no_login": True, "no_real_model": True,
                     "cleanup_request": str(self.output / "cleanup.request"),
                     "vite_environment": {"VELELLA_DATA_DIR": str(self.data), "VELELLA_RUNTIME": str(self.runtime)}}
            (self.output / "ready.json").write_text(json.dumps(ready, ensure_ascii=False, indent=2) + "\n")
            print(json.dumps(ready, ensure_ascii=False), flush=True)
            while not STOP.wait(.5) and not (self.output / "cleanup.request").exists():
                snapshot = self.call("workbench.snapshot")
                (self.output / "observed-workbench.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n")
                for workspace in snapshot["workspaces"]:
                    for record in self.call("workbench.sessions", workspace_id=workspace["id"])["sessions"]:
                        sid = record["id"]
                        if sid not in self.host_labels and record.get("status", {}).get("pid"):
                            self.capture("browser/" + sid + "/host", record["status"]["pid"], parent=self.daemon.pid)
                            self.host_labels[sid] = "browser/" + sid + "/host"
                            case = self.output / "sessions" / sid
                            case.mkdir(parents=True)
                            self.session_artifacts[sid] = (self.data / "sandbox-chats" / sid, case)
            self.cleanup()
            summary = fixture.summary()
            self.receipt["browser_provider"] = summary
            self.check("browser_fixed_provider_only",
                       summary["complete"] and summary["main"]["request_count"] == 1
                       and 0 <= summary["title_requests"] <= 1
                       and summary["request_count"] == 1 + summary["title_requests"]
                       and not summary["errors"], summary)
            self.receipt["browser_assertions_recorded_separately"] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--codex", required=True, type=Path, help="Actual Codex 0.153.4 native ELF")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--browser", action="store_true")
    parser.add_argument("--shell-only", action="store_true")
    args = parser.parse_args()
    if args.browser and args.shell_only:
        parser.error("--browser and --shell-only are mutually exclusive")
    if args.output.is_symlink() or args.output.exists() and any(args.output.iterdir()):
        parser.error("output must be new or empty; previous evidence is preserved")
    args.output.mkdir(parents=True, exist_ok=True)
    runtime, cli = args.runtime.resolve(strict=True), args.codex.resolve(strict=True)
    with cli.open("rb") as stream:
        if stream.read(4) != b"\x7fELF":
            parser.error("--codex must name the actual native ELF, not an npm wrapper")
    test = TerminalAcceptance(runtime, cli, args.output.resolve(), REPOSITORY)
    temporary = tempfile.TemporaryDirectory(prefix="vl-native-terminal-")
    previous = {sig: signal.signal(sig, lambda *_: STOP.set()) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        test.prepare(Path(temporary.name).resolve())
        if args.browser:
            test.browser()
        elif args.shell_only:
            test.shell_scenario()
            test.host_crash_scenario()
        else:
            test.main_scenario()
            test.shell_scenario()
            test.host_crash_scenario()
        test.receipt["status"] = "passed"
    except Exception as error:
        test.receipt.update(status="failed", error=f"{type(error).__name__}: {error}")
    finally:
        for sid, (_, output) in test.session_artifacts.items():
            stream = test.streams.get(sid)
            if stream:
                (output / "terminal.raw").write_bytes(stream["raw"])
                (output / "terminal-stripped.txt").write_text(plain_terminal(stream["raw"]))
        test.finish(temporary)
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    return 0 if test.receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
