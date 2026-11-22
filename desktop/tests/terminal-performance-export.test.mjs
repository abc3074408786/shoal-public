import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import helper from '../electron/terminal-performance-export.cjs';
const {sanitizeSnapshot,exportTerminalPerformance}=helper;
const snapshot=()=>({version:1,sessionId:'test-session',dropped:0,entries:[{sequence:1,atMs:100,stage:'input_ack',units:1,ms:2,failed:false}]});

test('exports a readable receipt without overwriting earlier exports',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'velella-export-'));
 const first=await exportTerminalPerformance(snapshot(),dir),second=await exportTerminalPerformance(snapshot(),dir);
 assert.notEqual(first.path,second.path);
 const value=JSON.parse(await readFile(first.path,'utf8'));
 assert.equal(value.retained,1);assert.deepEqual(value.entries,snapshot().entries);
 assert.equal((await readdir(dir)).length,2);
});
test('drops arbitrary content and never accepts a renderer filename',async()=>{
 const value=snapshot();value.command='SECRET';value.privacy='SECRET';value.path='../SECRET';value.entries[0].text='SECRET';
 assert.ok(!JSON.stringify(sanitizeSnapshot(value)).includes('SECRET'));
 assert.throws(()=>sanitizeSnapshot({...value,sessionId:'../SECRET'}));
});
test('rejects malformed or excessive records',()=>{
 for(const value of [{...snapshot(),entries:Array(513).fill(snapshot().entries[0])},{...snapshot(),dropped:-1},{...snapshot(),entries:[{sequence:1,atMs:1,stage:'SECRET'}]},{...snapshot(),entries:[{...snapshot().entries[0],ms:NaN}]}])assert.throws(()=>sanitizeSnapshot(value));
});
test('write failure rejects instead of returning a success receipt',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'velella-export-failure-')),file=join(dir,'blocked');await writeFile(file,'keep');
 await assert.rejects(exportTerminalPerformance(snapshot(),file));
 assert.equal(await readFile(file,'utf8'),'keep');
});
