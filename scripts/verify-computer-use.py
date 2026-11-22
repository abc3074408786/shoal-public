#!/usr/bin/env python3
"""真实鱼群目录/中继与自有窗口验收；不调用模型，不修改实时配置。"""
from __future__ import annotations
import argparse
import base64
import ctypes
import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import uuid

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / 'tools/computer-use'
DEFAULT_RUNTIME = Path('D:/Velella/artifacts/computer-use-20261117/runtime/velella-runtime.exe')
LIMIT = 1048576
FLAGS = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
sys.path.insert(0, str(PACKAGE / 'src'))
from velella_computer_use import catalog, windows
from velella_computer_use.control import process_alive


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def hashes():
    paths = list((PACKAGE / 'src').rglob('*.py')) + list((PACKAGE / 'tests').rglob('*.py'))
    paths += [PACKAGE / n for n in ('pyproject.toml', 'uv.lock', 'README.md', 'DEVELOPMENT.md')]
    paths += [Path(__file__), *sorted((ROOT / 'scripts/fixtures').glob('computer-use-*'))]
    paths += list((ROOT / 'openspec/changes/computer-use-for-swarm').glob('*.md'))
    return {p.relative_to(ROOT).as_posix(): sha(p) for p in sorted(paths) if p.is_file()}


def wait_for(read, label, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = read()
            if result:
                return result
        except (FileNotFoundError, json.JSONDecodeError, PermissionError):
            pass
        time.sleep(.05)
    raise TimeoutError(label)


def ok(result):
    assert not result.get('isError'), str(result)[:500]
    return result


def text(result):
    return json.loads(next(b['text'] for b in result['content'] if b['type'] == 'text'))


class Peer:
    """Bounded real newline JSON-RPC client; measures complete wire frames."""
    def __init__(self, command):
        self.proc = subprocess.Popen(list(map(str, command)), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, creationflags=FLAGS)
        self.frames, self.maximum, self.sequence = queue.Queue(), 0, 0
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            while True:
                line = self.proc.stdout.readline(LIMIT + 1)
                if not line:
                    break
                assert len(line) < LIMIT, 'oversized MCP frame'
                self.maximum = max(self.maximum, len(line))
                self.frames.put(json.loads(line))
        except Exception as error:
            self.frames.put(error)
        finally:
            self.frames.put(EOFError('MCP process closed'))

    def send(self, value):
        self.proc.stdin.write((json.dumps(value, ensure_ascii=False) + '\n').encode())
        self.proc.stdin.flush()

    def rpc(self, method, params=None):
        self.sequence += 1
        request = {'jsonrpc': '2.0', 'id': self.sequence, 'method': method}
        if params is not None:
            request['params'] = params
        self.send(request)
        deadline = time.monotonic() + 35
        while True:
            value = self.frames.get(timeout=max(.01, deadline - time.monotonic()))
            if isinstance(value, Exception):
                raise value
            if 'id' not in value:
                continue
            assert value['id'] == self.sequence and 'error' not in value, value
            return value['result']

    def initialize(self):
        self.rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {},
                               'clientInfo': {'name': 'owned-verifier', 'version': '1'}})
        self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        return self

    def call(self, name, **arguments):
        return self.rpc('tools/call', {'name': name, 'arguments': arguments})

    def close(self):
        if self.proc.poll() is None:
            try:
                self.proc.stdin.close()
                self.proc.wait(timeout=6)
            except (OSError, subprocess.TimeoutExpired):
                subprocess.run(['taskkill', '/PID', str(self.proc.pid), '/T', '/F'], capture_output=True, creationflags=FLAGS)
                self.proc.wait(timeout=5)
        self.reader.join(timeout=2)
        for stream in (self.proc.stdin, self.proc.stdout):
            if not stream.closed:
                stream.close()

    def crash(self):
        self.proc.kill()
        self.proc.wait(timeout=5)
        self.close()


