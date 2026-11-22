import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-canvas-dpr.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {repairTerminalCanvasDpr}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
function fixture({width=1080,height=600,expectedWidth=1080,expectedHeight=600,dpr=1.5,cachedDpr=dpr,connected=true,throws=false}={}){
 const calls=[],canvas={width,height,isConnected:connected,ownerDocument:{defaultView:{devicePixelRatio:dpr}},getBoundingClientRect(){throw Error('Must not force layout')}};
 const renderer={_canvas:canvas,_devicePixelRatio:cachedDpr,dimensions:{device:{canvas:{width:expectedWidth,height:expectedHeight}}},handleDevicePixelRatioChange(){calls.push('dpr')},handleResize(cols,rows){calls.push(['resize',cols,rows]);if(throws)throw Error('disposed');canvas.width=expectedWidth;canvas.height=expectedHeight}};
 const term={cols:90,rows:25,_core:{_renderService:{_renderer:{value:renderer}}},refresh(...args){calls.push(['refresh',...args])}};
 return {term,calls};
}
test('repairs an undersized backing store at 150% without changing the PTY grid',()=>{
 const {term,calls}=fixture({width:720,height:400});
 assert.equal(repairTerminalCanvasDpr(term),'repaired');
 assert.deepEqual(calls,['dpr',['resize',90,25],['refresh',0,24]]);
 assert.equal(repairTerminalCanvasDpr(term),'current');
});
test('repairs an oversized backing store and stale DPR cache',()=>{
 for(const args of [{width:2160,height:1200},{cachedDpr:1}])assert.equal(repairTerminalCanvasDpr(fixture(args).term),'repaired');
});
test('only height changing is sufficient to repair',()=>{
 assert.equal(repairTerminalCanvasDpr(fixture({height:400}).term),'repaired');
});
test('matching dimensions and rounding differences do not cause repair loops',()=>{
 for(const args of [{},{width:1079},{width:1078,dpr:4}]){
  const {term,calls}=fixture(args);assert.equal(repairTerminalCanvasDpr(term),'current');assert.deepEqual(calls,[]);
 }
});
test('DOM, detached and unmeasurable renderers remain safe',()=>{
 assert.equal(repairTerminalCanvasDpr({}),'current');
 assert.equal(repairTerminalCanvasDpr(fixture({connected:false}).term),'deferred');
 assert.equal(repairTerminalCanvasDpr(fixture({expectedWidth:0}).term),'deferred');
});
test('renderer teardown defers repair without throwing',()=>{
 assert.equal(repairTerminalCanvasDpr(fixture({width:720,throws:true}).term),'deferred');
});
