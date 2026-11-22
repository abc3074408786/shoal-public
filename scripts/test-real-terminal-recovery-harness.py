#!/usr/bin/env python3
"""Offline evidence regressions; starts no daemon, browser, CLI or model."""
import json
import os
from pathlib import Path
import runpy
import tempfile
import types
import unittest

MODULE = runpy.run_path(str(Path(__file__).with_name("verify-desktop-real-terminal.py")))
Acceptance = MODULE["RealTerminalAcceptance"]


class RecoveryEvidence(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="vl-recovery-evidence-")
        self.root = Path(self.directory.name)
        self.test = Acceptance.__new__(Acceptance)
        self.test.nonce = "fixture-proof-nonce"
        self.test.recovery_memory = "RECOVERY_MEMORY_fixture-random-value"
        self.test.recovery_marker = "REAL_RECOVERY_MODEL_DONE"
        self.test.final_marker = "REAL_SANDBOX_MODEL_DONE_fixture"
        self.test.recovery_prompt = "Recall the earlier RECOVERY_MEMORY_ line; finish REAL_RECOVERY_MODEL_DONE."
        self.test.rollouts, self.test.receipt = {}, {}
        self.test.cases = {}
        self.test.prompt = "fixed original model prompt"
        self.test.secret = self.root / "outside-synthetic"
        self.test.source = self.root / "source-profile"
        self.test.rejecting_provider = types.SimpleNamespace(server=types.SimpleNamespace(server_port=1))
        self.state = self.root / "state"
        self.history = self.state / "controller/codex/sessions"
        self.history.mkdir(parents=True)
        self.rollout = self.history / "rollout.jsonl"
        self.case = {"sid": "fixture-session", "state": self.state, "work": self.root / "work"}
        self.case["work"].mkdir()

    def tearDown(self):
        self.directory.cleanup()

    def append(self, *events):
        with self.rollout.open("a") as stream:
            for event in events:
                stream.write(json.dumps(event) + "\n")

    def message(self, role, text):
        return {"type": "response_item", "payload": {
            "type": "message", "role": role, "content": [{"type": "output_text", "text": text}],
        }}

    def call(self, command):
        return {"type": "response_item", "payload": {
            "type": "function_call", "name": "exec_command", "arguments": json.dumps({"cmd": command}),
        }}

    def output(self):
        return {"type": "response_item", "payload": {"type": "function_call_output", "output": "fixed output"}}

    def completed(self):
        return {"type": "event_msg", "payload": {"type": "task_complete"}}

    def start_recovery(self):
        baseline = self.test.model_evidence(self.case)
        self.case["recovery_baseline"] = {"model_evidence": baseline}
        self.case["recovery"] = {"proofs_reset_at_ns": 1}
        return baseline

    def test_old_assistant_and_manual_output_cannot_satisfy_new_assistant_memory(self):
        self.append(self.message("assistant", self.test.recovery_memory + " " + self.test.recovery_marker),
                    self.message("user", "<user_shell_command>" + self.test.recovery_memory),
                    self.completed())
        baseline = self.start_recovery()
        self.assertTrue(baseline["recovery_memory_in_shell_history"])
        self.assertFalse(baseline["recovery_memory_in_new_assistant"])
        self.assertFalse(baseline["recovery_final_reply_seen"])
        self.assertIsNone(self.test.recovery_complete(self.case))
        self.append(self.message("user", self.test.recovery_memory),
                    self.output(), self.completed())
        self.assertIsNone(self.test.recovery_complete(self.case))
        self.assertFalse(self.test.model_evidence(self.case)["recovery_memory_in_new_assistant"])

    def test_new_assistant_new_calls_and_completion_are_independent_evidence(self):
        self.append(self.call("initial fixture"), self.output(),
                    self.message("assistant", self.test.final_marker), self.completed())
        baseline = self.start_recovery()
        self.append(self.call("rg -n . clamp.py"), self.output(),
                    self.call("python3 -m unittest -v test_clamp.py"), self.output(),
                    self.message("assistant", self.test.recovery_memory + "\n" + self.test.recovery_marker))
        self.assertIsNone(self.test.recovery_complete(self.case), "final text alone is not turn completion")
        self.append(self.completed())
        evidence = self.test.recovery_complete(self.case)
        self.assertIsNotNone(evidence)
        self.assertGreater(evidence["function_calls"], baseline["function_calls"])
        self.assertGreater(evidence["function_outputs"], baseline["function_outputs"])
        self.assertTrue(evidence["recovery_rg_call_seen"])
        self.assertTrue(evidence["recovery_test_call_seen"])
        self.assertTrue(evidence["recovery_memory_in_new_assistant"])
        self.assertEqual(self.test.model_evidence(self.case), evidence, "same bytes were counted twice")

    def test_pending_jsonl_fragment_is_not_counted_until_complete(self):
        self.start_recovery()
        encoded = json.dumps(self.message("assistant", self.test.recovery_memory)).encode()
        self.rollout.write_bytes(encoded[:-2])
        self.assertEqual(self.test.model_evidence(self.case)["assistant_messages"], 0)
        with self.rollout.open("ab") as stream:
            stream.write(encoded[-2:] + b"\n")
        evidence = self.test.model_evidence(self.case)
        self.assertEqual(evidence["assistant_messages"], 1)
        self.assertTrue(evidence["recovery_memory_in_new_assistant"])

    def test_memory_command_prints_only_and_recovery_prompt_never_contains_nonce(self):
        self.test.commands = lambda work: {
            "boundary_command": "!fixed boundary", "independent_verify_command": "!fixed verify",
        }
        commands = self.test.recovery_commands(self.case["work"])
        self.assertEqual(commands["recovery_memory_command"],
                         "!printf '%s\\n' RECOVERY_MEMORY_fixture-random-value")
        self.assertNotIn("fixture-random-value", commands["recovery_prompt"])
        self.assertNotIn("fixture-proof-nonce", commands["recovery_prompt"])
        self.assertEqual(commands["recovery_boundary_command"], "!fixed boundary")

    def test_missing_or_dirty_new_proofs_are_not_ready(self):
        home = self.state / "tool-home"
        home.mkdir()
        checkpoint = {"shell_history_dirty": False}
        self.test.checkpoint = lambda case: checkpoint
        self.assertFalse(self.test.recovery_proofs_ready(self.case))
        for name in ("boundary-fixture-proof-nonce.json", "verify-fixture-proof-nonce.json",
                     "verify-fixture-proof-nonce.log"):
            (home / name).write_text("{}")
        checkpoint["shell_history_dirty"] = True
        self.assertFalse(self.test.recovery_proofs_ready(self.case))
        checkpoint["shell_history_dirty"] = False
        self.assertTrue(self.test.recovery_proofs_ready(self.case))

    def prepare_shell_case(self):
        (self.state / "tool-home").mkdir(mode=0o700)
        self.case["output"] = self.root / "evidence"
        self.case["output"].mkdir()
        self.test.cases[self.case["sid"]] = self.case

    def test_long_inline_fixtures_become_short_readonly_home_scripts_after_case_exists(self):
        inline = self.test.commands(self.case["work"])
        self.assertGreater(len(inline["boundary_command"]), 1024)
        self.assertGreater(len(inline["independent_verify_command"]), 1024)
        self.prepare_shell_case()
        commands = self.test.commands(self.case["work"])
        for name, key in (("boundary", "boundary_command"), ("verify", "independent_verify_command")):
            command = commands[key]
            self.assertLess(len(command.encode()), 1024)
            self.assertTrue(command.startswith('!/bin/bash "$HOME/'))
            record = self.case["shell_fixtures"][name]
            path = self.state / "tool-home" / record["file"]
            self.assertEqual(path.stat().st_mode & 0o777, 0o444)
            self.assertEqual(path.read_text(), "#!/bin/bash\n" + inline[key][1:] + "\n")
            self.assertEqual(record["sha256"], MODULE["digest"](path))
        self.assertFalse(list(self.case["work"].iterdir()), "fixture scripts must not enter the worktree")
        saved = json.loads((self.case["output"] / "shell-fixtures.json").read_text())
        self.assertEqual(saved, self.case["shell_fixtures"])
        self.test.check_shell_fixtures(self.case)

    def test_recovery_reuses_exact_short_fixture_files(self):
        self.prepare_shell_case()
        original = self.test.commands(self.case["work"])
        files = [self.state / "tool-home" / record["file"] for record in self.case["shell_fixtures"].values()]
        before = [(path.stat().st_ino, path.stat().st_mtime_ns, path.read_bytes()) for path in files]
        recovered = self.test.recovery_commands(self.case["work"])
        self.assertEqual(recovered["recovery_boundary_command"], original["boundary_command"])
        self.assertEqual(recovered["recovery_independent_verify_command"], original["independent_verify_command"])
        self.assertEqual(before, [(path.stat().st_ino, path.stat().st_mtime_ns, path.read_bytes()) for path in files])

    def test_shell_fixture_replacement_is_rejected_without_repair_or_execution(self):
        self.prepare_shell_case()
        self.test.commands(self.case["work"])
        path = self.state / "tool-home" / self.case["shell_fixtures"]["verify"]["file"]
        original = path.read_bytes()
        path.chmod(0o600)
        path.write_text("exit 0\n")
        path.chmod(0o444)
        with self.assertRaises(RuntimeError):
            self.test.commands(self.case["work"])
        with self.assertRaises(RuntimeError):
            self.test.check_shell_fixtures(self.case)
        self.assertEqual(path.read_text(), "exit 0\n")
        path.unlink()
        target = self.root / "unrelated.sh"
        target.write_bytes(original)
        target.chmod(0o444)
        path.symlink_to(target)
        with self.assertRaises(RuntimeError):
            self.test.commands(self.case["work"])
        path.unlink()
        os.link(target, path)
        with self.assertRaises(RuntimeError):
            self.test.commands(self.case["work"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
