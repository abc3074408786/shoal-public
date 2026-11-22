// Actual runtime host + IPC + inert PTY. No CLI account or model request.
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),net=require('node:net');
const {spawn,spawnSync}=require('node:child_process'),assert=require('node:assert/strict');
const {randomUUID}=require('node:crypto');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const runtime=path.resolve(__dirname,'../../target/debug/velella-runtime.exe');
const output=path.resolve(process.argv[2]);
const root=fs.mkdtempSync(path.join(os.tmpdir(),'velella-context-host-'));
const endpoint='velella-context-'+randomUUID();
const descriptor={id:'context-fixture',endpoint,directory:root,spawn:{program:process.execPath,args:['-e','setInterval(()=>{},1000)'],cwd:root,env:{VELELLA_PROVIDER:'claude-code'},adapter:'generic'}};
const desc=path.join(root,'descriptor.json');fs.writeFileSync(desc,JSON.stringify(descriptor));
let host,log='';
const request=payload=>new Promise((resolve,reject)=>{
  const socket=net.connect('\\\\.\\pipe\\'+endpoint);let data='';
  const timer=setTimeout(()=>{socket.destroy();reject(Error('IPC timeout'));},5000);
  socket.on('connect',()=>socket.write(JSON.stringify(payload)+'\n'));
  socket.on('data',chunk=>{data+=chunk;if(data.endsWith('\n')){clearTimeout(timer);socket.end();try{resolve(JSON.parse(data));}catch(error){reject(error);}}});
  socket.on('error',error=>{clearTimeout(timer);reject(error);});
});
async function start(){
  host=spawn(runtime,['host',desc],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  host.stdout.on('data',b=>log+=b);host.stderr.on('data',b=>log+=b);
  for(let i=0;i<100;i++){if(host.exitCode!==null)throw Error('host exited '+log);try{const r=await request({cmd:'status'});if(r.ok)return;}catch{}await pause(50);}
  throw Error('host not ready '+log);
}
async function stop(){
  await request({cmd:'kill'});
  for(let i=0;i<100&&host.exitCode===null;i++)await pause(50);
  assert.notEqual(host.exitCode,null,'owned host exited');
}
async function statusline(payload){
  return new Promise((resolve,reject)=>{
    const child=spawn(runtime,['context-usage'],{env:{...process.env,VELELLA_SESSION_ENDPOINT:endpoint},windowsHide:true,stdio:['pipe','pipe','pipe']});
    let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);child.on('error',reject);
    child.on('exit',code=>resolve({code,out,err}));child.stdin.end(JSON.stringify(payload));
  });
}
(async()=>{
  try{
    await start();await request({cmd:'hook',event:{hook_event_name:'SessionStart',session_id:'native-fixture'}});
    const payload={session_id:'native-fixture',secret:'SYNTHETIC-MUST-NOT-PERSIST',context_window:{context_window_size:200000,current_usage:{input_tokens:80000,output_tokens:95000,cache_read_input_tokens:0,cache_creation_input_tokens:0}}};
    const line=await statusline(payload);assert.equal(line.code,0,line.err);assert.match(line.out,/40.0%/);
    const observed=await request({cmd:'status'});assert.equal(observed.status.context_usage.handoff_required,true);
    const before=observed.status.input_sequence;
    assert.equal((await request({cmd:'input',automated:true,text:'must-not-send',enter:true})).ok,false);
    assert.equal((await request({cmd:'status'})).status.input_sequence,before,'blocked input did not advance receipt');
    assert.equal((await request({cmd:'input',input_kind:'user',automated:false,text:'manual',enter:false})).ok,true,'user retains control');
    assert.equal((await statusline({...payload,session_id:'wrong-native'})).code,1,'wrong native session rejected');
    assert.doesNotMatch(fs.readFileSync(path.join(root,'context-usage.json'),'utf8'),/SYNTHETIC-MUST-NOT-PERSIST/);
    await stop();await start();
    assert.equal((await request({cmd:'input',automated:true,text:'after-recovery',enter:true})).ok,false,'threshold survives host recovery');
    await stop();
    fs.writeFileSync(output,JSON.stringify({scope:'actual runtime host and statusline IPC; synthetic usage, inert PTY, no CLI authentication',root,thresholdBlocksAutomaticInput:true,manualInputAllowed:true,recoveryRetainsThreshold:true,crossSessionRejected:true,rawPayloadNotPersisted:true},null,2));
    console.log('Context host IPC and recovery checks passed');
  }finally{
    if(host?.pid&&host.exitCode===null)spawnSync('taskkill.exe',['/PID',String(host.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});
    fs.writeFileSync(path.join(root,'host.log'),log);
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