def relay(runtime, binding, work, synthetic_args=None):
    binding = dict(binding)
    if synthetic_args is not None:
        binding['args'] = synthetic_args
        binding.pop('source', None)  # Explicit synthetic fixture, never an attested production source.
    path = work / ('binding-' + uuid.uuid4().hex + '.json')
    raw = json.dumps(binding, ensure_ascii=False).encode()
    path.write_bytes(raw)
    peer = Peer([runtime, 'external-mcp', path, hashlib.sha256(raw).hexdigest()])
    try:
        return peer.initialize()
    except BaseException:
        peer.close()
        raise


class Evidence:
    def __init__(self, args):
        self.args, self.checks, self.details, self.failures = args, [], {}, []
        self.initial = hashes()

    def check(self, label, detail=None):
        self.checks.append(label)
        if detail is not None:
            self.details[label] = detail
        print('[ok]', label, flush=True)

    def write(self):
        final = hashes()
        if final != self.initial:
            self.failures.append('source changed during verification')
        receipt = {'mode': self.args.mode, 'at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   'completed': not self.failures, 'source_hashes': final, 'runtime': str(self.args.runtime),
                   'runtime_sha256': sha(self.args.runtime), 'cli_sha256': sha(self.args.runtime.with_name('velella-cli.exe')),
                   'checks': self.checks, 'details': self.details, 'failures': self.failures,
                   'real_model_requests': False, 'live_profile_mutated': False}
        (self.args.evidence / 'receipt.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2), encoding='utf-8')


class CatalogSession:
    """Actual desktop daemon and public CLI on a disposable profile."""
    def __init__(self, runtime, work, owned_pid=None):
        self.runtime, self.work, self.data = runtime, work, work / 'profile'
        self.cli, self.state = runtime.with_name('velella-cli.exe'), work / 'control-state'
        home = work / 'home'
        home.mkdir()
        self.env = dict(os.environ)
        self.env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / 'roaming'),
                        LOCALAPPDATA=str(home / 'local'), CODEX_HOME=str(home / 'codex'), VELELLA_DATA_DIR=str(self.data))
        manifest = catalog.build_manifest(Path(sys.executable))
        for entry in manifest['entries']:
            entry['args'] += ['--state-dir', str(self.state), '--lease-ttl', '120' if owned_pid else '5']
            if owned_pid:
                entry['args'] += ['--owned-pid', str(owned_pid)]
        self.path = work / 'catalog.json'
        self.path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
        self.daemon = subprocess.Popen([str(runtime), 'daemon', str(self.data)], env=self.env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=FLAGS)
        try:
            wait_for(lambda: self.invoke(['status'], allow_error=True).get('ok'), 'isolated desktop daemon', 15)
        except BaseException:
            self.close()
            raise

    def invoke(self, argv, allow_error=False):
        result = subprocess.run([str(self.cli), '--data-dir', str(self.data), '--json', *map(str, argv)],
                                env=self.env, capture_output=True, timeout=12, creationflags=FLAGS)
        value = json.loads(result.stdout.decode('utf-8-sig'))
        if not allow_error:
            assert result.returncode == 0 and value['ok'], value
        return value

    def confirm(self, identifier, tools):
        preview = self.invoke(['mcp', 'preview', self.path])['result']['catalog']
        argv = ['mcp', 'confirm', self.path, '--digest', preview['digest'], '--entry', identifier, '--confirm']
        for name in tools:
            argv += ['--tool', name]
        server = self.invoke(argv)['result']['server']
        assert server['tools'] == tools and server['source']['digest'] == preview['digest']
        return server

    def close(self):
        if self.daemon.poll() is None:
            self.daemon.terminate()
            self.daemon.wait(timeout=8)


def shot(peer):
    result = ok(peer.call('computer_screenshot'))
    assert any(b['type'] == 'image' for b in result['content'])
    return result


