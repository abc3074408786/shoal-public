import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
async function load(name){
 const source=readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8');
 const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
 return import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
}
const wheel=await load('terminal-wheel-distance'),selection=await load('terminal-selection');
test('pixel wheel preserves fractional distance, resets on reversal and bounds huge inputs',()=>{
 const state=wheel.createTerminalTuiMouseWheelDistanceState();
 const move=y=>wheel.resolveTerminalTuiMouseWheelReportCount({deltaY:y,deltaMode:0},1,state,{cellHeight:20,rows:25});
 assert.equal(move(7),0);assert.equal(move(7),0);assert.equal(move(6),1);
 assert.equal(move(15),0);assert.equal(move(-5),0);assert.equal(move(-15),1);
 assert.equal(move(Infinity),0);assert.equal(move(NaN),0);assert.equal(move(-1e9),120);
});
test('line and page wheel units respect gain without an acceleration curve',()=>{
 const state=wheel.createTerminalTuiMouseWheelDistanceState();
 assert.equal(wheel.resolveTerminalTuiMouseWheelReportCount({deltaY:3,deltaMode:1},2,state),6);
 assert.equal(wheel.resolveTerminalTuiMouseWheelReportCount({deltaY:1,deltaMode:2},1,state,{rows:25}),25);
 assert.equal(wheel.normalizeTerminalTuiMouseWheelMultiplier(NaN),1);
 assert.equal(wheel.normalizeTerminalTuiMouseWheelMultiplier(100),10);
});
test('selection retains blank lines, tabs, Unicode and partial-line indentation',()=>{
 const strip=selection.stripTerminalSelectionGutter;
 for(const [input,expected] of [
  ['  你好\r\n    code\r\n \r\n  完成','你好\r\n  code\r\n\r\n完成'],
  ['\n    one\n      two\n','\none\n  two\n'],
  ['text\n  code','text\n  code'],['\tcode\n  text','\tcode\n  text'],['  \r\n ','  \r\n '],['',''],
 ])assert.equal(strip(input),expected);
});
