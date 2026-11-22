"""Local skill lifecycle + real native discovery in a fresh home and daemon.

No real credentials, model calls, scripts, live skill stores or production writes.
"""
import argparse
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
                'live_profile_mutated': False, 'real_model_requests': False, 'scripts_executed': False}
    daemon = None
    with tempfile.TemporaryDirectory(prefix='velella-cli-skills-') as directory:
        root = Path(directory)
        home, data, source = root / 'fixture home', root / 'profile', root / '源 package'
        for folder in (home, data, source):
            folder.mkdir()
        env = {**os.environ, 'USERPROFILE': str(home), 'HOME': str(home),
               'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1'}
        source.joinpath('SKILL.md').write_bytes(b'---\r\nname: sample\r\ndescription: Fixture only\r\n---\r\nNever execute.\r\n')
        source.joinpath('assets').mkdir()
        source.joinpath('assets/data.bin').write_bytes(bytes([0, 255, 128, 13, 10]))

        def run(*argv, **kw):
            return subprocess.run([str(v) for v in argv], capture_output=True, timeout=20,
                                  env=env, creationflags=flags, **kw)

        def invoke(*argv, ok=True):
            output = run(cli, '--data-dir', data, '--json', '--timeout-ms', '1500', *argv)
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

        def discover():
            return invoke('skills', 'list')['skills']

        try:
            schema = invoke('schema')['command']
            skills = next(row for row in schema['commands'] if row['name'] == 'skills')
            assert {row['name'] for row in skills['commands']} >= {'list', 'show', 'preview', 'inspect', 'install', 'remove', 'backups', 'restore'}
            digest = invoke('skills', 'preview', source)['digest']
            assert invoke('skills', 'backups')['backups'] == []
            assert not home.joinpath('.agents').exists(), 'preview/list must be read-only'
            invoke('skills', 'install', source, '--name', 'sample', '--digest', 'sha256:stale', '--confirm', ok=False)
            install = ('skills', 'install', source, '--name', 'sample', '--digest', digest, '--confirm')
            invoke(*install)
            installed = home / '.agents/skills/sample'
            assert installed.joinpath('assets/data.bin').read_bytes() == source.joinpath('assets/data.bin').read_bytes()
            assert installed.joinpath('SKILL.md').read_bytes() == source.joinpath('SKILL.md').read_bytes()
            assert invoke('skills', 'inspect', 'sample')['digest'] == digest
            invoke(*install, ok=False)
            check('offline discovery, preview, stale digest rejection, exact byte installation, inspect and collision guard')

            remote = invoke('--ssh-profile', 'missing', '--server-id', str(uuid.uuid4()), '--remote-data-dir', '/fixture',
                            'skills', 'remove', 'sample', '--digest', digest, '--confirm', ok=False)
            assert 'local-only' in remote['error']['message']
            assert installed.is_dir()
            check('remote selector rejected before any SSH profile lookup or local mutation')

            daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
            deadline = time.monotonic() + 20
            while True:
                output = run(cli, '--data-dir', data, '--json', '--timeout-ms', '300', 'status')
                if output.returncode == 0:
                    break
                assert daemon.poll() is None and time.monotonic() < deadline, 'isolated daemon failed to start'
                time.sleep(0.15)
            assert invoke('status')['pid'] == daemon.pid
            row = next(row for row in discover() if row['name'] == 'sample')
            assert row['digest'] == digest
            assert invoke('skills', 'show', 'sample')['content_is_untrusted'] is True
            check('real daemon native skill discovery and read observe installed resources without a restart')

            invoke('skills', 'remove', 'sample', '--digest', 'sha256:stale', '--confirm', ok=False)
            removed = invoke('skills', 'remove', 'sample', '--digest', digest, '--confirm')
            backup_id = removed['backup']['id']
            assert not installed.exists()
            assert not any(row['name'] == 'sample' for row in discover())
            backups = invoke('skills', 'backups')['backups']
            assert backups[0]['id'] == backup_id and backups[0]['package_present'] is True
            invoke('skills', 'restore', backup_id, '--digest', 'sha256:stale', '--confirm', ok=False)
            restore = ('skills', 'restore', backup_id, '--digest', digest, '--confirm')
            invoke(*restore)
            assert next(row for row in discover() if row['name'] == 'sample')['digest'] == digest
            assert invoke('status')['pid'] == daemon.pid
            invoke(*restore, ok=False)
            assert invoke('skills', 'backups')['backups'][0]['package_present'] is False
            check('reviewed removal, native disappearance, backup receipt, exact restore and native rediscovery; daemon PID unchanged')

            invoke('skills', 'install', source, '--name', 'sample', '--target', 'codex', '--digest', digest, '--confirm')
            assert len([row for row in discover() if row['name'] == 'sample']) == 2
            invoke('skills', 'show', 'sample', ok=False)
            assert invoke('skills', 'inspect', 'sample', '--target', 'codex')['digest'] == digest
            codex_backup = invoke('skills', 'remove', 'sample', '--target', 'codex', '--digest', digest, '--confirm')
            assert installed.is_dir()
            invoke('skills', 'restore', codex_backup['backup']['id'], '--target', 'codex', '--digest', digest, '--confirm')
            check('independent agents/codex targets, duplicate-name guard, scoped backup and restoration')

            removed = invoke('skills', 'remove', 'sample', '--digest', digest, '--confirm')
            backup = Path(removed['backup_path']) / 'package'
            backup.joinpath('SKILL.md').write_bytes(b'tampered fixture')
            invoke('skills', 'restore', removed['backup']['id'], '--digest', digest, '--confirm', ok=False)
            assert not installed.exists() and backup.exists()
            check('backup tampering rejected without publishing or discarding the retained package')
            evidence['passed'] = True
        except Exception as error:
            evidence.update(passed=False, error=str(error))
            raise
        finally:
            try:
                if daemon and daemon.poll() is None:
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
