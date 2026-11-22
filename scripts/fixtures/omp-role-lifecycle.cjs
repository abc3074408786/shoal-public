// Installed OMP consumes production-generated role config and extension.
// No model request: the synthetic IPC rejects every submitted prompt.
const fs=require('node:fs'),path=require('node:path'),net=require('node:net');
const assert=require('node:assert/strict'),crypto=require('node:crypto');
const {spawn,spawnSync}=require('node:child_process');
const {pathToFileURL}=require('node:url');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
if(process.argv[2]==='--mcp'){
  const root=process.argv[3];
  fs.appendFileSync(path.join(root,'mcp-started.log'),'velella\n');
  let pending='';process.stdin.on('data',part=>{pending+=part;let at;while((at=pending.indexOf('\n'))>=0){const line=pending.slice(0,at);pending=pending.slice(at+1);if(!line.trim())continue;const req=JSON.parse(line);if(req.id===undefined)continue;let result={};if(req.method==='initialize')result={protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'velella-fixture',version:'1'}};if(req.method==='tools/list')result={tools:[{name:'selected_probe',description:'Synthetic selected MCP tool',inputSchema:{type:'object',properties:{}}}]};process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:req.id,result})+'\n');}});
}else main().catch(error=>{console.error(error);process.exitCode=1;});

