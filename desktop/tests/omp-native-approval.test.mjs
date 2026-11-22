import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
const require=createRequire(import.meta.url);
const {createApprovalGuard,APPROVAL_ARM_MS,approveOnce,pendingNativeApprovals,lastApproval,nativeApprovalAllowed}=require('../../scripts/fixtures/omp-native-approval.cjs');
const message='Allow tool: write Path: xd://mcp__velella_collaboration Arguments: { "cmd": "collab.receive" }';
const marker='\x1b]7501;state=blocked:kind=permission:app=omp:msg='+Buffer.from(message).toString('base64')+'\x1b\\';
const rendered=marker+'\n╭─ Allow tool: write\nApprove\nDeny';
const status={state:'waiting_permission',exit:null,pid:99,input_sequence:3};

test('identical permission refreshes retain a fresh rendering within the current permission epoch',()=>{
 const frame='\x1b[2J\x1b[H╭─ Allow tool: write\n❯ Approve\nDeny\n';
 const raw=marker+frame+marker+marker;
 assert.equal(lastApproval(raw).rendered,true);
 assert.equal(lastApproval(marker+marker).rendered,false);
 const working='\x1b]7501;state=working:app=omp\x07';
 assert.equal(lastApproval(rendered+working+marker+marker).rendered,false);
 const other='\x1b]7501;state=blocked:kind=permission:app=omp:msg='+Buffer.from('Allow tool: bash Command: unknown').toString('base64')+'\x07';
 assert.equal(lastApproval(rendered+other+marker+marker).rendered,false);
 const g=createApprovalGuard(),input={sid:'fixture',status,raw,pendingCalls:[{id:'current',message}],allowed:m=>m===message};
 assert.equal(g.observe({...input,now:0}),null);
 assert.equal(g.observe({...input,now:170}),null);
 assert(g.observe({...input,now:APPROVAL_ARM_MS+1}));
 assert.equal(g.observe({...input,pendingCalls:[{id:'changed',message}],now:2000}),null);
 assert.equal(g.observe({...input,pendingCalls:[{id:'changed',message:'unknown'}],now:3000}),null);
});

test('full repaint before identical permission marker is fresh only with clear/home and focused choices',()=>{
 const frame='\x1b[2J\x1b[H╭─ Allow tool: write\n❯ Approve\nDeny\n';
 assert.equal(lastApproval(marker+frame+marker).rendered,true);
 assert.equal(lastApproval(frame+marker).rendered,false);
 assert.equal(lastApproval(marker+frame.replace('\x1b[2J','')+marker).rendered,false);
 const different=marker.replace(Buffer.from(message).toString('base64'),Buffer.from('Allow tool: bash Command: unknown').toString('base64'));
 assert.equal(lastApproval(different+frame+marker).rendered,false);
 assert.equal(lastApproval(marker+frame.replace('❯ Approve','Approve')+marker).rendered,false);
});

