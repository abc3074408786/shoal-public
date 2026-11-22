#!/usr/bin/env python3
"""Real native TUI stop/resume regression with fixed local model responses.

No account or real model is used. The same daemon/session/worktree and native
thread must survive two recoveries; the resumed model and manual tools must
still pass the kernel boundary probes.
"""
import argparse
import json
from pathlib import Path
import runpy
import shlex
import signal
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BASE = runpy.run_path(str(ROOT / "scripts/verify-desktop-restricted-terminal.py"))
TerminalAcceptance, TitleAwareServer = (BASE[k] for k in ("TerminalAcceptance", "TitleAwareServer"))
HELPERS, rpc, wait_for, STOP = (BASE[k] for k in ("HELPERS", "rpc", "wait_for", "STOP"))


class RecoveryAcceptance(TerminalAcceptance):
    def stop_terminal(self, sid, state, label):
        wait_for(lambda: (state / "terminal-resume.json").exists()
                 and json.loads((state / "terminal-resume.json").read_text())["shell_history_dirty"] is False,
                 10, "manual history flush before normal stop")
        self.call("workbench.terminal.stop", workspace_id=self.workspace_id, session_id=sid, confirmed=True)
        wait_for(lambda: (state / "terminal-stopped.json").exists(), 10, "completed cleanup marker")
        self.bridge_removed(label, sid)
        wait_for(lambda: self.session_state(sid)["recoverable"], 10, "host ownership released after stop")
        self.check(label + "_recoverable", self.session_state(sid)["recoverable"] is True)
        checkpoint = json.loads((state / "terminal-resume.json").read_text())
        self.check(label + "_history_saved", checkpoint["shell_history_dirty"] is False)
        return checkpoint

    def recover(self, sid, state, label, expected_thread):
        before = self.streams[sid]["offset"]
        old_pid = self.session_state(sid)["status"]["pid"]
        result = self.call("workbench.terminal.recover", workspace_id=self.workspace_id, session_id=sid)
        self.check(label + "_same_session", result["session_id"] == sid)
        record = self.session_state(sid)
        self.capture(label + "/host", record["status"]["pid"], parent=self.daemon.pid)
        self.host_labels[sid] = label + "/host"
        self.call("workbench.terminal.resize", workspace_id=self.workspace_id, session_id=sid, rows=40, cols=140)
        def ready():
            self.pump(sid)
            checkpoint = json.loads((state / "terminal-resume.json").read_text())
            bridge = json.loads((state / "terminal-bridge.json").read_text())
            return (checkpoint["thread_id"] == expected_thread
                    and checkpoint["generation"] == bridge["generation"]
                    and self.streams[sid]["offset"] > before)
        wait_for(ready, 25, "same native thread resumed")
        bridge = json.loads((state / "terminal-bridge.json").read_text())
        self.bridge_sockets[sid] = Path(bridge["socket"])
        self.check(label + "_new_generation_marker_invalidated", not (state / "terminal-stopped.json").exists())
        self.check(label + "_live_not_recoverable", self.session_state(sid)["recoverable"] is False)
        self.capture_tree(label, sid)
        for operation, arguments in (
            ("input", {"text": "!printf STALE_INPUT_MUST_NOT_RUN", "enter": True}),
            ("resize", {"rows": 1, "cols": 1}),
            ("control", {"human": False}),
        ):
            rejected = rpc(self.endpoint, {"cmd": "workbench.terminal." + operation,
                "workspace_id": self.workspace_id, "session_id": sid,
                "expected_host_pid": old_pid, **arguments}, require_ok=False)
            self.check(label + "_stale_" + operation + "_rejected", rejected["ok"] is False)
        self.call("workbench.terminal.resize", workspace_id=self.workspace_id, session_id=sid,
                  expected_host_pid=record["status"]["pid"], rows=40, cols=140)

    def scenario(self):
        command = "printf 'SAVED_SHELL_%s\\n' CONTEXT"
        marker = "SAVED_SHELL_CONTEXT"
        steps = [
            {"kind": "tool", "name": "exec_command", "arguments": {},
             "expect_output_contains": "VELELLA_DESKTOP_RESTRICTED_BOUNDARY_PASSED"},
            {"kind": "final", "text": "RECOVERY_FIXTURE_MODEL_DONE"},
        ]
        with TitleAwareServer(steps, self.output / "responses",
                              expect_shell_context=(command, marker)) as fixture:
            (self.codex_home / "config.toml").write_text(HELPERS["config_text"](fixture.base_url))
            empty_sid, empty_state, empty_output = self.open_terminal("empty")
            self.capture_tree("empty", empty_sid)
            self.call("workbench.terminal.stop", workspace_id=self.workspace_id,
                      session_id=empty_sid, confirmed=True)
            wait_for(lambda: (empty_state / "terminal-stopped.json").exists(), 10, "empty terminal cleanup")
            self.check("empty_host_stopped", self.stopped("empty/host", 10))
            self.check("empty_not_recoverable", self.session_state(empty_sid)["recoverable"] is False)
            rejected = rpc(self.endpoint, {"cmd": "workbench.terminal.recover",
                "workspace_id": self.workspace_id, "session_id": empty_sid}, require_ok=False)
            self.check("empty_recovery_rejected_before_spawn", rejected["ok"] is False)
            self.bridge_removed("empty", empty_sid)
            self.save_terminal(empty_sid, empty_output)
            sid, state, output = self.open_terminal("original")
            self.capture_tree("original", sid)
            self.shell_enter(sid, command)
            self.shell_marker(sid, marker)
            self.check("shell_only_before_stop", fixture.summary()["request_count"] == 0)
            first = self.stop_terminal(sid, state, "original")
            self.recover(sid, state, "resumed", first["thread_id"])
            proof = self.work / "resumed-model-boundary.json"
            code = HELPERS["BOUNDARY"].format(
                work=str(self.work), home=str(state / "tool-home"), outside=str(self.secret),
                control=str(state), proof=str(proof), port=fixture.port)
            fixture.steps[0]["arguments"] = {
                "cmd": "/usr/bin/python3 -I -c " + shlex.quote(code),
                "workdir": str(self.work), "max_output_tokens": 1600,
            }
            self.enter(sid, "Verify the resumed sandbox using the fixed local test response.")
            wait_for(lambda: self.text_ready(sid, "RECOVERY_FIXTURE_MODEL_DONE"), 45, "resumed model output")
            wait_for(lambda: fixture.summary()["complete"], 10, "resumed fixed response complete")
            self.check("shell_context_survived_without_prior_model_turn",
                       fixture.summary()["shell_context_verified"] is True)
            checks = json.loads(proof.read_text())
            self.check("resumed_model_all_12_boundaries", len(checks) == 12 and all(checks.values()), checks)
            second = self.stop_terminal(sid, state, "resumed")
            self.check("same_native_thread_after_model", first["thread_id"] == second["thread_id"])
            self.check("generation_changed", first["generation"] != second["generation"])
            # A changed provider must be rejected before launching another controller.
            config = self.codex_home / "config.toml"
            original_config = config.read_text()
            config.write_text(original_config.replace("mock-model", "changed-model"))
            rejected = rpc(self.endpoint, {"cmd": "workbench.terminal.recover",
                "workspace_id": self.workspace_id, "session_id": sid}, require_ok=False)
            self.check("changed_provider_profile_rejected", rejected["ok"] is False)
            config.write_text(original_config)
            self.recover(sid, state, "resumed-twice", first["thread_id"])
            shell_proof = self.work / "resumed-shell-boundary.json"
            shell_code = HELPERS["BOUNDARY"].format(
                work=str(self.work), home=str(state / "tool-home"), outside=str(self.secret),
                control=str(state), proof=str(shell_proof), port=fixture.port)
            shell_command = self.shell_script("recovery-boundary.py", shell_code)
            self.shell_enter(sid, shell_command)
            wait_for(shell_proof.exists, 20, "resumed manual sandbox boundary file")
            checks = json.loads(shell_proof.read_text())
            self.check("resumed_manual_all_12_boundaries", len(checks) == 12 and all(checks.values()), checks)
            self.shell_enter(sid, "rg --version; printf 'RECOVERED_RG_%s\\n' READY")
            self.shell_marker(sid, "RECOVERED_RG_READY")
            self.check("rg_visible_in_native_terminal", "ripgrep" in self.pump(sid))
            wait_for(lambda: json.loads((state / "terminal-resume.json").read_text())["shell_history_dirty"] is False,
                     10, "last manual history flush")
            self.stop_terminal(sid, state, "resumed-twice")
            self.check("workspace_traps_not_executed", not any(path.exists() for path in self.traps)
                       and self.rejecting_provider.summary()["connection_count"] == 0)
            self.check("no_auth_files", not (state / "controller/codex/auth.json").exists())
            for label in self.pidfds:
                if label.startswith(("empty/", "original/", "resumed/", "resumed-twice/")):
                    self.check("stopped_" + label, self.stopped(label, 5))
            self.save_terminal(sid, output)
            self.receipt["native_thread_preserved"] = True
            self.receipt["recoveries"] = 2
            self.receipt["provider"] = fixture.summary()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("runtime", "codex", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    if args.output.is_symlink() or args.output.exists() and any(args.output.iterdir()):
        parser.error("output must be new or empty")
    args.output.mkdir(parents=True)
    test = RecoveryAcceptance(args.runtime.resolve(strict=True), args.codex.resolve(strict=True),
                              args.output.resolve(), ROOT)
    test.receipt["source_hashes"]["scripts/verify-desktop-restricted-recovery.py"] = BASE["digest"](Path(__file__))
    temporary = tempfile.TemporaryDirectory(prefix="vl-terminal-recovery-")
    previous = {sig: signal.signal(sig, lambda *_: STOP.set()) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        test.prepare(Path(temporary.name).resolve())
        test.scenario()
        test.receipt["status"] = "passed"
    except Exception as error:
        test.receipt.update(status="failed", error=f"{type(error).__name__}: {error}")
    finally:
        for state, output in test.session_artifacts.values():
            for name in ("startup-error.txt", "terminal-resume.json", "terminal-stopped.json"):
                path = state / name
                if path.is_file() and not path.is_symlink() and path.stat().st_size <= 8192:
                    (output / name).write_bytes(path.read_bytes())
            stream = test.streams.get(next((sid for sid, item in test.session_artifacts.items()
                                           if item[0] == state), ""))
            if stream:
                (output / "terminal-stripped.txt").write_text(BASE["plain_terminal"](stream["raw"]))
        test.finish(temporary)
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    return 0 if test.receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
