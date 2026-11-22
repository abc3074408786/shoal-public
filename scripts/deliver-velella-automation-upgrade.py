"""Explicitly authorized maintenance upgrade. Prepare is read-only apart from backups.
Apply requires normal desktop exit and retains every captured terminal process.
"""
import argparse
from contextlib import closing
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time
import psutil

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'artifacts/velella-automation-management-20261029'
PLAN = json.loads((OUT / 'upgrade-package.json').read_text(encoding='utf-8'))
BASE = ROOT / 'artifacts/installed-no-codex-chat-20261027'
DATA = Path(os.environ['LOCALAPPDATA']) / 'Velella'
OLD = BASE / 'resources/runtime/terminal-reconnect-20261027/velella-runtime.exe'
NEW_DIR = BASE / 'resources/runtime/automation-management-20261029'
NEW = NEW_DIR / 'velella-runtime.exe'
APP = BASE / 'resources/app.asar'
GUI = BASE / 'Velella.exe'
CLI = Path(os.environ['LOCALAPPDATA']) / 'Programs/Velella CLI/velella-cli.exe'
PRIVATE = OUT / 'deployment-private'
STATE = PRIVATE / 'state.json'
FLAGS = subprocess.CREATE_NO_WINDOW


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest() if hasattr(hashlib, 'file_digest') else hashlib.sha256(stream.read()).hexdigest()


def identity(pid):
    p = psutil.Process(pid)
    return {'pid': pid, 'created': p.create_time(), 'exe': p.exe()}


def rpc(q, binary=OLD):
    result = subprocess.run([str(binary), 'rpc', str(DATA), '--stdin'], input=json.dumps(q).encode(),
                            capture_output=True, timeout=20, creationflags=FLAGS)
    value = json.loads(result.stdout)
    assert result.returncode == 0 and value.get('ok'), value.get('error', 'RPC failed')
    return value


def no_desktop():
    for p in psutil.process_iter(['name', 'exe', 'cmdline']):
        info = p.info
        if info['exe'] and Path(info['exe']) == GUI:
            raise RuntimeError('Desktop is still running; normal tray exit required')
        if 'desktop-bridge' in (info['cmdline'] or []) and str(DATA) in info['cmdline']:
            raise RuntimeError('Desktop bridge is still running')


def sql(path):
    return sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=10)


def quiet():
    snapshot = rpc({'cmd': 'snapshot'})
    assert snapshot['tasks'] == [], 'tasks exist; do not change runtime during work'
    with closing(sql(DATA / 'capabilities.db')) as db:
        assert db.execute('SELECT count(*) FROM automations').fetchone()[0] == 0, 'schedule set changed'
    with closing(sql(DATA / 'collaboration.db')) as db:
        counts = dict(db.execute('SELECT kind,count(*) FROM objects GROUP BY kind'))
        # Current deployment contract is zero collaboration work, not a guessed terminal-state list.
        assert not any(n for k, n in counts.items() if k in ('run', 'orchestration', 'work', 'attempt')), f'collaboration work exists: {counts}'
    return snapshot


def db_state():
    with closing(sql(DATA / 'workbench.db')) as db:
        return {'schema': db.execute('PRAGMA user_version').fetchone()[0],
                'workspaces': [r[0] for r in db.execute('SELECT id FROM workspaces ORDER BY id')],
                'sessions': [r[0] for r in db.execute('SELECT id FROM sessions ORDER BY id')],
                'layouts': [r[0] for r in db.execute('SELECT workspace_id FROM layouts ORDER BY workspace_id')]}


