import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtempSync,rmSync,readFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
const {BrowserHost}=createRequire(import.meta.url)('../electron/browser.cjs');

function fixture(conflict=false) {
  const root=mkdtempSync(join(tmpdir(),'velella-browser-forget-'));
  let closed=false,layout={revision:5,closed:[],panes:[
    {id:'main',active:'owned-tab',tabs:[{id:'shell',kind:'terminal',target:'s1'},{id:'owned-tab',kind:'browser',target:'owned'}]},
    {id:'right',active:'other-tab',tabs:[{id:'duplicate',kind:'browser',target:'owned'},{id:'other-tab',kind:'browser',target:'other'}]},
  ]};
  const calls=[],events=[];
  const runtime={root,async request(request){
    calls.push(request);
    if(request.cmd==='workbench.snapshot')return {ok:true,workspaces:[{id:'w',archived:false}]};
    if(request.cmd==='workbench.layout')return {ok:true,layout:structuredClone(layout)};
    if(request.cmd==='workbench.layout.save'){
      if(conflict)return {ok:false,error:'layout_conflict'};
      assert.equal(request.layout.revision,layout.revision);
      layout={...structuredClone(request.layout),revision:layout.revision+1};return {ok:true,layout:structuredClone(layout)};
    }
    throw Error('Unexpected request '+request.cmd);
  }};
  const host=new BrowserHost({contentView:{removeChildView(){}},webContents:{send:(channel,value)=>events.push({channel,value})}},runtime);
  host.pages.set('owned',{id:'owned',workspace_id:'w',url:'about:blank',history:[],ready:true,view:{webContents:{close(){closed=true;}}}});
  host.pages.set('other',{id:'other',workspace_id:'w',url:'about:blank',history:[],ready:false,view:null});
  host.frames=[{id:'owned',workspace_id:'w',x:0,y:0,width:100,height:100}];host.save();
  return {host,calls,events,get layout(){return layout;},get closed(){return closed;},root,dispose(){rmSync(root,{recursive:true,force:true});}};
}

test('forget removes only owned browser tabs and publishes the saved layout to the real workbench channel',async()=>{
  const f=fixture();try{
    await f.host.command({op:'forget',workspace_id:'w',id:'owned'});
    assert.deepEqual(f.layout.panes.map(p=>p.tabs.map(t=>t.id)),[['shell'],['other-tab']]);
    assert.equal(f.layout.panes[0].active,'shell');assert.equal(f.layout.panes[1].active,'other-tab');
    assert.equal(f.closed,true);assert.equal(f.host.pages.has('owned'),false);assert.equal(f.host.pages.has('other'),true);
    assert.deepEqual(JSON.parse(readFileSync(join(f.root,'browser-pages.json'),'utf8')).map(p=>p.id),['other']);
    assert.equal(f.events.length,1);assert.equal(f.events[0].channel,'velella:browser-open');
    assert.deepEqual(f.events[0].value.layout,f.layout);assert.equal(f.events[0].value.workspace_id,'w');
    assert.deepEqual(f.host.frames,[]);
  }finally{f.dispose();}
});

test('a layout conflict rejects forget before closing the view or removing metadata',async()=>{
  const f=fixture(true);try{
    await assert.rejects(f.host.command({op:'forget',workspace_id:'w',id:'owned'}),/layout_conflict/);
    assert.equal(f.closed,false);assert.equal(f.host.pages.has('owned'),true);assert.equal(f.events.length,0);
    assert.equal(JSON.parse(readFileSync(join(f.root,'browser-pages.json'),'utf8')).length,2);
  }finally{f.dispose();}
});

test('forget also removes closed-history-only entries without changing other recovery history',async()=>{
  const f=fixture();try{
    for(const pane of f.layout.panes){pane.tabs=pane.tabs.filter(t=>t.target!=='owned');if(pane.active==='owned-tab')pane.active='shell';}
    f.layout.closed=[{id:'closed-owned',kind:'browser',target:'owned'},{id:'closed-shell',kind:'terminal',target:'s2'},{id:'closed-other',kind:'browser',target:'other'}];
    await f.host.command({op:'forget',workspace_id:'w',id:'owned'});
    assert.equal(f.layout.revision,6);
    assert.deepEqual(f.layout.closed.map(t=>t.id),['closed-shell','closed-other']);
    assert.deepEqual(f.events[0].value.layout.closed,f.layout.closed);
    assert.equal(f.host.pages.has('owned'),false);
  }finally{f.dispose();}
});

test('close retains saved page and layout so the view can be reopened',async()=>{
  const f=fixture();try{
    await f.host.command({op:'close',workspace_id:'w',id:'owned'});
    assert.equal(f.closed,true);assert.equal(f.host.pages.has('owned'),true);assert.equal(f.layout.revision,5);
    assert.equal(f.calls.some(c=>c.cmd==='workbench.layout.save'),false);assert.equal(f.events.length,0);
  }finally{f.dispose();}
});