test('truncated collaboration caption requires exact approval rather than falling through JSON parsing',()=>{
 const truncated='Allow tool: write Path: xd://mcp__velella_collaboration Arguments: { "cmd": "collab.review.report", […100ch elided…]';
 assert.equal(nativeApprovalAllowed(truncated,[]),false);
 assert.equal(nativeApprovalAllowed(truncated,[truncated]),true);
 assert.equal(nativeApprovalAllowed(truncated+'changed',[truncated]),false);
 assert.equal(nativeApprovalAllowed(message,[]),true);
 assert.equal(nativeApprovalAllowed('Allow tool: bash Command: unknown',[]),false);
});
test('native approval waits for guard interval and actual rendered choices',()=>{
 const g=createApprovalGuard(),input={sid:'fixture',status,allowed:m=>m===message};
 assert.equal(g.observe({...input,raw:marker,now:0}),null);
 assert.equal(g.observe({...input,raw:rendered,now:170}),null);
 assert.equal(g.observe({...input,raw:marker,now:APPROVAL_ARM_MS}),null);
 const ticket=g.observe({...input,raw:rendered,now:APPROVAL_ARM_MS+1});assert.equal(ticket.message,message);
 g.consume(ticket);assert.equal(g.observe({...input,raw:rendered,now:2000}),null);
});
test('incremental redraw can mount a fresh dialog without repeating its choices',()=>{
 const g=createApprovalGuard(),input={sid:'fixture',status,allowed:()=>true,raw:marker+'\n╭─ Allow tool: write'};
 assert.equal(g.observe({...input,now:0}),null);assert(g.observe({...input,now:APPROVAL_ARM_MS+1}));
});
test('new dialogs, restarted hosts and changed input sequences require a new wait',()=>{
 const g=createApprovalGuard(),input={sid:'fixture',status,raw:rendered,allowed:()=>true};
 g.observe({...input,now:0});
 assert.equal(g.observe({...input,status:{...status,pid:100},now:900}),null);
 assert.equal(g.observe({...input,status:{...status,pid:100,input_sequence:4},now:1800}),null);
 assert.equal(g.observe({...input,status:{...status,pid:100,input_sequence:4},raw:rendered+'\n'+rendered,now:2700}),null);
 assert.equal(g.observe({...input,allowed:()=>false,now:4000}),null);
});
test('queued native calls all require authorization regardless of latest marker',()=>{
 const g=createApprovalGuard(),pendingCalls=[{id:'first',message:'unknown command'},{id:'second',message}];
 const input={sid:'fixture',status,raw:rendered,pendingCalls,allowed:m=>m===message};
 assert.equal(g.observe({...input,now:0}),null);assert.equal(g.observe({...input,now:2000}),null);
 assert.equal(g.observe({...input,allowed:()=>true,now:3000}),null);
 assert(g.observe({...input,allowed:()=>true,now:4000}));
 assert.equal(lastApproval(rendered+'\x1b]7501;state=busy:app=omp\x07'),null);
});
test('native history tracks concurrent calls independently until their results',()=>{
 const log=[{message:{role:'assistant',content:[{type:'toolCall',id:'first',name:'bash',arguments:{command:'git status'}},{type:'toolCall',id:'second',name:'write',arguments:{path:'xd://mcp__velella_collaboration',content:'{"cmd":"collab.receive"}'}}]}},{message:{role:'toolResult',toolCallId:'first',content:[]}}].map(v=>JSON.stringify(v)).join('\n');
 const pending=pendingNativeApprovals(log);assert.equal(pending.length,1);assert.equal(pending[0].id,'second');assert(pending[0].message.includes('collab.receive'));
});
test('a changed dialog sends no key; a stable dialog uses only approve-once',async()=>{
 const folder=fs.mkdtempSync(path.join(os.tmpdir(),'velella-approval-'));
 const statusFile=path.join(folder,'status.json'),terminalFile=path.join(folder,'terminal.bin');
 const sent=[],recorded=[],g=createApprovalGuard(),input={sid:'fixture',status,raw:rendered,allowed:()=>true};
 fs.writeFileSync(statusFile,JSON.stringify(status));fs.writeFileSync(terminalFile,rendered);
 g.observe({...input,now:0});const ticket=g.observe({...input,now:APPROVAL_ARM_MS+1});
 const args={ticket,statusFile,terminalFile,workspaceId:'fixture',request:async r=>{sent.push(r);return {ok:true};},record:r=>recorded.push(r)};
 try{
  fs.writeFileSync(statusFile,JSON.stringify({...status,pid:100}));await assert.rejects(()=>approveOnce(args),/changed/);assert.equal(sent.length,0);
  fs.writeFileSync(statusFile,JSON.stringify(status));fs.writeFileSync(terminalFile,rendered+'\n'+rendered);await assert.rejects(()=>approveOnce(args),/changed/);assert.equal(sent.length,0);
  fs.writeFileSync(terminalFile,rendered);await approveOnce(args);
  assert.equal(sent[0].text,'\x11');assert.equal(sent[0].enter,false);assert.equal(sent[0].expected_input_sequence,3);assert.equal(sent[0].expected_host_pid,99);
  assert.equal(recorded[0].state,'input_acknowledged');assert.equal(sent[1].human,false);
 }finally{assert.equal(path.dirname(path.resolve(folder)),path.resolve(os.tmpdir()));assert(path.basename(folder).startsWith('velella-approval-'));fs.rmSync(folder,{recursive:true});}
});