def capture(snapshot):
    descriptors = {}
    with closing(sql(DATA / 'workbench.db')) as db:
        for text, in db.execute('SELECT document FROM sessions'):
            row = json.loads(text)
            descriptors[row['descriptor']['id']] = row['descriptor']
    for project in snapshot['projects']:
        for desc in project['terminals']:
            descriptors[desc['id']] = desc
    protected, histories, hosts, stale_hosts = {}, [], [], []
    for desc in descriptors.values():
        directory = Path(desc['directory'])
        assert not (directory / 'terminal-input-pending.json').exists(), 'unacknowledged terminal input'
        recording = directory / 'terminal.bin'
        if recording.exists():
            size = recording.stat().st_size
            with recording.open('rb') as stream:
                digest = hashlib.sha256(stream.read(size)).hexdigest()
            histories.append({'path': str(recording), 'size': size, 'sha256': digest})
        status_file = directory / 'status.json'
        if not status_file.exists():
            continue
        status = json.loads(status_file.read_text(encoding='utf-8'))
        if status.get('exit') is not None:
            continue
        try:
            process = psutil.Process(status['pid'])
        except psutil.NoSuchProcess:
            # Historical session metadata can outlive its host. Keep its recording
            # in the preservation set, but do not treat a nonexistent PID as live.
            stale_hosts.append({'id': desc['id'], 'pid': status['pid']})
            continue
        command = process.cmdline()
        assert 'host' in command and any(Path(v) == directory / 'launch.json' for v in command[1:]), 'live host identity mismatch'
        hosts.append({'id': desc['id'], 'endpoint': desc['endpoint'], **identity(process.pid)})
        for child in [process, *process.children(recursive=True)]:
            protected[child.pid] = identity(child.pid)
    assert hosts, 'no live hosts captured'
    return {'daemon': identity(snapshot['pid']), 'projects': snapshot['projects'], 'db': db_state(),
            'protected': list(protected.values()), 'hosts': hosts, 'stale_hosts': stale_hosts, 'histories': histories,
            'projects_sha256': sha(DATA / 'projects.json'), 'browser_pages_sha256': sha(DATA / 'browser-pages.json')}


def preserved(state):
    for item in state['protected']:
        assert identity(item['pid']) == item, f'protected process changed: {item["pid"]}'
    for item in state['histories']:
        with Path(item['path']).open('rb') as stream:
            assert hashlib.sha256(stream.read(item['size'])).hexdigest() == item['sha256'], 'recording prefix changed'
    assert sha(DATA / 'projects.json') == state['projects_sha256']
    assert sha(DATA / 'browser-pages.json') == state['browser_pages_sha256']


def backup(state, name):
    target_dir = PRIVATE / name
    target_dir.mkdir()
    paths = sorted(DATA.glob('*.db')) + [Path(p['path']) / '.velella/state.db' for p in state['projects']]
    copies = []
    for index, path in enumerate(paths):
        assert path.is_file(), f'missing database: {path}'
        target = target_dir / f'{index}-{path.name}'
        with closing(sql(path)) as source, closing(sqlite3.connect(target)) as dest:
            source.backup(dest)
            assert dest.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
            version = dest.execute('PRAGMA user_version').fetchone()[0]
        copies.append({'source': str(path), 'backup': str(target), 'sha256': sha(target), 'schema': version})
    shutil.copy2(DATA / 'projects.json', target_dir / 'projects.json')
    state[name] = copies


def save(state):
    STATE.write_text(json.dumps(state, ensure_ascii=False, indent=2), encoding='utf-8')


def start(binary):
    return subprocess.Popen([str(binary), 'daemon', str(DATA)], stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, creationflags=FLAGS)


def ready(process, binary):
    until = time.monotonic() + 20
    while time.monotonic() < until:
        assert process.poll() is None, 'new daemon exited'
        try:
            value = rpc({'cmd': 'snapshot'}, binary)
            assert value['pid'] == process.pid, 'different daemon owns profile'
            return value
        except (subprocess.SubprocessError, ValueError):
            time.sleep(0.2)
    raise RuntimeError('daemon startup timeout')


def verify_candidate():
    assert sha(APP) == PLAN['originalSha256'], 'installed archive drift'
    assert sha(OLD) == '01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b'
    assert sha(PLAN['candidate']) == PLAN['candidateSha256']
    assert sha(CLI) == '8de3af97ada4f46aa1fa658d3053e56f8fe950791f2ff7cb637d406cf55dbe5b'
    for name, digest in PLAN['files'].items():
        assert sha(OUT / 'candidate' / name) == digest, f'candidate drift: {name}'


