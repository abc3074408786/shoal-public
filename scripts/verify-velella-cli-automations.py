"""Owned disposable profiles only; no model, credentials or production writes."""
import argparse
from contextlib import closing
import datetime
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--cli', type=Path, required=True)
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--legacy-runtime', type=Path, required=True)
    p.add_argument('--receipt', type=Path, required=True)
    args = p.parse_args()
    cli, runtime, legacy = (v.resolve(strict=True) for v in (args.cli, args.runtime, args.legacy_runtime))
    evidence = {'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'binaries': {str(v): hashlib.sha256(v.read_bytes()).hexdigest() for v in (cli, runtime, legacy)},
                'checks': [], 'production_mutated': False, 'real_model_requests': False}
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    daemon = None
    try:
        with tempfile.TemporaryDirectory(prefix='velella-automations-') as temp:
            root = Path(temp)
            data, repo, home = (root / name for name in ('profile', 'repo', 'home'))
            for d in (data, repo, home):
                d.mkdir()
            env = {**os.environ, 'USERPROFILE': str(home), 'HOME': str(home), 'CODEX_HOME': str(home / '.codex'),
                   'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1', 'GIT_CONFIG_NOSYSTEM': '1',
                   'GIT_CONFIG_GLOBAL': str(home / '.gitconfig')}

            def run(argv, **kwargs):
                return subprocess.run([str(v) for v in argv], capture_output=True, timeout=25, env=env, creationflags=flags, **kwargs)

            def invoke(*argv, ok=True):
                result = run([cli, '--data-dir', data, '--timeout-ms', '3000', '--json', *argv])
                reply = json.loads(result.stdout.decode('utf-8-sig'))
                assert (result.returncode == 0) == ok and reply['ok'] == ok, (argv, reply)
                return reply['result'] if ok else reply

            def start(binary):
                nonlocal daemon
                daemon = subprocess.Popen([str(binary), 'daemon', str(data)], env=env,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
                deadline = time.monotonic() + 15
                while True:
                    status = run([cli, '--data-dir', data, '--timeout-ms', '300', '--json', 'status'])
                    if status.returncode == 0:
                        assert json.loads(status.stdout)['result']['pid'] == daemon.pid
                        return
                    assert daemon.poll() is None and time.monotonic() < deadline, 'isolated startup failed'
                    time.sleep(0.1)

            def stop():
                nonlocal daemon
                if daemon is not None and daemon.poll() is None:
                    result = run([runtime, 'rpc', data, '--stdin'], input=b'{"cmd":"service.stop"}')
                    assert result.returncode == 0, result.stderr
                    daemon.wait(timeout=10)
                daemon = None

            def check(name):
                evidence['checks'].append(name)

            try:
                for cmd in (['git', 'init', '--template=', '-b', 'main', repo],
                            ['git', '-C', repo, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--allow-empty', '-m', 'fixture']):
                    result = run(cmd)
                    assert result.returncode == 0, result.stderr
                start(runtime)
                project = invoke('project', 'add', repo)['project']['id']
                spec = root / 'spec.yaml'
                spec.write_text('name: archive fixture\nagent: codex\ninstruction: never execute this isolated fixture\n', encoding='utf-8')
                a = invoke('automation', 'create', '--project', project, '--file', spec, '--start-at', '2099-01-01T00:00:00Z', '--confirm')['automation']
                aid = a['id']
                assert a['enabled'] is False
                assert invoke('automation', 'runs', '--limit', '40')['runs'] == []
                check('create disabled schedule and empty paginated history')
                stop()
                with closing(sqlite3.connect(data / 'capabilities.db')) as db, db:
                    db.executemany('INSERT INTO runs(id,automation_id,task_id,started_at,state) VALUES(?,?,?,?,?)',
                        [(f'fixture-{i}', aid if i % 2 == 0 else 'other', f'task-{i}', i // 3, 'delivered') for i in range(605)])
                start(runtime)
                first = invoke('automation', 'runs', '--automation', aid, '--limit', '37')
                cursor = first['next_cursor']
                stop()
                # Membership remains fixed even across a restart and a backdated insertion.
                with closing(sqlite3.connect(data / 'capabilities.db')) as db, db:
                    db.execute('INSERT INTO runs(id,automation_id,task_id,started_at,state) VALUES(?,?,?,?,?)',
                               ('new', aid, 'new-task', 0, 'delivered'))
                start(runtime)
                seen = [r['id'] for r in first['runs']]
                page = first
                while page['has_more']:
                    page = invoke('automation', 'runs', '--automation', aid, '--limit', '37', '--cursor', page['next_cursor'])
                    seen += [r['id'] for r in page['runs']]
                assert seen == [f'fixture-{i}' for i in reversed(range(605)) if i % 2 == 0]
                assert page['next_cursor'] is None
                check('303 filtered records beyond 200, timestamp ties, restart and backdated insertion')
                invoke('automation', 'runs', '--automation', 'other', '--cursor', cursor, ok=False)
                invoke('automation', 'runs', '--cursor', 'invalid', ok=False)
                invoke('automation', 'runs', '--limit', '0', ok=False)
                invoke('automation', 'runs', '--limit', '501', ok=False)
                check('cross-filter cursor, malformed cursor and invalid page sizes rejected')
                invoke('automation', 'remove', aid, ok=False)
                removed = invoke('automation', 'remove', aid, '--confirm')
                assert removed['archived'] and removed['history_retained']
                assert invoke('automation', 'list')['automations'] == []
                assert invoke('automation', 'list', '--archived')['automations'][0]['id'] == aid
                assert invoke('automation', 'show', aid, '--archived')['automation']['enabled'] is False
                invoke('automation', 'run', aid, '--confirm', ok=False)
                assert len(invoke('automation', 'runs', '--automation', aid, '--limit', '500')['runs']) == 304
                check('reversible removal, archived discovery, manual run rejection and retained history')
                stop()
                start(legacy)
                assert invoke('automation', 'list')['automations'] == []
                old = invoke('automation', 'runs')
                assert old['history_scope'] == 'legacy_global_latest_200' and len(old['runs']) == 200
                for argv in [('automation', 'runs', '--limit', '100'), ('automation', 'restore', aid, '--confirm'),
                             ('automation', 'remove', aid, '--confirm'), ('automation', 'list', '--archived')]:
                    assert '需要升级 runtime' in json.dumps(invoke(*argv, ok=False), ensure_ascii=False)
                assert invoke('task', 'list')['tasks'] == []
                check('old runtime rollback keeps removed schedules hidden and rejects new operations explicitly')
                stop()
                start(runtime)
                restored = invoke('automation', 'restore', aid, '--confirm')
                assert restored['archived'] is False and restored['automation']['enabled'] is False
                assert invoke('automation', 'list', '--archived')['automations'] == []
                assert invoke('automation', 'show', aid)['automation']['enabled'] is False
                assert len(invoke('automation', 'runs', '--automation', aid, '--limit', '500')['runs']) == 304
                assert invoke('task', 'list')['tasks'] == []
                check('restoration after old-runtime round trip stays disabled, zero tasks/model launches')
                stop()
            finally:
                if daemon is not None and daemon.poll() is None:
                    try:
                        stop()
                    finally:
                        if daemon is not None and daemon.poll() is None:
                            daemon.terminate()  # only the process created by this isolated fixture
                            daemon.wait(timeout=10)
        evidence['passed'] = True
    except Exception as error:
        evidence['passed'] = False
        evidence['error'] = repr(error)
        raise
    finally:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
