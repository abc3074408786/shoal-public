import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
async function load(file){const js=ts.transpileModule(readFileSync(new URL(file,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'))}
const {capScrollbarThumb}=await load('../src/terminal-scrollbar.ts');
const {ScrollbarState}=await load('../node_modules/@xterm/xterm/src/browser/scrollable/scrollbarState.ts');

test('short thumb and native drag snapshot both reach the complete history range',()=>{
 const state=new ScrollbarState(0,6,0,800,820,0),dispose=capScrollbarThumb(state);
 assert.equal(state.getSliderSize(),80);
 assert.equal(state.getSliderPosition(),0);
 assert.equal(state.clone().getDesiredScrollPositionFromDelta(720),20,'native dragging uses a clone of scrollbar state');
 state.setScrollPosition(10);assert.equal(state.getSliderPosition(),360);
 state.setScrollPosition(20);assert.equal(state.getSliderPosition()+state.getSliderSize(),800);
 assert.equal(state.getDesiredScrollPositionFromOffset(400),10,'track clicks use the short thumb center');
 dispose();assert.ok(state.getSliderSize()>80);
});
test('long histories retain their smaller proportional thumb; resizing recomputes the cap',()=>{
 const state=new ScrollbarState(0,6,0,800,32000,0),dispose=capScrollbarThumb(state);
 assert.equal(state.getSliderSize(),20);
 state.setScrollSize(1600);assert.equal(state.getSliderSize(),80);
 state.setVisibleSize(600);state.setScrollPosition(1000);assert.equal(state.getSliderPosition(),520);
 state.setScrollSize(600);assert.equal(state.isNeeded(),false);
 dispose();
});
test('arrows retain their space and unknown xterm internals fail back to native behavior',()=>{
 const state=new ScrollbarState(6,6,0,800,820,20),dispose=capScrollbarThumb(state);
 assert.equal(state.getSliderPosition()+state.getSliderSize()+state.getArrowSize()*2,800);
 dispose();assert.doesNotThrow(()=>capScrollbarThumb(undefined)());assert.doesNotThrow(()=>capScrollbarThumb({})());
});
