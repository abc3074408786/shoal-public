"""Apply the authorized, verified terminal reconnect build while retaining live hosts."""
from pathlib import Path
import datetime, hashlib, json, os, shutil, sqlite3, subprocess, time
import psutil

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'artifacts/terminal-reconnect-delivery-20261027'
PLAN=json.loads((OUT/'install-plan.json').read_text())
BASE=Path(PLAN['base']); DATA=Path(os.environ['LOCALAPPDATA'])/'Velella'
OLD=BASE/'resources/runtime/velella-runtime.exe'; NEW=Path(PLAN['runtime'])
GUI=BASE/'Velella.exe'; APP=BASE/'resources/app.asar'; FLAGS=subprocess.CREATE_NO_WINDOW
BACKUP=OUT/'backup'; RECEIPT=OUT/'installation.json'
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def rpc(exe,q):
    p=subprocess.run([str(exe),'rpc',str(DATA),json.dumps(q)],capture_output=True,timeout=15,creationflags=FLAGS,check=True)
    r=json.loads(p.stdout);assert r.get('ok'),r.get('error');return r
def identity(pid):
    p=psutil.Process(pid);return {'pid':pid,'created':p.create_time(),'exe':p.exe()}
def verify_processes(items):
    for item in items:assert identity(item['pid'])==item, f"protected process changed: {item['pid']}"
def start(exe,args):return subprocess.Popen([str(exe),*args],creationflags=FLAGS,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def wait_runtime(exe):
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        try:return rpc(exe,{'cmd':'snapshot'})
        except (subprocess.SubprocessError,ValueError,AssertionError):time.sleep(.2)
    raise RuntimeError('daemon startup did not complete')
def history(directory):
    p=Path(directory)/'terminal.bin'
    if not p.exists():return None
    size=p.stat().st_size
    # A live log may append during hashing. Capture a fixed prefix.
    with p.open('rb') as f:digest=hashlib.sha256(f.read(size)).hexdigest()
    return {'path':str(p),'bytes':size,'sha256':digest}
def verify_history(items):
    for item in items:
        with Path(item['path']).open('rb') as f:assert hashlib.sha256(f.read(item['bytes'])).hexdigest()==item['sha256'],'history prefix changed'

assert json.loads(Path(PLAN['acceptance']).read_text())['ok'] is True
assert sha(APP)==PLAN['old_app_sha256'] and sha(NEW)==PLAN['runtime_sha256']
assert sha(PLAN['app'])==PLAN['app_sha256']
snapshot=rpc(OLD,{'cmd':'snapshot'})
assert not any(t.get('phase') in ['starting','running','verifying','preparing','delivering'] for t in snapshot['tasks']),'active supervised task; handoff deferred'
daemon=identity(snapshot['pid']);assert Path(daemon['exe']).resolve()==OLD.resolve()
protected=[];histories=[]
with sqlite3.connect((DATA/'workbench.db').as_uri()+'?mode=ro',uri=True) as db:
    assert db.execute('pragma integrity_check').fetchone()[0]=='ok'
    for row in db.execute('SELECT * FROM sessions').fetchall():
        value=json.loads(row[-1]);directory=value['descriptor']['directory']
        assert not (Path(directory)/'terminal-input-pending.json').exists(),'unacknowledged terminal input'
        if record:=history(directory):histories.append(record)
        try:
            status=json.loads((Path(directory)/'status.json').read_text())
            if status.get('exit') is None:
                host=psutil.Process(status['pid'])
                if 'host' in host.cmdline() and str(Path(directory)/'launch.json').replace('\\','/').lower() in ' '.join(host.cmdline()).replace('\\','/').lower():
                    protected.extend(identity(p.pid) for p in [host,*host.children(recursive=True)])
        except (FileNotFoundError,psutil.NoSuchProcess):pass
protected=list({p['pid']:p for p in protected}.values())
guis=[]
for p in psutil.process_iter(['name']):
    if (p.info['name'] or '').lower()!='velella.exe':continue
    try:
        if os.path.normcase(os.path.abspath(p.exe()))==os.path.normcase(os.path.abspath(GUI)) and not any(a.startswith('--type=') for a in p.cmdline()):guis.append(p)
    except (psutil.NoSuchProcess,psutil.AccessDenied):continue
assert len(guis)==1,'desktop identity changed';gui=guis[0];gui_id=identity(gui.pid)
bridges=[p for p in gui.children() if 'desktop-bridge' in p.cmdline()]
assert len(bridges)==1,'desktop relay not uniquely identified'
assert not BACKUP.exists();BACKUP.mkdir()
shutil.copy2(APP,BACKUP/'app.asar')
for name in ['workbench.db','capabilities.db','task-sources.db']:
    if (DATA/name).exists():
        with sqlite3.connect((DATA/name).as_uri()+'?mode=ro',uri=True) as source,sqlite3.connect(BACKUP/name) as target:source.backup(target)
receipt={'status':'prepared','time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'plan':PLAN,'protected':protected,'histories':histories,'old_daemon':daemon,'old_gui':gui_id}
def save():RECEIPT.write_text(json.dumps(receipt,ensure_ascii=False,indent=2),encoding='utf8')
save();current=None;swapped=False
try:
    # The installed main's relay-exit handler calls app.quit(), which runs its
    # normal before-quit cleanup. No session stop or recursive process kill.
    bridges[0].terminate();gui.wait(timeout=20)
    verify_processes(protected)
    old_process=psutil.Process(daemon['pid']);assert identity(old_process.pid)==daemon
    old_process.terminate();old_process.wait(timeout=15);verify_processes(protected)
    pending=APP.with_suffix('.reconnect-pending');shutil.copy2(PLAN['app'],pending);os.replace(pending,APP);swapped=True
    current=start(NEW,['daemon',str(DATA)]);after=wait_runtime(NEW)
    assert after['pid']==current.pid
    verify_processes(protected);verify_history(histories)
    start(GUI,[])
    receipt.update(status='installed',new_daemon=identity(current.pid),app_sha256=sha(APP),runtime_sha256=sha(NEW),history_prefixes_preserved=True,protected_processes_preserved=True)
    save();print(json.dumps({k:v for k,v in receipt.items() if k not in ['histories','protected','plan']},ensure_ascii=False,indent=2))
except Exception as error:
    receipt.update(status='rollback',error=str(error));save()
    if current and current.poll() is None:current.terminate();current.wait(timeout=15)
    if swapped:shutil.copy2(BACKUP/'app.asar',APP)
    try:wait_runtime(OLD)
    except Exception:start(OLD,['daemon',str(DATA)]);wait_runtime(OLD)
    if not psutil.pid_exists(gui_id['pid']):start(GUI,[])
    verify_processes(protected);verify_history(histories);receipt['status']='rolled_back';save();raise
