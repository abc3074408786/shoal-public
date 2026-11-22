import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
async function moduleAt(name,transform=s=>s){const source=transform(readFileSync(new URL('../src/'+name,import.meta.url),'utf8'));const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));}
const prefs=await moduleAt('workbench-preferences.ts');
const {TaskNotificationTracker}=await moduleAt('task-notifications.ts');
const {commitSettings}=await moduleAt('settings-storage.ts');
const accounts=await moduleAt('account-preferences.ts',source=>source.replace("import {namespacedKey,type ExecutionTarget} from './execution-target';",'const namespacedKey=(target,key)=>target.kind===\'local\'?key:target.host_id+\':\'+key;').replace("import {rpc} from './api';",'let handler;export function useRpc(value){handler=value}const rpc=(...args)=>handler(...args);'));
function storage(){const data=new Map();let failKey;global.localStorage={getItem:key=>data.get(key)??null,setItem:(key,value)=>{if(key===failKey)throw Error('quota');data.set(key,value)},removeItem:key=>data.delete(key)};global.window=new EventTarget();return {data,fail:key=>failKey=key};}
test('safe defaults preserve manual saves and opt-in notifications; malformed values stay bounded',()=>{
  const p=prefs.parseWorkbenchPreferences({uiScale:999,editor:{fontSize:-1,autoSave:'yes',autoSaveDelay:20},notifications:{enabled:'yes'}});
  assert.equal(p.uiScale,130);assert.equal(p.editor.fontSize,10);assert.equal(p.editor.autoSave,false);assert.equal(p.editor.autoSaveDelay,500);assert.equal(p.notifications.enabled,false);
});
test('custom shortcuts match exactly on each platform and reject collisions against defaults',()=>{
  const p=prefs.parseWorkbenchPreferences({shortcuts:{settings:'Ctrl+Alt+Y'}});
  const key={key:'y',ctrlKey:true,altKey:true,metaKey:false,shiftKey:false};
  assert.equal(prefs.shortcutMatches('settings',key,p,false),true);assert.equal(prefs.shortcutMatches('settings',{...key,shiftKey:true},p,false),false);
  assert.equal(prefs.shortcutMatches('saveFile',{...key,key:'s',ctrlKey:false,altKey:false,metaKey:true},p,true),true);
  assert.ok(prefs.shortcutConflicts({settings:'Ctrl+Shift+T'}).length);
  assert.equal(prefs.validShortcut('Ctrl+Shift+Y'),true);assert.equal(prefs.validShortcut('Shift+Y'),false);assert.equal(prefs.reservedShortcut('Ctrl+C'),true);
});
test('a failed multi-key save restores preceding writes and announces no changes',()=>{
  const s=storage();s.data.set('old','before');s.fail('blocked');let events=0;window.addEventListener('changed',()=>events++);
  assert.throws(()=>commitSettings([{key:'old',before:'before',after:'after',event:'changed'},{key:'blocked',before:null,after:'value',event:'changed'}]),/quota/);
  assert.equal(localStorage.getItem('old'),'before');assert.equal(localStorage.getItem('blocked'),null);assert.equal(events,0);
});
test('save rejects a stale window before writing anything, then emits one event after a valid commit',()=>{
  const s=storage();s.data.set('one','external');let events=0;window.addEventListener('changed',()=>events++);
  assert.throws(()=>commitSettings([{key:'one',before:'old',after:'new',event:'changed'}]),/其他窗口/);assert.equal(localStorage.getItem('one'),'external');
  commitSettings([{key:'one',before:'external',after:'new',event:'changed'},{key:'two',before:null,after:'second',event:'changed'}]);assert.equal(events,1);assert.equal(localStorage.getItem('two'),'second');
});
const task=(id,phase)=>({id,phase,spec:{name:'Task '+id}});
test('task notifications suppress history, repeats, new historical rows, foreground transitions and reconnects',()=>{
  const tracker=new TaskNotificationTracker(),p={...prefs.defaultWorkbenchPreferences.notifications,enabled:true};
  assert.deepEqual(tracker.update('local',[task('a','running'),task('b','review')],p,true),[]);
  assert.equal(tracker.update('local',[task('a','review'),task('b','review')],p,true).length,1);
  assert.equal(tracker.update('local',[task('a','review'),task('b','review'),task('c','review')],p,true).length,0);
  assert.equal(tracker.update('local',[task('a','needs_attention')],p,false).length,0);
  assert.equal(tracker.update('local',[task('a','needs_attention')],p,true).length,0);
  tracker.reset();assert.equal(tracker.update('local',[task('a','review')],p,true).length,0);
  assert.equal(tracker.update('remote',[task('a','needs_attention')],p,true).length,0);
});
test('directory selection never silently falls back when a saved profile becomes unavailable',()=>{
  storage();const target={kind:'local'};localStorage.setItem(accounts.ACCOUNT_PREFERENCES_KEY,JSON.stringify({profiles:[],defaults:{codex:'missing'}}));
  assert.throws(()=>accounts.defaultAccountHome('codex',target),/已不可用/);
  assert.equal(accounts.defaultAccountHome('claude-code',target),undefined);
});
test('custom account launches require support from the exact host; inherited accounts do not probe',async()=>{
  const target={kind:'ssh',host_id:'host'};
  accounts.useRpc(()=>{throw Error('unexpected')});assert.deepEqual(await accounts.accountLaunchFields(undefined,target),{});
  accounts.useRpc(async(_req,bound)=>{assert.equal(bound,target);return {capabilities:[]}});await assert.rejects(accounts.accountLaunchFields('D:/accounts/work',target),/尚不支持/);
  accounts.useRpc(async(_req,bound)=>{assert.equal(bound,target);return {capabilities:['terminal-accounts-v1']}});assert.deepEqual(await accounts.accountLaunchFields('D:/accounts/work',target),{account_home:'D:/accounts/work'});
});