def lifecycle(ev, session):
    binding = session.confirm(catalog.CONTROL_ENTRY_ID, catalog.CONTROL_TOOLS)
    peers = []
    def new():
        peer = relay(session.runtime, binding, session.work)
        peers.append(peer)
        return peer
    try:
        a, b = new(), new()
        token = text(ok(a.call('computer_acquire_control', role='fixture-a')))['lease']['token']
        assert b.call('computer_acquire_control', role='fixture-b')['isError']
        assert text(ok(a.call('computer_control_status')))['lease_held_by_us']
        assert b.call('computer_type_text', text='MUST NOT TYPE', lease_token=token)['isError']
        ev.check('two real processes reject competing ownership and foreign input')
        assert a.call('computer_click', x=1.5, y=0, lease_token=token)['isError']
        assert a.call('computer_click', x=0, y=0)['isError']
        ev.check('schema-invalid coordinates and missing credential reject before input')
        pid = text(ok(a.call('computer_control_status')))['pid']
        a.close()
        wait_for(lambda: not process_alive(pid), 'graceful server exit')
        ok(b.call('computer_acquire_control'))
        ev.check('disconnect exits server and permits explicit new acquisition')
        pid = text(ok(b.call('computer_control_status')))['pid']
        b.crash()
        wait_for(lambda: not process_alive(pid), 'relay job cleanup')
        c = new()
        token = text(ok(c.call('computer_acquire_control')))['lease']['token']
        ev.check('relay crash kills owned server and dead-owner lease is reclaimable')
        time.sleep(5.2)
        expired = c.call('computer_type_text', text='MUST NOT TYPE', lease_token=token)
        assert expired.get('isError') and '过期' in str(expired)
        ok(c.call('computer_acquire_control'))
        ev.check('actual five-second expiry refuses input until reacquired')
        for action in ('--stop', '--resume'):
            result = subprocess.run([sys.executable, '-m', 'velella_computer_use.server', '--state-dir', str(session.state), action],
                                    capture_output=True, timeout=10, creationflags=FLAGS)
            assert result.returncode == 0, result.stderr.decode(errors='replace')
            if action == '--stop':
                assert c.call('computer_acquire_control')['isError']
                assert c.call('computer_resume_control')['isError']
        assert c.call('computer_type_text', text='MUST NOT TYPE', lease_token=token)['isError']
        new_token = text(ok(c.call('computer_acquire_control')))['lease']['token']
        assert new_token != token
        stale = c.call('computer_type_text', text='MUST NOT TYPE', lease_token=token)
        assert stale.get('isError') and '凭据' in str(stale)
        ok(c.call('computer_release_control'))
        ev.check('local stop revokes other owner; resume never revives old credentials')
    finally:
        for peer in reversed(peers):
            peer.close()


def protocol(ev, work):
    session = CatalogSession(ev.args.runtime, work)
    try:
        observe = session.confirm(catalog.OBSERVE_ENTRY_ID, catalog.OBSERVE_TOOLS)
        ev.check('actual Rust catalog preview and explicit confirmation', {'catalog_sha256': sha(session.path), 'server': observe})
        peer = relay(ev.args.runtime, observe, work)
        try:
            assert [t['name'] for t in peer.rpc('tools/list')['tools']] == catalog.OBSERVE_TOOLS
            assert peer.call('computer_acquire_control')['isError']
            result = shot(peer)
            assert not session.state.exists()
            ev.check('observation exposes screenshot only and cannot acquire control or write lease state')
            ev.check('live single-desktop screenshot passes real relay under 1 MiB', {'wire_bytes': peer.maximum, 'metadata': text(result)})
        finally:
            peer.close()
        assert not session.state.exists()
        fixture = ROOT / 'scripts/fixtures/computer-use-entropy.py'
        peer = relay(ev.args.runtime, observe, work, synthetic_args=[str(fixture), '--state-dir', str(session.state)])
        try:
            from PIL import Image
            result = shot(peer)
            image = Image.open(io.BytesIO(base64.b64decode(next(b['data'] for b in result['content'] if b['type'] == 'image'))))
            meta = text(result)
            assert meta['origin_px'] == [-1920, -120] and meta['physical_size_px'] == [3840, 2160]
            assert meta['size_px'] == list(image.size) and image.width < 3840 and peer.maximum < LIMIT
            ev.check('synthetic high-entropy negative-origin multi-screen image passes real relay with correct resized dimensions',
                     {'wire_bytes': peer.maximum, 'metadata': meta, 'scope': 'synthetic geometry, not a second physical monitor'})
        finally:
            peer.close()
        lifecycle(ev, session)
    finally:
        session.close()
    ev.check('all owned relays, servers and temporary desktop daemon closed')


