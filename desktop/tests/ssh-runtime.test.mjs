import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {EventEmitter} from 'node:events';
import {PassThrough} from 'node:stream';
const require=createRequire(import.meta.url);
const {createSshRuntime,sshArguments,encodeFrame,FrameDecoder,MAX_REQUEST,MAX_RESPONSE}=require('../electron/ssh-runtime.cjs');
const serverId='018fa61b-4db2-7000-8000-000000000001';
const identity={protocol:1,server_id:serverId,data_root:'/home/test/中文 space',os:'linux',runtime_version:'0.2.0',persistence:'prestarted_unverified',capabilities:['identity-v1','multiplex-v1']};
function fixture(overrides={}) {
  const child=new EventEmitter();child.stdin=new PassThrough();child.stdout=new PassThrough();child.stderr=new PassThrough();child.kills=0;
  child.kill=()=>{child.kills++;child.emit('exit',0,null);return true;};
  const received=[];new FrameDecoder(MAX_REQUEST,value=>received.push(value)).attach(child.stdin);
  const spawns=[];const runtime=createSshRuntime({host:'fixture.test',user:'tester',port:2222},{spawnProcess:(...args)=>{spawns.push(args);return child;},...overrides});
  const reply=(request,result=identity)=>child.stdout.write(encodeFrame({id:request.id,ok:true,result},MAX_RESPONSE));
  return {runtime,child,received,spawns,reply};
}
async function connected(f) {
  const hello=f.runtime.inspect({data_root:identity.data_root});f.reply(f.received.at(-1));await hello;
  const confirm=f.runtime.connect({server_id:serverId,data_root:identity.data_root,confirm:true});f.reply(f.received.at(-1));await confirm;
}
test('SSH uses fixed machine command and disables interactive trust and forwarding',()=>{
  const args=sshArguments({host:'host-alias',user:'DOMAIN\\tester',port:22,identity:'C:/keys/test key',directory:'$(never)',local_port:9000,remote_port:80});
  assert.equal(args.at(-1),'velella-runtime remote-stdio');assert.ok(args.includes('-T'));
  for(const value of ['BatchMode=yes','StrictHostKeyChecking=yes','ForwardAgent=no','ForwardX11=no','ClearAllForwardings=yes','PermitLocalCommand=no','RemoteCommand=none','ControlMaster=no','ControlPath=none']) assert.ok(args.includes(value),value);
  assert.ok(!args.includes('-L'));assert.ok(!args.join(' ').includes('$(never)'));
  for(const host of ['-option','x;touch bad','user@host','host\nnext','host/path']) assert.throws(()=>sshArguments({host}));
  assert.throws(()=>sshArguments({host:'safe',port:0}));assert.throws(()=>sshArguments({host:'safe',identity:'relative'}));
});
test('frame decoder handles split headers, UTF-8 and concatenated frames',()=>{
  const received=[],decoder=new FrameDecoder(512,value=>received.push(value));const first={id:1,text:'中文'},second={id:2};
  const bytes=Buffer.concat([encodeFrame(first,512),encodeFrame(second,512)]);
  for(const byte of bytes)decoder.push(Buffer.from([byte]));decoder.end();assert.deepEqual(received,[first,second]);
  assert.throws(()=>encodeFrame({text:'x'.repeat(512)},512),/large/);
});
test('frame decoder rejects zero, oversized, partial and non-UTF8 messages',()=>{
  for(const size of [0,513]) {const header=Buffer.alloc(4);header.writeUInt32BE(size);assert.throws(()=>new FrameDecoder(512,()=>{}).push(header));}
  const partial=new FrameDecoder(512,()=>{});partial.push(Buffer.from([0,0]));assert.throws(()=>partial.end(),/truncated/);
  const invalid=Buffer.from([0,0,0,1,255]);assert.throws(()=>new FrameDecoder(512,()=>{}).push(invalid));
});
test('inspect requires explicit confirmation, preserves remote path and binds identity',async()=>{
  const f=fixture();await assert.rejects(f.runtime.request({cmd:'workbench.snapshot'}),/connect/);
  const hello=f.runtime.inspect({data_root:identity.data_root,expected_server_id:serverId});assert.deepEqual(f.received[0],{id:1,method:'hello',protocol:1,data_root:identity.data_root});f.reply(f.received[0]);
  const result=await hello;assert.equal(result.data_root,identity.data_root);assert.equal(f.runtime.state,'awaiting_confirmation');
  await assert.rejects(f.runtime.connect({server_id:serverId,data_root:identity.data_root,confirm:false}),/confirm/);
  await assert.rejects(f.runtime.connect({server_id:serverId,data_root:'D:/local',confirm:true}),/identity/);
  const confirm=f.runtime.connect({...result,confirm:true});f.reply(f.received.at(-1));await confirm;
  const read=f.runtime.request({cmd:'workbench.snapshot'});const rpc=f.received.at(-1);assert.equal(rpc.server_id,serverId);assert.equal(rpc.data_root,identity.data_root);f.reply(rpc,{ok:true,workspaces:[]});assert.deepEqual(await read,{ok:true,workspaces:[]});
  assert.equal(f.spawns.length,1);assert.equal(f.spawns[0][0],'ssh');assert.equal(f.spawns[0][2].shell,false);f.runtime.disconnect();assert.equal(f.child.kills,1);
});
test('wrong server, protocol or connect identity closes without business requests',async()=>{
  for(const result of [{...identity,server_id:'018fa61b-4db2-7000-8000-000000000002'},{...identity,protocol:2},{...identity,os:'unknown'}]){
    const f=fixture();const pending=f.runtime.inspect({expected_server_id:serverId});f.reply(f.received[0],result);await assert.rejects(pending);assert.equal(f.runtime.state,'disconnected');assert.equal(f.received.length,1);
  }
  const f=fixture();const hello=f.runtime.inspect();f.reply(f.received[0]);await hello;const connect=f.runtime.connect({...identity,confirm:true});f.reply(f.received.at(-1),{...identity,data_root:'/other'});await assert.rejects(connect,/identity/);assert.equal(f.received.length,2);
});
test('requests multiplex out of order but duplicate responses fail closed',async()=>{
  const f=fixture();await connected(f);const one=f.runtime.request({cmd:'workbench.snapshot'}),r1=f.received.at(-1),two=f.runtime.request({cmd:'workbench.sessions'}),r2=f.received.at(-1);
  f.reply(r2,{ok:true,value:2});f.reply(r1,{ok:true,value:1});assert.equal((await one).value,1);assert.equal((await two).value,2);
  const pending=f.runtime.request({cmd:'workbench.snapshot'});f.reply(r1,{ok:true});await assert.rejects(pending,/response/);assert.equal(f.runtime.state,'disconnected');
});
test('in-flight limit rejects without queueing or losing accepted writes',async()=>{
  const f=fixture();await connected(f);const pending=Array.from({length:8},()=>f.runtime.request({cmd:'workbench.snapshot'}));const results=Promise.allSettled(pending);
  await assert.rejects(f.runtime.request({cmd:'workbench.create'}),/busy/);assert.equal(f.received.length,10);f.runtime.disconnect();assert.ok((await results).every(r=>r.status==='rejected'));assert.equal(f.spawns.length,1);
});
test('lost acknowledgement times out and never reconnects or replays',async()=>{
  const f=fixture({timeoutMs:25});await connected(f);const pending=f.runtime.request({cmd:'workbench.create',request_id:'original'});
  await assert.rejects(pending,error=>error.uncertain===true&&/not replayed/.test(error.message));assert.equal(f.received.length,3);assert.equal(f.spawns.length,1);assert.equal(f.child.kills,1);
  await assert.rejects(f.runtime.request({cmd:'workbench.create'}),/connect/);assert.equal(f.received.length,3);
});
test('disconnect rejects outstanding requests, bounds stderr and does not stop daemon',async()=>{
  const f=fixture();await connected(f);f.child.stderr.write(Buffer.alloc(100000,120));assert.ok(Buffer.byteLength(f.runtime.diagnostics())<=65536);
  const pending=f.runtime.request({cmd:'workbench.terminal.input',text:'once'});f.child.emit('exit',255,null);await assert.rejects(pending,error=>error.uncertain===true);assert.equal(f.received.length,3);assert.ok(f.received.every(r=>r.request?.cmd!=='service.stop'));
});
test('oversized request and malformed response cannot be transmitted or accepted',async()=>{
  const f=fixture();await connected(f);await assert.rejects(f.runtime.request({text:'x'.repeat(MAX_REQUEST)}),/large/);assert.equal(f.received.length,2);
  const pending=f.runtime.request({cmd:'workbench.snapshot'});f.child.stdout.write(encodeFrame({id:f.received.at(-1).id,ok:'true',result:{}},MAX_RESPONSE));await assert.rejects(pending,/response/);assert.equal(f.runtime.state,'disconnected');
});
test('late handshake completion cannot restore a closed connection',async()=>{
  for(const during of ['hello','connect']) {
    const f=fixture();const hello=f.runtime.inspect();f.reply(f.received[0]);
    let pending=hello;
    if(during==='connect'){await hello;pending=f.runtime.connect({...identity,confirm:true});f.reply(f.received.at(-1));}
    f.child.emit('exit',255,null);await assert.rejects(pending,/closed/);assert.equal(f.runtime.state,'disconnected');
  }
});
test('write backpressure rejects new requests rather than building an unbounded queue',async()=>{
  const f=fixture();await connected(f);const write=f.child.stdin.write.bind(f.child.stdin);
  f.child.stdin.write=frame=>{write(frame);return false;};
  const one=f.runtime.request({cmd:'workbench.snapshot'});f.reply(f.received.at(-1),{ok:true});await one;
  await assert.rejects(f.runtime.request({cmd:'workbench.create'}),/busy/);assert.equal(f.received.length,3);
  f.child.stdin.emit('drain');const two=f.runtime.request({cmd:'workbench.snapshot'});f.reply(f.received.at(-1),{ok:true});await two;f.runtime.disconnect();
});
test('outer protocol errors close the channel while business errors remain scoped',async()=>{
  const f=fixture();await connected(f);const business=f.runtime.request({cmd:'workbench.create'});f.reply(f.received.at(-1),{ok:false,error:'invalid base'});assert.equal((await business).ok,false);assert.equal(f.runtime.state,'connected');
  const rejected=f.runtime.request({cmd:'workbench.snapshot'});f.child.stdout.write(encodeFrame({id:f.received.at(-1).id,ok:false,error:'server identity changed'},MAX_RESPONSE));await assert.rejects(rejected,/identity/);assert.equal(f.runtime.state,'disconnected');
});
