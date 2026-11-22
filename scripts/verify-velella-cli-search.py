"""Search real PTY recordings in isolated workspaces; no native model calls."""
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
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    cli, runtime = args.cli.resolve(strict=True), args.runtime.resolve(strict=True)
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    evidence = {'cli': str(cli), 'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'runtime': str(runtime), 'runtime_sha256': hashlib.sha256(runtime.read_bytes()).hexdigest(),
                'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'checks': [],
                'live_profile_mutated': False, 'real_model_requests': False, 'real_user_conversations_read': False}
    daemon = None
    sessions = []
    with tempfile.TemporaryDirectory(prefix='velella-cli-search-') as directory:
        root = Path(directory)
        home, data = root / 'fixture home', root / 'profile'
        home.mkdir()
        data.mkdir()
        env = {**os.environ, 'USERPROFILE': str(home), 'HOME': str(home),
               'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1'}

        def run(*argv, **kw):
            return subprocess.run([str(v) for v in argv], capture_output=True, timeout=25,
                                  env=env, creationflags=flags, **kw)

        def invoke(*argv, ok=True):
            output = run(cli, '--data-dir', data, '--json', '--timeout-ms', '5000', *argv)
            value = json.loads(output.stdout.decode('utf-8-sig'))
            assert (output.returncode == 0) == ok and value['ok'] == ok, (argv, output.returncode, value)
            return value['result'] if ok else value

        def check(name):
            evidence['checks'].append(name)

        def rpc(request):
            output = run(runtime, 'rpc', data, '--stdin', input=json.dumps(request).encode())
            value = json.loads(output.stdout)
            assert output.returncode == 0 and value['ok'], value
            return value

        def create_terminal(workspace):
            session = invoke('terminal', 'create', '--workspace', workspace)['session_id']
            sessions.append((workspace, session))
            token = uuid.uuid4().hex[:12]
            # Full marker is absent from the input text: matching it requires
            # actual shell output rather than the echoed command line.
            marker = token + '鱼群' + '_search_' + token
            if os.name == 'nt':
                text = "[Console]::WriteLine(('padding-0123456789 ' * 40)); [Console]::WriteLine(('" + token + "' + '鱼群' + '_search_' + '" + token + "'))"
            else:
                text = r"printf '%s\n' '" + ('padding-0123456789 ' * 40) + "'; "
                text += r"printf '%s%s%s%s\n' '" + token + "' '鱼群' '_search_' '" + token + "'"
            invoke('terminal', 'send', '--workspace', workspace, '--terminal', session, '--text', text, '--enter')
            invoke('terminal', 'wait', '--workspace', workspace, '--terminal', session,
                   '--for', 'output', '--contains', marker, '--deadline-ms', '15000')
            return session, marker

        try:
            schema = invoke('schema')['command']
            terminal = next(row for row in schema['commands'] if row['name'] == 'terminal')
            assert any(row['name'] == 'search' for row in terminal['commands'])
            assert 'raw-terminal-recording' in invoke('guide')['guide']
            check('offline schema and bundled recording-search guide')
            daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
            deadline = time.monotonic() + 20
            while True:
                output = run(cli, '--data-dir', data, '--json', '--timeout-ms', '300', 'status')
                if output.returncode == 0:
                    break
                assert daemon.poll() is None and time.monotonic() < deadline, 'isolated daemon failed to start'
                time.sleep(0.15)
            workspaces = []
            for name in ('first project', 'second project'):
                folder = root / name
                folder.mkdir()
                project = invoke('project', 'add', folder)['project']['id']
                workspaces.append(invoke('workspace', 'list', '--project', project)['workspaces'][0]['id'])
            first, second = workspaces
            session, marker = create_terminal(first)
            create_terminal(first)
            foreign, foreign_marker = create_terminal(second)
            check('real shell output in three PTYs across two isolated workspaces; no native agents')

            found = invoke('terminal', 'search', marker, '--workspace', first)
            assert found['complete'] is True and found['listed_sessions'] == 2
            assert found['native_transcripts_indexed'] is False and found['representation'] == 'raw-terminal-recording'
            assert found['matches'] and all(row['session_id'] == session for row in found['matches'])
            for hit in found['matches']:
                page = invoke('terminal', 'read', '--workspace', first, '--terminal', session,
                              '--cursor', str(hit['byte_start']), '--limit', str(len(marker.encode())))
                assert base64.b64decode(page['data']) == marker.encode(), (hit, page)
            assert not invoke('terminal', 'search', foreign_marker, '--workspace', first)['matches']
            missing = invoke('terminal', 'search', foreign_marker, '--workspace', first, '--terminal', foreign, ok=False)
            assert missing['error']['code'] == 'not_found'
            check('UTF-8 literal hits locate original session and exact readable bytes; foreign session rejected')

            limited = invoke('terminal', 'search', marker, '--workspace', first, '--max-sessions', '1')
            assert limited['complete'] is False and limited['omitted_sessions'] == 1
            partial = invoke('terminal', 'search', marker, '--workspace', first, '--terminal', session, '--max-bytes', '256')
            assert partial['complete'] is False and partial['sessions'][0]['stop_reason'] == 'byte_limit'
            snapshot_end = partial['sessions'][0]['snapshot_end']
            hits = list(partial['matches'])
            for _ in range(200):
                if partial['complete']:
                    break
                cursor = partial['sessions'][0]['next_cursor']
                partial = invoke('terminal', 'search', marker, '--workspace', first, '--terminal', session,
                                 '--cursor', str(cursor), '--snapshot-end', str(snapshot_end), '--max-bytes', '256')
                assert partial['sessions'][0]['snapshot_end'] == snapshot_end
                assert partial['sessions'][0]['error'] is None
                hits.extend(partial['matches'])
            assert partial['complete'] is True, 'bounded continuation did not reach frozen end'
            offsets = [row['byte_start'] for row in hits]
            assert offsets and len(offsets) == len(set(offsets))
            assert set(offsets) == {row['byte_start'] for row in found['matches']}
            check('session limit and bounded byte continuation are explicit and preserve all hits without duplication')

            invoke('terminal', 'stop', '--workspace', first, '--terminal', session, '--confirm')
            invoke('terminal', 'wait', '--workspace', first, '--terminal', session, '--for', 'exit', '--deadline-ms', '10000')
            retained = invoke('terminal', 'search', marker, '--workspace', first, '--terminal', session)
            assert retained['complete'] is True and retained['matches']
            assert invoke('status')['pid'] == daemon.pid
            check('exited-session recording remains searchable; isolated daemon PID unchanged')
            evidence['passed'] = True
        except Exception as error:
            evidence.update(passed=False, error=str(error))
            raise
        finally:
            try:
                if daemon and daemon.poll() is None:
                    for workspace, session in sessions:
                        rpc({'cmd': 'workbench.terminal.stop', 'workspace_id': workspace, 'session_id': session, 'confirmed': True})
                    rpc({'cmd': 'service.stop'})
                    daemon.wait(timeout=15)
            finally:
                if daemon and daemon.poll() is None:
                    daemon.terminate()
                    daemon.wait(timeout=10)
                evidence['isolated_daemon_stopped'] = daemon is None or daemon.poll() is not None
                evidence['completed_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
                args.receipt.parent.mkdir(parents=True, exist_ok=True)
                args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
