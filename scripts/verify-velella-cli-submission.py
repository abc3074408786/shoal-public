"""Real isolated daemon/PTY + inert native fixture. Never calls a model."""
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
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--legacy', action='store_true', help='Check refusal/compatibility against a frozen pre-tracking runtime')
    args = parser.parse_args()
    cli, runtime, fixture = (p.resolve(strict=True) for p in (args.cli, args.runtime, args.fixture))
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    evidence = {'at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'runtime_sha256': hashlib.sha256(runtime.read_bytes()).hexdigest(),
                'real_model_requests': False, 'production_mutated': False, 'checks': []}

    with tempfile.TemporaryDirectory(prefix='vl-submission-integration-') as temp:
        root = Path(temp)
        data, work, native_home, bin_dir = (root / name for name in ('data', 'work', 'home', 'bin'))
        for path in (data, work, native_home, bin_dir):
            path.mkdir()
        native = bin_dir / ('codex.exe' if os.name == 'nt' else 'codex')
        native.write_bytes(fixture.read_bytes())
        native.chmod(0o700)
        log = root / 'fixture-inputs.txt'
        env = {**os.environ, 'USERPROFILE': str(native_home), 'HOME': str(native_home),
               'CODEX_HOME': str(native_home / '.codex'), 'CLAUDE_CONFIG_DIR': str(native_home / '.claude'),
               'VELELLA_DATA_DIR': str(data), 'ORCA_BACKGROUND_LAUNCH': '1',
               'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
               'VL_TEST_RUNTIME': str(runtime), 'VL_TEST_EVENTS': str(log)}

        def invoke(*arguments, ok=True):
            run = subprocess.run([str(cli), '--data-dir', str(data), '--json', *map(str, arguments)],
                                 env=env, capture_output=True, timeout=20, creationflags=flags)
            reply = json.loads(run.stdout)
            assert (run.returncode == 0) == ok and reply['ok'] == ok, (arguments, reply)
            return reply['result'] if ok else reply

        def hook(endpoint, event):
            run = subprocess.run([str(runtime), 'hook', endpoint], input=json.dumps(event).encode(),
                                 env=env, capture_output=True, timeout=5, creationflags=flags)
            assert run.returncode == 0, run.stderr.decode(errors='replace')

        daemon = subprocess.Popen([str(runtime), 'daemon', str(data)], env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags)
        sessions = []
        try:
            deadline = time.monotonic() + 15
            while True:
                result = subprocess.run([str(cli), '--data-dir', str(data), '--timeout-ms', '100', '--json', 'status'],
                                        env=env, capture_output=True, creationflags=flags)
                if result.returncode == 0:
                    break
                assert daemon.poll() is None and time.monotonic() < deadline
                time.sleep(.1)
            project = invoke('project', 'add', work)['project']['id']
            workspace = invoke('workspace', 'list', '--project', project)['workspaces'][0]['id']

            if args.legacy:
                rejected = invoke('terminal', 'create', '--workspace', workspace, '--launch', 'codex', '--track-submission', ok=False)
                assert 'does not support submission tracking' in rejected['error']['message']
                assert invoke('terminal', 'list', '--workspace', workspace)['sessions'] == []
                sid = invoke('terminal', 'create', '--workspace', workspace, '--launch', 'codex')['session_id']
                sessions.append(sid)
                rejected = invoke('terminal', 'send', '--workspace', workspace, '--terminal', sid, '--text', 'submission-fixture-prompt', '--enter', '--wait-submit', 1, ok=False)
                assert rejected['error']['code'] == 'submission_not_supported'
                assert not log.exists()
                evidence['checks'].append('legacy runtime refuses tracked creation without a session effect')
                evidence['checks'].append('legacy native session refuses observation before any prompt input')
                return finish(args.receipt, evidence)

            def create():
                sid = invoke('terminal', 'create', '--workspace', workspace, '--launch', 'codex', '--track-submission')['session_id']
                sessions.append(sid)
                target = ['--workspace', workspace, '--terminal', sid]
                deadline = time.monotonic() + 10
                while True:
                    status = invoke('terminal', 'show', *target)['session']['status']
                    if status and status['state'] == 'waiting_input' and status.get('native_id'):
                        assert status['submission_protocol'] == 1
                        return sid, target
                    assert time.monotonic() < deadline, status
                    time.sleep(.1)

            sid, target = create()
            request = str(uuid.uuid4())
            sent = invoke('terminal', 'send', *target, '--text', 'submission-fixture-prompt', '--enter', '--request-id', request, '--wait-submit', 3)
            assert sent['turn_started_confirmed'] is True, sent
            assert sent['submission']['event'] in ('PreToolUse', 'Stop')
            assert 'submission-fixture-prompt' not in json.dumps(sent)
            repeat = invoke('terminal', 'send', *target, '--text', 'submission-fixture-prompt', '--enter', '--request-id', request, '--wait-submit', 3)
            assert repeat['turn_started_confirmed'] is True and len(log.read_text().splitlines()) == 1
            proof = invoke('terminal', 'receipt', *target, '--request-id', request)
            assert proof['turn_started_confirmed'] is True
            evidence['checks'].append('real daemon/PTY request-bound proof, replay and content-free receipt')

            bad_sid, bad_target = create()
            wrong = invoke('terminal', 'receipt', *bad_target, '--request-id', request, ok=False)
            assert wrong['error']['code'] == 'request_rejected'
            endpoint = json.loads((work / '.velella' / 'sessions' / bad_sid / 'launch.json').read_text())['endpoint']
            hook(endpoint, {'hook_event_name': 'PermissionRequest', 'session_id': '00000000-0000-0000-0000-000000000007'})
            blocked = invoke('terminal', 'send', *bad_target, '--text', 'submission-fixture-prompt', '--enter', '--wait-submit', 1, ok=False)
            assert blocked['error']['code'] == 'request_rejected' and len(log.read_text().splitlines()) == 1
            evidence['checks'].append('wrong-session reads and permission-blocked send have no input effects')

            _, silent = create()
            request = str(uuid.uuid4())
            timed = invoke('terminal', 'send', *silent, '--text', 'submission-fixture-silent', '--enter', '--request-id', request, '--wait-submit', 1)
            assert timed['turn_started_confirmed'] is False and timed['observation_timed_out'] is True
            timed_again = invoke('terminal', 'send', *silent, '--text', 'submission-fixture-silent', '--enter', '--request-id', request, '--wait-submit', 1)
            assert timed_again['turn_started_confirmed'] is False and len(log.read_text().splitlines()) == 2
            assert invoke('terminal', 'receipt', *silent, '--request-id', request)['turn_started_confirmed'] is False
            busy = invoke('terminal', 'send', *silent, '--text', 'submission-fixture-prompt', '--enter', '--wait-submit', 1, ok=False)
            assert busy['error']['code'] == 'request_rejected' and len(log.read_text().splitlines()) == 2
            evidence['checks'].append('timeout observation and replay never resend; busy input remains blocked')

            _, mismatch = create()
            mismatched = invoke('terminal', 'send', *mismatch, '--text', 'submission-fixture-mismatch', '--enter', '--wait-submit', 1)
            assert mismatched['turn_started_confirmed'] is False
            assert mismatched['submission']['prompt_observed'] is False
            evidence['checks'].append('unrelated prompt and later tool/Stop events cannot confirm this request')

            invoke('terminal', 'stop', *target, '--confirm')
            assert invoke('terminal', 'receipt', *target, '--request-id', sent['request_id'])['turn_started_confirmed'] is True
            sessions.remove(sid)
            evidence['checks'].append('execution proof remains readable after host stop')
        finally:
            for sid in sessions:
                try:
                    invoke('terminal', 'stop', '--workspace', workspace, '--terminal', sid, '--confirm')
                except Exception:
                    pass
            daemon.terminate()
            daemon.wait(timeout=10)

    finish(args.receipt, evidence)


def finish(path, evidence):
    path.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'passed': len(evidence['checks']), 'real_model_requests': False}))


if __name__ == '__main__':
    main()
