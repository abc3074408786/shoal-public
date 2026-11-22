// Actual Velella hook binary -> fake owned IPC -> installed Gemini hook consumer.
// No real session, daemon, credentials, trust approval, or model request.
const fs=require('node:fs'),path=require('node:path'),net=require('node:net');
const crypto=require('node:crypto'),assert=require('node:assert/strict');
const {spawn}=require('node:child_process');
const {pathToFileURL}=require('node:url');

async function main(){
  const root=path.resolve(__dirname,'../../artifacts/orchestration-v2-20261025/native-skill-hook');fs.mkdirSync(root,{recursive:true});
  const attempt=fs.mkdtempSync(path.join(root,'probe-'));
  const home=path.join(attempt,'home');fs.mkdirSync(home);
  process.env.HOME=home;process.env.USERPROFILE=home;process.env.GEMINI_CLI_HOME=home;
  const bundle=process.env.FIXTURE_GEMINI_BUNDLE||'C:/home/zpq/.npm-global/node_modules/@google/.gemini-cli-eEbuV9on/bundle';
  const core=path.join(bundle,'chunk-JW7ZLS43.js');
  const {GeminiClient,createHookOutput}=await import(pathToFileURL(core).href);
  const runtime=path.resolve(__dirname,'../../target/debug/velella-runtime.exe');
  const instruction=path.join(attempt,'instructions.md');const body='Selected skill body <div> & \\u003c T<Type>\nResource base: '+attempt+'\nRead references/rules.txt';fs.writeFileSync(instruction,body);
  const endpoint='velella-skill-fixture-'+crypto.randomUUID();const received=[];let ipcAccept=true;
  const server=net.createServer(socket=>{let text='';socket.on('data',chunk=>{text+=chunk;if(text.endsWith('\n')){received.push(JSON.parse(text));socket.end(JSON.stringify({ok:ipcAccept})+'\n');}});});
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen('\\\\.\\pipe\\'+endpoint,resolve);});
  const env={...process.env,VELELLA_SESSION_ENDPOINT:endpoint,VELELLA_PROVIDER:'gemini',
    VELELLA_SKILL_POLICY:'selected_snapshot_context_v1',VELELLA_SKILL_BINDINGS:'[]',VELELLA_ROLE_FILE:instruction,
    VELELLA_ROLE_INSTRUCTION_SHA256:crypto.createHash('sha256').update(body).digest('hex')};
  const run=event=>new Promise((resolve,reject)=>{
    const child=spawn(runtime,['orchestration-hook',event],{env,cwd:attempt,windowsHide:true,stdio:['pipe','pipe','pipe']});
    let stdout='',stderr='';const timeout=setTimeout(()=>{child.kill();reject(new Error('owned hook exceeded 5 seconds'));},5000);
    child.stdout.on('data',x=>stdout+=x);child.stderr.on('data',x=>stderr+=x);
    child.once('error',error=>{clearTimeout(timeout);reject(error);});child.once('exit',code=>{clearTimeout(timeout);resolve({code,stdout,stderr});});
    child.stdin.end('{"session_id":"synthetic-session"}\n');
  });
  try{
    const client={hookStateMap:new Map(),config:{getHookSystem:()=>({fireBeforeAgentEvent:async()=>{
      const result=await run('UserPromptSubmit');assert.equal(result.code,0,result.stderr);
      return createHookOutput('BeforeAgent',JSON.parse(result.stdout));
    }})}};
    for(const id of ['startup-task','resumed-task','after-clear-task']){
      const result=await GeminiClient.prototype.fireBeforeAgentHookSafe.call(client,'synthetic task',id);
      assert.equal(JSON.parse(result.additionalContext).velella_role_context,body);
    }
    assert.equal(received.length,3);assert.ok(received.every(event=>event.cmd==='hook'&&event.event.hook_event_name==='UserPromptSubmit'));
    assert.equal(await GeminiClient.prototype.fireBeforeAgentHookSafe.call(client,'tool continuation','after-clear-task'),undefined);
    assert.equal(received.length,3,'native hook must not repeat for a tool continuation');
    fs.writeFileSync(instruction,'tampered');
    const blocked=await GeminiClient.prototype.fireBeforeAgentHookSafe.call(client,'tampered context','tampered-task');
    assert.equal(blocked.type,'agent_execution_blocked');assert.equal(received.length,3,'invalid context must not report lifecycle readiness');
    fs.writeFileSync(instruction,body);ipcAccept=false;
    const rejectedIPC=await GeminiClient.prototype.fireBeforeAgentHookSafe.call(client,'rejected receipt','ipc-error-task');assert.equal(rejectedIPC.type,'agent_execution_blocked');
    env.VELELLA_SESSION_ENDPOINT='velella-nonexistent-'+crypto.randomUUID();
    const disconnected=await GeminiClient.prototype.fireBeforeAgentHookSafe.call(client,'no session','disconnected-task');assert.equal(disconnected.type,'agent_execution_blocked');
    const result={scope:'real runtime hook and installed native consumer; fake IPC and task IDs, no model/TUI workflow',
      taskContextAccepted:true,nativeDeduplication:true,tamperedContextBlockedBeforeIPC:true,ipcFailureBlocksTask:true,markupRoundTripsExactly:true,
      sources:[runtime,core].map(file=>({path:file,sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')}))};
    fs.writeFileSync(path.join(attempt,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({attempt,...result},null,2));
  }finally{await new Promise(resolve=>server.close(resolve));}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
