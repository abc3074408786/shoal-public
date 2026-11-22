// Operator for the existing isolated validation run. Never creates or resumes a run.
// Each input is explicit, fenced to the observed host and input sequence, and not retried.
const fs=require('node:fs');
const path=require('node:path');
const {execFileSync}=require('node:child_process');
const {socketRequest}=require('../electron/runtime.cjs');
const dir=path.resolve(__dirname,'../../artifacts/agent-orchestration/native-5');
const own=JSON.parse(fs.readFileSync(path.join(dir,'ownership.json'),'utf8'));
const saved=JSON.parse(fs.readFileSync(path.join(dir,'run.json'),'utf8'));
const endpoint=JSON.parse(execFileSync(own.executable,['ipc-endpoint',own.data,'daemon'],{encoding:'utf8',windowsHide:true}));
const rpc=r=>socketRequest(endpoint,r);
const strip=text=>text.replace(/\x1b\][\s\S]*?(?:\x07|\x1b\\)/g,'').replace(/\x1b\[[0-?]*[ -/]*[@-~]/g,'');
(async()=>{
  const snapshot=await rpc({cmd:'orchestration.snapshot',id:saved.id});
  if(!snapshot.ok)throw Error(JSON.stringify(snapshot));
  const run=snapshot.run;
  const mode=process.argv[2]||'status';
  if(mode==='summary'){
    fs.writeFileSync(path.join(dir,'current.json'),JSON.stringify(run,null,2));
    console.log(JSON.stringify({id:run.id,state:run.state,blocker:run.blocker,revision:run.revision,members:run.status.members.map(m=>({name:m.name,state:m.state,session:m.identity.session_id})),work:run.status.work.map(w=>({id:w.id,key:w.work_key,state:w.state,attempt_count:w.attempt_count})),reviews:run.status.reviews.map(r=>({id:r.id,state:r.state,summary:r.summary}))}));return;
  }
  if(mode==='snapshot'){fs.writeFileSync(path.join(dir,'current.json'),JSON.stringify(run,null,2));console.log(JSON.stringify(run));return;}
  const sessionId=process.env.VELELLA_OPERATOR_SESSION||run.lead_session;
  if(!run.status.members.some(m=>m.identity.session_id===sessionId))throw Error('Session is not a member of the isolated run');
  const sessions=await rpc({cmd:'workbench.sessions',workspace_id:saved.wid});
  const session=sessions.sessions.find(s=>s.id===sessionId);
  if(!session)throw Error('Isolated member session not found');
  if(mode==='status'){console.log(JSON.stringify({run:run.id,state:run.state,blocker:run.blocker,session:{...session,status:{...session.status,last_event:session.status?.last_event?.hook_event_name}}}));return;}
  const base={workspace_id:saved.wid,session_id:sessionId};
  if(mode==='output'){
    const output=await rpc({cmd:'workbench.terminal.output',...base,since:0});
    console.log(strip(Buffer.from(output.data||'','base64').toString('utf8')).slice(-14000));return;
  }
  if(!['input','control'].includes(mode)||['accepted','stopped','stopping'].includes(run.state))throw Error('Mutation not allowed');
  if(session.status.exit!==null||session.status.pid!==Number(process.argv[3]))throw Error('Host changed or exited');
  let request={cmd:'workbench.terminal.'+mode,...base,expected_host_pid:Number(process.argv[3])};
  if(mode==='input'){
    const sequence=Number(process.argv[4]);
    if(session.status.input_sequence!==sequence)throw Error('Input sequence changed; inspect before acting');
    request={...request,text:Buffer.from(process.argv[5],'base64').toString('utf8'),enter:false,input_kind:'user',expected_input_sequence:sequence};
  }else request.human=false;
  const receipt=path.join(dir,'operator-'+Date.now()+'.json');
  fs.writeFileSync(receipt,JSON.stringify({request,outcome:'issued'},null,2));
  const response=await rpc(request);
  fs.writeFileSync(receipt,JSON.stringify({request,response},null,2));
  console.log(JSON.stringify({receipt,response}));
})().catch(error=>{console.error(error);process.exitCode=1});
