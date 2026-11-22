#!/usr/bin/env python3
"""Explicit, interactive acceptance: actual Codex runs inside Velella isolation.

No host credentials are read/copied. Device login happens in the task's private
HOME. Only challenge.json and result.json are suitable for CI artifact upload.
--fixture-agent exercises the same runtime protocol with a fixed local fixture,
denied networking and no login. It never produces a real-model acceptance receipt.
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shlex
import shutil
import signal
import stat
import subprocess
import tempfile
import time

ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07]*(?:\x07|\x1b\\)')
TESTS = '''import unittest
from clamp import clamp
class ClampTests(unittest.TestCase):
    def test_inside(self): self.assertEqual(clamp(5, 0, 10), 5)
    def test_below(self): self.assertEqual(clamp(-5, 0, 10), 0)
    def test_above(self): self.assertEqual(clamp(20, 0, 10), 10)
    def test_boundary(self): self.assertEqual(clamp(10, 0, 10), 10)
    def test_float(self): self.assertEqual(clamp(1.25, 0.5, 2.5), 1.25)
    def test_reversed(self):
        with self.assertRaises(ValueError): clamp(5, 10, 0)
if __name__ == '__main__': unittest.main()
'''
ORIGINAL = 'def clamp(value, lower, upper):\n    return min(value, lower)\n'
GITIGNORE = '.velella/\n__pycache__/\n'
# -I ignores the worktree/PYTHONPATH when loading stdlib. The candidate is loaded
# explicitly from source, so a generated unittest.py or bytecode cannot replace
# the trusted test suite. This command is only executed by an isolated task.
VERIFY_COMMAND = '/usr/bin/python3 -I -c ' + shlex.quote(
    "import pathlib, sys, types, unittest\n"
    "successful = False\n"
    "try:\n"
    "    candidate = types.ModuleType('clamp'); sys.modules['clamp'] = candidate\n"
    "    exec(compile(pathlib.Path('clamp.py').read_bytes(), 'clamp.py', 'exec'), candidate.__dict__)\n"
    "    tests = types.ModuleType('acceptance_tests')\n"
    f"    exec(compile({TESTS!r}, '<acceptance-tests>', 'exec'), tests.__dict__)\n"
    "    suite = unittest.TestLoader().loadTestsFromTestCase(tests.ClampTests)\n"
    "    result = unittest.TextTestRunner(verbosity=2).run(suite)\n"
    "    successful = result.testsRun == 6 and result.wasSuccessful()\n"
    "except BaseException as error:\n"
    "    print('Acceptance verification failed: ' + type(error).__name__, file=sys.stderr)\n"
    "sys.exit(0 if successful else 1)\n"
)


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def source_hashes():
    repository = pathlib.Path(__file__).resolve().parents[1]
    paths = [repository / path for path in (
        'Cargo.toml', 'Cargo.lock', 'crates/velella/Cargo.toml',
        'crates/velella-core/Cargo.toml', 'scripts/verify-real-sandbox-agent.py',
        'scripts/fixtures/sandbox-codex-protocol.py',
    )]
    for crate in ('velella', 'velella-core'):
        paths.extend((repository / 'crates' / crate / 'src').rglob('*.rs'))
    return {str(path.relative_to(repository)): file_sha256(path) for path in sorted(paths)}


def regular_text(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 65536:
        raise RuntimeError('Fixture contains an unsafe or oversized file')
    return path.read_text()


def fixture_inventory(work, root, repo, git_marker=None, *, changed=True):
    if work.resolve(strict=True) != work or not work.is_relative_to(root) or work == repo:
        raise RuntimeError('Worktree is outside the owned test directory')
    expected = {'clamp.py', 'test_clamp.py', '.gitignore', '.git'}
    names = {p.name for p in work.iterdir()}
    if names - expected - {'__pycache__'} or not expected.issubset(names):
        raise RuntimeError('Unexpected or missing worktree files')
    cache = work / '__pycache__'
    if '__pycache__' in names:
        if cache.is_symlink() or not cache.is_dir():
            raise RuntimeError('Unsafe fixture cache directory')
        for item in cache.iterdir():
            info = item.lstat()
            if (not re.fullmatch(r'(?:clamp|test_clamp)\.cpython-\d+(?:\.opt-\d+)?\.pyc', item.name)
                    or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1):
                raise RuntimeError('Unexpected fixture cache file')
    contents = {name: regular_text(work / name) for name in expected}
    if contents['test_clamp.py'] != TESTS or contents['.gitignore'] != GITIGNORE:
        raise RuntimeError('Fixed fixture files were modified')
    marker = contents['.git']
    if git_marker is None:
        prefix = 'gitdir: '
        if not marker.startswith(prefix):
            raise RuntimeError('Invalid worktree metadata')
        gitdir = pathlib.Path(marker[len(prefix):].strip())
        if not gitdir.is_absolute() or not gitdir.is_relative_to(repo / '.git' / 'worktrees'):
            raise RuntimeError('Worktree metadata points outside the test repository')
    elif marker != git_marker:
        raise RuntimeError('Worktree metadata was modified')
    if changed and contents['clamp.py'] == ORIGINAL:
        raise RuntimeError('Candidate source was not changed')
    for name, expected_text in [('clamp.py', ORIGINAL), ('test_clamp.py', TESTS), ('.gitignore', GITIGNORE)]:
        if regular_text(repo / name) != expected_text:
            raise RuntimeError('Original checkout was modified')
    return marker


def passed_task(task):
    checks = task.get('checks', [])
    return (task.get('verified') is True and len(checks) == 1
            and checks[0].get('command') == VERIFY_COMMAND
            and checks[0].get('passed') is True and checks[0].get('exit') == 0
            and task.get('agent') and task.get('status', {}).get('hooks_ready') is True
            and task['status'].get('turns', 0) >= 1)


def device_challenge(text):
    text = ANSI.sub('', text)
    url = 'https://auth.openai.com/codex/device'
    code = re.search(r'\b[A-Z0-9]{4,5}-[A-Z0-9]{4,5}\b', text)
    return {'url': url, 'code': code.group()} if url in text and code else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', required=True)
    parser.add_argument('--output', required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--allow-device-login', action='store_true')
    mode.add_argument('--fixture-agent', action='store_true',
                      help='fixed protocol fixture; no login, model or networking')
    parser.add_argument('--execution', choices=['isolated', 'restricted'], default='isolated',
                        help='restricted is experimental and currently accepts only the offline fixture')
    parser.add_argument('--timeout', type=int, default=900)
    args = parser.parse_args()
    if not args.allow_device_login and not args.fixture_agent:
        parser.error('explicit --allow-device-login required; no host auth is reused')
    if args.execution == 'restricted' and not args.fixture_agent:
        parser.error('restricted acceptance is offline and requires --fixture-agent')
    if args.timeout <= 0:
        parser.error('timeout must be positive')
    runtime = pathlib.Path(args.runtime).resolve(strict=True)
    output = pathlib.Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error('output directory must be empty; refusing stale acceptance evidence')
    network = 'denied' if args.fixture_agent else 'host'
    receipt = {'status': 'running',
               'scope': (f'fixed fixture through real Velella {args.execution} task; no model or login'
                         if args.fixture_agent else 'real Codex through Velella isolated task'),
               'agent_kind': 'fixture' if args.fixture_agent else 'real_codex',
               'execution_mode': args.execution,
               'network': network, 'host_credentials_copied': False,
               'runtime_sha256': file_sha256(runtime), 'source_sha256': source_hashes()}
    daemon = None
    task_id = None
    root = None
    result_code = 1

    def run(command, **kwargs):
        return subprocess.run(command, capture_output=True, text=True, timeout=20, **kwargs)

    def rpc(request):
        reply = run([str(runtime), 'rpc', str(root / 'data'), '--stdin'], input=json.dumps(request))
        if reply.returncode:
            raise RuntimeError('runtime RPC transport failed')
        value = json.loads(reply.stdout)
        if not value.get('ok'):
            raise RuntimeError(str(value.get('error', 'runtime RPC failed')))
        return value

    def interrupted(_signum, _frame):
        raise KeyboardInterrupt('test interrupted')

    previous_handler = signal.signal(signal.SIGTERM, interrupted)
    try:
        probe = run([str(runtime), 'sandbox-capabilities'])
        capability = json.loads(probe.stdout)
        receipt['capabilities'] = capability
        if args.execution == 'restricted' and isinstance(capability, dict):
            capability = capability.get('restricted')
        if not isinstance(capability, dict) or type(capability.get('available')) is not bool:
            raise RuntimeError('Invalid sandbox capability response')
        if probe.returncode or capability['available'] is not True:
            receipt.update(status='blocked', reason='Required kernel sandbox is unavailable')
            return 2
        if not args.fixture_agent:
            version = run(['codex', '--version'])
            if version.returncode:
                raise RuntimeError('Codex executable is unavailable')
            receipt['cli_version'] = version.stdout.strip()
        root = pathlib.Path(tempfile.mkdtemp(prefix='vl-real-')).resolve()
        (output / 'owned-root.txt').write_text(str(root))
        repo = root / 'repo'
        repo.mkdir()
        (root / 'data').mkdir()
        (root / 'outside-marker').write_text('must remain outside task sandbox')
        (repo / 'clamp.py').write_text(ORIGINAL)
        (repo / 'test_clamp.py').write_text(TESTS)
        (repo / '.gitignore').write_text(GITIGNORE)
        for command in (['git', 'init', '-q', str(repo)], ['git', '-C', str(repo), 'add', '.'],
                        ['git', '-C', str(repo), '-c', 'user.name=Acceptance Test', '-c',
                         'user.email=test@example.invalid', 'commit', '-qm', 'test fixture']):
            if run(command).returncode:
                raise RuntimeError('Could not initialize test repository')
        read_only = []
        if args.fixture_agent:
            # A private runtime directory is first on descriptor PATH. Never
            # replace a host CLI or rely on a host Codex installation.
            tools = root / 'tools'
            tools.mkdir()
            staged = tools / 'velella-runtime'
            shutil.copy2(runtime, staged)
            runtime = staged
            fixture = pathlib.Path(__file__).parent / 'fixtures/sandbox-codex-protocol.py'
            shutil.copyfile(fixture, tools / 'codex')
            (tools / 'codex').chmod(0o755)
            read_only = [str(tools)]
        with (root / 'daemon.log').open('w') as log:
            daemon = subprocess.Popen([str(runtime), 'daemon', str(root / 'data')], stdout=log, stderr=log)
        for _ in range(100):
            try:
                rpc({'cmd': 'snapshot'})
                break
            except RuntimeError:
                if daemon.poll() is not None:
                    raise RuntimeError('Test daemon exited during startup')
                time.sleep(.1)
        project = rpc({'cmd': 'project.add', 'path': str(repo)})['project']
        # The login process itself runs under the same outer sandbox as the Agent.
        if args.execution == 'restricted':
            # Landlock denies opening file contents; stat can still reveal names.
            setup_probe = (
                "import errno, os, pathlib, socket\n"
                f"outside = pathlib.Path({str(root / 'outside-marker')!r})\n"
                "try:\n"
                "    outside.read_text()\n"
                "except PermissionError:\n"
                "    pass\n"
                "else:\n"
                "    raise RuntimeError('outside fixture was readable')\n"
                "for family, kind in [(socket.AF_INET, socket.SOCK_STREAM), "
                "(socket.AF_INET, socket.SOCK_DGRAM), (socket.AF_UNIX, socket.SOCK_STREAM)]:\n"
                "    try:\n"
                "        socket.socket(family, kind)\n"
                "    except PermissionError:\n"
                "        pass\n"
                "    else:\n"
                "        raise RuntimeError('socket creation was allowed')\n"
                "pathlib.Path(os.environ['HOME'], 'outside-marker-path').write_text(str(outside))\n"
            )
            setup = '/usr/bin/python3 -I -c ' + shlex.quote(setup_probe) + ' && '
        else:
            setup = f'test "$HOME" = /home/velella && test ! -e {shlex.quote(str(root / "outside-marker"))} && '
        setup += ('printf fixture-prepared > "$HOME/setup-state"' if args.fixture_agent else
                  "codex -c 'cli_auth_credentials_store=\"file\"' login --device-auth")
        rpc({'cmd': 'project.update', 'project_id': project['id'], 'setup': setup})
        task = rpc({'cmd': 'task.create', 'project_id': project['id'], 'spec': {
            'name': ('Fixed protocol fixture with kernel isolation' if args.fixture_agent else
                     'Real model with kernel isolation'), 'agent': 'codex', 'base': 'HEAD',
            'instruction': 'Fix clamp.py to clamp values to [lower, upper], raising ValueError when lower > upper. '
                           'Only change clamp.py. Do not change tests, read credentials, install dependencies, '
                           'use browser tools, commit or push. Run python3 -m unittest -v test_clamp.py.',
            'policy': {'permissions': 'ask', 'execution': {'mode': args.execution, 'network': network,
                       'read_only': read_only, 'writable': []}, 'budget': {'wall_clock': '20m'}},
            'done': [{'run': VERIFY_COMMAND}],
            'verify': {'retries': 0}, 'on_done': 'review'}})['task']
        task_id = task['id']
        receipt['task_id'] = task_id
        trusted = set()
        git_marker = None
        reverifying = False
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            task = next(t for t in rpc({'cmd': 'snapshot'})['tasks'] if t['id'] == task_id)
            receipt.update(phase=task['phase'], reason='Runtime task phase: ' + task['phase'])
            work = pathlib.Path(task['worktree'])
            if git_marker is None and (task.get('command') or task.get('agent')):
                git_marker = fixture_inventory(work, root, repo, changed=False)
            for target in ('command', 'agent'):
                desc = task.get(target)
                if not desc:
                    continue
                if (desc['spawn']['sandbox']['mode'] != args.execution
                        or desc['spawn']['sandbox']['network'] != network):
                    raise RuntimeError('Descriptor lost required isolation')
                terminal = pathlib.Path(desc['directory']) / 'terminal.bin'
                if not terminal.exists():
                    continue
                text = terminal.read_bytes()[-65536:].decode(errors='replace')
                challenge = device_challenge(text) if target == 'command' and not args.fixture_agent else None
                if challenge:
                    temporary = output / 'challenge.tmp'
                    temporary.write_text(json.dumps(challenge) + '\n')
                    temporary.replace(output / 'challenge.json')
                compact = ''.join(ANSI.sub('', text).split()).lower()
                if target == 'agent' and desc['id'] not in trusted and 'doyoutrustthecontentsofthisdirectory?' in compact:
                    # This is the test-owned repo, created above from fixed fixtures.
                    rpc({'cmd': 'terminal.input', 'task_id': task_id, 'target': 'agent',
                         'text': '', 'enter': True, 'input_kind': 'user'})
                    rpc({'cmd': 'terminal.control', 'task_id': task_id, 'target': 'agent', 'human': False})
                    trusted.add(desc['id'])
            if task['phase'] == 'review':
                if not passed_task(task):
                    raise RuntimeError('Isolated acceptance checks did not pass')
                if not reverifying:
                    # Stop the Agent before inspecting the candidate; it cannot
                    # race the fixed-file checks or alter the re-verification.
                    rpc({'cmd': 'task.cancel', 'task_id': task_id})
                    fixture_inventory(work, root, repo, git_marker)
                    rpc({'cmd': 'task.verify', 'task_id': task_id})
                    reverifying = True
                    continue
                fixture_inventory(work, root, repo, git_marker)
                receipt.update(status='passed', verified=True, independent_test_exit=0,
                               independent_test_execution=f'{args.execution} task.verify after Agent stopped',
                               changed_files=['clamp.py'], hooks_ready=task['status']['hooks_ready'],
                               turns=task['status']['turns'], test_sha256=hashlib.sha256(TESTS.encode()).hexdigest())
                result_code = 0
                break
            if task['phase'] in ('needs_attention', 'needs_recovery', 'cancelled'):
                receipt.update(status='failed')
                break
            time.sleep(1)
        else:
            receipt.update(status='blocked', reason=('Timed out waiting for fixture completion'
                           if args.fixture_agent else 'Timed out waiting for device login or Agent completion'))
            result_code = 2
    except (Exception, KeyboardInterrupt) as error:
        receipt.update(status='failed', reason=str(error))
    finally:
        cleanup_errors = []
        if task_id:
            try:
                rpc({'cmd': 'task.cancel', 'task_id': task_id})
                receipt['task_stopped'] = True
            except Exception:
                receipt['task_stopped'] = False
                cleanup_errors.append('Could not confirm test task stopped')
        if daemon:
            try:
                if daemon.poll() is None:
                    daemon.terminate()
                    try:
                        daemon.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        daemon.kill()
                        daemon.wait(timeout=8)
            except Exception:
                cleanup_errors.append('Could not confirm private daemon stopped')
        if root:
            try:
                shutil.rmtree(root)
            except OSError:
                cleanup_errors.append('Could not remove temporary test HOME')
            receipt['temporary_home_removed'] = not root.exists()
        if cleanup_errors:
            receipt.update(execution_status=receipt['status'], status='failed',
                           cleanup_errors=cleanup_errors)
            result_code = 1
        signal.signal(signal.SIGTERM, previous_handler)
        temporary = output / 'result.tmp'
        temporary.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
        temporary.replace(output / 'result.json')
    return result_code


if __name__ == '__main__':
    raise SystemExit(main())
