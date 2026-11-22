"""Real SSH/Codex acceptance in a new disposable project. Approves fixture-only requests."""
import argparse
import hashlib
import json
import os
import platform
import signal
import subprocess
import sys
import time
import uuid
from pathlib import Path

def stop_fixture(child):
    if os.name=='nt':
        subprocess.run(['taskkill','/PID',str(child.pid),'/T','/F'],capture_output=True,timeout=15)
    else:
        os.killpg(child.pid,signal.SIGTERM)
    child.wait(timeout=15)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--ssh-profile',default='agent')
    parser.add_argument('--runtime',type=Path,default=Path('target/debug/velella-runtime.exe' if os.name=='nt' else 'target/debug/velella-runtime'))
    opts=parser.parse_args()
    runtime=opts.runtime.resolve()
    base=Path('artifacts/minimax-hybrid-20261112')/('live-'+str(uuid.uuid4()))
    base.mkdir(parents=True)
    base=base.resolve();project=base/'project';project.mkdir();session=base/'session';session.mkdir()
    session_id=str(uuid.uuid4());profile={'host':opts.ssh_profile,'user':'','port':22,'identity':''}
    canonical=json.dumps(profile,sort_keys=True,separators=(',',':')).encode()
    config={'session_id':session_id,'workspace_id':'hybrid-fixture','project_root':str(project),'directory':str(session),'profile':profile,'profile_fingerprint':'sha256:'+hashlib.sha256(canonical).hexdigest()}
    (session/'hybrid.json').write_text(json.dumps(config),encoding='utf-8')
    (project/'seed.txt').write_text('LOCAL_SEED_'+session_id,encoding='utf-8')
    command_args=['-c',"from pathlib import Path; import platform; Path('local-command.txt').write_text('LOCAL_EXECUTED',encoding='utf-8'); print(platform.system())"]
    expected={'fixture-write':('local_write_file',{'path':'result.txt','content':'HYBRID_'+session_id,'expected_digest':'absent'}),'fixture-command':('local_run_command',{'program':sys.executable,'args':command_args,'cwd':'.'}),'fixture-reject':('local_write_file',{'path':'rejected.txt','content':'DENIED','expected_digest':'absent'})}
    prompt='This is an authorized isolated Velella integration test. Your native shell executes REMOTELY on Linux. The MCP tools named local_* execute on the LOCAL computer. Do exactly these steps, sequentially, and do not run other changes:\n'
    prompt+='1. Use local_read_file path seed.txt and preserve the returned marker in your final answer.\n'
    for number,(rid,(tool,fields)) in enumerate(expected.items(),2):
        prompt+=f'{number}. Call {tool} with exactly {json.dumps(dict(request_id=rid,**fields))}. Velella will approve the first two fixture mutations and reject fixture-reject; report rejection without retrying.\n'
    prompt+='5. Use your native REMOTE shell to print uname -s, pwd and /proc/meminfo first line. In your final answer include REMOTE_COMPUTE_OK only after the Linux command succeeds. Do not use a local command for this step.\n'
    prompt_file=base/'prompt.txt';prompt_file.write_text(prompt,encoding='utf-8')
    out=(base/'stdout.jsonl').open('wb');err=(base/'stderr.log').open('wb')
    child=subprocess.Popen([str(runtime),'hybrid-session',str(session),'--exec',str(prompt_file)],stdin=subprocess.DEVNULL,stdout=out,stderr=err,start_new_session=os.name!='nt')
    decisions=[];deadline=time.monotonic()+240
    try:
        while child.poll() is None and time.monotonic()<deadline:
            probe=subprocess.run([str(runtime),'hybrid-status',str(session)],capture_output=True,text=True,timeout=10)
            if probe.returncode:raise RuntimeError('local status failed: '+probe.stderr)
            for operation in json.loads(probe.stdout)['operations']:
                if operation['state']!='pending':continue
                rid=operation['id'];target=expected.get(rid);approved=False
                if target:
                    actual=dict(operation['arguments']);actual.pop('request_id',None)
                    # Explicit utf8 is equivalent to the tool's documented default.
                    if target[0]=='local_write_file' and actual.get('encoding')=='utf8':actual.pop('encoding')
                    approved=operation['tool']==target[0] and actual==target[1] and rid!='fixture-reject'
                decision=subprocess.run([str(runtime),'hybrid-decide',str(session),rid,operation['digest'],'approve' if approved else 'reject'],capture_output=True,text=True,timeout=10)
                if decision.returncode:raise RuntimeError('fixture decision failed: '+decision.stderr)
                decisions.append({'id':rid,'approved':approved})
            time.sleep(.25)
        if child.poll() is None:
            stop_fixture(child);raise RuntimeError('real Codex acceptance timed out; inspect logs before retry')
    finally:
        if child.poll() is None:stop_fixture(child)
        out.close();err.close()
    status=subprocess.run([str(runtime),'hybrid-status',str(session)],capture_output=True,text=True,check=True,timeout=10)
    audit=json.loads(status.stdout);(base/'audit.json').write_text(json.dumps(audit,indent=2),encoding='utf-8')
    result_path=project/'result.txt';command_path=project/'local-command.txt'
    passed=(child.returncode==0 and result_path.is_file() and result_path.read_text(encoding='utf-8')=='HYBRID_'+session_id and command_path.is_file() and command_path.read_text(encoding='utf-8')=='LOCAL_EXECUTED' and not (project/'rejected.txt').exists() and len(decisions)==3 and 'REMOTE_COMPUTE_OK' in (base/'stdout.jsonl').read_text(encoding='utf-8'))
    evidence={'passed':passed,'exit_code':child.returncode,'local_os':platform.system(),'session_id':session_id,'path':str(base),'decisions':decisions}
    (base/'result.json').write_text(json.dumps(evidence,indent=2),encoding='utf-8');print(json.dumps(evidence,ensure_ascii=False))
    if not passed:raise SystemExit(1)

if __name__=='__main__':main()
