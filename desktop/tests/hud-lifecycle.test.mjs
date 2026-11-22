import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {createRequire} from 'node:module';
import {readFileSync,mkdtempSync,rmSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {tmpdir} from 'node:os';
import vm from 'node:vm';
const file=new URL('../electron/hud.cjs',import.meta.url),require=createRequire(file);

function fixture(t){
 const prefix=join(tmpdir(),'velella-hud-'),root=mkdtempSync(prefix),windows=[];
 let quitting=false;
 class FakeWindow extends EventEmitter{
  constructor(options){super();this.options=options;this.bounds={x:options.x??50,y:options.y??50,width:options.width,height:options.height};this.visible=false;this.dead=false;this.shows=0;this.loads=[];this.sent=[];this.webContents=new EventEmitter();this.webContents.send=(...args)=>this.sent.push(args);this.webContents.setWindowOpenHandler=handler=>{this.openHandler=handler};windows.push(this)}
  setAlwaysOnTop(){} isDestroyed(){return this.dead} isVisible(){return this.visible}
  showInactive(){this.visible=true;this.shows++} hide(){this.visible=false}
  getPosition(){return [this.bounds.x,this.bounds.y]} getSize(){return [this.bounds.width,this.bounds.height]}
  setPosition(x,y){Object.assign(this.bounds,{x,y});this.emit('moved')}
  setBounds(bounds){this.bounds={...bounds}}
  loadURL(url){this.loads.push(url);return new Promise((resolve,reject)=>{this.loaded=resolve;this.failed=reject})}
  destroy(){this.dead=true;this.visible=false;this.emit('closed')}
  close(){const event={prevented:false,preventDefault(){this.prevented=true}};this.emit('close',event);if(!event.prevented)this.destroy();return event}
 }
 const module={exports:{}};
 vm.runInNewContext(readFileSync(file,'utf8'),{module,require:id=>id==='electron'?{BrowserWindow:FakeWindow,screen:{getDisplayNearestPoint:()=>({workArea:{x:0,y:0,width:1920,height:1080}})}}:require(id),__dirname:resolve(new URL('../electron',import.meta.url).pathname.replace(/^\/(.:\/)/,'$1')),setTimeout,clearTimeout},{filename:'hud.cjs'});
 const create=()=>module.exports.createHud({root,preload:'preload.cjs',isQuitting:()=>quitting});
 t.after(()=>{quitting=true;for(const win of windows)if(!win.dead)win.close();assert.ok(resolve(root).startsWith(resolve(prefix)));rmSync(root,{recursive:true,force:true})});
 return {create,windows,root,quit:()=>{quitting=true}};
}

test('startup, digest updates, hide, flush and window lookup allocate no HUD window',t=>{
 const f=fixture(t),hud=f.create();hud.setDigest({version:1});hud.setDigest({version:2});hud.flush();hud.hide();
 assert.equal(hud.window,null);assert.equal(f.windows.length,0);
});
test('first show loads one local page and delivers the latest digest; repeated show shares the load',async t=>{
 const f=fixture(t),hud=f.create();hud.setDigest({version:1});const first=hud.show(),second=hud.show(),win=f.windows[0];
 assert.equal(f.windows.length,1);assert.equal(win.loads.length,1);assert.match(win.loads[0],/\/dist\/hud\.html$/);assert.equal(win.visible,false);
 hud.setDigest({version:2});hud.flush();assert.equal(win.sent.length,0);win.loaded();await Promise.all([first,second]);
 assert.equal(win.visible,true);assert.equal(win.sent.at(-1)[1].version,2);hud.hide();await hud.show();assert.equal(f.windows.length,1);assert.equal(win.loads.length,1);
});
test('hide during loading cancels reveal without automatic reopening',async t=>{
 const f=fixture(t),hud=f.create(),opening=hud.show(),win=f.windows[0];hud.hide();win.loaded();await opening;
 assert.equal(win.visible,false);assert.equal(win.shows,0);await hud.toggle();assert.equal(win.visible,true);
});
test('failed load disposes its window; only explicit later opening retries',async t=>{
 const f=fixture(t),hud=f.create(),opening=hud.show(),win=f.windows[0];const rejected=assert.rejects(opening,/blocked/);win.failed(Error('blocked'));await rejected;
 assert.equal(win.dead,true);assert.equal(hud.window,null);assert.equal(f.windows.length,1);hud.setDigest({version:3});assert.equal(f.windows.length,1);
 const retry=hud.show(),next=f.windows[1];next.loaded();await retry;assert.equal(next.sent.at(-1)[1].version,3);
});
test('sandbox, navigation refusal, height bounds, saved position and hide/quit semantics remain',async t=>{
 const f=fixture(t),hud=f.create(),opening=hud.show(),win=f.windows[0];win.loaded();await opening;
 assert.equal(win.options.focusable,false);assert.equal(win.options.show,false);assert.equal(win.options.webPreferences.sandbox,true);assert.equal(win.options.webPreferences.contextIsolation,true);assert.equal(win.options.webPreferences.nodeIntegration,false);
 assert.equal(win.openHandler().action,'deny');let blocked=false;win.webContents.emit('will-navigate',{preventDefault(){blocked=true}});assert.equal(blocked,true);
 hud.resize(99999);assert.equal(win.getSize()[1],600);hud.resize(-100);assert.equal(win.getSize()[1],120);win.setPosition(300,200);
 const close=win.close();assert.equal(close.prevented,true);assert.equal(win.visible,false);assert.equal(win.dead,false);await hud.show();assert.deepEqual(win.getPosition(),[300,200]);
 f.quit();assert.equal(win.close().prevented,false);assert.equal(hud.window,null);
 const saved=JSON.parse(readFileSync(join(f.root,'hud-window.json'),'utf8'));assert.deepEqual([saved.x,saved.y,saved.height],[300,200,120]);
 const fresh=f.create(),reopen=fresh.show(),next=f.windows.at(-1);next.loaded();await reopen;assert.deepEqual(next.getPosition(),[300,200]);
});
