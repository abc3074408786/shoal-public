"""Isolated, hidden native startup probe; does not drive the user's desktop."""
from pathlib import Path
import hashlib,json,os,subprocess,time
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/task-sources-20261018'/('native-startup-'+str(int(time.time())))
out.mkdir(parents=True);data=out/'data';data.mkdir()
gui=root/'desktop/src-tauri/target/debug/velella-desktop.exe'
runtime=root/'target/debug/velella-runtime.exe'
env=os.environ.copy();env.update(VELELLA_DATA_DIR=str(data),VELELLA_RUNTIME_PATH=str(runtime),VELELLA_DESKTOP_BACKGROUND='1')
receipt={'data_dir':str(data),'native_interaction':False,'checks':[]}
def rpc(value):
    p=subprocess.run([str(runtime),'rpc',str(data),'--stdin'],input=json.dumps(value),capture_output=True,text=True,encoding='utf-8',timeout=5,creationflags=subprocess.CREATE_NO_WINDOW)
    if p.returncode:raise RuntimeError('isolated native runtime not ready')
    result=json.loads(p.stdout)
    if not result.get('ok'):raise RuntimeError(result.get('error','RPC failed'))
    return result
proc=None;ready=False
try:
    receipt['gui_sha256']=hashlib.sha256(gui.read_bytes()).hexdigest();receipt['runtime_sha256']=hashlib.sha256(runtime.read_bytes()).hexdigest()
    proc=subprocess.Popen([str(gui)],cwd=root,env=env,creationflags=subprocess.CREATE_NO_WINDOW)
    receipt['owned_gui_pid']=proc.pid
    deadline=time.monotonic()+25
    while time.monotonic()<deadline:
        if proc.poll() is not None:raise RuntimeError('native candidate exited during startup')
        try:
            snapshot=rpc({'cmd':'workbench.snapshot'});ready=True;break
        except Exception:time.sleep(.25)
    if not ready:raise RuntimeError('native candidate did not start its isolated runtime')
    assert snapshot['projects']==[] and snapshot['tasks']==[]
    result=rpc({'cmd':'sources.list'});assert result['sources']==[] and result['links']==[]
    receipt['checks']=['hidden native GUI starts','GUI launches configured isolated runtime','task source RPC available','empty private profile; installed projects untouched']
    receipt['status']='passed'
except Exception as error:receipt['status']='failed';receipt['error']=str(error)
finally:
    # Stop only the process handle launched above. No user GUI or host is selected.
    if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=10)
    if ready:
        try:rpc({'cmd':'service.stop'})
        except Exception as error:receipt['cleanup_error']=str(error)
    (out/'result.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(receipt,ensure_ascii=False,indent=2))
if receipt['status']!='passed':raise SystemExit(1)
