const fs=require('node:fs');const path=require('node:path');
const {execFileSync}=require('node:child_process');
const {socketRequest}=require('../electron/runtime.cjs');
const dir=path.resolve(__dirname,'../../artifacts/agent-orchestration/native-5');
const own=JSON.parse(fs.readFileSync(path.join(dir,'ownership.json'),'utf8'));
const saved=JSON.parse(fs.readFileSync(path.join(dir,'run.json'),'utf8'));
const phase=process.argv[2];if(!['before','after'].includes(phase))throw Error('before or after required');
const endpoint=JSON.parse(execFileSync(own.executable,['ipc-endpoint',own.data,'daemon'],{encoding:'utf8',windowsHide:true}));
(async()=>{
  const r=await socketRequest(endpoint,{cmd:'orchestration.snapshot',id:saved.id});
  const s=await socketRequest(endpoint,{cmd:'workbench.sessions',workspace_id:saved.wid});
  const evidence={run:r.run.id,state:r.run.state,revision:r.run.revision,work:r.run.status.work.map(w=>({id:w.id,state:w.state,attempt:w.attempt_id,count:w.attempt_count})),members:r.run.status.members.map(m=>m.id),sessions:s.sessions.map(s=>({id:s.id,pid:s.status?.pid,input_sequence:s.status?.input_sequence,turns:s.status?.turns,exit:s.status?.exit}))};
  fs.writeFileSync(path.join(dir,'restart-'+phase+'.json'),JSON.stringify(evidence,null,2));
  if(phase==='after')require('node:assert/strict').deepEqual(evidence,JSON.parse(fs.readFileSync(path.join(dir,'restart-before.json'),'utf8')));
  console.log(JSON.stringify({phase,passed:true,evidence}));
})().catch(e=>{console.error(e);process.exitCode=1});
