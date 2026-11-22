import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const {createTerminalSwitcher}=createRequire(import.meta.url)('../electron/terminal-switch.cjs');
const target={workspace_id:'w',session_id:'s',expires_at_ms:Date.now()+5000};

test('switch waits for exact renderer focus acknowledgement and permits repeated selection',async()=>{
 let sent,activations=0;
 const switcher=createTerminalSwitcher({send:r=>{sent=r},activate:()=>activations++});
 let settled=false;
 const reply=switcher.request({...target,expires_at_ms:Date.now()+5000}).then(r=>{settled=true;return r});
 await new Promise(setImmediate);
 assert.equal(settled,false);assert.equal(activations,1);
 assert.equal(switcher.complete({...sent,session_id:'other',ok:true,focused:true}),false);
 assert.equal(switcher.complete({...sent,request_id:'old',ok:true,focused:true}),false);
 const busy=await switcher.request(target);assert.equal(busy.code,'desktop_busy');
 assert.equal(switcher.complete({...sent,ok:true,focused:true}),true);
 assert.deepEqual(await reply,{ok:true,protocol:'velella-terminal-switch-v1',workspace_id:'w',session_id:'s',focused:true});
 const previous=sent.request_id;
 const second=switcher.request(target);assert.notEqual(sent.request_id,previous);
 switcher.complete({...sent,ok:true,focused:true});assert.equal((await second).ok,true);
});

test('renderer rejection and absent focus never become success',async()=>{
 let sent;
 const switcher=createTerminalSwitcher({send:r=>{sent=r},activate:()=>{}});
 const rejected=switcher.request(target);switcher.complete({...sent,ok:false,error:'dialog is open'});
 assert.equal((await rejected).error,'dialog is open');
 const unfocused=switcher.request(target);switcher.complete({...sent,ok:true});
 assert.equal((await unfocused).ok,false);
});

test('expired, invalid and timed out requests do not replay; late acknowledgements are ignored',async()=>{
 let sent,count=0;
 const switcher=createTerminalSwitcher({send:r=>{sent=r;count++},activate:()=>{}});
 assert.equal((await switcher.request({...target,expires_at_ms:0})).code,'desktop_switch_timeout');
 assert.equal((await switcher.request({...target,session_id:'\n'})).code,'invalid_target');
 assert.equal(count,0);
 const timeout=await switcher.request({...target,expires_at_ms:Date.now()+20});
 assert.equal(timeout.code,'desktop_switch_timeout');assert.equal(count,1);
 assert.equal(switcher.complete({...sent,ok:true,focused:true}),false);
});

test('closed window or send failure releases pending state without claiming focus',async()=>{
 const switcher=createTerminalSwitcher({send:()=>{},activate:()=>{}});
 const reply=switcher.request(target);switcher.dispose();
 assert.equal((await reply).code,'desktop_unavailable');
 const failed=createTerminalSwitcher({send:()=>{throw Error('renderer gone')},activate:()=>{}});
 assert.equal((await failed.request(target)).error,'renderer gone');
 assert.equal((await failed.request(target)).error,'renderer gone');
});
