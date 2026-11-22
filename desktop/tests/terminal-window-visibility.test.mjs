import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';
const {createWindowVisibility}=createRequire(import.meta.url)('../electron/window-visibility.cjs');
const flush=async()=>{for(let i=0;i<6;i++)await Promise.resolve()};
function renderer(bridge){
 const document=new EventEmitter();document.visibilityState='visible';
 document.addEventListener=document.on.bind(document);document.removeEventListener=document.removeListener.bind(document);
 const exports={};const code=ts.transpileModule(readFileSync(new URL('../src/terminal-window-visibility.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 runInNewContext(code,{exports,window:{velellaDesktop:bridge},document});
 return {subscribe:exports.subscribeTerminalWindowVisibility,document};
}
function bridgeFixture(){
 const callbacks=new Set(),pending=[];let calls=0;
 const bridge={listen(event,fn){assert.equal(event,'window-visibility');callbacks.add(fn);return()=>callbacks.delete(fn)},invoke(command,args){assert.equal(command,'window');assert.equal(args.action,'visibility');calls++;return new Promise(resolve=>pending.push(resolve))}};
 return {bridge,callbacks,pending,calls:()=>calls,emit:payload=>{for(const cb of callbacks)cb({payload})}};
}
test('native show/hide/minimize/restore reports actual state and disposes listeners',()=>{
 const win=new EventEmitter(),contents=new EventEmitter(),sent=[];let visible=false,minimized=false,dead=false;
 Object.assign(win,{webContents:contents,isVisible:()=>visible,isMinimized:()=>minimized,isDestroyed:()=>dead});
 Object.assign(contents,{isDestroyed:()=>dead,send:(event,state)=>sent.push({event,...state})});
 const observer=createWindowVisibility(win);assert.deepEqual(observer.read(),{visible:false,minimized:false});
 visible=true;win.emit('show');minimized=true;win.emit('minimize');win.emit('show');
 minimized=false;win.emit('restore');visible=false;win.emit('hide');contents.emit('did-finish-load');
 assert.deepEqual(sent.map(x=>[x.visible,x.minimized]),[[true,false],[true,true],[true,false],[false,false],[false,false]]);
 assert.ok(sent.every(x=>x.event==='window-visibility'));
 dead=true;win.emit('closed');for(const event of ['show','hide','minimize','restore','closed'])assert.equal(win.listenerCount(event),0);
 assert.equal(contents.listenerCount('did-finish-load'),0);observer.dispose();assert.deepEqual(observer.read(),{visible:false,minimized:false});
});
test('all terminal renderers share one subscription and initial hidden snapshot',async()=>{
 const f=bridgeFixture(),r=renderer(f.bridge),a=[],b=[];
 const offA=r.subscribe(x=>a.push(x)),offB=r.subscribe(x=>b.push(x));
 assert.equal(f.calls(),1);assert.equal(f.callbacks.size,1);
 f.pending[0]({visible:false,minimized:false});await flush();assert.deepEqual(a,[true,false]);assert.deepEqual(b,[true,false]);
 offA();assert.equal(f.callbacks.size,1);f.emit({visible:true,minimized:false});assert.deepEqual(a,[true,false]);assert.deepEqual(b,[true,false,true]);
 offB();offB();assert.equal(f.callbacks.size,0);
});
test('new native event wins over a stale initial snapshot; invalid states ignored',async()=>{
 const f=bridgeFixture(),r=renderer(f.bridge),seen=[];const off=r.subscribe(x=>seen.push(x));
 f.emit({visible:true,minimized:true});f.emit({visible:'true',minimized:false});
 f.pending[0]({visible:true,minimized:false});await flush();assert.deepEqual(seen,[true,false]);
 f.emit({visible:true,minimized:false});assert.deepEqual(seen,[true,false,true]);off();
});
test('old subscription snapshot cannot overwrite a new subscription',async()=>{
 const f=bridgeFixture(),r=renderer(f.bridge),old=[],next=[];
 r.subscribe(x=>old.push(x))();const off=r.subscribe(x=>next.push(x));
 f.pending[0]({visible:false,minimized:false});await flush();assert.deepEqual(next,[true]);
 f.pending[1]({visible:true,minimized:true});await flush();assert.deepEqual(next,[true,false]);off();
});
test('web fallback observes document visibility and older bridge stays drawable',()=>{
 const r=renderer(),seen=[];const off=r.subscribe(x=>seen.push(x));
 r.document.visibilityState='hidden';r.document.emit('visibilitychange');r.document.visibilityState='visible';r.document.emit('visibilitychange');
 assert.deepEqual(seen,[true,false,true]);off();assert.equal(r.document.listenerCount('visibilitychange'),0);
 const old=renderer({listen(){throw Error('Unknown desktop event')}}),values=[];old.subscribe(x=>values.push(x))();assert.deepEqual(values,[true]);
});
