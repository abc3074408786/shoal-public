"""Check real MiniMax TUI launch and explicit native-session recovery in an isolated daemon."""
import json
import os
import subprocess
import time
import uuid
from pathlib import Path

def main():
    assert os.name=='nt','Windows native acceptance entry'
    evidence=Path('artifacts/minimax-hybrid-20261112').resolve()
    runtime=Path('target/debug/velella-runtime.exe').resolve()
    project=evidence/'minimax-live'
    # Nested fixture repository prevents project.add from discovering D:/Velella
    # as this temporary folder's enclosing repository.
    if not (project/'.git').exists():subprocess.run(['git','init','-q',str(project)],check=True)
    events=[json.loads(l) for l in (evidence/'minimax-live-first.jsonl').read_text(encoding='utf-8-sig').splitlines() if l.startswith('{')]
    native=next(e['sessionId'] for e in events if e.get('sessionId'))
    data=evidence/('native-profile-'+str(uuid.uuid4()));data.mkdir()
    endpoint=json.loads(subprocess.check_output([str(runtime),'ipc-endpoint',str(data),'daemon'],text=True))
    log=(data/'daemon.log').open('wb')
    daemon=subprocess.Popen([str(runtime),'daemon',str(data)],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW)
    def rpc(cmd,**fields):
        with open(endpoint,'r+b',buffering=0) as pipe:
            pipe.write(json.dumps(dict(cmd=cmd,**fields)).encode()+b'\n')
            result=json.loads(pipe.readline(8*1024*1024))
        assert result.get('ok') is True,(cmd,result)
        return result
    wid=sid=None
    try:
        deadline=time.monotonic()+15
        while True:
            try:rpc('protocol');break
            except (FileNotFoundError,OSError):
                if time.monotonic()>deadline:raise
                time.sleep(.1)
        check=rpc('agent.check',agent='minimax');assert check['available'] is True
        added=rpc('project.add',path=str(project));assert Path(added['project']['path'])==project,'fixture import escaped to enclosing repository'
        wid=next(w['id'] for w in rpc('workbench.snapshot')['workspaces'] if Path(w['path'])==project)
        sid=rpc('workbench.terminal.open',workspace_id=wid,launch='minimax',request_id=str(uuid.uuid4()))['session_id']
        directory=project/'.velella/sessions'/sid
        deadline=time.monotonic()+20
        while True:
            status=json.loads((directory/'status.json').read_text())
            if status.get('input_protocol',0)>=1 and (directory/'terminal.bin').stat().st_size>200:break
            if time.monotonic()>deadline:raise TimeoutError('native MiniMax TUI not ready')
            time.sleep(.2)
        rpc('workbench.terminal.input',workspace_id=wid,session_id=sid,text='/exit',enter=True)
        time.sleep(1)
        rpc('workbench.terminal.stop',workspace_id=wid,session_id=sid,confirmed=True)
        deadline=time.monotonic()+10
        while True:
            row=next(s for s in rpc('workbench.sessions',workspace_id=wid)['sessions'] if s['id']==sid)
            if row.get('restore_selection_required'):break
            if time.monotonic()>deadline:raise TimeoutError('explicit native identity selection not exposed')
            time.sleep(.2)
        choices=rpc('workbench.terminal.restore.list',workspace_id=wid,session_id=sid)['sessions'];assert any(s['id']==native for s in choices)
        rpc('workbench.terminal.restore.select',workspace_id=wid,session_id=sid,native_id=native,confirmed=True)
        original_bytes=(directory/'terminal.bin').read_bytes()
        rpc('workbench.terminal.recover',workspace_id=wid,session_id=sid)
        launch=json.loads((directory/'launch.json').read_text());assert launch['spawn']['args'][:2]==['--session',native]
        time.sleep(3)
        assert (directory/'terminal.bin').read_bytes().startswith(original_bytes),'original recording changed'
        result={'passed':True,'profile':str(data),'workspace_id':wid,'terminal_id':sid,'native_id':native,'version':check['version'],'recording_preserved':True}
        (evidence/'minimax-native-result.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
    finally:
        if wid and sid:
            try:rpc('workbench.terminal.stop',workspace_id=wid,session_id=sid,confirmed=True)
            except Exception:pass
        daemon.terminate();daemon.wait(timeout=10);log.close()

if __name__=='__main__':main()
