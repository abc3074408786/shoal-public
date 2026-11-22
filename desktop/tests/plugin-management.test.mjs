import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtemp,writeFile,readFile,mkdir,rename,rm} from 'node:fs/promises';
import {join} from 'node:path';import {tmpdir} from 'node:os';
const {createExtensions}=createRequire(import.meta.url)('../electron/extensions.cjs');
const local={kind:'local'};
async function fixture(t,call=async()=>({worked:true})){
 const profileRoot=await mkdtemp(join(tmpdir(),'velella-plugin-management-'));
 t.after(()=>rm(profileRoot,{recursive:true,force:true}));
 const options={profileRoot,plugins:[{manifest:{id:'ontology',name:'开发本体',version:'1.1.0'},create:({dataDir})=>({call:req=>call(req,dataDir)})}]};
 const host=()=>createExtensions(options);const service=host();const request=(action,extra={})=>service.request({target:local,action,pluginId:'ontology',...extra});
 return {profileRoot,host,service,request};
}
test('existing plugin state migrates; enabled state survives host restart',async t=>{
 const f=await fixture(t);await mkdir(join(f.profileRoot,'extensions'),{recursive:true});
 await writeFile(join(f.profileRoot,'extensions/state.json'),JSON.stringify({ontology:{enabled:true,dataDir:join(f.profileRoot,'old-data')}}));
 const result=await f.request('list');assert.equal(result.result.plugins[0].installed,true);assert.equal(result.result.plugins[0].enabled,true);
 assert.equal((await f.request('setEnabled',{enabled:false})).result.enabled,false);
 const next=await f.host().request({target:local,action:'list'});assert.equal(next.result.plugins[0].installed,true);assert.equal(next.result.plugins[0].enabled,false);
 assert.equal((await f.request('call',{request:{operation:'read'}})).ok,false);
});
test('uninstall requires confirmation, denies use and preserves data for reinstall',async t=>{
 const f=await fixture(t);const first=(await f.request('list')).result.plugins[0];await mkdir(first.dataDir,{recursive:true});const data=join(first.dataDir,'valuable-facts.txt');await writeFile(data,'retained facts');
 await f.request('setEnabled',{enabled:true});assert.equal((await f.request('uninstall')).ok,false);assert.equal((await f.request('list')).result.plugins[0].enabled,true);
 const removed=await f.request('uninstall',{confirmed:true});assert.equal(removed.ok,true);assert.equal(removed.result.installed,false);assert.equal(removed.result.enabled,false);
 assert.equal((await f.request('setEnabled',{enabled:true})).ok,false);assert.equal((await f.request('call',{request:{operation:'read'}})).ok,false);
 const restarted=f.host();assert.equal((await restarted.request({target:local,action:'list'})).result.plugins[0].installed,false);
 const installed=await restarted.request({target:local,action:'install',pluginId:'ontology'});assert.equal(installed.result.installed,true);assert.equal(installed.result.enabled,false);assert.equal(installed.result.dataDir,first.dataDir);assert.equal(await readFile(data,'utf8'),'retained facts');
 assert.equal((await restarted.request({target:local,action:'setEnabled',pluginId:'ontology',enabled:true})).ok,true);
 assert.equal((await restarted.request({target:local,action:'call',pluginId:'ontology',request:{operation:'read'}})).result.worked,true);
});
test('uninstall drains in-flight calls and rejects new calls while changing state',async t=>{
 let release,started;const didStart=new Promise(r=>started=r);const f=await fixture(t,()=>{started();return new Promise(r=>release=r)});
 await f.request('setEnabled',{enabled:true});const call=f.request('call',{request:{operation:'wait'}});await didStart;
 let finished=false;const uninstall=f.request('uninstall',{confirmed:true}).then(r=>{finished=true;return r});
 await new Promise(r=>setImmediate(r));assert.equal(finished,false);assert.equal((await f.request('call',{request:{operation:'new'}})).ok,false);
 release({done:true});assert.equal((await call).result.done,true);assert.equal((await uninstall).result.installed,false);
});
test('failed uninstall publication retains installed/enabled state and usable instance',async t=>{
 const f=await fixture(t);await f.request('setEnabled',{enabled:true});const config=join(f.profileRoot,'extensions/state.json');await rename(config,config+'.retained');await mkdir(config);
 assert.equal((await f.request('uninstall',{confirmed:true})).ok,false);const state=(await f.request('list')).result.plugins[0];assert.equal(state.installed,true);assert.equal(state.enabled,true);assert.equal((await f.request('call',{request:{operation:'read'}})).ok,true);
});
test('remote callers and unknown plugin ids cannot install or uninstall',async t=>{
 const f=await fixture(t);assert.equal((await f.service.request({target:{kind:'ssh'},action:'install',pluginId:'ontology'})).ok,false);
 assert.equal((await f.request('install',{pluginId:'not-bundled'})).ok,false);
});
