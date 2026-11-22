// Real stdio/process-tree fixture. No provider, account or external service used.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawn} = require('node:child_process');
const readline = require('node:readline');
const assert = require('node:assert/strict');

if (process.argv[2] === 'server') {
  const folder = process.argv[3], mode = process.argv[4];
  const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], {stdio:'ignore', windowsHide:true});
  fs.writeFileSync(path.join(folder, 'pids.json'), JSON.stringify([process.pid, child.pid]));
  const send = value => process.stdout.write(JSON.stringify(value)+'\n');
  let listId, callId, initialized = false;
  readline.createInterface({input:process.stdin}).on('line', line => {
    const msg = JSON.parse(line);
    if (mode === 'oversize') { process.stdout.write('x'.repeat(1024*1024+1)); return; }
    if (msg.method === 'initialize') {
      assert.deepEqual(msg.params.capabilities, {roots:{}});
      send({jsonrpc:'2.0', id:msg.id, result:{protocolVersion:'2024-11-05',capabilities:{tools:{},resources:{},prompts:{}},serverInfo:{name:'fixture',version:'1'}}});
    } else if (msg.method === 'notifications/initialized') initialized = true;
    else if (msg.method === 'tools/list') listId = msg.id;
    else if (msg.method === 'tools/call') {
      assert.equal(msg.params.name, 'read'); callId = msg.id;
      send({jsonrpc:'2.0',method:'notifications/progress',params:{progressToken:'p',progress:1}});
      send({jsonrpc:'2.0',id:11,method:'roots/list'});
    } else if (msg.id === 11 && msg.result) {
      assert.deepEqual(msg.result.roots, []);
      send({jsonrpc:'2.0',id:callId,result:{content:[{type:'text',text:JSON.stringify({initialized,controlEnvPresent:Object.keys(process.env).some(k=>k.startsWith('VELELLA_'))})}]}});
      send({jsonrpc:'2.0',id:listId,result:{tools:[{name:'read',inputSchema:{type:'object'}},{name:'write',inputSchema:{type:'object'}}]}});
    } else throw new Error('unexpected request reached fixture');
  });
} else {
  main().catch(error => { console.error(error); process.exitCode=1; });
}

async function main() {
  const runtime = path.resolve(process.argv[2] || 'target/debug/velella-runtime.exe');
  const root = path.resolve(process.argv[3] || 'artifacts/orchestration-v2-20261025/external-mcp-protocol');
  fs.mkdirSync(root, {recursive:true});
  const checks=[];
  const live = pid => { try { process.kill(pid,0); return true; } catch { return false; } };
  const pause = ms => new Promise(resolve=>setTimeout(resolve,ms));
  async function stopped(folder) {
    const pids=JSON.parse(fs.readFileSync(path.join(folder,'pids.json'),'utf8'));
    for(let i=0;i<60 && pids.some(live);i++) await pause(50);
    assert.ok(pids.every(pid=>!live(pid)), 'relay must terminate its server and descendant');
  }
  function start(mode, corrupt=false) {
    const folder=path.join(root,mode+'-'+crypto.randomUUID());fs.mkdirSync(folder);
    const descriptor=path.join(folder,'descriptor.json');
    const bytes=Buffer.from(JSON.stringify({id:'fixture',command:process.execPath,args:[__filename,'server',folder,mode],tools:['read']}));
    const digest=crypto.createHash('sha256').update(bytes).digest('hex');
    fs.writeFileSync(descriptor,corrupt?Buffer.from(bytes.toString().replace('"read"','"write"')):bytes);
    const proc=spawn(runtime,['external-mcp',descriptor,digest],{stdio:['pipe','pipe','pipe'],windowsHide:true,env:{...process.env,VELELLA_FIXTURE_SENTINEL:'must-not-reach-server'}});
    let stderr='', waiter, ended=false; const queue=[];
    proc.stderr.on('data',bytes=>stderr+=bytes.toString());
    readline.createInterface({input:proc.stdout}).on('line',line=>{const msg=JSON.parse(line);if(waiter){const done=waiter;waiter=null;done(msg);}else queue.push(msg);});
    const exit=new Promise(resolve=>proc.on('exit',(code,signal)=>{ended=true;resolve({code,signal});}));
    const timer=setTimeout(()=>{if(!ended)proc.kill();},15000);
    exit.finally(()=>clearTimeout(timer));
    return {folder,proc,exit,send:msg=>proc.stdin.write(JSON.stringify(msg)+'\n'),error:()=>stderr,
      next:async()=>{if(queue.length)return queue.shift();return Promise.race([new Promise(resolve=>waiter=resolve),pause(5000).then(()=>{throw new Error('protocol fixture timeout');})]);}};
  }
  const normal=start('normal');
  try {
    normal.send({jsonrpc:'2.0',id:0,method:'initialize',params:{capabilities:{roots:{},sampling:{},elicitation:{}}}});
    const init=await normal.next();assert.deepEqual(init.result.capabilities,{tools:{}});
    normal.send({jsonrpc:'2.0',method:'notifications/initialized'});
    normal.send({jsonrpc:'2.0',id:11,method:'tools/list'});
    normal.send({jsonrpc:'2.0',id:'11',method:'tools/call',params:{name:'read',arguments:{}}});
    assert.equal((await normal.next()).method,'notifications/progress');
    const roots=await normal.next();assert.equal(roots.method,'roots/list');assert.equal(roots.id,11);
    normal.send({jsonrpc:'2.0',id:roots.id,result:{roots:[]}});
    const call=await normal.next();assert.equal(call.id,'11');assert.deepEqual(JSON.parse(call.result.content[0].text),{initialized:true,controlEnvPresent:false});
    const list=await normal.next();assert.equal(list.id,11);assert.deepEqual(list.result.tools.map(t=>t.name),['read']);
    normal.send({jsonrpc:'2.0',id:12,method:'tools/call',params:{name:'write'}});assert.equal((await normal.next()).result.isError,true);
    normal.proc.stdin.end();assert.equal((await normal.exit).code,0,normal.error());await stopped(normal.folder);
    checks.push('real duplex stdio, out-of-order typed IDs, notifications, selected tools, capability filtering, environment stripping, full process-tree cleanup');
  } finally { if(normal.proc.exitCode===null)normal.proc.kill(); }
  const oversized=start('oversize');oversized.send({jsonrpc:'2.0',id:1,method:'ping'});
  assert.notEqual((await oversized.exit).code,0);await stopped(oversized.folder);checks.push('oversized server frame rejects and cleans process tree');
  const drift=start('drift',true);assert.notEqual((await drift.exit).code,0);assert.ok(!fs.existsSync(path.join(drift.folder,'pids.json')));checks.push('descriptor drift rejected before spawning server');
  fs.writeFileSync(path.join(root,'result.json'),JSON.stringify({status:'passed',checks,scope:'real fixture subprocesses, not five-provider model acceptance',runtime_sha256:crypto.createHash('sha256').update(fs.readFileSync(runtime)).digest('hex'),at:new Date().toISOString()},null,2));
  console.log(JSON.stringify({status:'passed',checks:checks.length,result:path.join(root,'result.json')}));
}
