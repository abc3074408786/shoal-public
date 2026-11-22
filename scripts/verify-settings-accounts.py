"""Isolated real daemon/PTY account-directory verification, using fake providers."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    # Keep the project outside any existing Git checkout: project.add correctly
    # resolves nested folders to their owning repository and would hit its lock.
    root = Path(tempfile.mkdtemp(prefix='velella-settings-accounts-'))
    data, project, tools = [root / name for name in ('data', 'project', 'tools')]
    homes = {name: root / name for name in ('work', 'personal', 'default-codex', 'default-claude')}
    for directory in [data, project, tools, *homes.values()]:
        directory.mkdir()
    fixture = tools / 'provider.py'
    fixture.write_text('''import json, os, pathlib, sys, time
provider=sys.argv[1];args=sys.argv[2:]
home=os.environ.get('CODEX_HOME' if provider=='codex' else 'CLAUDE_CONFIG_DIR')
if args==['--version']: print('synthetic-provider-1');sys.exit(0)
if args==['--help']: print('synthetic help');sys.exit(0)
if args in (['login','status'],['auth','status']):
    signed=pathlib.Path(home).name=='work'
    print(json.dumps({'loggedIn':signed,'privateToken':'SYNTHETIC-MUST-NOT-BE-RETURNED'}))
    sys.exit(0 if signed else 1)
pathlib.Path(home,'observed-'+str(os.getpid())+'.json').write_text(json.dumps({'provider':provider,'args':args,'home':home}),encoding='utf-8')
print('SYNTHETIC TERMINAL, NO MODEL',flush=True)
time.sleep(45)
''', encoding='utf-8')
    if os.name != 'nt':
        raise RuntimeError('This harness is explicitly Windows-only; no fallback to real providers.')
    for provider, program in [('codex', 'codex'), ('claude', 'claude')]:
        (tools / (program + '.cmd')).write_text(f'@echo off\n"{sys.executable}" "{fixture}" {provider} %*\n', encoding='utf-8')
    env = {**os.environ, 'PATH': str(tools) + os.pathsep + os.environ.get('PATH', ''),
           'HOME': str(root), 'USERPROFILE': str(root), 'VELELLA_DATA_DIR': str(data),
           'CODEX_HOME': str(homes['default-codex']), 'CLAUDE_CONFIG_DIR': str(homes['default-claude']),
           'ORCA_BACKGROUND_LAUNCH': '1'}
    # The only provider commands reachable on PATH are our local fixtures.
    checks = []
    flags = subprocess.CREATE_NO_WINDOW
    daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, creationflags=flags)

    def rpc(request, ok=True):
        result = subprocess.run([str(runtime), 'rpc', str(data), '--stdin'], input=json.dumps(request).encode(),
                                capture_output=True, env=env, timeout=25, creationflags=flags)
        response = json.loads(result.stdout)
        assert response.get('ok') == ok, (request, response)
        assert 'SYNTHETIC-MUST-NOT-BE-RETURNED' not in result.stdout.decode(), response
        return response

    def check(name):
        checks.append(name)
        print('PASS ' + name, flush=True)

    def wait_for(predicate):
        deadline = time.monotonic() + 15
        while not predicate():
            assert time.monotonic() < deadline, 'fixture observation deadline'
            time.sleep(0.05)

    try:
        deadline = time.monotonic() + 15
        while True:
            assert daemon.poll() is None, 'isolated daemon exited'
            probe = subprocess.run([str(runtime), 'rpc', str(data), '--stdin'], input=b'{"cmd":"protocol"}', capture_output=True, env=env, timeout=2, creationflags=flags)
            if probe.returncode == 0:
                break
            assert time.monotonic() < deadline, 'isolated startup deadline'
            time.sleep(0.1)
        assert 'terminal-accounts-v1' in rpc({'cmd': 'protocol'})['capabilities']
        for provider in ('codex', 'claude-code'):
            assert rpc({'cmd': 'agent.check', 'agent': provider, 'account_home': str(homes['work'])})['authenticated'] is True
            assert rpc({'cmd': 'agent.check', 'agent': provider, 'account_home': str(homes['personal'])})['authenticated'] is False
        check('profile-specific signed-in and signed-out checks omit provider auth payloads')
        project_id = rpc({'cmd': 'project.add', 'path': str(project)})['project']['id']
        workspace = 'main:' + project_id
        for provider in ('codex', 'claude-code'):
            sid = str(uuid.uuid4())
            request = {'cmd': 'workbench.terminal.open', 'workspace_id': workspace,
                       'launch': provider, 'request_id': sid, 'account_home': str(homes['work'])}
            assert rpc(request)['session_id'] == sid
            descriptor = json.loads((project / '.velella' / 'sessions' / sid / 'launch.json').read_text())
            variable = 'CODEX_HOME' if provider == 'codex' else 'CLAUDE_CONFIG_DIR'
            assert Path(descriptor['spawn']['env'][variable]) == homes['work']
            assert rpc(request)['session_id'] == sid
            rpc({**request, 'account_home': str(homes['personal'])}, ok=False)
            rpc({**request, 'account_action': 'login'}, ok=False)
            rpc({'cmd': 'workbench.terminal.recover', 'workspace_id': workspace, 'session_id': sid,
                 'account_home': str(homes['personal'])}, ok=False)
        wait_for(lambda: len(list(homes['work'].glob('observed-*.json'))) >= 2)
        check('real native PTYs use the chosen directory; replay and recovery cannot switch it')
        sid = str(uuid.uuid4())
        login = {'cmd': 'workbench.terminal.open', 'workspace_id': workspace, 'launch': 'codex',
                 'request_id': sid, 'account_home': str(homes['personal']), 'account_action': 'login'}
        rpc(login)
        wait_for(lambda: bool(list(homes['personal'].glob('observed-*.json'))))
        observations = [json.loads(path.read_text()) for path in homes['personal'].glob('observed-*.json')]
        assert observations[0]['args'] == ['login'], observations
        check('login terminal runs only fixed login argv in the selected directory')
        rpc({**login, 'request_id': str(uuid.uuid4()), 'permission_mode': 'full-access'}, ok=False)
        rpc({**login, 'request_id': str(uuid.uuid4()), 'launch': 'omp'}, ok=False)
        rpc({**login, 'request_id': str(uuid.uuid4()), 'account_home': 'relative'}, ok=False)
        check('unsupported providers, relative paths and permission overrides fail closed')
        assert not list(homes['default-codex'].glob('observed-*.json'))
        assert not list(homes['default-claude'].glob('observed-*.json'))
        check('default account directories remain unused')
    finally:
        if daemon.poll() is None:
            try:
                rpc({'cmd': 'service.stop'})
                daemon.wait(timeout=15)
            except Exception:
                daemon.terminate()
                daemon.wait(timeout=10)
        receipt = {'checks': checks, 'runtime': str(runtime), 'runtime_sha256': hashlib.sha256(runtime.read_bytes()).hexdigest(),
                   'isolated_root': str(root), 'daemon_stopped': daemon.poll() is not None,
                   'synthetic_providers_only': True, 'real_model_requests': False, 'real_credentials_accessed': False,
                   'completed_at': datetime.datetime.now(datetime.timezone.utc).isoformat()}
        (output / 'accounts-integration.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
