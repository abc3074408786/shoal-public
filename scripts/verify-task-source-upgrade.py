"""Test old -> new -> old daemon ownership with an isolated, live ConPTY host."""
from pathlib import Path
import base64,hashlib,json,os,psutil,subprocess,time,uuid
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/task-sources-20261018/continuation'/('upgrade-'+str(int(time.time())))
out.mkdir(parents=True);data=out/'data';data.mkdir();repo=out/'repo';repo.mkdir()
old=Path(r'D:\orcas\artifacts\installer-workspace-20261012\versions\terminal-perf-20261018-136af505\velella-runtime.exe')
new=Path(os.environ.get('VELELLA_CANDIDATE_RUNTIME',root/'target/release/velella-runtime.exe'))
assert hashlib.sha256(old.read_bytes()).hexdigest()=='4b90aa3c8f8aa55757b6707b568323ea9f4629d255485de7f8312e04a1ad4675'
flags=subprocess.CREATE_NO_WINDOW;proc=None;active=old;host=None;checks=[]
log=(out/'daemons.log').open('wb')
def rpc(request):
    # The installed version predates --stdin. Requests here contain only fixture data.
    p=subprocess.run([str(active),'rpc',str(data),json.dumps(request)],capture_output=True,timeout=10,creationflags=flags,check=True)
    value=json.loads(p.stdout)
    if not value.get('ok'):raise RuntimeError(value.get('error','RPC failed'))
    return value
def identity(pid):
    p=psutil.Process(pid);return {'pid':pid,'created':p.create_time(),'exe':str(Path(p.exe()).resolve())}
def wait(fn,seconds=15):
    deadline=time.monotonic()+seconds
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (RuntimeError,subprocess.CalledProcessError):pass
        time.sleep(.1)
    raise TimeoutError('isolated upgrade readiness condition')
def start(binary):
    global active,proc
    active=binary;proc=subprocess.Popen([str(binary),'daemon',str(data)],stdout=log,stderr=log,creationflags=flags)
    wait(lambda:rpc({'cmd':'snapshot'}).get('pid')==proc.pid)
def handoff(binary):
    # Exact test-owned daemon only. service.stop would kill the test host.
    assert proc and proc.poll() is None
    proc.terminate();proc.wait(timeout=10)
    assert identity(host['pid'])==host
    start(binary)
    assert identity(host['pid'])==host
def output():
    response=rpc({'cmd':'workbench.terminal.output','workspace_id':wid,'session_id':sid,'since':0})
    return base64.b64decode(response['data'])
def marker(suffix):
    rpc({'cmd':'workbench.terminal.input','workspace_id':wid,'session_id':sid,'input_kind':'user','text':"[Console]::WriteLine('VELELLA_' + '"+suffix+"')",'enter':True})
    wait(lambda:('VELELLA_'+suffix).encode() in output())

receipt={'data':str(data),'old_runtime_sha256':hashlib.sha256(old.read_bytes()).hexdigest(),'new_runtime_sha256':hashlib.sha256(new.read_bytes()).hexdigest(),'checks':checks,'touches_installed_data':False}
try:
    for command in [['git','init',str(repo)],['git','-C',str(repo),'-c','user.name=Fixture','-c','user.email=fixture@example.test','commit','--allow-empty','-m','fixture']]:subprocess.run(command,check=True,capture_output=True,creationflags=flags)
    start(old);project=rpc({'cmd':'project.add','path':str(repo)})['project'];wid='main:'+project['id']
    sid=rpc({'cmd':'workbench.terminal.open','workspace_id':wid,'launch':'terminal','request_id':str(uuid.uuid4())})['session_id']
    session=wait(lambda:next((s for s in rpc({'cmd':'workbench.sessions','workspace_id':wid})['sessions'] if s['id']==sid and s.get('status',{}).get('pid')),None))
    host=identity(session['status']['pid']);receipt['host']=host
    shell_before=[identity(p.pid) for p in psutil.Process(host['pid']).children(recursive=True)]
    wait(lambda:b'>' in output());marker('OLD_READY')
    before=output();layout=rpc({'cmd':'workbench.layout','workspace_id':wid})['layout']
    checks.append('installed runtime fixture creates a live host and shell')
    handoff(new);assert rpc({'cmd':'sources.list'})['sources']==[];marker('NEW_READY')
    assert output().startswith(before);assert rpc({'cmd':'workbench.layout','workspace_id':wid})['layout']==layout
    assert [identity(p['pid']) for p in shell_before]==shell_before
    checks.append('new runtime reattaches old host with same PID, shell, output prefix and layout')
    newer=output();handoff(old);marker('ROLLBACK_READY')
    assert output().startswith(newer);assert rpc({'cmd':'workbench.layout','workspace_id':wid})['layout']==layout
    assert [identity(p['pid']) for p in shell_before]==shell_before
    checks.append('old runtime rollback reattaches the same still-running host and shell')
    receipt.update(status='passed',session_id=sid,history_bytes=len(output()))
except Exception as error:receipt.update(status='failed',error=str(error))
finally:
    if proc and proc.poll() is None:
        try:rpc({'cmd':'service.stop'});proc.wait(timeout=12)
        except Exception as error:receipt['cleanup_error']=str(error)
    log.close();(out/'result.json').write_text(json.dumps(receipt,indent=2),encoding='utf-8');print(json.dumps(receipt,indent=2))
if receipt['status']!='passed':raise SystemExit(1)
