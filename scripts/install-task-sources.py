"""Prepare a concrete upgrade plan. --apply is only for separately authorized installation.
The GUI must be closed normally first. Never calls service.stop on user data.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,os,psutil,shutil,sqlite3,subprocess,time
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'artifacts/task-sources-20261018/continuation'
INSTALL=Path(r'D:\orcas\artifacts\installer-workspace-20261012')
DATA=Path(os.environ['LOCALAPPDATA'])/'Velella'
GUI=INSTALL/'velella-desktop.exe'
CANDIDATE=ROOT/'desktop/src-tauri/target/release/velella-desktop.exe'
RUNTIME=ROOT/'target/release/velella-runtime.exe'
EXPECTED_GUI='4318cc9af4a437bb700d10e4088e483387d1f1ec544417792060285493b557b2'
EXPECTED_RUNTIME='4b90aa3c8f8aa55757b6707b568323ea9f4629d255485de7f8312e04a1ad4675'
FLAGS=subprocess.CREATE_NO_WINDOW
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def history_prefix(path):
    size=path.stat().st_size
    with path.open('rb') as stream:content=stream.read(size)
    assert len(content)==size,'history truncated while reading'
    return {'bytes':size,'sha256':hashlib.sha256(content).hexdigest()}
def save(name,value):(OUT/name).write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8')
def identity(pid):
    p=psutil.Process(pid);return {'pid':pid,'created':p.create_time(),'exe':str(Path(p.exe()).resolve())}
def old_runtime():
    value=json.loads((INSTALL/'active-runtime.json').read_text(encoding='utf-8-sig'));path=(INSTALL/value['runtime']).resolve()
    assert path.is_relative_to(INSTALL.resolve()) and path.name=='velella-runtime.exe';return path
def rpc(exe,request):
    value=json.loads(subprocess.run([str(exe),'rpc',str(DATA),json.dumps(request)],check=True,capture_output=True,timeout=15,creationflags=FLAGS).stdout)
    assert value['ok'],value.get('error');return value
def guis():
    result=[]
    for p in psutil.process_iter(['name']):
        if p.info['name']=='velella-desktop.exe' and Path(p.exe()).resolve()==GUI.resolve():result.append(identity(p.pid))
    return result
def all_guis():
    return [identity(p.pid) for p in psutil.process_iter(['name']) if p.info['name']=='velella-desktop.exe']
def metadata():
    result={'tables':{},'files':{},'histories':{},'databases':{}};directories=set()
    for name in ['workbench.db','capabilities.db','task-sources.db']:
        if (DATA/name).exists():
            with sqlite3.connect((DATA/name).as_uri()+'?mode=ro',uri=True) as db:
                assert db.execute('pragma integrity_check').fetchone()[0]=='ok',name+' integrity'
                result['databases'][name]={'user_version':db.execute('pragma user_version').fetchone()[0],'sha256':hashlib.sha256('\n'.join(sorted(db.iterdump())).encode()).hexdigest()}
    with sqlite3.connect((DATA/'workbench.db').as_uri()+'?mode=ro',uri=True) as db:
        for table in ['workspaces','sessions','layouts']:
            rows=db.execute('SELECT * FROM '+table+' ORDER BY 1').fetchall()
            result['tables'][table]={'count':len(rows),'sha256':hashlib.sha256(json.dumps(rows,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()}
            if table=='sessions':
                for row in rows:
                    directory=json.loads(row[-1]).get('descriptor',{}).get('directory')
                    if directory:directories.add(directory)
    projects=json.loads((DATA/'projects.json').read_text(encoding='utf-8-sig'));result['project_count']=len(projects)
    for project in projects:
        for terminal in project.get('terminals',[]):directories.add(terminal['directory'])
    for name in ['projects.json','browser-pages.json']:
        if (DATA/name).exists():result['files'][name]=sha(DATA/name)
    for directory in sorted(directories):
        folder=Path(directory)
        assert not (folder/'terminal-input-pending.json').exists(),'pending terminal input; postpone upgrade'
        result['histories'][directory]={p.name:history_prefix(p) for p in folder.glob('terminal*') if p.is_file()}
    return result
def hosts():
    # Read host PIDs through the installed daemon; never infer all similarly named processes are ours.
    snapshot=rpc(old_runtime(),{'cmd':'workbench.snapshot'});result=[]
    sessions=[]
    for workspace in snapshot['workspaces']:
        sessions.extend(rpc(old_runtime(),{'cmd':'workbench.sessions','workspace_id':workspace['id']})['sessions'])
    for project in rpc(old_runtime(),{'cmd':'snapshot'})['projects']:
        sessions.extend(rpc(old_runtime(),{'cmd':'workspace.list','project_id':project['id']})['terminals'])
    for session in sessions:
        status=session.get('status') or {}
        if status.get('pid') and status.get('exit') is None:
            host=psutil.Process(status['pid']);result.extend(identity(p.pid) for p in [host,*host.children(recursive=True)])
    return list({p['pid']:p for p in result}.values())
def verify_processes(before):
    for p in before:assert identity(p['pid'])==p,'protected process changed'
def verify_data(before):
    after=metadata()
    for key in ['tables','files','project_count']:assert after[key]==before[key],key+' changed'
    for name,value in before['databases'].items():assert after['databases'].get(name)==value,name+' changed'
    appended=0
    for directory,files in before['histories'].items():
        for name,old in files.items():
            p=Path(directory)/name;assert p.stat().st_size>=old['bytes']
            with p.open('rb') as f:assert hashlib.sha256(f.read(old['bytes'])).hexdigest()==old['sha256'],'history prefix changed'
            appended+=p.stat().st_size-old['bytes']
    return appended
def start(exe):
    p=subprocess.Popen([str(exe),'daemon',str(DATA)],creationflags=FLAGS,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    deadline=time.monotonic()+20
    try:
        while time.monotonic()<deadline:
            assert p.poll() is None,'new daemon exited'
            try:
                if rpc(exe,{'cmd':'snapshot'})['pid']==p.pid:return p
            except (AssertionError,subprocess.SubprocessError,ValueError):pass
            time.sleep(.1)
        raise TimeoutError('daemon startup')
    except Exception:
        if p.poll() is None:p.terminate();p.wait(timeout=12)
        raise
def prepare():
    assert sha(GUI)==EXPECTED_GUI and sha(old_runtime())==EXPECTED_RUNTIME,'installed baseline changed'
    snapshot=rpc(old_runtime(),{'cmd':'snapshot'});assert not snapshot['tasks'],'supervised tasks exist; inspect before upgrading'
    daemon=identity(snapshot['pid']);assert Path(daemon['exe']).resolve()==old_runtime()
    plan={'status':'prepared','created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'old_gui_sha256':sha(GUI),'old_runtime':str(old_runtime()),'old_runtime_sha256':sha(old_runtime()),'new_gui':str(CANDIDATE),'new_gui_sha256':sha(CANDIDATE),'new_runtime':str(RUNTIME),'new_runtime_sha256':sha(RUNTIME),'daemon':daemon,'guis':guis(),'protected':hosts(),'metadata':metadata(),'requires_separate_install_authorization':True}
    version='task-sources-20261018-'+plan['new_runtime_sha256'][:8]
    plan['version']=version;plan['backup']=str(INSTALL/'.updates'/version);save('install-plan.json',plan)
    print(json.dumps({k:v for k,v in plan.items() if k not in ['metadata','protected']},ensure_ascii=False,indent=2));return plan
def apply():
    plan=json.loads((OUT/'install-plan.json').read_text(encoding='utf-8-sig'));assert not all_guis(),'Close all Velella GUI instances normally before applying'
    assert sha(GUI)==plan['old_gui_sha256']==EXPECTED_GUI and sha(old_runtime())==EXPECTED_RUNTIME
    assert sha(CANDIDATE)==plan['new_gui_sha256'] and sha(RUNTIME)==plan['new_runtime_sha256']
    acceptance=json.loads((OUT/'native-ui-acceptance.json').read_text(encoding='utf-8-sig'));assert acceptance['status']=='passed' and acceptance['gui_sha256']==sha(CANDIDATE)
    receipt=json.loads((OUT/'upgrade-1792337138/result.json').read_text(encoding='utf-8-sig'));assert receipt['status']=='passed' and receipt['new_runtime_sha256']==sha(RUNTIME)
    daemon=identity(rpc(old_runtime(),{'cmd':'snapshot'})['pid']);assert daemon==plan['daemon']
    assert not rpc(old_runtime(),{'cmd':'snapshot'})['tasks'];protected=hosts();verify_processes(plan['protected'])
    before=metadata();backup=Path(plan['backup']);version=INSTALL/'versions'/plan['version'];assert not backup.exists() and not version.exists()
    assert backup.resolve().is_relative_to(INSTALL.resolve()) and version.resolve().is_relative_to(INSTALL.resolve())
    backup.mkdir(parents=True);version.mkdir(parents=True)
    shutil.copy2(GUI,backup/GUI.name);shutil.copy2(INSTALL/'active-runtime.json',backup/'active-runtime.json')
    for name in ['workbench.db','capabilities.db','task-sources.db']:
        if (DATA/name).exists():
            with sqlite3.connect((DATA/name).as_uri()+'?mode=ro',uri=True) as src,sqlite3.connect(backup/name) as dst:src.backup(dst);assert dst.execute('pragma integrity_check').fetchone()[0]=='ok'
    for name in before['files']:shutil.copy2(DATA/name,backup/name)
    for index,(directory,files) in enumerate(before['histories'].items()):
        folder=backup/'session-records'/str(index);folder.mkdir(parents=True)
        for name,record in files.items():
            with (Path(directory)/name).open('rb') as src:(folder/name).write_bytes(src.read(record['bytes']))
            assert sha(folder/name)==record['sha256']
    save('installation-data-before.json',before)
    shutil.copy2(RUNTIME,version/'velella-runtime.exe')
    helper=Path(plan['old_runtime']).parent/'browser-agent';assert (helper/'server.mjs').is_file()
    shutil.copytree(helper,version/'browser-agent')
    pending=INSTALL/'velella-desktop.task-sources.pending';assert not pending.exists();shutil.copy2(CANDIDATE,pending)
    assert sha(pending)==plan['new_gui_sha256'];verify_processes(protected);assert identity(daemon['pid'])==daemon
    result={'status':'backed_up','backup':str(backup),'old_daemon':daemon,'protected':protected,'version':str(version)};save('installation.json',result)
    current=None
    try:
        assert not all_guis(),'A Velella GUI reopened; installation was not started'
        # Separately authorized daemon handoff; preserve every independently running host.
        old_process=psutil.Process(daemon['pid']);assert identity(old_process.pid)==daemon;old_process.terminate();old_process.wait(timeout=12)
        verify_processes(protected)
        pointer=INSTALL/'active-runtime.task-sources.pending';pointer_data=json.loads((INSTALL/'active-runtime.json').read_text(encoding='utf-8-sig'));pointer_data['runtime']=str(Path('versions')/plan['version']/'velella-runtime.exe');pointer.write_text(json.dumps(pointer_data),encoding='utf-8')
        assert not all_guis(),'A Velella GUI reopened during handoff'
        os.replace(pointer,INSTALL/'active-runtime.json');os.replace(pending,GUI)
        current=start(version/'velella-runtime.exe');verify_processes(protected);rpc(version/'velella-runtime.exe',{'cmd':'sources.list'});appended=verify_data(before)
        result.update(status='installed_backend_verified',gui_verified=False,new_daemon=identity(current.pid),gui_sha256=sha(GUI),runtime_sha256=sha(version/'velella-runtime.exe'),history_appended_bytes=appended)
        save('installation.json',result);print(json.dumps(result,ensure_ascii=False,indent=2))
    except Exception as error:
        rollback={}
        def attempt(name,operation):
            try:operation();rollback[name]={'ok':True}
            except Exception as recovery_error:rollback[name]={'ok':False,'error':str(recovery_error)}
        def stop_new():
            if current and current.poll() is None:current.terminate();current.wait(timeout=12)
        def restore_gui():
            if sha(GUI)!=plan['old_gui_sha256']:
                staged=INSTALL/'velella-desktop.rollback.pending';shutil.copy2(backup/GUI.name,staged);os.replace(staged,GUI)
            assert sha(GUI)==plan['old_gui_sha256']
        def restore_pointer():
            staged=INSTALL/'active-runtime.rollback.pending';shutil.copy2(backup/'active-runtime.json',staged);os.replace(staged,INSTALL/'active-runtime.json')
            assert old_runtime()==Path(plan['old_runtime'])
        def restore_daemon():
            try:
                existing=rpc(Path(plan['old_runtime']),{'cmd':'snapshot'})
                assert Path(identity(existing['pid'])['exe']).resolve()==Path(plan['old_runtime'])
            except subprocess.SubprocessError:start(Path(plan['old_runtime']))
        attempt('stop_new_daemon',stop_new)
        attempt('restore_gui',restore_gui)
        attempt('restore_pointer',restore_pointer)
        # Every earlier recovery result is recorded; none can skip this attempt.
        attempt('restore_old_daemon',restore_daemon)
        attempt('protected_processes',lambda:verify_processes(protected))
        attempt('preserved_data',lambda:verify_data(before))
        result.update(status='rolled_back' if all(step['ok'] for step in rollback.values()) else 'rollback_incomplete',error=str(error),rollback=rollback,data_rollback_performed=False,rollback_scope='GUI, runtime pointer and daemon restored; preexisting databases, project files and history prefixes verified unchanged; newly created source database retained')
        save('installation.json',result);raise

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--apply',action='store_true');args=parser.parse_args()
    if args.apply:apply()
    else:prepare()
