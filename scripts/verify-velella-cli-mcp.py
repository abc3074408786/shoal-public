"""Native Codex reads CLI-managed fixture MCP config; no connections or models."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--codex', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    cli, codex = args.cli.resolve(strict=True), args.codex.resolve(strict=True)
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    evidence = {'cli': str(cli), 'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'codex': str(codex), 'codex_sha256': hashlib.sha256(codex.read_bytes()).hexdigest(),
                'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'checks': [],
                'live_config_mutated': False, 'real_model_requests': False, 'mcp_servers_started': False}
    with tempfile.TemporaryDirectory(prefix='velella-cli-native-mcp-') as directory:
        root = Path(directory)
        home, codex_home = root / 'fixture user', root / 'custom Codex home'
        home.mkdir()
        codex_home.mkdir()
        env = {key: value for key, value in os.environ.items() if key.upper() in {
            'PATH', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT', 'TEMP', 'TMP'}}
        env.update(USERPROFILE=str(home), HOME=str(home), CODEX_HOME=str(codex_home),
                   APPDATA=str(home / 'roaming'), LOCALAPPDATA=str(home / 'local'),
                   VELELLA_DATA_DIR=str(root / 'offline profile'))
        config = codex_home / 'config.toml'
        original = b'# native fixture comment\r\nmodel="fixture-not-used"\r\n[mcp_servers.keep]\r\ncommand="velella-fixture-never-existing"\r\nargs=["fixture-private-value"]\r\nenabled=false\r\n'
        config.write_bytes(original)
        spec = root / 'server.json'
        spec_bytes = b'{"command":"velella-fixture-never-selected","args":["fixture-private-value"],"env_vars":["VELELLA_FIXTURE_TOKEN"],"enabled":true}'
        spec.write_bytes(spec_bytes)

        def run(*argv, environment=env, **kw):
            return subprocess.run([str(v) for v in argv], capture_output=True, timeout=25,
                                  env=environment, creationflags=flags, **kw)

        def invoke(*argv, ok=True, environment=env):
            output = run(cli, '--json', *argv, environment=environment)
            value = json.loads(output.stdout.decode('utf-8-sig'))
            assert (output.returncode == 0) == ok and value['ok'] == ok, (argv, output.returncode, value)
            assert 'fixture-private-value' not in output.stdout.decode('utf-8-sig')
            return value['result'] if ok else value

        def native(environment=env):
            output = run(codex, '-C', root, 'mcp', 'list', '--json', environment=environment)
            assert output.returncode == 0, f'native MCP list failed: exit {output.returncode}; output omitted'
            return {row['name']: row for row in json.loads(output.stdout)}

        def check(label):
            evidence['checks'].append(label)

        def private(path):
            if os.name != 'nt':
                assert stat.S_IMODE(path.stat().st_mode) == 0o600
            else:
                acl_env = {**env, 'VELELLA_FIXTURE_ACL_PATH': str(path)}
                command = "$mcpAcl=Get-Acl -LiteralPath $env:VELELLA_FIXTURE_ACL_PATH; [bool]($mcpAcl.AreAccessRulesProtected -and $mcpAcl.Access.Count -eq 1 -and $mcpAcl.Access[0].IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -eq 'S-1-3-4')"
                output = run('powershell.exe', '-NoProfile', '-NonInteractive', '-Command', command, environment=acl_env)
                assert output.returncode == 0 and output.stdout.strip() == b'True', 'fixture config/backup ACL is not owner-only'

        try:
            version = run(codex, '--version')
            assert version.returncode == 0
            evidence['codex_version'] = version.stdout.decode().strip()
            inspected = invoke('mcp', 'codex', 'inspect')
            assert inspected['config_path'] == str(config)
            assert inspected['servers'] == [{'enabled': False, 'name': 'keep', 'transport': 'stdio'}]
            preview = invoke('mcp', 'codex', 'preview', 'sample', '--file', spec)
            assert preview['config_digest'] == inspected['config_digest'] and config.read_bytes() == original
            assert not (codex_home / 'velella-mcp-backups').exists()
            check('offline inspect and reviewed preview are redacted and do not create backups or change config')

            install = ('mcp', 'codex', 'install', 'sample', '--file', spec,
                       '--config-digest', preview['config_digest'], '--spec-digest', preview['spec_digest'], '--confirm')
            stale = invoke(*install[:-3], '--spec-digest', 'sha256:stale', '--confirm', ok=False)
            assert 'JSON digest is stale' in stale['error']['message']
            applied = invoke(*install)
            installed_bytes = config.read_bytes()
            assert applied['config_digest'] == preview['proposed_digest']
            assert b'# native fixture comment\r\n' in installed_bytes
            assert b'fixture-private-value' in installed_bytes
            servers = native()
            assert set(servers) == {'keep', 'sample'}
            assert servers['sample']['enabled'] is True and servers['keep']['enabled'] is False
            assert servers['sample']['transport']['command'] == 'velella-fixture-never-selected'
            private(config)
            private(Path(applied['backup_path']) / 'before.bin')
            check('CLI installation visible to installed Codex offline loader; unrelated config/CRLF retained; config and raw backup owner-only')

            invoke(*install, ok=False)
            conflict = invoke('mcp', 'codex', 'preview', 'sample', '--file', spec, ok=False)
            assert 'already exists' in conflict['error']['message']
            assert config.read_bytes() == installed_bytes
            remote = invoke('--ssh-profile', 'missing', '--server-id', str(uuid.uuid4()), '--remote-data-dir', '/fixture',
                            'mcp', 'codex', 'inspect', ok=False)
            assert 'local-only' in remote['error']['message']
            check('stale/full-file digest, duplicate-name and remote-selector guards reject without mutation')

            removed = invoke('mcp', 'codex', 'remove', 'sample', '--config-digest', applied['config_digest'], '--confirm')
            assert set(native()) == {'keep'}
            invoke('mcp', 'codex', 'restore', removed['backup']['id'], '--config-digest', removed['config_digest'], '--confirm')
            assert config.read_bytes() == installed_bytes and set(native()) == {'keep', 'sample'}
            invoke('mcp', 'codex', 'restore', applied['backup']['id'], '--config-digest', applied['config_digest'], '--confirm')
            assert config.read_bytes() == original and set(native()) == {'keep'}
            assert len(invoke('mcp', 'codex', 'backups')['backups']) == 4
            check('remove and whole-file rollback are visible to native loader; original bytes restored exactly, audit retained')

            empty_home = root / 'initially absent config'
            empty_home.mkdir()
            empty_env = {**env, 'CODEX_HOME': str(empty_home)}
            empty_preview = invoke('mcp', 'codex', 'preview', 'sample', '--file', spec, environment=empty_env)
            assert empty_preview['config_digest'] == 'absent'
            created = invoke('mcp', 'codex', 'install', 'sample', '--file', spec, '--config-digest', 'absent',
                             '--spec-digest', empty_preview['spec_digest'], '--confirm', environment=empty_env)
            assert set(native(empty_env)) == {'sample'}
            invoke('mcp', 'codex', 'restore', created['backup']['id'], '--config-digest', created['config_digest'], '--confirm', environment=empty_env)
            assert not (empty_home / 'config.toml').exists()
            assert invoke('mcp', 'codex', 'inspect', environment=empty_env)['config_digest'] == 'absent'
            check('CODEX_HOME targets are isolated; restoring an initially missing config restores absence')

            preview = invoke('mcp', 'codex', 'preview', 'sample', '--file', spec)
            applied = invoke('mcp', 'codex', 'install', 'sample', '--file', spec, '--config-digest', preview['config_digest'],
                             '--spec-digest', preview['spec_digest'], '--confirm')
            (Path(applied['backup_path']) / 'before.bin').write_bytes(b'tampered')
            before = config.read_bytes()
            invoke('mcp', 'codex', 'restore', applied['backup']['id'], '--config-digest', applied['config_digest'], '--confirm', ok=False)
            assert config.read_bytes() == before
            config.write_bytes(before + b'\n# later external edit\n')
            invoke('mcp', 'codex', 'restore', applied['backup']['id'], '--config-digest', applied['config_digest'], '--confirm', ok=False)
            check('backup tampering and newer native edits reject restore without overwriting current bytes')
            evidence['passed'] = True
        except Exception as error:
            evidence.update(passed=False, error=str(error))
            raise
        finally:
            evidence['completed_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(evidence, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
