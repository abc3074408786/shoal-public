import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {EventEmitter} from 'node:events';
import vm from 'node:vm';
import ts from 'typescript';
const require=createRequire(import.meta.url);
const source=readFileSync(new URL('../vite.config.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
function bridge(execFile,socketRequest=async()=>({ok:true,pid:42})){
  const module={exports:{}};
  vm.runInNewContext(compiled,{module,exports:module.exports,process:{platform:'win32',env:{LOCALAPPDATA:'C:/test-profile'}},Buffer,require:id=>{
    if(id==='vite')return {defineConfig:config=>config};
    if(id==='@vitejs/plugin-react')return {default:()=>({name:'test-react'})};
    if(id==='node:child_process')return {execFile};
    if(id==='node:module')return {createRequire:()=>()=>({socketRequest})};
    return require(id);
  }});
  let handler;
  module.exports.default.plugins.find(plugin=>plugin.name==='local-runtime-development-bridge').configureServer({middlewares:{use:(_path,middleware)=>{handler=middleware}}});
  return body=>{
    const req=new EventEmitter();Object.assign(req,{method:'POST',headers:{origin:'http://127.0.0.1:1424'},destroy(){}});
    let finished;
    const res={statusCode:200,headers:{},done:new Promise(resolve=>{finished=resolve}),setHeader(key,value){this.headers[key]=value},end(value){this.body=value;this.writableEnded=true;finished()}};
    handler(req,res);req.emit('data',body);req.emit('end');return res;
  };
}
test('synchronous endpoint lookup failure returns an error and a later request can reconnect',async()=>{
  let attempts=0;
  const request=bridge((_exe,_args,_options,done)=>{if(++attempts===1)throw Object.assign(new Error('spawn UNKNOWN'),{code:'UNKNOWN'});done(null,'"test-pipe"','')});
  let failed;
  assert.doesNotThrow(()=>{failed=request('{"cmd":"snapshot"}')});
  await failed.done;
  assert.equal(JSON.parse(failed.body).ok,false);
  assert.match(JSON.parse(failed.body).error,/spawn UNKNOWN/);
  const connected=request('{"cmd":"snapshot"}');await connected.done;
  assert.equal(JSON.parse(connected.body).pid,42);assert.equal(attempts,2);
});
test('a failed pipe mutation is not retried, avoiding duplicate task creation',async()=>{
  let attempts=0;
  const request=bridge((_exe,_args,_options,done)=>done(null,'"test-pipe"'),async()=>{attempts++;throw Error('pipe disconnected')});
  const response=request('{"cmd":"task.create"}');
  await response.done;
  assert.equal(JSON.parse(response.body).ok,false);assert.equal(attempts,1);
});
test('a disconnected browser does not receive a late RPC response',async()=>{
  let complete;
  const request=bridge((_exe,_args,_options,done)=>done(null,'"test-pipe"'),()=>new Promise(resolve=>{complete=resolve}));
  const response=request('{"cmd":"snapshot"}');
  await new Promise(setImmediate);
  response.destroyed=true;
  assert.doesNotThrow(()=>complete({ok:true}));
  await new Promise(setImmediate);
  assert.equal(response.body,undefined);
});

test('RPC payloads travel through the pipe, never endpoint lookup arguments',async()=>{
  let args,input;
  const request=bridge((_exe,values,_options,done)=>{args=values;done(null,'"test-pipe"')},async(_endpoint,value)=>{input=value;return {ok:true}});
  const response=request('{"cmd":"sources.save","token":"fixture-secret"}');await response.done;
  assert.equal(args[0],'ipc-endpoint');assert.ok(!args.join(' ').includes('fixture-secret'));assert.equal(input.token,'fixture-secret');
});

test('repeated terminal input resolves the endpoint once and uses direct pipe calls',async()=>{
  let lookups=0,calls=0;
  const request=bridge((_exe,_args,_options,done)=>{lookups++;done(null,'"test-pipe"')},async()=>{calls++;return {ok:true}});
  await Promise.all(['a','b','c'].map(text=>request(JSON.stringify({cmd:'workbench.terminal.input',text})).done));
  assert.equal(lookups,1);assert.equal(calls,3);
});