def prepare():
    assert PRIVATE.is_dir() and not STATE.exists(), 'private backup folder missing or preparation already exists'
    no_desktop()
    verify_candidate()
    snapshot = quiet()
    state = capture(snapshot)
    assert Path(state['daemon']['exe']) == OLD
    assert state['db']['schema'] == 2
    shutil.copy2(APP, PRIVATE / 'app.original.asar')
    shutil.copy2(CLI, PRIVATE / 'velella-cli.original.exe')
    backup(state, 'preflight')
    state.update(status='prepared', prepared_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
    save(state)
    print(json.dumps({'status': state['status'], 'daemon': state['daemon']['pid'], 'protected_hosts': len(state['hosts']),
                      'protected_processes': len(state['protected']), 'recordings': len(state['histories']),
                      'databases': len(state['preflight']), 'workspaces': len(state['db']['workspaces'])}))


def apply():
    state = json.loads(STATE.read_text(encoding='utf-8'))
    assert state['status'] == 'prepared', 'not prepared; no replay of partially completed deployment'
    no_desktop()
    verify_candidate()
    snapshot = quiet()
    assert identity(snapshot['pid']) == state['daemon']
    assert db_state() == state['db'], 'workbench changed since preparation'
    preserved(state)
    if NEW_DIR.exists():
        assert state.get('attempt') == 2, 'version directory already exists; inspect rather than overwrite'
        assert {p.name for p in NEW_DIR.iterdir()} == set(PLAN['files']) - {'velella-cli.exe'}
    else:
        NEW_DIR.mkdir()
    for name, digest in PLAN['files'].items():
        if name == 'velella-cli.exe':
            continue
        if not (NEW_DIR / name).exists():
            shutil.copy2(OUT / 'candidate' / name, NEW_DIR / name)
        assert sha(NEW_DIR / name) == digest
    state['status'] = 'switching'
    save(state)
    current = None
    stopped = False
    try:
        no_desktop()
        assert identity(state['daemon']['pid']) == state['daemon']
        process = psutil.Process(state['daemon']['pid'])
        process.terminate()  # exact, independently identified daemon; never a process tree
        process.wait(timeout=15)
        stopped = True
        preserved(state)
        backup_name = 'final' if state.get('attempt', 1) == 1 else 'final-attempt-2'
        backup(state, backup_name)
        state['final'] = state[backup_name]
        save(state)
        pending = APP.with_name('app.automation-upgrade-pending')
        assert not pending.exists()
        shutil.copy2(PLAN['candidate'], pending)
        assert sha(pending) == PLAN['candidateSha256']
        os.replace(pending, APP)
        current = start(NEW)
        after = ready(current, NEW)
        state['candidate_observation'] = after
        save(state)
        expected_projects = json.loads(json.dumps(state['projects']))
        for project in expected_projects:
            # Project.setup_default is serde(default) SetupPolicy::Skip in the
            # reviewed candidate; no existing field is discarded by comparison.
            project.setdefault('setup_default', 'skip')
        assert after['tasks'] == [], 'unexpected task after migration'
        assert after['projects'] == expected_projects, 'project contents changed beyond documented setup_default=skip'
        caps = rpc({'cmd': 'protocol'}, NEW)['capabilities']
        assert 'automation-archive-v1' in caps and 'automation-history-v1' in caps
        assert rpc({'cmd': 'automation.archived'}, NEW)['automations'] == []
        assert rpc({'cmd': 'automation.runs', 'limit': 1}, NEW)['runs'] == []
        result = db_state()
        assert result == {**state['db'], 'schema': 3}
        preserved(state)
        shutil.copy2(OUT / 'candidate/velella-cli.exe', CLI)
        assert sha(CLI) == PLAN['files']['velella-cli.exe'] and sha(APP) == PLAN['candidateSha256']
        state.update(status='installed_desktop_not_started', new_daemon=identity(current.pid),
                     verified_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
        save(state)
        print(json.dumps({'status': state['status'], 'new_daemon': current.pid, 'protected_hosts': len(state['hosts']),
                          'protected_processes': len(state['protected']), 'recording_prefixes_preserved': True, 'schema': 3}))
    except Exception as error:
        state['error'] = str(error)
        state['status'] = 'rollback_in_progress'
        save(state)
        if current is not None and current.poll() is None:
            current.terminate()
            current.wait(timeout=15)
        if stopped:
            for item in state.get('final', state['preflight']):
                assert sha(item['backup']) == item['sha256']
                with closing(sql(Path(item['backup']))) as source, closing(sqlite3.connect(item['source'])) as dest:
                    source.backup(dest)
                    assert dest.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
            shutil.copy2(PRIVATE / 'app.original.asar', APP)
            shutil.copy2(PRIVATE / 'velella-cli.original.exe', CLI)
            old = start(OLD)
            ready(old, OLD)
            preserved(state)
            state['status'] = 'rolled_back_desktop_not_started'
            save(state)
        raise


def resume():
    state = json.loads(STATE.read_text(encoding='utf-8'))
    assert state['status'] == 'rolled_back_desktop_not_started' and state.get('attempt', 1) == 1
    no_desktop()
    verify_candidate()
    snapshot = quiet()
    assert snapshot['projects'] == state['projects'] and db_state() == state['db']
    preserved(state)
    restored_daemon = identity(snapshot['pid'])
    assert Path(restored_daemon['exe']) == OLD
    shutil.copy2(STATE, PRIVATE / 'attempt-1-state.json')
    state.update(status='prepared', daemon=restored_daemon, attempt=2,
                 retry_reason='Compare explicit new setup_default=skip while retaining strict equality for all existing project fields')
    save(state)
    print(json.dumps({'status': 'prepared_retry_2', 'daemon': restored_daemon['pid']}))


def verify():
    state = json.loads(STATE.read_text(encoding='utf-8'))
    assert state['status'] == 'installed_desktop_not_started'
    assert identity(state['new_daemon']['pid']) == state['new_daemon']
    preserved(state)
    snapshot = rpc({'cmd': 'snapshot'}, NEW)
    assert snapshot['pid'] == state['new_daemon']['pid'] and snapshot['tasks'] == []
    checks = []

    def invoke(*args):
        result = subprocess.run([str(CLI), '--json', *args], capture_output=True, timeout=15, creationflags=FLAGS)
        value = json.loads(result.stdout)
        assert result.returncode == 0 and value.get('ok'), value
        return value['result']

    assert invoke('status')['pid'] == state['new_daemon']['pid']
    assert invoke('automation', 'list')['automations'] == []
    assert invoke('automation', 'list', '--archived')['automations'] == []
    page = invoke('automation', 'runs', '--limit', '1')
    assert page['runs'] == [] and page['has_more'] is False and page['next_cursor'] is None
    checks.append('installed CLI uses new daemon; archive discovery and paginated history respond')
    browser = invoke('browser', 'status')
    assert Path(browser['runtime_path']) == NEW
    expected_pages = json.loads((DATA / 'browser-pages.json').read_text(encoding='utf-8'))
    counts = {}
    for workspace in state['db']['workspaces']:
        value = invoke('browser', 'list', '--workspace', workspace)
        expected_ids = sorted(p['id'] for p in expected_pages if p['workspace_id'] == workspace)
        assert sorted(p['id'] for p in value['pages']) == expected_ids
        counts[workspace] = len(expected_ids)
    checks.append('desktop browser bridge points to new runtime and all saved page identities remain')
    live_ids = {h['id']: h['pid'] for h in state['hosts']}
    observed = {}
    for workspace in state['db']['workspaces']:
        for row in rpc({'cmd': 'workbench.sessions', 'workspace_id': workspace}, NEW)['sessions']:
            if row['id'] in live_ids:
                observed[row['id']] = row['status']['pid']
    assert observed == live_ids
    preserved(state)
    checks.append('nine original hosts respond through new daemon; all captured processes and recording prefixes remain')
    assert sha(APP) == PLAN['candidateSha256'] and sha(CLI) == PLAN['files']['velella-cli.exe']
    assert sha(NEW) == PLAN['files']['velella-runtime.exe']
    checks.append('installed archive, CLI and runtime match frozen hashes')
    gui = [p for p in psutil.process_iter(['exe', 'cmdline']) if p.info['exe'] and Path(p.info['exe']) == GUI and
           not any(a.startswith('--type=') for a in (p.info['cmdline'] or []))]
    assert len(gui) == 1
    state.update(status='installed_online_verified', desktop=identity(gui[0].pid),
                 completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
    save(state)
    result = {'status': state['status'], 'time': state['completed_at'], 'checks': checks,
              'daemon': state['new_daemon'], 'desktop': state['desktop'], 'protected_hosts': len(state['hosts']),
              'protected_processes': len(state['protected']), 'recording_prefixes': len(state['histories']),
              'databases_backed_up': len(state['final']), 'schema': db_state()['schema'], 'page_counts': counts,
              'runtime_sha256': sha(NEW), 'cli_sha256': sha(CLI), 'app_sha256': sha(APP),
              'browser_runtime_path': browser['runtime_path'], 'real_model_requests': False, 'production_automation_mutations': False,
              'business_uat': 'not performed', 'attempts': 2, 'first_attempt_rolled_back': True}
    (OUT / 'deployment-online.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('phase', choices=['prepare', 'apply', 'resume', 'verify'])
    phase = parser.parse_args().phase
    {'prepare': prepare, 'apply': apply, 'resume': resume, 'verify': verify}[phase]()
