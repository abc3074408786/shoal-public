import importlib.util
import contextlib
import io
import json
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('real_agent', Path(__file__).with_name('verify-real-sandbox-agent.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ChallengeTests(unittest.TestCase):
    def test_extracts_only_expected_device_challenge(self):
        value = module.device_challenge('Open https://auth.openai.com/codex/device then enter \x1b[32mABCD-EFGH\x1b[0m')
        self.assertEqual(value, {'url':'https://auth.openai.com/codex/device', 'code':'ABCD-EFGH'})

    def test_does_not_publish_unrelated_text_or_arbitrary_urls(self):
        self.assertIsNone(module.device_challenge('https://example.com ABCD-EFGH'))
        self.assertIsNone(module.device_challenge('https://auth.openai.com/codex/device authentication pending'))


class ReceiptTests(unittest.TestCase):
    """Protocol simulations only; subprocess APIs are mocked, no login is started."""
    def run_case(self, *, available=True, mode='isolated', changed_tests=False,
                 verified=True, hooks=True, cancel_fails=False, remove_fails=False,
                 extra_file=False, modified_ignore=False, candidate_symlink=False,
                 reverify_fails=False, first_cancel_fails=False, fixture_mode=False,
                 requests=None, execution='isolated'):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            runtime = base / 'runtime'
            runtime.touch()
            output = base / 'evidence'
            owned = base / 'owned'
            calls = []
            task = None
            verify_count = 0

            def temporary_root(**_kwargs):
                owned.mkdir()
                return str(owned)

            def command(argv, **kwargs):
                nonlocal task, verify_count
                calls.append(argv)
                value = None
                stdout = ''
                if argv[1] == 'sandbox-capabilities':
                    value = {'available': available, 'restricted': {'available': available}}
                elif argv[0] == 'codex':
                    stdout = 'codex fixture version'
                elif argv[1] == 'rpc':
                    request = json.loads(kwargs['input'])
                    if requests is not None:
                        requests.append(request)
                    operation = request['cmd']
                    if operation == 'snapshot':
                        value = {'ok': True, 'tasks': [] if task is None else [task]}
                    elif operation == 'project.add':
                        value = {'ok': True, 'project': {'id': 'project'}}
                    elif operation == 'task.create':
                        work = owned / 'work'
                        work.mkdir()
                        (work / 'clamp.py').write_text('def clamp(value, lower, upper):\n    pass\n')
                        (work / 'test_clamp.py').write_text('modified' if changed_tests else module.TESTS)
                        (work / '.gitignore').write_text('modified' if modified_ignore else module.GITIGNORE)
                        (work / '.git').write_text(f'gitdir: {owned}/repo/.git/worktrees/test\n')
                        if extra_file:
                            (work / 'unittest.py').write_text('raise SystemExit(0)\n')
                        if candidate_symlink:
                            (work / 'clamp.py').unlink()
                            (work / 'clamp.py').symlink_to(owned / 'repo' / 'clamp.py')
                        task = {'id': 'task', 'phase': 'review', 'reason': 'fixture',
                                'worktree': str(work), 'verified': verified,
                                'checks': [{'command': module.VERIFY_COMMAND, 'passed': True, 'exit': 0}],
                                'status': {'hooks_ready': hooks, 'turns': 1},
                                'agent': {'id': 'agent', 'directory': str(owned / 'session'),
                                          'spawn': {'sandbox': {'mode': mode,
                                                    'network': 'denied' if fixture_mode else 'host'}}}}
                        value = {'ok': True, 'task': task}
                    elif operation == 'task.verify':
                        verify_count += 1
                        if reverify_fails:
                            task['verified'] = False
                        value = {'ok': True}
                    elif operation == 'task.cancel' and (first_cancel_fails or (cancel_fails and verify_count)):
                        value = {'ok': False, 'error': 'fixture cancellation failure'}
                    else:
                        value = {'ok': True}
                return subprocess.CompletedProcess(argv, 0, json.dumps(value) if value is not None else stdout, '')

            daemon = Mock()
            daemon.poll.return_value = None
            arguments = ['test', '--runtime', str(runtime), '--output', str(output),
                         '--fixture-agent' if fixture_mode else '--allow-device-login',
                         '--execution', execution]
            with patch.object(module.subprocess, 'run', side_effect=command), \
                    patch.object(module.subprocess, 'Popen', return_value=daemon), \
                    patch.object(module.tempfile, 'mkdtemp', side_effect=temporary_root), \
                    patch('sys.argv', arguments), contextlib.ExitStack() as stack:
                if remove_fails:
                    stack.enter_context(patch.object(module.shutil, 'rmtree', side_effect=PermissionError('fixture')))
                code = module.main()
                receipt = json.loads((output / 'result.json').read_text())
            return code, receipt, calls, owned.exists()

    def test_unavailable_environment_never_starts_codex(self):
        code, receipt, calls, exists = self.run_case(available=False)
        self.assertEqual((code, receipt['status']), (2, 'blocked'))
        self.assertEqual(len(calls), 1)
        self.assertFalse(exists)

    def test_truthy_malformed_capability_is_not_accepted(self):
        code, receipt, calls, _ = self.run_case(available='true')
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertEqual(len(calls), 1)

    def test_native_descriptor_cannot_pass_isolation_acceptance(self):
        code, receipt, _, exists = self.run_case(mode='native')
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertIn('lost required isolation', receipt['reason'])
        self.assertTrue(receipt['task_stopped'])
        self.assertFalse(exists)

    def test_tampered_tests_unverified_task_or_missing_hooks_cannot_pass(self):
        for kwargs in ({'changed_tests': True}, {'verified': False}, {'hooks': False}):
            with self.subTest(**kwargs):
                code, receipt, _, exists = self.run_case(**kwargs)
                self.assertEqual((code, receipt['status']), (1, 'failed'))
                self.assertFalse(exists)

    def test_success_requires_cleanup(self):
        code, receipt, calls, exists = self.run_case()
        self.assertEqual((code, receipt['status']), (0, 'passed'))
        self.assertTrue(receipt['task_stopped'])
        self.assertTrue(receipt['temporary_home_removed'])
        self.assertFalse(exists)
        self.assertEqual(receipt['independent_test_execution'], 'isolated task.verify after Agent stopped')
        # All host-side git commands create the pristine fixture. No candidate
        # Python command or git diff/status is ever executed on the host.
        self.assertFalse(any(argv[0] in ('python3', '/usr/bin/python3') for argv in calls))
        self.assertFalse(any('diff' in argv or 'status' in argv for argv in calls))

    def test_additional_or_modified_files_and_symlinks_cannot_pass(self):
        for kwargs in ({'extra_file': True}, {'modified_ignore': True}, {'candidate_symlink': True}):
            with self.subTest(**kwargs):
                code, receipt, _, exists = self.run_case(**kwargs)
                self.assertEqual((code, receipt['status']), (1, 'failed'))
                self.assertFalse(exists)

    def test_reverification_failure_cannot_reuse_first_pass(self):
        code, receipt, _, _ = self.run_case(reverify_fails=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))

    def test_agent_must_stop_before_reverification(self):
        code, receipt, _, _ = self.run_case(first_cancel_fails=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertNotIn('independent_test_execution', receipt)

    def test_cancellation_failure_is_not_hidden_by_passing_tests(self):
        code, receipt, _, _ = self.run_case(cancel_fails=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertEqual(receipt['execution_status'], 'passed')
        self.assertFalse(receipt['task_stopped'])

    def test_cleanup_failure_still_writes_a_failure_receipt(self):
        code, receipt, _, exists = self.run_case(remove_fails=True)
        self.assertEqual((code, receipt['status']), (1, 'failed'))
        self.assertFalse(receipt['temporary_home_removed'])
        self.assertTrue(exists)

    def test_stale_evidence_is_rejected_before_any_process(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / 'runtime'
            runtime.touch()
            output = root / 'evidence'
            output.mkdir()
            previous = output / 'result.json'
            previous.write_text('old evidence')
            with patch('sys.argv', ['test', '--runtime', str(runtime), '--output', str(output),
                                    '--allow-device-login']), \
                    patch.object(module.subprocess, 'run') as command, \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                module.main()
            self.assertEqual(raised.exception.code, 2)
            command.assert_not_called()
            self.assertEqual(previous.read_text(), 'old evidence')

    def test_missing_authorization_starts_no_process(self):
        with patch('sys.argv', ['test', '--runtime', 'unused', '--output', 'unused']), \
                patch.object(module.subprocess, 'run') as command, \
                patch.object(module.subprocess, 'Popen') as daemon, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
            module.main()
        self.assertEqual(raised.exception.code, 2)
        command.assert_not_called()
        daemon.assert_not_called()

    def test_fixture_mode_cannot_claim_real_model_or_start_login(self):
        requests = []
        code, receipt, calls, exists = self.run_case(fixture_mode=True, requests=requests)
        self.assertEqual((code, receipt['status'], receipt['agent_kind']), (0, 'passed', 'fixture'))
        self.assertEqual(receipt['network'], 'denied')
        self.assertIn('no model or login', receipt['scope'])
        self.assertNotIn('cli_version', receipt)
        self.assertFalse(any(argv[0] == 'codex' for argv in calls))
        setup = next(r['setup'] for r in requests if r['cmd'] == 'project.update')
        self.assertNotIn('login', setup)
        spec = next(r['spec'] for r in requests if r['cmd'] == 'task.create')
        self.assertEqual(spec['policy']['execution']['network'], 'denied')
        self.assertEqual(len(spec['policy']['execution']['read_only']), 1)
        operations = [r['cmd'] for r in requests]
        self.assertLess(operations.index('task.cancel'), operations.index('task.verify'))
        self.assertFalse(exists)

    def test_fixture_and_real_login_options_are_mutually_exclusive(self):
        with patch('sys.argv', ['test', '--runtime', 'unused', '--output', 'unused',
                                '--fixture-agent', '--allow-device-login']), \
                patch.object(module.subprocess, 'run') as command, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
            module.main()
        self.assertEqual(raised.exception.code, 2)
        command.assert_not_called()

    def test_restricted_fixture_has_explicit_mode_and_no_login(self):
        requests = []
        code, receipt, _, exists = self.run_case(fixture_mode=True, mode='restricted',
                                               execution='restricted', requests=requests)
        self.assertEqual((code, receipt['status'], receipt['execution_mode']), (0, 'passed', 'restricted'))
        self.assertEqual(receipt['independent_test_execution'], 'restricted task.verify after Agent stopped')
        spec = next(r['spec'] for r in requests if r['cmd'] == 'task.create')
        self.assertEqual(spec['policy']['execution']['mode'], 'restricted')
        self.assertFalse(exists)

    def test_restricted_cannot_start_device_login(self):
        with patch('sys.argv', ['test', '--runtime', 'unused', '--output', 'unused',
                                '--execution', 'restricted', '--allow-device-login']), \
                patch.object(module.subprocess, 'run') as command, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
            module.main()
        self.assertEqual(raised.exception.code, 2)
        command.assert_not_called()


class VerificationCommandTests(unittest.TestCase):
    def test_fixed_suite_runs_six_tests_without_importing_worktree_unittest(self):
        # This source is a fixed test fixture, never model-generated code.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'clamp.py').write_text(
                'def clamp(value, lower, upper):'
                '\n    if lower > upper: raise ValueError("reversed")'
                '\n    return min(max(value, lower), upper)\n'
            )
            (root / 'unittest.py').write_text('raise RuntimeError("untrusted unittest module")\n')
            result = subprocess.run(shlex.split(module.VERIFY_COMMAND), cwd=root,
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('Ran 6 tests', result.stderr)

    def test_fixed_suite_rejects_original_broken_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'clamp.py').write_text(module.ORIGINAL)
            result = subprocess.run(shlex.split(module.VERIFY_COMMAND), cwd=root,
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Ran 6 tests', result.stderr)
            self.assertIn('FAILED', result.stderr)

    def test_candidate_successful_early_exit_cannot_skip_fixed_suite(self):
        # This hand-written fixture exercises import-time exit, without a model.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'clamp.py').write_text('raise SystemExit(0)\n')
            result = subprocess.run(shlex.split(module.VERIFY_COMMAND), cwd=root,
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Acceptance verification failed: SystemExit', result.stderr)

    def test_workflow_requires_manual_login_opt_in(self):
        workflow = (Path(__file__).parents[1] / '.github/workflows/sandbox-real-agent.yml').read_text()
        self.assertIn('workflow_dispatch:', workflow)
        self.assertIn('default: false', workflow)
        self.assertIn('if: ${{ inputs.allow_device_login == true }}', workflow)
        self.assertNotIn('\n  push:', workflow)
        self.assertNotIn('\n  pull_request:', workflow)
        self.assertNotIn('ignore_errors=True', workflow)


if __name__ == '__main__':
    unittest.main()
