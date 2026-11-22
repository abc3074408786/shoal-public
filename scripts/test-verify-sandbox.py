"""Tests of receipt semantics, without launching commands or changing host policy."""
import contextlib
import importlib.util
import io
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    'verify_sandbox', pathlib.Path(__file__).with_name('verify-sandbox.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ReceiptTests(unittest.TestCase):
    def run_case(self, available=False, malformed=False, failed_test=False, timeout=False):
        calls = []

        def command(args, **kwargs):
            calls.append(args)
            if args[1] == 'metadata':
                return subprocess.CompletedProcess(args, 0, '{"target_directory":"/tmp/custom-target"}', '')
            if args[1] == 'sandbox-capabilities':
                self.assertEqual(args[0], '/tmp/custom-target/debug/velella-runtime')
                body = [] if malformed else {'available': available, 'reason': 'fixture refusal'}
                return subprocess.CompletedProcess(args, 0, json.dumps(body), '')
            if args[1] == 'test':
                self.assertEqual(kwargs['env']['VELELLA_REQUIRE_SANDBOX'], '1')
                if timeout:
                    raise subprocess.TimeoutExpired(args, 900)
                return subprocess.CompletedProcess(args, int(failed_test), '', '')
            return subprocess.CompletedProcess(args, 0, '', '')

        with tempfile.TemporaryDirectory() as directory, \
                patch.object(runner.sys, 'argv', ['verify-sandbox.py', '--output', directory]), \
                patch.object(runner.sys, 'platform', 'linux'), \
                patch.object(runner, 'host_diagnostics', return_value={'python_executable': True}), \
                patch.object(runner.subprocess, 'run', side_effect=command), \
                contextlib.redirect_stdout(io.StringIO()):
            code = runner.main()
            receipt = json.loads((pathlib.Path(directory) / 'result.json').read_text())
        return code, receipt, calls

    def test_refusal_is_blocked_and_never_runs_acceptance(self):
        code, receipt, calls = self.run_case()
        self.assertEqual((code, receipt['status']), (2, 'blocked'))
        self.assertFalse(any(call[1] == 'test' for call in calls))
        self.assertIn('next_step', receipt)

    def test_all_four_checks_required_for_pass(self):
        code, receipt, calls = self.run_case(available=True)
        self.assertEqual((code, receipt['status']), (0, 'passed'))
        self.assertEqual(sum(call[1] == 'test' for call in calls), 4)
        self.assertIn('--ignored', calls[-1])

    def test_malformed_capability_is_failure(self):
        code, receipt, _ = self.run_case(malformed=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))

    def test_failed_kernel_stops_later_checks(self):
        code, receipt, calls = self.run_case(available=True, failed_test=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertEqual(sum(call[1] == 'test' for call in calls), 1)

    def test_timeout_still_writes_failure_receipt(self):
        code, receipt, _ = self.run_case(available=True, timeout=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertFalse(receipt['checks'][-1]['passed'])


if __name__ == '__main__':
    unittest.main()
