"""Explicit live desktop browser acceptance using an owned loopback test page.

The chosen workspace must already be displayed in the real desktop workbench.
No daemon, desktop, native agent or user terminal is started/stopped by this test.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--workspace', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--confirm-live-test', action='store_true', required=True)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    profile = args.data_dir.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    token = 'velella-live-' + uuid.uuid4().hex
    evidence = {'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                'cli': str(cli), 'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'profile': str(profile), 'workspace': args.workspace, 'checks': [], 'page_ids': [],
                'scope': 'Actual installed desktop/workbench browser with owned loopback page',
                'terminal_input_sent': False, 'native_model_requests': False, 'source_changed': False}
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if not self.path.startswith('/' + token + '/'):
                self.send_error(404)
                return
            if self.path.endswith('/ping'):
                payload = b'{"ok":true}'
                content_type = 'application/json'
            else:
                payload = ("<!doctype html><html><head><meta charset='utf-8'><title>Velella live browser acceptance</title>"
                           "<style>body{font:20px system-ui;padding:40px;background:#f4f6f5;color:#172c27}input,button{font:inherit;padding:10px;margin:12px}p{padding:12px}</style>"
                           "</head><body><h1>Velella browser acceptance</h1><label>Probe value<input id='probe'></label>"
                           "<button id='apply'>Apply</button><p id='result'>Before</p>"
                           "<script>document.getElementById('apply').onclick=()=>{document.getElementById('result').textContent=document.getElementById('probe').value;"
                           "console.log('velella-live-probe-applied');fetch('ping')};</script></body></html>").encode()
                content_type = 'text/html; charset=utf-8'
            self.send_response(200)
            self.send_header('Content-Type', content_type)
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    base = f'http://127.0.0.1:{server.server_port}/{token}/'
    before_ids = set()
    before_layout = None

    def save():
        (output / 'result.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')

    def invoke(*argv, ok=True):
        child = subprocess.run([str(cli), '--data-dir', str(profile), '--json', '--timeout-ms', '8000', *map(str, argv)],
                               capture_output=True, timeout=15, creationflags=flags)
        reply = json.loads(child.stdout.decode('utf-8-sig'))
        assert (child.returncode == 0) == ok and reply['ok'] == ok, (argv, child.returncode, reply)
        return reply['result'] if ok else reply

    def listing():
        return invoke('browser', 'list', '--workspace', args.workspace)['pages']

    def ready(page_id, suffix):
        until = time.monotonic() + 12
        while True:
            page = invoke('browser', 'show', '--workspace', args.workspace, '--page', page_id)['page']
            if page.get('ready') and page.get('presented') and page['url'] == base + suffix:
                return
            assert time.monotonic() < until, f'owned page not ready/presented: {page}'
            time.sleep(0.15)

    def check(label):
        evidence['checks'].append(label)
        save()

    try:
        before = invoke('status')
        evidence['daemon_pid_before'] = before['pid']
        status = invoke('browser', 'status')
        assert status['backend'] == 'electron-chromium'
        before_ids = {page['id'] for page in listing()}
        evidence['existing_page_count'] = len(before_ids)
        before_layout = invoke('workspace', 'layout', args.workspace)['layout']
        (output / 'layout-before.json').write_text(json.dumps(before_layout, ensure_ascii=False, indent=2), encoding='utf-8')
        created = invoke('browser', 'create', '--workspace', args.workspace, '--url', base + 'one')['page']
        page_id = created['id']
        assert page_id not in before_ids and created['url'] == base + 'one'
        evidence['page_ids'].append(page_id)
        save()
        target = ['--workspace', args.workspace, '--page', page_id]
        ready(page_id, 'one')
        check('live desktop creates and presents the owned loopback page through the real workbench')

        snapshot = invoke('browser', 'snapshot', *target)
        assert snapshot['content_is_untrusted'] is True
        textbox = next(node for node in snapshot['nodes'] if node['role'].lower() in ('textbox', 'textfield') and node['name'] == 'Probe value')
        button = next(node for node in snapshot['nodes'] if node['role'] == 'button' and node['name'] == 'Apply')
        message = 'Velella live browser accepted'
        invoke('browser', 'fill', *target, '--snapshot', snapshot['snapshot_id'], '--element', textbox['ref'], '--value', message)
        stale = invoke('browser', 'click', *target, '--snapshot', snapshot['snapshot_id'], '--element', button['ref'], ok=False)
        assert 'snapshot_stale' in stale['error']['message']
        snapshot = invoke('browser', 'snapshot', *target)
        button = next(node for node in snapshot['nodes'] if node['role'] == 'button' and node['name'] == 'Apply')
        invoke('browser', 'click', *target, '--snapshot', snapshot['snapshot_id'], '--element', button['ref'])
        snapshot = invoke('browser', 'snapshot', *target)
        assert any(node['name'] == message for node in snapshot['nodes'])
        check('live AX references, fill/click DOM result and stale-reference rejection')

        screenshot = output / 'screenshot.png'
        captured = invoke('browser', 'screenshot', *target, '--out', screenshot)
        assert captured['bytes'] > 100 and screenshot.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        evidence['screenshot'] = str(screenshot)
        invoke('browser', 'context', *target)
        console = invoke('browser', 'console', *target)
        network = invoke('browser', 'network', *target)
        assert 'velella-live-probe-applied' in json.dumps(console)
        assert any(request.get('url') == base + 'ping' for request in network['requests'])
        check('live page PNG, context, actual console message and loopback network metadata')

        invoke('browser', 'goto', *target, '--url', base + 'two')
        ready(page_id, 'two')
        invoke('browser', 'back', *target)
        ready(page_id, 'one')
        invoke('browser', 'forward', *target)
        ready(page_id, 'two')
        invoke('browser', 'reload', *target)
        ready(page_id, 'two')
        invoke('browser', 'close', *target)
        assert invoke('browser', 'show', *target)['page']['open'] is False
        invoke('browser', 'snapshot', *target, ok=False)
        invoke('browser', 'open', *target)
        ready(page_id, 'two')
        check('live navigation/back/forward/reload and view close/reopen')
        history_layout = invoke('workspace', 'layout', args.workspace)['layout']
        if len(history_layout.get('closed', [])) < 30:
            owned_tab = next(tab for pane in history_layout['panes'] for tab in pane['tabs'] if tab.get('kind') == 'browser' and tab.get('target') == page_id)
            history_layout.setdefault('closed', []).insert(0, {**owned_tab, 'id': 'live-probe-closed-' + uuid.uuid4().hex})
            history_file = output / 'layout-history-probe.json'
            history_file.write_text(json.dumps(history_layout, ensure_ascii=False, indent=2), encoding='utf-8')
            invoke('workspace', 'layout-save', args.workspace, '--file', history_file)
            evidence['owned_closed_history_probe_added'] = True
        evidence['passed'] = True
    except Exception as error:
        evidence.update(passed=False, error=str(error))
    finally:
        cleanup_errors = []
        try:
            # Recover an uncertain creation only by unique origin/path and absence
            # from the initial set; never replay the create request.
            owned = {page['id'] for page in listing() if page['id'] not in before_ids and page.get('url', '').startswith(base)}
            owned.update(evidence['page_ids'])
            for page_id in sorted(owned):
                invoke('browser', 'forget', '--workspace', args.workspace, '--page', page_id, '--confirm')
            layout = invoke('workspace', 'layout', args.workspace)['layout']
            evidence['native_forget_removed_tabs'] = not any(
                tab.get('kind') == 'browser' and tab.get('target') in owned
                for pane in layout['panes'] for tab in pane['tabs']) and not any(
                    tab.get('kind') == 'browser' and tab.get('target') in owned for tab in layout.get('closed', []))
            if not evidence['native_forget_removed_tabs']:
                evidence['passed'] = False
                cleanup_errors.append('native forget left workbench tabs; fixture fallback cleanup was required')
            closed_before_cleanup = layout.get('closed', [])
            layout['closed'] = [tab for tab in closed_before_cleanup if not (tab.get('kind') == 'browser' and tab.get('target') in owned)]
            changed = len(layout['closed']) != len(closed_before_cleanup)
            for pane in layout['panes']:
                removed = {tab['id'] for tab in pane['tabs'] if tab.get('kind') == 'browser' and tab.get('target') in owned}
                if not removed:
                    continue
                changed = True
                pane['tabs'] = [tab for tab in pane['tabs'] if tab['id'] not in removed]
                if pane.get('active') in removed:
                    old = next((p.get('active') for p in (before_layout or {}).get('panes', []) if p['id'] == pane['id']), None)
                    pane['active'] = old if any(tab['id'] == old for tab in pane['tabs']) else (pane['tabs'][0]['id'] if pane['tabs'] else None)
            if changed:
                cleanup_file = output / 'layout-cleanup.json'
                cleanup_file.write_text(json.dumps(layout, ensure_ascii=False, indent=2), encoding='utf-8')
                invoke('workspace', 'layout-save', args.workspace, '--file', cleanup_file)
            after_ids = {page['id'] for page in listing()}
            assert before_ids.issubset(after_ids) and not (owned & after_ids)
            final_layout = invoke('workspace', 'layout', args.workspace)['layout']
            assert not any(tab.get('kind') == 'browser' and tab.get('target') in owned for pane in final_layout['panes'] for tab in pane['tabs'])
            assert not any(tab.get('kind') == 'browser' and tab.get('target') in owned for tab in final_layout.get('closed', []))
            evidence['owned_pages_and_tabs_removed'] = True
            evidence['original_pages_retained'] = True
            evidence['daemon_pid_after'] = invoke('status')['pid']
            assert evidence['daemon_pid_after'] == evidence.get('daemon_pid_before')
            if evidence['native_forget_removed_tabs']:
                check('native forget removes open tabs and owned closed-history entries without fixture fallback; original pages and daemon retained')
        except Exception as error:
            cleanup_errors.append(str(error))
            evidence['passed'] = False
        server.shutdown()
        server.server_close()
        server_thread.join(timeout=2)
        evidence['loopback_server_stopped'] = not server_thread.is_alive()
        evidence['cleanup_errors'] = cleanup_errors
        evidence['completed_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        save()
    print(json.dumps(evidence, ensure_ascii=False, indent=2))
    raise SystemExit(0 if evidence.get('passed') and not cleanup_errors and evidence['loopback_server_stopped'] else 1)


if __name__ == '__main__':
    main()
