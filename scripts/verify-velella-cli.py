"""Exercise the real CLI/daemon/PTY in a new profile, never the live desktop."""
import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--receipt', type=Path)
    parser.add_argument('--management', action='store_true', help='Also exercise Velella management commands without model requests')
    parser.add_argument('--extended', action='store_true', help='Exercise workspace, terminal, automation, SSH profile, file/Git, skill and MCP management')
    args = parser.parse_args()
    cli, runtime = args.cli.resolve(strict=True), args.runtime.resolve(strict=True)
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    evidence = {'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'cli': str(cli), 'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'runtime': str(runtime), 'runtime_sha256': hashlib.sha256(runtime.read_bytes()).hexdigest(),
                'checks': [], 'live_profile_mutated': False, 'real_model_requests': False}

    def run(argv, **kw):
        return subprocess.run([str(v) for v in argv], capture_output=True, timeout=30,
                              creationflags=flags, **kw)

    with tempfile.TemporaryDirectory(prefix='velella-cli-integration-') as temp:
        root = Path(temp)
        data = root / 'isolated profile'
        data.mkdir()
        folder = root / '普通文件夹 project'
        folder.mkdir()
        repository = root / 'git project'
        repository.mkdir()
        fixture_home = root / 'fixture-home'
        skill_path = fixture_home / '.codex' / 'skills' / 'cli-fixture'
        skill_path.mkdir(parents=True)
        (skill_path / 'SKILL.md').write_text('---\nname: cli-fixture\ndescription: Isolated CLI verification fixture\n---\nFixture text, never executed.\n', encoding='utf-8')
        evidence['isolated_data_dir'] = str(data)
        sessions = []
        daemon = None
        remote_daemon = None
        remote_data = None

        def invoke(*argv, ok=True, profile=data, environment=None, timeout_ms=5000):
            result = run([cli, '--data-dir', profile, '--timeout-ms', str(timeout_ms), '--json', *argv], env=environment)
            value = json.loads(result.stdout.decode('utf-8-sig'))
            assert (result.returncode == 0) == ok, (argv, result.returncode, value, result.stderr.decode(errors='replace'))
            assert value['ok'] == ok, value
            return value['result'] if ok else value

        def rpc(request, profile=data):
            result = run([runtime, 'rpc', profile, '--stdin'], input=json.dumps(request).encode())
            value = json.loads(result.stdout)
            assert result.returncode == 0 and value['ok'], value
            return value

        def check(name):
            evidence['checks'].append(name)

        try:
            offline = invoke('status', ok=False)
            assert offline['error']['code'] == 'runtime_unavailable'
            assert 'CLI' in invoke('guide')['guide']
            if args.management:
                assert 'Velella CLI' in invoke('guide')['guide']
                assert 'on_done: review' in invoke('task', 'template')['text']
                check('Velella identity and offline task template')
            invalid = run([cli, '--json', 'terminal', 'stop', '--workspace', 'w', '--terminal', 's'])
            assert invalid.returncode == 2 and json.loads(invalid.stdout)['error']['code'] == 'invalid_arguments'
            check('offline status, offline guide and JSON argument failures')
            env = {**os.environ, 'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1',
                   'USERPROFILE': str(fixture_home), 'HOME': str(fixture_home)}
            daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                      creationflags=flags)
            deadline = time.monotonic() + 20
            while True:
                status = run([cli, '--data-dir', data, '--timeout-ms', '500', '--json', 'status'])
                if status.returncode == 0:
                    break
                assert daemon.poll() is None, f'isolated daemon exited: {daemon.returncode}'
                assert time.monotonic() < deadline, 'isolated daemon startup deadline'
                time.sleep(0.15)
            assert invoke('status')['pid'] == daemon.pid
            check('real desktop daemon identity in isolated profile')

            project = invoke('project', 'add', folder)['project']
            assert project['direct'] is True
            shown_project = invoke('project', 'show', project['id'])['project']
            assert all(shown_project.get(key) == value for key, value in project.items())
            assert invoke('project', 'add', folder)['project']['id'] == project['id']
            workspaces = invoke('workspace', 'list', '--project', project['id'])['workspaces']
            assert len(workspaces) == 1 and workspaces[0]['kind'] == 'folder'
            workspace = workspaces[0]['id']
            assert invoke('workspace', 'show', workspace)['workspace']['id'] == workspace
            assert invoke('task', 'list')['tasks'] == []
            assert invoke('task', 'show', 'does-not-exist', ok=False)['error']['code'] == 'not_found'
            check('folder import, stable project IDs, workspace and desktop task queries')

            launch_id = str(uuid.uuid4())
            launched = invoke('terminal', 'create', '--workspace', workspace, '--request-id', launch_id)
            terminal = launched['session_id']
            sessions.append((workspace, terminal))
            assert terminal == launch_id
            assert invoke('terminal', 'create', '--workspace', workspace, '--request-id', launch_id)['session_id'] == terminal
            target = ['--workspace', workspace, '--terminal', terminal]
            rows = invoke('terminal', 'list', '--workspace', workspace)['sessions']
            assert len(rows) == 1
            session = invoke('terminal', 'show', *target)['session']
            assert session['status']['pid']
            sequence = session['status']['input_sequence']
            stale = invoke('terminal', 'send', *target, '--text', 'never sent', '--expected-sequence', str(sequence + 10), ok=False)
            assert 'newer input' in stale['error']['message']
            check('real PTY creation, idempotent launch and stale input rejection')

            marker = 'VELELLA_CLI_' + uuid.uuid4().hex
            # The terminal echoes the source in reversed form; only execution emits marker.
            command = ("Write-Output (-join ('%s'.ToCharArray()[%d..0]))" % (marker[::-1], len(marker)-1)
                       if os.name == 'nt' else "printf '%b\\n' '" + ''.join('\\%03o' % ord(c) for c in marker) + "'")
            text_file = root / '命令 UTF8.txt'
            text_file.write_text(command, encoding='utf-8-sig')
            accepted = invoke('terminal', 'send', *target, '--text-file', text_file, '--enter', '--expected-sequence', str(sequence))
            assert accepted['input_accepted'] is True and accepted['completion_verified'] is False
            cursor = 0
            collected = bytearray()
            deadline = time.monotonic() + 15
            while marker.encode() not in collected:
                page = invoke('terminal', 'read', *target, '--cursor', str(cursor), '--limit', '257')
                chunk = base64.b64decode(page['data'])
                assert len(chunk) <= 257 and page['next_cursor'] == page['from'] + len(chunk)
                collected.extend(chunk)
                cursor = page['next_cursor']
                assert time.monotonic() < deadline, collected.decode(errors='replace')[-2000:]
                if not page['has_more']:
                    time.sleep(0.1)
            sequence_after = invoke('terminal', 'show', *target)['session']['status']['input_sequence']
            assert sequence_after > sequence
            replay = invoke('terminal', 'send', *target, '--text', 'never resent', '--enter', '--expected-sequence', str(sequence), ok=False)
            assert 'newer input' in replay['error']['message']
            check('UTF-8 file input executes in real shell, byte cursor paging, no stale replay')

            # Git changes occur only in this disposable repository.
            for command in (["init", "-b", "main"], ["config", "user.name", "CLI Test"],
                            ["config", "user.email", "cli-test@example.invalid"]):
                result = run(['git', '-C', repository, *command])
                assert result.returncode == 0, result.stderr
            (repository / 'README.md').write_text('CLI integration fixture\n', encoding='utf-8')
            assert run(['git', '-C', repository, 'add', 'README.md']).returncode == 0
            assert run(['git', '-C', repository, 'commit', '-m', 'fixture']).returncode == 0
            git_project = invoke('project', 'add', repository)['project']
            creation_id = str(uuid.uuid4())
            create_args = ['workspace', 'create', '--project', git_project['id'], '--name', 'cli-check', '--base', 'main', '--request-id', creation_id]
            created = invoke(*create_args)
            assert created['workspace']['kind'] == 'worktree'
            assert invoke(*create_args)['workspace']['id'] == created['workspace']['id']
            assert invoke('terminal', 'list', '--workspace', created['workspace']['id'])['sessions'] == []
            wrong_target = invoke('terminal', 'show', '--workspace', created['workspace']['id'], '--terminal', terminal, ok=False)
            assert wrong_target['error']['code'] == 'not_found'
            check('real Git worktree creation and replay, no implicit setup/agent, workspace isolation')

            if args.management:
                assert invoke('swarm', 'list')['runs'] == []
                assert invoke('swarm', 'preflight', '--workspace', created['workspace']['id'])['ready'] is True
                assert invoke('swarm', 'preflight', '--workspace', workspace)['ready'] is False
                invoke('swarm', 'capabilities')
                assert invoke('agent', 'list', '--workspace', workspace)['sessions'][0]['id'] == terminal
                missing = invoke('swarm', 'show', 'missing', ok=False)
                assert missing['error']['code'] == 'request_rejected'
                request_id = str(uuid.uuid4())
                rejected = invoke('swarm', 'create', '--workspace', workspace,
                                  '--goal', 'Must not start on a folder', '--acceptance', 'Must reject',
                                  '--request-id', request_id, ok=False)
                assert rejected['error']['request_id'] == request_id
                assert invoke('swarm', 'list')['runs'] == []
                for action in (['swarm', 'stop', 'missing'],
                               ['swarm', 'accept', 'missing', '--summary', 'test'],
                               ['swarm', 'plan', 'confirm', 'missing', '--revision', '0'],
                               ['task', 'cancel', 'missing']):
                    invalid = run([cli, '--json', *action])
                    assert invalid.returncode == 2
                    assert json.loads(invalid.stdout)['error']['code'] == 'invalid_arguments'
                check('real swarm preflight/capabilities/list, Agent alias, rejected creation preserves UUID, explicit decision guards')

            if args.extended:
                schema = invoke('schema')['command']
                assert {c['name'] for c in schema['commands']} >= {'automation', 'host', 'browser', 'skills', 'mcp', 'file', 'git'}
                assert invoke('agent-context')['schema_version'] == 1
                invoke('project', 'configure', git_project['id'], '--default-base', 'main', '--setup-default', 'skip', '--confirm')
                assert invoke('project', 'show', git_project['id'])['project']['setup_default'] == 'skip'
                invoke('workspace', 'update', workspace, '--name', 'CLI renamed', '--favorite', 'false', '--no-parent')
                renamed = invoke('workspace', 'show', workspace)['workspace']
                assert renamed['name'] == 'CLI renamed' and renamed['favorite'] is False
                invoke('workspace', 'archive', workspace, '--confirm', ok=False)
                split_id = str(uuid.uuid4())
                split_args = ['terminal', 'split', *target, '--request-id', split_id, '--direction', 'vertical']
                assert invoke(*split_args)['session_id'] == split_id
                sessions.append((workspace, split_id))
                assert invoke(*split_args)['already_attached'] is True
                invoke('terminal', 'rename', *target, '--title', 'CLI pane')
                layout = invoke('workspace', 'layout', workspace)['layout']
                assert layout['panes'][0]['tabs'][0]['title'] == 'CLI pane'
                assert len(invoke('terminal', 'list', '--workspace', workspace)['sessions']) == 2
                assert invoke('terminal', 'wait', *target, '--for', 'output', '--contains', marker)['matched'] is True
                screen = invoke('terminal', 'screen', *target)
                assert screen['representation'] == 'vt100-replay' and screen['complete_history'] is True
                assert marker in screen['screen'], screen
                check('offline schema, project policy, workspace metadata, live-session archive guard, terminal split replay/title/screen/output wait')

                git_workspace = created['workspace']['id']
                before = root / 'before.txt'
                after = root / 'after.txt'
                # The runtime preserves content exactly. Keep fixture bytes fixed
                # instead of letting Python translate LF to CRLF on Windows.
                before.write_bytes(b'fixture before\n')
                after.write_bytes(b'fixture after\n')
                invoke('file', 'create', '--workspace', git_workspace, 'cli-note.txt', '--file', before)
                assert invoke('file', 'read', '--workspace', git_workspace, 'cli-note.txt')['content'] == 'fixture before\n'
                invoke('file', 'write', '--workspace', git_workspace, 'cli-note.txt', '--file', after, '--previous-file', before)
                invoke('file', 'write', '--workspace', git_workspace, 'cli-note.txt', '--file', before, '--previous-file', before, ok=False)
                assert invoke('file', 'read', '--workspace', git_workspace, 'cli-note.txt')['content'] == 'fixture after\n'
                assert invoke('file', 'search', '--workspace', git_workspace, 'fixture after')['matches']
                invoke('file', 'read', '--workspace', git_workspace, '../outside.txt', ok=False)
                invoke('git', 'stage', '--workspace', git_workspace, 'cli-note.txt')
                assert 'fixture after' in invoke('git', 'diff', '--workspace', git_workspace, '--staged')['diff']
                invoke('git', 'unstage', '--workspace', git_workspace, 'cli-note.txt')
                invoke('git', 'stage', '--workspace', git_workspace, 'cli-note.txt')
                invoke('git', 'commit', '--workspace', git_workspace, '--message', 'CLI isolated fixture', '--confirm')
                assert invoke('git', 'status', '--workspace', git_workspace)['git'] is True
                invoke('file', 'rename', '--workspace', git_workspace, 'cli-note.txt', 'renamed.txt')
                invoke('file', 'remove', '--workspace', git_workspace, 'renamed.txt', '--confirm')
                check('real file CRUD/search, traversal rejection, stale-write rejection, local Git stage/unstage/diff/commit')

                task_file = root / 'task.json'
                task_file.write_text(json.dumps({'name': 'Disabled CLI schedule', 'agent': 'codex',
                    'instruction': 'Isolated verification; must never launch while disabled', 'on_done': 'review'}), encoding='utf-8')
                saved = invoke('automation', 'create', '--project', git_project['id'], '--file', task_file,
                               '--start-at', str(int(time.time()) + 86400), '--interval-minutes', '60', '--confirm')['automation']
                assert saved['enabled'] is False
                assert len(invoke('automation', 'list')['automations']) == 1
                saved = invoke('automation', 'show', saved['id'])['automation']
                automation_file = root / 'reviewed-automation.json'
                automation_file.write_text(json.dumps(saved), encoding='utf-8')
                invoke('automation', 'save', saved['id'], '--file', automation_file, '--confirm')
                invoke('automation', 'pause', saved['id'])
                assert invoke('automation', 'runs', '--automation', saved['id'])['runs'] == []
                assert invoke('task', 'list')['tasks'] == []
                check('disabled schedule create/show/save/pause/history, no Agent or model launched')

                ssh_file = root / 'ssh-profile.json'
                ssh_file.write_text(json.dumps({'name': 'CLI fixture', 'host': 'example.invalid', 'user': 'fixture', 'port': 22}), encoding='utf-8')
                profile = invoke('host', 'save', '--file', ssh_file, '--confirm')['profile']
                assert invoke('host', 'show', profile['id'])['profile']['host'] == 'example.invalid'
                assert invoke('host', 'inspect', profile['id'])['profile'] == profile
                assert invoke('host', 'diagnose', profile['id'])['network'] is False
                # No network/credentials: a temporary executable shim feeds the real
                # framed remote client into a second real isolated daemon.
                shim_dir = root / 'ssh-shim'
                shim_dir.mkdir()
                shim = shim_dir / ('ssh.exe' if os.name == 'nt' else 'ssh')
                compiled = run(['rustc', '--edition=2021', Path(__file__).parent / 'fixtures' / 'cli-ssh-fixture.rs', '-o', shim])
                assert compiled.returncode == 0, compiled.stderr.decode(errors='replace')
                remote_data = root / 'isolated-remote-profile'
                remote_data.mkdir()
                remote_project = root / 'isolated-remote-project'
                remote_project.mkdir()
                remote_daemon = subprocess.Popen([str(runtime), 'daemon', str(remote_data)], env={**env, 'VELELLA_DATA_DIR': str(remote_data)},
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
                deadline = time.monotonic() + 15
                while run([cli, '--data-dir', remote_data, '--timeout-ms', '500', '--json', 'status']).returncode:
                    assert remote_daemon.poll() is None and time.monotonic() < deadline
                    time.sleep(0.1)
                remote_env = {**os.environ, 'PATH': str(shim_dir) + os.pathsep + os.environ.get('PATH', ''),
                    'VELELLA_FIXTURE_REMOTE_RUNTIME': str(runtime), 'VELELLA_FIXTURE_REMOTE_DATA': str(remote_data),
                    'VELELLA_FIXTURE_SSH_LOG': str(root / 'ssh-args.txt')}
                identity = invoke('host', 'inspect-runtime', profile['id'], '--remote-root', remote_data, environment=remote_env)['identity']
                remote_flags = ['--ssh-profile', profile['id'], '--server-id', identity['server_id'], '--remote-data-dir', identity['data_root']]
                assert invoke(*remote_flags, 'status', environment=remote_env)['projects'] == 0
                remote_added = invoke(*remote_flags, 'project', 'add', remote_project, environment=remote_env)['project']
                assert len(invoke(*remote_flags, 'project', 'list', environment=remote_env)['projects']) == 1
                assert len(invoke('project', 'list')['projects']) == 2
                remote_workspace = invoke(*remote_flags, 'workspace', 'list', '--project', remote_added['id'], environment=remote_env)['workspaces'][0]['id']
                invoke(*remote_flags, 'file', 'create', '--workspace', remote_workspace, 'remote.txt', '--file', before, environment=remote_env)
                assert invoke(*remote_flags, 'file', 'read', '--workspace', remote_workspace, 'remote.txt', environment=remote_env)['content'] == 'fixture before\n'
                invoke(*remote_flags, 'automation', 'list', environment=remote_env, ok=False)
                invoke(*remote_flags, 'browser', 'status', environment=remote_env, ok=False)
                wrong_flags = ['--ssh-profile', profile['id'], '--server-id', str(uuid.uuid4()), '--remote-data-dir', identity['data_root']]
                invoke(*wrong_flags, 'project', 'add', folder, environment=remote_env, ok=False)
                assert invoke(*remote_flags, 'status', environment=remote_env)['projects'] == 1
                delayed_env = {**remote_env, 'VELELLA_FIXTURE_SSH_DELAY_MS': '2000'}
                uncertain = invoke(*remote_flags, 'file', 'create', '--workspace', remote_workspace,
                    'never-created.txt', '--file', before, environment=delayed_env, timeout_ms=500, ok=False)
                assert uncertain['error']['code'] == 'delivery_uncertain'
                invoke(*remote_flags, 'file', 'read', '--workspace', remote_workspace, 'never-created.txt', environment=remote_env, ok=False)
                ssh_args = (root / 'ssh-args.txt').read_text(encoding='utf-8')
                assert 'StrictHostKeyChecking=yes' in ssh_args and 'BatchMode=yes' in ssh_args and 'ForwardAgent=no' in ssh_args
                rpc({'cmd': 'service.stop'}, profile=remote_data)
                remote_daemon.wait(timeout=15)
                evidence['remote_transport_scope'] = 'Temporary SSH shim plus real remote-stdio and isolated daemon; no SSH network or account used'
                check('remote identity inspection/confirmation, framed RPC target isolation, no local fallback, identity rejection, timeout without replay')
                invoke('host', 'remove', profile['id'], '--confirm')
                assert invoke('host', 'list')['profiles'] == []
                assert any(s['name'] == 'cli-fixture' for s in invoke('skills', 'list')['skills'])
                skill = invoke('skills', 'show', 'cli-fixture')
                assert skill['content_is_untrusted'] is True and 'Fixture text' in skill['content']
                manifest = root / 'mcp.json'
                manifest.write_text(json.dumps({'version': 1, 'entries': [{'id': 'cli-fixture',
                    'command': 'fixture-mcp-not-executed', 'args': [], 'catalog': ['read']}]}), encoding='utf-8')
                preview = invoke('mcp', 'preview', manifest)['catalog']
                confirm_args = ['mcp', 'confirm', manifest, '--digest', preview['digest'], '--entry', 'cli-fixture', '--tool', 'read', '--request-id', str(uuid.uuid4()), '--confirm']
                confirmed = invoke(*confirm_args)
                assert invoke(*confirm_args) == confirmed
                invoke('mcp', 'confirm', manifest, '--digest', 'sha256:stale', '--entry', 'cli-fixture', '--tool', 'read', '--confirm', ok=False)
                check('SSH profile CRUD/inspect/local-only diagnosis, isolated skill discovery/read, MCP preview/confirmed replay/stale digest guard')

                invoke('terminal', 'stop', '--workspace', workspace, '--terminal', split_id, '--confirm')
                invoke('terminal', 'wait', '--workspace', workspace, '--terminal', split_id, '--for', 'exit', '--deadline-ms', '5000')
                sessions.remove((workspace, split_id))

            invoke('terminal', 'stop', *target, '--confirm')
            sessions.clear()
            assert invoke('terminal', 'send', *target, '--text', 'after stop', '--enter', ok=False)['ok'] is False
            check('explicit stop and rejected post-stop input')
            if args.extended:
                invoke('terminal', 'wait', *target, '--for', 'exit', '--deadline-ms', '5000')
                deadline = time.monotonic() + 10
                while not invoke('terminal', 'show', *target)['session']['recoverable']:
                    assert time.monotonic() < deadline, 'PTY exited but the owned generic host did not settle'
                    time.sleep(0.1)
                invoke('workspace', 'archive', workspace, '--confirm')
                assert invoke('workspace', 'show', workspace)['workspace']['archived'] is True
                invoke('workspace', 'restore', workspace)
                assert invoke('workspace', 'show', workspace)['workspace']['archived'] is False
                check('observed real terminal exits and safe archive/restore after sessions stopped')
            evidence['passed'] = True
        except Exception as error:
            evidence['passed'] = False
            evidence['error'] = str(error)
            raise
        finally:
            try:
                if remote_daemon and remote_daemon.poll() is None:
                    try:
                        rpc({'cmd': 'service.stop'}, profile=remote_data)
                        remote_daemon.wait(timeout=15)
                    except Exception:
                        remote_daemon.terminate()
                        remote_daemon.wait(timeout=10)
                if daemon and daemon.poll() is None:
                    for workspace_id, session_id in sessions:
                        rpc({'cmd': 'workbench.terminal.stop', 'workspace_id': workspace_id,
                             'session_id': session_id, 'confirmed': True})
                    rpc({'cmd': 'service.stop'})
                    daemon.wait(timeout=15)
            finally:
                if daemon and daemon.poll() is None:
                    daemon.terminate()
                    daemon.wait(timeout=10)
                evidence['isolated_daemon_stopped'] = daemon is None or daemon.poll() is not None
                evidence['isolated_remote_daemon_stopped'] = remote_daemon is None or remote_daemon.poll() is not None
                evidence['completed_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
                if args.receipt:
                    args.receipt.parent.mkdir(parents=True, exist_ok=True)
                    args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