async function main(){
  const descriptor=path.resolve(process.argv[2]),desc=JSON.parse(fs.readFileSync(descriptor,'utf8'));
  const root=desc.directory,runtime=path.resolve(__dirname,'../../target/debug/velella-runtime.exe');
  // API contract checks for the selected-only boundary, including OMP's native
  // xd:// projection. This is separate from the installed CLI checks below.
  const savedTools=process.env.VELELLA_OMP_TOOLS;process.env.VELELLA_OMP_TOOLS=desc.spawn.env.VELELLA_OMP_TOOLS;
  const handlers=new Map(),catalog=[{name:'read',sourceInfo:{source:'builtin'}},{name:'write',sourceInfo:{source:'builtin'}},{name:'task',sourceInfo:{source:'builtin'}},{name:'mcp__velella_selected_probe',sourceInfo:{source:'mcp'}},{name:'unselected',sourceInfo:{source:'extension'}}];
  const extension=(await import(pathToFileURL(path.join(root,'role/omp-lifecycle.mjs')))).default;
  extension({on:(name,handler)=>handlers.set(name,handler),getAllTools:()=>catalog,setActiveTools:async()=>{}});
  if(savedTools===undefined)delete process.env.VELELLA_OMP_TOOLS;else process.env.VELELLA_OMP_TOOLS=savedTools;
  const checkTool=handlers.get('tool_call');
  assert.equal(await checkTool({toolName:'write',input:{path:'xd://mcp__velella_selected_probe'}}),undefined);
  for(const event of [{toolName:'write',input:{path:'source.txt'}},{toolName:'write',input:{path:'xd://task'}},{toolName:'task',input:{}},{toolName:'unselected',input:{}}])assert.equal((await checkTool(event)).block,true);
  const endpoint='velella-omp-fixture-'+crypto.randomUUID(),events=[];
  const ipc=net.createServer(socket=>{let text='';socket.on('data',part=>{text+=part;if(text.endsWith('\n')){const message=JSON.parse(text);events.push(message);socket.end(JSON.stringify({ok:message.event?.hook_event_name!=='UserPromptSubmit'})+'\n');}});});
  await new Promise((resolve,reject)=>{ipc.once('error',reject);ipc.listen('\\\\.\\pipe\\'+endpoint,resolve);});
  // Keep OS launch plumbing only; never use real account environment credentials.
  const env={};for(const key of ['SystemRoot','WINDIR','PATH','PATHEXT','TEMP','TMP','COMSPEC','LOCALAPPDATA','APPDATA'])if(process.env[key])env[key]=process.env[key];
  Object.assign(env,desc.spawn.env,{VELELLA_SESSION_ENDPOINT:endpoint,VELELLA_RUNTIME_EXE:runtime,OPENAI_API_KEY:'synthetic-never-send',OMP_MCP_STARTUP_TIMEOUT_MS:'10000'});
  const project=path.join(root,'.omp');fs.mkdirSync(project,{recursive:true});
  fs.writeFileSync(path.join(project,'mcp.json'),JSON.stringify({mcpServers:{unselected:{command:'must-not-run-unselected'}}}));
  const skills=path.join(project,'skills','unselected');fs.mkdirSync(skills,{recursive:true});fs.writeFileSync(path.join(skills,'SKILL.md'),'---\nname: unselected\ndescription: Must not load\n---\nUNSELECTED_OMP_SKILL');
  const ext=path.join(project,'extensions');fs.mkdirSync(ext,{recursive:true});fs.writeFileSync(path.join(ext,'unselected.mjs'),"throw Error('UNSELECTED_OMP_EXTENSION');");
  let child,output='',pending='',responses=[],failure;
  try{
    child=spawn(desc.spawn.program,[...desc.spawn.args,'--mode','rpc','--no-ui','--model','openai/gpt-5.4'],{cwd:root,env,windowsHide:true,stdio:['pipe','pipe','pipe']});
    child.once('error',error=>failure=error);child.stderr.on('data',part=>output+=part);
    child.stdout.on('data',part=>{output+=part;pending+=part;let at;while((at=pending.indexOf('\n'))>=0){const line=pending.slice(0,at);pending=pending.slice(at+1);try{responses.push(JSON.parse(line));}catch{}}});
    const until=async(fn,label,ms=45000)=>{const end=Date.now()+ms;while(Date.now()<end){if(failure)throw failure;if(child.exitCode!==null)throw Error('OMP exited: '+output);const value=fn();if(value)return value;await pause(50);}throw Error('Timed out '+label+': '+output);};
    await until(()=>events.some(e=>e.event?.hook_event_name==='SessionStart'),'native SessionStart');
    await until(()=>fs.existsSync(path.join(root,'mcp-started.log')),'selected MCP startup');
    child.stdin.write(JSON.stringify({id:'state',type:'get_state'})+'\n');
    const state=await until(()=>responses.find(r=>r.id==='state'),'RPC state');assert.equal(state.success,true);
    const prompt=state.data.systemPrompt.join('\n');
    // Native resolvePromptInput trims surrounding file whitespace only.
    assert.ok(prompt.includes(fs.readFileSync(path.join(root,'role/instructions.md'),'utf8').trim()),'complete selected role context');
    assert.ok(prompt.includes('xd://mcp__velella_selected_probe'),'native selected MCP route');
    assert.deepEqual(state.data.dumpTools.map(tool=>tool.name).sort(),['read','write'],'only read and required MCP routing write are projected');
    assert.equal(state.data.fastModeEnabled,false);assert.equal(state.data.fastModeActive,false);
    child.stdin.write(JSON.stringify({id:'blocked',type:'prompt',message:'This fixture input must be handled before any model request.'})+'\n');
    await until(()=>events.some(e=>e.event?.hook_event_name==='UserPromptSubmit'),'native input hook');
    const usage=events.findLast(e=>e.event?.hook_event_name==='UserPromptSubmit').event.context_usage;
    assert.ok(Number.isFinite(usage?.tokens)&&usage.tokens>0,'native extension supplies context tokens');
    assert.equal(usage.contextWindow,state.data.contextUsage.contextWindow,'native context window matches RPC');
    assert.ok(Math.abs(usage.percent-usage.tokens/usage.contextWindow*100)<0.0001,'native context percentage is consistent');
    const blocked=await until(()=>responses.find(r=>r.id==='blocked'),'blocked input response');assert.equal(blocked.data.agentInvoked,false);
    assert.equal(events.some(e=>e.event?.hook_event_name==='Stop'),false,'rejected input must not execute an agent turn');
    assert.doesNotMatch(output,/UNSELECTED_OMP_EXTENSION|must-not-run-unselected|UNSELECTED_OMP_SKILL/);
    for(const binding of JSON.parse(desc.spawn.env.VELELLA_ROLE_FILE_BINDINGS))assert.equal(crypto.createHash('sha256').update(fs.readFileSync(binding.path)).digest('hex'),binding.sha256,'launch file changed: '+binding.path);
    const result={scope:'Installed OMP RPC, production Rust role helper and real runtime hook; synthetic account and IPC, no model or authenticated workflow',root,nativeSessionStart:true,selectedMcpStarted:true,selectedContextAndMcpRoute:true,unselectedSourcesNotObserved:true,nativeInputRejection:true,launchFilesUnchanged:true,routingContractChecks:true,fastModeDisabled:true,state,events};
    fs.writeFileSync(path.join(root,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
  }finally{
    if(child?.pid&&child.exitCode===null)spawnSync('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});
    fs.writeFileSync(path.join(root,'native.log'),output);await new Promise(resolve=>ipc.close(resolve));
  }
}
