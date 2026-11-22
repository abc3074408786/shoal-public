"""Read a public GitHub issue, bind a real isolated worktree, and reopen it.
Never posts to an external provider and never uses the installed data directory.
"""
from pathlib import Path
import json, subprocess, time, uuid

root=Path(__file__).resolve().parents[1]
run=root/'artifacts/task-sources-20261018'/('runtime-'+str(int(time.time())))
run.mkdir(parents=True)
data=run/'data';data.mkdir()
repo=run/'repo';repo.mkdir()
exe=root/'target/debug/velella-runtime.exe'
flags=subprocess.CREATE_NO_WINDOW
def command(args,**kwargs):
    return subprocess.run(args,check=True,capture_output=True,text=True,encoding='utf-8',creationflags=flags,**kwargs)
def rpc(req):
    result=command([str(exe),'rpc',str(data),'--stdin'],input=json.dumps(req),timeout=45)
    value=json.loads(result.stdout)
    if not value.get('ok'):raise RuntimeError(value.get('error','RPC failed'))
    return value
def start():
    process=subprocess.Popen([str(exe),'daemon',str(data)],stdout=subprocess.DEVNULL,stderr=log,creationflags=flags)
    for _ in range(50):
        if process.poll() is not None:raise RuntimeError('isolated daemon exited')
        try:rpc({'cmd':'snapshot'});return process
        except Exception:time.sleep(.1)
    raise RuntimeError('isolated daemon not ready')

checks=[];proc=None;log=(run/'daemon.log').open('w',encoding='utf-8')
receipt={'data_dir':str(data),'repository':str(repo),'external_writes':0,'checks':checks}
try:
    command(['git','init',str(repo)])
    (repo/'README.md').write_text('Isolated task-source verification.\n',encoding='utf-8')
    command(['git','-C',str(repo),'add','README.md'])
    command(['git','-C',str(repo),'-c','user.name=Velella Fixture','-c','user.email=fixture@example.test','commit','-m','fixture'])
    proc=start();receipt['owned_daemon_pid']=proc.pid
    project=rpc({'cmd':'project.add','path':str(repo)})['project']
    source=rpc({'cmd':'sources.save','source':{'id':'','project_id':project['id'],'provider':'github','name':'Public GitHub smoke','base_url':'https://github.com','scope':'microsoft/terminal','login':'','jira_kind':''},'token':''})['source']
    page=rpc({'cmd':'sources.items','source_id':source['id'],'state':'open'})
    assert page['items'],'public issue list must be non-empty'
    issue=rpc({'cmd':'sources.detail','source_id':source['id'],'key':page['items'][0]['key']})['item']
    receipt['public_issue_url']=issue['url'];checks.append('live public GitHub list and detail')
    request={'cmd':'workbench.create','project_id':project['id'],'name':'Task source smoke','base':'HEAD','source_key':issue['key']}
    workspace=rpc(request)['workspace'];again=rpc(request)['workspace'];assert workspace['id']==again['id']
    assert (Path(workspace['path'])/'README.md').is_file();checks.append('real Git worktree creation and duplicate reopening')
    links=rpc({'cmd':'workbench.snapshot'})['source_links'];assert len(links)==1 and links[0]['workspace_id']==workspace['id']
    rpc({'cmd':'service.stop'});proc.wait(timeout=10);proc=start()
    links=rpc({'cmd':'workbench.snapshot'})['source_links'];assert len(links)==1 and links[0]['key']==issue['key'];checks.append('source association survives isolated daemon restart')
    rpc({'cmd':'sources.remove','source_id':source['id']})
    assert rpc({'cmd':'sources.cached','key':issue['key']})['item']['url']==issue['url'];assert len(rpc({'cmd':'workbench.snapshot'})['source_links'])==1;checks.append('disconnect keeps original task and workspace association')
    receipt['status']='passed'
except Exception as error:
    receipt['status']='failed';receipt['error']=str(error)
finally:
    if proc and proc.poll() is None:
        try:rpc({'cmd':'service.stop'});proc.wait(timeout=10)
        except Exception as error:receipt['cleanup_error']=str(error)
    log.close();(run/'result.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(receipt,ensure_ascii=False,indent=2))
if receipt['status']!='passed':raise SystemExit(1)
