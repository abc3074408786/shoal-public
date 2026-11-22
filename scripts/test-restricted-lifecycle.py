#!/usr/bin/env python3
"""Exercise real host/PTy cleanup with fixed, local-only Restricted processes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import uuid

FIXTURE = r'''
import json, os, pathlib, signal, time
root = pathlib.Path.cwd()
signal.signal(signal.SIGHUP, signal.SIG_IGN)
child = os.fork()
if child == 0:
    grandchild = os.fork()
    if grandchild == 0:
        (root / 'grandchild.pid').write_text(str(os.getpid()))
    else:
        (root / 'child.pid').write_text(str(os.getpid()))
    while True:
        time.sleep(1)
(root / 'leader.pid').write_text(str(os.getpid()))
while not (root / 'exit-now').exists():
    time.sleep(.02)
os._exit(0)
'''


def rpc(endpoint, command):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(3)
        stream.connect(str(endpoint))
        stream.sendall(json.dumps(command).encode() + b'\n')
        data = bytearray()
        while b'\n' not in data:
            chunk = stream.recv(8192)
            if not chunk:
                raise RuntimeError('host disconnected before response')
            data.extend(chunk)
            if len(data) > 65536:
                raise RuntimeError('oversized host response')
        result = json.loads(bytes(data).split(b'\n', 1)[0])
        if result.get('ok') is not True:
            raise RuntimeError(f'host command failed: {result}')
        return result


def process_state(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(') ', 1)[1].split()
        return {'state': fields[0], 'ppid': int(fields[1]), 'pgid': int(fields[2]), 'start': int(fields[19])}
    except FileNotFoundError:
        return None


def alive(identity):
    current = process_state(identity['pid'])
    return (current is not None and current['start'] == identity['start']
            and current['state'] not in ('Z', 'X'))


def wait_until(condition, seconds, description):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(.025)
    raise RuntimeError(f'timed out: {description}')


def scenario(runtime, root, cancel):
    base = root / ('cancel' if cancel else 'natural-exit')
    base.mkdir(mode=0o700)
    work, home, session = (base / name for name in ('work', 'home', 'session'))
    for path in (work, home, session):
        path.mkdir(mode=0o700)
    fixture = work / 'fixed-fixture.py'
    fixture.write_text(FIXTURE)
    endpoint = base / 'control.sock'
    descriptor = {'id': str(uuid.uuid4()), 'endpoint': str(endpoint), 'directory': str(session),
                  'spawn': {'sandbox': {'mode': 'restricted', 'read_only': [], 'writable': [],
                                        'network': 'denied', 'home': str(home)},
                            'program': '/usr/bin/python3', 'args': ['-I', str(fixture)],
                            'cwd': str(work), 'env': {}, 'adapter': 'generic'}}
    descpath = session / 'descriptor.json'
    descpath.write_text(json.dumps(descriptor))
    handles = []
    identities = []
    host = None
    with (base / 'host.log').open('wb') as log:
        try:
            host = subprocess.Popen([str(runtime), 'host', str(descpath)], stdin=subprocess.DEVNULL,
                                    stdout=log, stderr=log, start_new_session=True)
            names = ('leader', 'child', 'grandchild')
            wait_until(lambda: all((work / f'{name}.pid').is_file() for name in names), 15,
                       'fixed child and grandchild readiness')
            for name in names:
                pid = int((work / f'{name}.pid').read_text())
                before = process_state(pid)
                if before is None:
                    raise RuntimeError(f'{name} exited before lifecycle check')
                handle = os.pidfd_open(pid)
                handles.append(handle)
                after = process_state(pid)
                if after is None or before['start'] != after['start']:
                    raise RuntimeError('fixture process identity changed')
                identities.append({'name': name, 'pid': pid, **after})
            leader, child, grandchild = identities
            if (leader['pgid'] != leader['pid'] or child['ppid'] != leader['pid']
                    or grandchild['ppid'] != child['pid']
                    or any(item['pgid'] != leader['pid'] for item in identities)):
                raise RuntimeError('fixture processes do not form the owned PTY tree')
            if not all(alive(item) for item in identities):
                raise RuntimeError('fixture tree was not running before cleanup')
            if cancel:
                rpc(endpoint, {'cmd': 'kill'})
            else:
                (work / 'exit-now').write_text('exit')

            def exited_status():
                try:
                    status = json.loads((session / 'status.json').read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    return None
                return status if status.get('exit') is not None else None

            status = wait_until(exited_status, 10, 'host exit status')
            wait_until(lambda: not any(alive(item) for item in identities), 5,
                       'background child and grandchild termination')
            expected = 'stopped' if cancel else 'done'
            if status['state'] != expected or (not cancel and status['exit'] != 0):
                raise RuntimeError(f'unexpected final status: {status}')
            # A repeated cancellation before host shutdown must be harmless.
            if host.poll() is None:
                try:
                    rpc(endpoint, {'cmd': 'kill'})
                except (FileNotFoundError, ConnectionRefusedError):
                    if host.poll() is None:
                        raise
            host.wait(timeout=8)
            if host.returncode != 0:
                raise RuntimeError(f'host failed with exit {host.returncode}')
            return {'scenario': 'cancel' if cancel else 'natural_exit', 'status': 'passed',
                    'final_state': expected, 'background_child_terminated': True,
                    'background_grandchild_terminated': True, 'host_exited': True}
        finally:
            # Readiness may fail before every descendant pidfd is captured. Ask
            # the still-owned host to close its entire group before fallback cleanup.
            if host is not None and host.poll() is None:
                try:
                    rpc(endpoint, {'cmd': 'kill'})
                except (OSError, RuntimeError, ValueError):
                    pass
            # Only pidfds captured from this test's known, fixed process tree are
            # signaled. A reused numeric PID can never be targeted during cleanup.
            for handle in handles:
                try:
                    signal.pidfd_send_signal(handle, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                finally:
                    os.close(handle)
            if host is not None and host.poll() is None:
                host.kill()
                host.wait(timeout=5)


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def terminated(_signum, _frame):
    raise InterruptedError('lifecycle test received SIGTERM')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise SystemExit('output directory must be empty')
    repository = Path(__file__).resolve().parent.parent
    sources = ('crates/velella/src/process.rs', 'crates/velella/src/sessions.rs',
               'crates/velella/src/restricted.rs', 'crates/velella/src/restricted_fs.rs',
               'scripts/test-restricted-lifecycle.py')
    result = {'status': 'failed', 'execution': 'restricted', 'model_called': False,
              'network': 'denied', 'cases': [],
              'runtime': {'path': str(runtime), 'sha256': sha256(runtime)},
              'source_sha256': {name: sha256(repository / name) for name in sources}}
    previous_sigterm = signal.signal(signal.SIGTERM, terminated)
    try:
        with tempfile.TemporaryDirectory(prefix='vl-life-') as temporary:
            root = Path(temporary)
            for cancel in (True, False):
                result['cases'].append(scenario(runtime, root, cancel))
        result['temporary_files_removed'] = True
        result['status'] = 'passed'
    except Exception as error:
        result['error'] = str(error)
    finally:
        signal.signal(signal.SIGTERM, previous_sigterm)
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    return 0 if result['status'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
