import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-search.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {findTerminalText}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('Chinese selection includes both columns of each character',()=>{const cells=[{chars:'A',width:1},{chars:'中',width:2},{chars:'',width:0},{chars:'文',width:2},{chars:'',width:0}];assert.deepEqual(findTerminalText(cells,'中文'),{column:1,length:4})});
test('wide prefixes and combining sequences map to terminal columns',()=>{const cells=[{chars:'中',width:2},{chars:'',width:0},{chars:'é',width:1},{chars:'😀',width:2},{chars:'',width:0}];assert.deepEqual(findTerminalText(cells,'é😀'),{column:2,length:3})});
test('missing text is not selected',()=>{assert.equal(findTerminalText([{chars:'x',width:1}],'no'),null)});
