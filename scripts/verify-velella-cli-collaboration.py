"""Public CLI compatibility against a frozen runtime in a disposable profile.

The native Agent executable is a local inert fixture, not a model. The daemon,
session hosts, desktop identity adapter, collaboration DB and IPC are real.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    cli, runtime = args.cli.resolve(strict=True), args.runtime.resolve(strict=True)
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    evidence = {'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'runtime_sha256': hashlib.sha256(runtime.read_bytes()).hexdigest(),
                'scope': 'real frozen runtime and IPC; inert native Agent executable fixture',
                'real_model_requests': False, 'production_profile_mutated': False, 'checks': []}
    with tempfile.TemporaryDirectory(prefix='velella-collab-compat-') as temp:
        root = Path(temp)
        data, home, repo, bin_dir = [root / name for name in ['data', 'home', 'repo', 'bin']]
        for path in [data, home, repo, bin_dir]:
            path.mkdir()
        node = shutil.which('node')
        assert node, 'Node is required only for the inert Agent test fixture'
        fixture = bin_dir / 'inert-agent.cjs'
        fixture.write_text('console.log("VELELLA_INERT_AGENT_FIXTURE"); setInterval(()=>{}, 1000);\n', encoding='utf-8')
        if os.name == 'nt':
            (bin_dir / 'codex.cmd').write_text(f'@echo off\n"{node}" "{fixture}"\n', encoding='utf-8')
        else:
            shim = bin_dir / 'codex'
            import shlex
            shim.write_text(f'#!/bin/sh\nexec {shlex.quote(node)} {shlex.quote(str(fixture))}\n', encoding='utf-8')
            shim.chmod(0o700)
        env = {**os.environ, 'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
               'HOME': str(home), 'USERPROFILE': str(home), 'CODEX_HOME': str(home / '.codex'),
               'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1',
               'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_NOSYSTEM': '1'}
        for name in ['VELELLA_SESSION_ID', 'VELELLA_COLLAB_CREDENTIALS']:
            env.pop(name, None)

        def run(argv, environment=env, **kw):
            return subprocess.run([str(v) for v in argv], env=environment, capture_output=True,
                                  timeout=20, creationflags=flags, **kw)

        def invoke(*argv, sid=None, ok=True, profile=data):
            call_env = dict(env)
            if sid:
                call_env['VELELLA_SESSION_ID'] = sid
            out = run([cli, '--data-dir', profile, '--json', '--timeout-ms', '5000', *argv], environment=call_env)
            value = json.loads(out.stdout)
            assert (out.returncode == 0) == ok and value['ok'] == ok, (argv[:2], value)
            return value['result'] if ok else value

        def rpc(request):
            out = run([runtime, 'rpc', data, '--stdin'], input=json.dumps(request).encode())
            value = json.loads(out.stdout)
            assert out.returncode == 0 and value['ok'], value
            return value

        for cmd in [['init'], ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                               'commit', '--allow-empty', '-m', 'isolated baseline']]:
            out = run(['git', '-C', repo, *cmd])
            assert out.returncode == 0, out.stderr.decode(errors='replace')
        sessions, daemon = [], None
        try:
            missing_profile = root / 'must-not-be-created'
            for name in ['velella-cli', 'velella-collaboration']:
                guide = invoke('skills', 'get', name, profile=missing_profile)
                assert guide['source'] == 'bundled-cli' and guide['guide_revision']
                plain = run([cli, '--data-dir', missing_profile, 'skills', 'get', name])
                assert plain.returncode == 0 and plain.stdout.decode('utf-8').startswith('# Velella CLI')
            assert len(invoke('skills', 'bundled', profile=missing_profile)['skills']) == 2
            assert invoke('skills', 'get', 'missing', ok=False, profile=missing_profile)['error']['code'] == 'unknown_bundled_skill'
            assert 'velella-collaboration' in json.dumps(invoke('schema', profile=missing_profile))
            denied = invoke('--ssh-profile', 'missing', '--server-id', str(uuid.uuid4()), '--remote-data-dir', '/missing',
                             'collab', 'status', ok=False, profile=missing_profile)
            assert 'local-session-only' in denied['error']['message']
            assert 'stdio stream' in invoke('collab', 'mcp', ok=False, profile=missing_profile)['error']['message']
            assert not missing_profile.exists()
            evidence['checks'].append('offline Markdown/JSON guides, schema, missing-guide and SSH/MCP refusal without disk writes')

            daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
            deadline = time.monotonic() + 15
            while True:
                out = run([cli, '--data-dir', data, '--json', '--timeout-ms', '300', 'status'])
                if out.returncode == 0:
                    assert json.loads(out.stdout)['result']['pid'] == daemon.pid
                    break
                assert daemon.poll() is None and time.monotonic() < deadline, 'isolated daemon startup'
                time.sleep(.1)
            project = invoke('project', 'add', repo)['project']['id']
            workspace = invoke('workspace', 'list', '--project', project)['workspaces'][0]['id']
            for _ in range(2):
                sid = invoke('terminal', 'create', '--workspace', workspace, '--launch', 'codex', '--request-id', str(uuid.uuid4()))['session_id']
                sessions.append((workspace, sid))
                deadline = time.monotonic() + 10
                while True:
                    output = invoke('terminal', 'read', '--workspace', workspace, '--terminal', sid)
                    if b'VELELLA_INERT_AGENT_FIXTURE' in base64.b64decode(output['data']):
                        break
                    assert time.monotonic() < deadline, 'inert Agent fixture was not selected on PATH'
                    time.sleep(.1)
            lead, member = [s[1] for s in sessions]
            request_id = str(uuid.uuid4())
            command = ['collab', 'create', '--goal', 'frozen runtime compatibility', '--request-id', request_id]
            created = invoke(*command, sid=lead)
            assert 'credentials' not in created and created['credentials_file']
            assert invoke(*command, sid=lead)['run']['id'] == created['run']['id']
            credential = json.loads(Path(created['credentials_file']).read_text(encoding='utf-8-sig'))
            assert credential['token'] not in json.dumps(created)
            invited = invoke('collab', 'invite', '--session', member, '--name', 'member', '--request-id', str(uuid.uuid4()), sid=lead)
            join = ['collab', 'join', '--run', created['run']['id'], '--member', invited['member']['id'],
                    '--invitation', invited['invitation'], '--request-id', str(uuid.uuid4())]
            invoke(*join, sid=member, ok=False)
            invoke('collab', 'status', sid=member, ok=False)
            invoke(*join, '--confirm', sid=member)
            evidence['checks'].append('desktop-resolved native fixture identity, create replay, private credentials, confirmed invite/join')
            send = ['collaboration', 'send', '--to', invited['member']['id'], '--text', 'one durable fixture message', '--request-id', str(uuid.uuid4())]
            invoke(*send, sid=lead)
            invoke(*send, sid=lead)
            received = invoke('collab', 'receive', '--wait-ms', '0', sid=member)
            assert sum(m['body'].get('text') == 'one durable fixture message' for m in received['messages']) == 1
            invoke('collab', 'ack', '--seq', str(received['messages'][-1]['seq']), '--request-id', str(uuid.uuid4()), sid=member)
            assert not invoke('collab', 'receive', '--wait-ms', '0', sid=member)['messages']
            evidence['checks'].append('direct CLI mailbox, stable request deduplication, explicit acknowledgement')
            legacy_env = {**env, 'VELELLA_SESSION_ID': lead}
            old = run([runtime, 'collab', 'status'], environment=legacy_env)
            assert old.returncode == 0 and json.loads(old.stdout)['run']['id'] == created['run']['id']
            invoke('collab', 'status', sid=str(uuid.uuid4()), ok=False)
            invoke('collab', 'status', sid=member, ok=False, profile=missing_profile)
            invoke('collab', 'create', '--goal', 'missing stable ID', sid=lead, ok=False)
            assert not missing_profile.exists()
            evidence['checks'].append('legacy runtime CLI compatibility and cross-session/profile/request-ID rejection')
            evidence['passed'] = True
        except Exception as error:
            evidence.update(passed=False, error=str(error))
            raise
        finally:
            if daemon and daemon.poll() is None:
                try:
                    for workspace, sid in sessions:
                        rpc({'cmd': 'workbench.terminal.stop', 'workspace_id': workspace, 'session_id': sid, 'confirmed': True})
                    rpc({'cmd': 'service.stop'})  # This exact disposable profile only.
                    daemon.wait(timeout=10)
                finally:
                    if daemon.poll() is None:
                        daemon.terminate()
                        daemon.wait(timeout=10)
            evidence['isolated_daemon_stopped'] = daemon is None or daemon.poll() is not None
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
