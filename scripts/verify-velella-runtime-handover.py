"""Real old/new daemon round trip, exclusively in an owned temporary profile."""
import argparse
import base64
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import tempfile
import time
import uuid
import psutil


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--old-runtime', required=True, type=Path)
    parser.add_argument('--new-runtime', required=True, type=Path)
    parser.add_argument('--cli', required=True, type=Path)
    parser.add_argument('--receipt', required=True, type=Path)
    args = parser.parse_args()
    old, new, cli = (p.resolve(strict=True) for p in (args.old_runtime, args.new_runtime, args.cli))
    evidence = {'checks': [], 'production_mutated': False, 'model_requests': False,
                'binaries': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in (old, new, cli)}}
    daemon = None
    flags = subprocess.CREATE_NO_WINDOW
    try:
        with tempfile.TemporaryDirectory(prefix='velella-handover-') as temp:
            root = Path(temp)
            data, repo, home = (root / name for name in ('profile', 'project', 'home'))
            for path in (data, repo, home):
                path.mkdir()
            env = {**os.environ, 'HOME': str(home), 'USERPROFILE': str(home),
                   'CODEX_HOME': str(home / '.codex'), 'VELELLA_DATA_DIR': str(data),
                   'ORCA_BACKGROUND_LAUNCH': '1'}
            live = []
            starts = 0

            def invoke(*argv):
                result = subprocess.run([str(cli), '--data-dir', str(data), '--json', *map(str, argv)],
                    env=env, capture_output=True, timeout=20, creationflags=flags)
                value = json.loads(result.stdout.decode('utf-8-sig'))
                assert result.returncode == 0 and value['ok'], (argv, value)
                return value['result']

            def start(binary):
                nonlocal daemon, starts
                starts += 1
                log_path = args.receipt.parent / f'{args.receipt.stem}-daemon-{starts}.log'
                with log_path.open('wb') as log:
                    daemon = subprocess.Popen([str(binary), 'daemon', str(data)], env=env,
                        stdout=log, stderr=log, creationflags=flags)
                until = time.monotonic() + 15
                while True:
                    try:
                        assert invoke('status')['pid'] == daemon.pid
                        return
                    except (AssertionError, ValueError):
                        assert daemon.poll() is None and time.monotonic() < until, log_path.read_text(errors='replace')
                        time.sleep(0.15)

            def stop_daemon_only():
                nonlocal daemon
                assert daemon is not None
                # Only this Popen-created isolated daemon; never a process found by name.
                daemon.terminate()
                daemon.wait(timeout=10)
                daemon = None

            def identity(pid):
                p = psutil.Process(pid)
                return {'pid': pid, 'created': p.create_time(), 'exe': p.exe()}

            def record(sid):
                return invoke('terminal', 'show', '--workspace', workspace, '--terminal', sid)['session']

            def exercise(sid, assign=False):
                status = record(sid)['status']
                marker = 'HANDOVER_' + uuid.uuid4().hex
                command = ("$handoverValue='retained'; " if assign else '')
                command += "Write-Output ((-join ('%s'.ToCharArray()[%d..0]))+'_'+$handoverValue)" % (marker[::-1], len(marker)-1)
                invoke('terminal', 'send', '--workspace', workspace, '--terminal', sid,
                       '--text', command, '--enter', '--expected-sequence', status['input_sequence'])
                collected = bytearray()
                cursor = 0
                until = time.monotonic() + 15
                while (marker + '_retained').encode() not in collected:
                    page = invoke('terminal', 'read', '--workspace', workspace, '--terminal', sid,
                                  '--cursor', cursor, '--limit', 65536)
                    collected.extend(base64.b64decode(page['data']))
                    cursor = page['next_cursor']
                    assert time.monotonic() < until, 'shell state/command execution not preserved'
                    if not page['has_more']:
                        time.sleep(0.1)

            def capture(sid):
                row = record(sid)
                host = psutil.Process(row['status']['pid'])
                processes = [identity(p.pid) for p in [host, *host.children(recursive=True)]]
                path = repo / '.velella' / 'sessions' / sid / 'terminal.bin'
                content = path.read_bytes()
                return {'session': sid, 'processes': processes, 'path': path, 'size': len(content),
                        'sha': hashlib.sha256(content).hexdigest()}

            def unchanged(items):
                for item in items:
                    assert record(item['session'])['status']['pid'] == item['processes'][0]['pid']
                    for p in item['processes']:
                        assert identity(p['pid']) == p
                    with item['path'].open('rb') as stream:
                        assert hashlib.sha256(stream.read(item['size'])).hexdigest() == item['sha']

            def backup_databases():
                backups = []
                for index, database in enumerate([data / 'workbench.db', data / 'capabilities.db', repo / '.velella' / 'state.db']):
                    backup = root / f'before-{index}.sqlite'
                    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as source, closing(sqlite3.connect(backup)) as target:
                        source.backup(target)
                        assert target.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
                        version = target.execute('PRAGMA user_version').fetchone()[0]
                    backups.append((database, backup, version))
                return backups

            def restore_databases(backups):
                assert daemon is None, 'restore only while the isolated daemon is stopped'
                for database, backup, version in backups:
                    # SQLite backup handles WAL consistently; no manual WAL deletion or schema downgrade.
                    with closing(sqlite3.connect(backup.as_uri() + '?mode=ro', uri=True)) as source, closing(sqlite3.connect(database)) as target:
                        source.backup(target)
                        assert target.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
                        assert target.execute('PRAGMA user_version').fetchone()[0] == version

            try:
                start(old)
                project = invoke('project', 'add', repo)['project']['id']
                workspace = invoke('workspace', 'list', '--project', project)['workspaces'][0]['id']
                first = invoke('terminal', 'create', '--workspace', workspace)['session_id']
                live.append(first)
                exercise(first, assign=True)
                protected = [capture(first)]
                backups = backup_databases()
                evidence['before_workbench_schema'] = backups[0][2]
                evidence['checks'].append('production-version runtime creates real isolated shell with retained variable')
                stop_daemon_only()
                for p in protected[0]['processes']:
                    assert identity(p['pid']) == p
                start(new)
                unchanged(protected)
                exercise(first)
                assert invoke('automation', 'runs', '--limit', '1')['runs'] == []
                with closing(sqlite3.connect((data / 'workbench.db').as_uri() + '?mode=ro', uri=True)) as db:
                    evidence['candidate_workbench_schema'] = db.execute('PRAGMA user_version').fetchone()[0]
                evidence['checks'].append('candidate reconnects same old host/children, history prefix, variable and working input')
                second = invoke('terminal', 'create', '--workspace', workspace)['session_id']
                live.append(second)
                exercise(second, assign=True)
                protected = [capture(first), capture(second)]
                evidence['checks'].append('candidate creates a second functioning host alongside retained old host')
                # Production rollout must stay in a maintenance window with no new user work.
                # This owned post-upgrade probe is explicitly stopped before rollback; only
                # pre-upgrade sessions are covered by snapshot restoration.
                invoke('terminal', 'stop', '--workspace', workspace, '--terminal', second, '--confirm')
                invoke('terminal', 'wait', '--workspace', workspace, '--terminal', second, '--for', 'exit', '--deadline-ms', '5000')
                protected = protected[:1]
                stop_daemon_only()
                restore_databases(backups)
                start(old)
                unchanged(protected)
                exercise(first)
                assert [s['id'] for s in invoke('terminal', 'list', '--workspace', workspace)['sessions']] == [first]
                assert invoke('automation', 'runs')['pagination_supported'] is False
                evidence['checks'].append('maintenance rollback restores pre-upgrade databases and reconnects original host with live shell state and intact history')
                stop_daemon_only()
                start(new)
                unchanged(protected)
                exercise(first)
                assert invoke('task', 'list')['tasks'] == []
                evidence['checks'].append('second forward switch retains original host; zero model tasks')
            finally:
                if live and (daemon is None or daemon.poll() is not None):
                    start(new)
                if daemon is not None and daemon.poll() is None:
                    result = subprocess.run([str(new), 'rpc', str(data), '--stdin'],
                        input=b'{"cmd":"service.stop"}', capture_output=True, timeout=15, env=env, creationflags=flags)
                    assert result.returncode == 0, result.stderr
                    daemon.wait(timeout=10)
                    daemon = None
                # Wait for owned terminal cleanup before TemporaryDirectory removes recordings.
                until = time.monotonic() + 10
                for item in locals().get('protected', []):
                    for p in item['processes']:
                        while psutil.pid_exists(p['pid']) and psutil.Process(p['pid']).create_time() == p['created']:
                            assert time.monotonic() < until, 'owned host cleanup deadline'
                            time.sleep(0.1)
        evidence['passed'] = True
    except Exception as error:
        evidence.update(passed=False, error=repr(error))
        raise
    finally:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
