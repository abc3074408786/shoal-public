import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/project-library.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {parseProjectLibrary,editProject,deleteProjectGroup,commitProjectLibrary,ownsProjectLibrary,parseWorkspaceView,defaultWorkspaceView}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const roundtrip=state=>parseProjectLibrary(JSON.stringify(state));
test('workspace display settings survive reload without erasing groups or workspace state',()=>{
  const state={version:1,groups:[{id:'g',name:'Team'}],projects:{p:{group:'g',icon:'book'}},workspaces:{w:{sleeping:true,status:'done'}},view:{...defaultWorkspaceView(),groupBy:'status',project:'p',hideSleeping:true,workspaceSort:'name_desc',layout:'compact',properties:['path','status']}};
  const next=roundtrip(state);assert.deepEqual(next.view,{...state.view,properties:['status','path']});assert.deepEqual(next.groups,state.groups);assert.deepEqual(next.workspaces,state.workspaces);assert.deepEqual(roundtrip(next),next);
});
test('invalid display preferences fall back safely and empty property selection stays empty',()=>{
  const next=parseWorkspaceView({project:[],groupBy:'evil',layout:'grid',hideSleeping:'true',workspaceSort:'activity',properties:['unknown','host','host']});
  assert.equal(next.project,'');assert.equal(next.groupBy,'project');assert.equal(next.hideSleeping,false);assert.equal(next.workspaceSort,'manual');assert.deepEqual(next.properties,['host']);assert.deepEqual(parseWorkspaceView({properties:[]}).properties,[]);
});
test('display preferences remain isolated between hosts and merge with latest stored presentation',()=>{
  const values=new Map(),storage={getItem:k=>values.get(k)||null,setItem:(k,v)=>values.set(k,v)};
  let target='local';commitProjectLibrary(storage,'local',s=>({...s,view:{...defaultWorkspaceView(),hideSleeping:true}}),()=>target==='local');
  target='remote';assert.throws(()=>commitProjectLibrary(storage,'local',s=>({...s,view:defaultWorkspaceView()}),()=>target==='local'));
  commitProjectLibrary(storage,'remote',s=>({...s,view:{...defaultWorkspaceView(),groupBy:'status'}}),()=>target==='remote');
  commitProjectLibrary(storage,'remote',s=>editProject(s,'p',{icon:'book'}),()=>target==='remote');
  assert.equal(roundtrip(JSON.parse(values.get('local'))).view.hideSleeping,true);assert.equal(roundtrip(JSON.parse(values.get('remote'))).view.groupBy,'status');assert.equal(roundtrip(JSON.parse(values.get('remote'))).projects.p.icon,'book');
});
test('corrupt and unrecognized preferences preserve a usable empty library',()=>{for(const raw of [null,'oops','[]','{"version":2}','null'])assert.deepEqual(parseProjectLibrary(raw),{version:1,groups:[],projects:{}})});
test('invalid icons and orphan groups do not hide projects or execute arbitrary data',()=>{const state=parseProjectLibrary(JSON.stringify({version:1,groups:[{id:'g',name:' Tools '},{id:'g',name:'duplicate'},{id:'bad',name:' '}],projects:{p:{icon:'url(javascript:evil)',group:'missing',removed:'yes'},q:{icon:'book',group:'g',removed:true}}}));assert.deepEqual(state.groups,[{id:'g',name:'Tools'}]);assert.deepEqual(state.projects.p,{});assert.deepEqual(state.projects.q,{icon:'book',group:'g',removed:true})});
test('remove and reopen preserve project icon and grouping without touching runtime records',()=>{const original={version:1,groups:[{id:'g',name:'Projects'}],projects:{p:{icon:'rocket',group:'g'}}};const removed=roundtrip(editProject(original,'p',{removed:true}));assert.equal(removed.projects.p.removed,true);const restored=roundtrip(editProject(removed,'p',{removed:false}));assert.deepEqual(restored,original);assert.equal(original.projects.p.removed,undefined)});
test('deleting a group retains every project, icon and removal flag',()=>{const state={version:1,groups:[{id:'a',name:'A'},{id:'b',name:'B'}],projects:{p:{group:'a',icon:'book'},q:{group:'a',removed:true},r:{group:'b'}}};const next=roundtrip(deleteProjectGroup(state,'a'));assert.deepEqual(Object.keys(next.projects),['p','q','r']);assert.deepEqual(next.groups,[{id:'b',name:'B'}]);assert.deepEqual(next.projects,{p:{icon:'book'},q:{removed:true},r:{group:'b'}});assert.equal(state.projects.p.group,'a')});
test('switching hosts rejects stale project rows and stale callbacks even when IDs overlap',()=>{
  const values=new Map(),storage={getItem:key=>values.get(key)||null,setItem:(key,value)=>values.set(key,value)};
  let active='local',snapshot='local';const localCallback=()=>snapshot==='local'&&active==='local';
  commitProjectLibrary(storage,'local',s=>editProject(s,'same-id',{icon:'book'}),localCallback);
  active='remote';
  assert.throws(()=>commitProjectLibrary(storage,'local',s=>editProject(s,'same-id',{removed:true}),localCallback));
  assert.throws(()=>commitProjectLibrary(storage,'remote',s=>editProject(s,'same-id',{removed:true}),()=>snapshot===active));
  assert.equal(values.has('remote'),false);assert.deepEqual(parseProjectLibrary(values.get('local')).projects['same-id'],{icon:'book'});
  snapshot='remote';commitProjectLibrary(storage,'remote',s=>editProject(s,'same-id',{icon:'rocket'}),()=>snapshot===active);
  assert.deepEqual(parseProjectLibrary(values.get('remote')).projects['same-id'],{icon:'rocket'});
  assert.deepEqual(parseProjectLibrary(values.get('local')).projects['same-id'],{icon:'book'});
});
test('A to B to A cannot authorize a retained B callback with A snapshot provenance',()=>{
  const values=new Map(),storage={getItem:key=>values.get(key)||null,setItem:(key,value)=>values.set(key,value)};
  let current='A';
  commitProjectLibrary(storage,'A',s=>editProject(s,'same-id',{icon:'book'}),()=>ownsProjectLibrary('A','A',current));
  current='B';
  const retainedBCallback=()=>commitProjectLibrary(storage,'B',s=>editProject(s,'same-id',{removed:true}),()=>ownsProjectLibrary('A','B',current));
  assert.throws(retainedBCallback);current='A';assert.throws(retainedBCallback);
  assert.equal(values.has('B'),false);assert.deepEqual(parseProjectLibrary(values.get('A')).projects['same-id'],{icon:'book'});
  current='B';commitProjectLibrary(storage,'B',s=>editProject(s,'same-id',{icon:'rocket'}),()=>ownsProjectLibrary('B','B',current));
  current='A';assert.throws(()=>commitProjectLibrary(storage,'B',s=>editProject(s,'same-id',{removed:true}),()=>ownsProjectLibrary('B','B',current)));
  assert.deepEqual(parseProjectLibrary(values.get('B')).projects['same-id'],{icon:'rocket'});
});
test('workspace status unread and sleep survive reload independently of project grouping',()=>{const state=roundtrip({version:1,groups:[{id:'g',name:'G'}],projects:{p:{group:'g'}},workspaces:{'main:p':{status:'in_review',unread:true,sleeping:true},bad:{status:'invalid',unread:'true',sleeping:1}}});assert.deepEqual(state.workspaces['main:p'],{status:'in_review',unread:true,sleeping:true});assert.deepEqual(state.workspaces.bad,{});assert.deepEqual(roundtrip(deleteProjectGroup(state,'g')).workspaces,state.workspaces)});