def fixture_state(work, action='dump'):
    state, command = work / 'state.json', work / 'command.txt'
    if state.exists():
        state.unlink()
    wait_for(lambda: not command.exists(), 'previous fixture command')
    command.write_text(action, encoding='utf-8')
    if action != 'dump':
        return wait_for(lambda: not command.exists(), 'fixture command consumed')
    return wait_for(lambda: json.loads(state.read_text(encoding='utf-8')), 'fixture state')


def native(ev, work):
    from ctypes import wintypes
    from PIL import Image
    windows.enable_dpi_awareness()
    windows.user32.GetClipboardSequenceNumber.restype = wintypes.DWORD
    windows.user32.GetCursorPos.argtypes = (ctypes.POINTER(wintypes.POINT),)
    cursor = wintypes.POINT()
    windows.user32.GetCursorPos(ctypes.byref(cursor))
    previous, clipboard = windows.user32.GetForegroundWindow(), windows.user32.GetClipboardSequenceNumber()
    folder = work / 'window'
    folder.mkdir()
    fixture = subprocess.Popen([sys.executable, str(ROOT / 'scripts/fixtures/computer-use-window.py'), str(folder)], creationflags=FLAGS)
    session = peer = None
    owned_pid = None
    try:
        ready = wait_for(lambda: json.loads((folder / 'ready.json').read_text(encoding='utf-8')), 'owned fixture ready')
        # Windows venv python.exe is a redirector: its child owns the real GUI.
        owned_pid = ready['pid']
        assert fixture.poll() is None and process_alive(owned_pid), 'owned fixture process is not alive'
        ev.details['fixture_ready'] = ready
        fixture_state(folder, 'focus')
        try:
            wait_for(lambda: windows.foreground_pid() == owned_pid, 'fixture foreground')
        except TimeoutError:
            ev.details['fixture_focus_blocker'] = fixture_state(folder)
            raise
        session = CatalogSession(ev.args.runtime, work, owned_pid)
        binding = session.confirm(catalog.CONTROL_ENTRY_ID, catalog.CONTROL_TOOLS)
        peer = relay(ev.args.runtime, binding, work)
        token = text(ok(peer.call('computer_acquire_control', role='owned-window-test')))['lease']['token']
        meta = None
        def point(x, y):
            return {'x': int((x - meta['origin_px'][0]) / meta['scale_to_physical'][0]),
                    'y': int((y - meta['origin_px'][1]) / meta['scale_to_physical'][1])}
        def capture(name):
            nonlocal meta
            result = shot(peer)
            meta = text(result)
            image = Image.open(io.BytesIO(base64.b64decode(next(b['data'] for b in result['content'] if b['type'] == 'image'))))
            current = fixture_state(folder)
            e, c = current['entry_rect'], current['canvas_rect']
            first = point(min(e['x'], c['x']), e['y'])
            last = point(max(e['x']+e['w'], c['x']+c['w']), c['y']+c['h'])
            image.crop((first['x'], first['y'], last['x'], last['y'])).save(ev.args.evidence / name)
            return current
        def act(name, **arguments):
            windows.require_owned_foreground([owned_pid])
            result = ok(peer.call(name, lease_token=token, **arguments))
            time.sleep(.15)
            return result
        before = capture('native-before.png')
        e = before['entry_rect']
        act('computer_move_mouse', **point(e['x']+40, e['y']+15))
        act('computer_click', **point(e['x']+40, e['y']+15))
        act('computer_type_text', text='鱼群 Computer Use 你好')
        typed = capture('native-unicode.png')
        assert typed['entry'] == '鱼群 Computer Use 你好', typed
        ev.check('real Unicode SendInput reaches owned fixture with state and screenshot evidence')
        act('computer_press_keys', keys='ctrl+a')
        act('computer_type_text', text='组合键已验证')
        combined = fixture_state(folder)
        assert combined['entry'] == '组合键已验证' and combined['counters']['hotkeys'] >= 1, combined
        ev.check('real Ctrl+A and replacement text prove keyboard combination')
        c = combined['canvas_rect']
        start, end = point(c['x']+110, c['y']+110), point(c['x']+210, c['y']+150)
        act('computer_click', **start)
        act('computer_drag', start_x=start['x'], start_y=start['y'], end_x=end['x'], end_y=end['y'])
        dragged = fixture_state(folder)
        assert dragged['counters']['clicks'] >= 2 and dragged['counters']['drags'] > 0, dragged
        assert dragged['box_bbox'] != before['box_bbox']
        ev.check('real click and drag move fixture object')
        act('computer_scroll', **point(c['x']+350, c['y']+100), delta=-3)
        after = capture('native-after.png')
        assert after['counters']['scrolls'] > 0 and after['scroll_view'] != before['scroll_view'], after
        ev.check('real wheel changes scroll view; after screenshot saved')
        refused = peer.call('computer_drag', start_x=start['x'], start_y=start['y'], end_x=meta['size_px'][0], end_y=end['y'], lease_token=token)
        assert refused.get('isError') and fixture_state(folder)['counters'] == after['counters']
        ev.check('out-of-range drag rejects before mouse input')
        ok(peer.call('computer_stop_control'))
        assert peer.call('computer_type_text', text='MUST NOT TYPE', lease_token=token).get('isError')
        assert fixture_state(folder)['entry'] == '组合键已验证'
        ev.check('stop rejects subsequent input without changing fixture')
        ok(peer.call('computer_resume_control'))
        peer.close(); peer = None
        assert windows.user32.GetClipboardSequenceNumber() == clipboard
        ev.check('clipboard sequence unchanged')
        ev.details.update(before=before, typed=typed, after=after, fixture_pid=owned_pid, fixture_launcher_pid=fixture.pid)
    finally:
        if peer:
            peer.close()
        if session:
            session.close()
        if fixture.poll() is None:
            (folder / 'command.txt').write_text('quit', encoding='utf-8')
            try:
                fixture.wait(timeout=5)
            except subprocess.TimeoutExpired:
                subprocess.run(['taskkill', '/PID', str(fixture.pid), '/T', '/F'], capture_output=True, creationflags=FLAGS)
                fixture.wait(timeout=3)
        windows.user32.SetCursorPos(cursor.x, cursor.y)
        if previous:
            windows.user32.SetForegroundWindow.argtypes = (wintypes.HWND,)
            windows.user32.SetForegroundWindow(previous)
    assert not process_alive(fixture.pid)
    assert owned_pid is None or not process_alive(owned_pid)
    ev.check('owned fixture, relays, servers and daemon closed; previous cursor/foreground restoration attempted')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['protocol', 'native'], required=True)
    parser.add_argument('--runtime', type=Path, default=DEFAULT_RUNTIME)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    if (args.evidence / 'receipt.json').exists():
        parser.error('evidence exists; never overwrite a prior attempt')
    ev = Evidence(args)
    try:
        with tempfile.TemporaryDirectory(prefix='velella-computer-use-') as folder:
            (protocol if args.mode == 'protocol' else native)(ev, Path(folder))
    except Exception as error:
        ev.failures.append('%s: %s' % (type(error).__name__, error))
        ev.details['traceback'] = traceback.format_exc()
        print('[failed]', ev.failures[-1], flush=True)
    ev.write()
    print('evidence:', args.evidence / 'receipt.json', flush=True)
    return 1 if ev.failures else 0


if __name__ == '__main__':
    raise SystemExit(main())
