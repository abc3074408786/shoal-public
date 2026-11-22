// Installed OpenCode consumes the Rust-generated role and calls the actual
// runtime hook. Synthetic IPC/MCP, no model request or account credentials.
const fs=require('node:fs'),path=require('node:path'),net=require('node:net');
const assert=require('node:assert/strict'),crypto=require('node:crypto');
const {spawn,spawnSync}=require('node:child_process');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

async function main(){
  const descriptor=path.resolve(process.argv[2]);const desc=JSON.parse(fs.readFileSync(descriptor,'utf8'));
  const root=desc.directory, runtime=path.resolve(__dirname,'../../target/debug/velella-runtime.exe');
  const endpoint='velella-opencode-fixture-'+crypto.randomUUID(),events=[];
  const ipc=net.createServer(socket=>{let text='';socket.on('data',part=>{text+=part;if(text.endsWith('\n')){events.push(JSON.parse(text));socket.end('{"ok":true}\n');}});});
  await new Promise((resolve,reject)=>{ipc.once('error',reject);ipc.listen('\\\\.\\pipe\\'+endpoint,resolve);});
  const env={...process.env,...desc.spawn.env,VELELLA_SESSION_ENDPOINT:endpoint,VELELLA_RUNTIME_EXE:runtime};
  const extra=path.join(root,'.opencode','plugins');fs.mkdirSync(extra,{recursive:true});
  fs.writeFileSync(path.join(extra,'unselected.mjs'),"throw new Error('UNSELECTED_PLUGIN_LOADED');");
  fs.writeFileSync(path.join(root,'opencode.json'),JSON.stringify({mcp:{unselected:{type:'local',command:['must-not-run'],enabled:true}}}));
  let output='',child,base;
  try{
    child=spawn(desc.spawn.program,['serve','--hostname','127.0.0.1','--port','0'],{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
    child.stdout.on('data',part=>output+=part);child.stderr.on('data',part=>output+=part);
    let spawnError;child.once('error',error=>spawnError=error);
    const deadline=Date.now()+45000;
    while(!base&&Date.now()<deadline){if(spawnError)throw spawnError;if(child.exitCode!==null)throw new Error('OpenCode exited before listen: '+output);base=output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];if(!base)await pause(100);}
    assert.ok(base,'OpenCode did not listen within 45 seconds: '+output);
    const request=async(route,method='GET',body)=>{
      const res=await fetch(base+route,{method,headers:{'content-type':'application/json','x-opencode-directory':root},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(45000)});
      assert.ok(res.ok,`${route}: HTTP ${res.status}`);return res.json();
    };
    const config=await request('/config');
    assert.deepEqual(Object.keys(config.mcp),['velella']);assert.equal(config.default_agent,'velella');
    assert.equal(config.agent.velella.permission.skill['*'],'deny');assert.equal(config.agent.velella.permission.task,'deny');
    const session=await request('/session','POST',{});assert.ok(session.id);
    const eventDeadline=Date.now()+10000;
    while(!events.some(item=>item.event?.hook_event_name==='SessionStart')&&Date.now()<eventDeadline)await pause(100);
    assert.ok(events.some(item=>item.event?.hook_event_name==='SessionStart'&&item.event.session_id===session.id),'native session.created must reach the runtime IPC hook');
    await request('/session/'+session.id,'DELETE');
    const deleteDeadline=Date.now()+5000;
    while(!events.some(item=>item.event?.hook_event_name==='Interrupt')&&Date.now()<deleteDeadline)await pause(100);
    assert.ok(events.some(item=>item.event?.hook_event_name==='Interrupt'&&item.event.session_id===session.id));
    assert.doesNotMatch(output,/UNSELECTED_PLUGIN_LOADED/);
    for(const binding of JSON.parse(desc.spawn.env.VELELLA_ROLE_FILE_BINDINGS)){
      assert.equal(crypto.createHash('sha256').update(fs.readFileSync(binding.path)).digest('hex'),binding.sha256,'native startup must preserve recovery binding: '+binding.path);
    }
    const result={scope:'installed OpenCode consumes production Rust-generated config/plugin and actual runtime hook; synthetic IPC, no model/authenticated workflow',root,
      roleConfigLoaded:true,unselectedMcpExcluded:true,skillAndTaskDenied:true,nativeSessionCreatedHook:true,nativeSessionDeletedHook:true,launchFilesUnchangedAfterNativeUse:true,
      sources:[desc.spawn.program,runtime,env.OPENCODE_CONFIG,path.join(root,'role/lifecycle.mjs')].map(file=>({path:file,sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')}))};
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
  }finally{
    if(child?.pid&&child.exitCode===null){spawnSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});}
    fs.writeFileSync(path.join(root,'native.log'),output);await new Promise(resolve=>ipc.close(resolve));
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
