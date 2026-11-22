import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url),{openWorkspaceFolder}=require('../electron/workspace-folder.cjs');
const folder=process.platform==='win32'?'D:\\fixture\\project':'/fixture/project';
const local={kind:'local'};
function deps(workspaces=[{id:'main:p',path:folder}]){const opened=[],queries=[];return {opened,queries,runtime:{request:async q=>{queries.push(q);return {ok:true,workspaces}}},shell:{openPath:async path=>{opened.push(path);return ''}},stat:async()=>({isDirectory:()=>true})}}
test('opens only the registered directory and ignores renderer-supplied path',async()=>{const d=deps();assert.deepEqual(await openWorkspaceFolder({workspace_id:'main:p',target:local,path:'C:\\Windows\\cmd.exe'},d),{ok:true});assert.deepEqual(d.opened,[folder]);assert.deepEqual(d.queries,[{cmd:'workbench.snapshot'}])});
test('remote targets and unknown workspaces cannot reach local shell',async()=>{const d=deps();await assert.rejects(openWorkspaceFolder({workspace_id:'main:p',target:{kind:'ssh',host_id:'h',server_id:'s'}},d));assert.equal(d.queries.length,0);await assert.rejects(openWorkspaceFolder({workspace_id:'other',target:local},d));assert.equal(d.opened.length,0)});
test('registered files and relative paths are rejected; shell errors surface',async()=>{const file=deps();file.stat=async()=>({isDirectory:()=>false});await assert.rejects(openWorkspaceFolder({workspace_id:'main:p',target:local},file));assert.equal(file.opened.length,0);const relative=deps([{id:'main:p',path:'relative.exe'}]);await assert.rejects(openWorkspaceFolder({workspace_id:'main:p',target:local},relative));const denied=deps();denied.shell.openPath=async()=> 'folder unavailable';await assert.rejects(openWorkspaceFolder({workspace_id:'main:p',target:local},denied),/folder unavailable/)});
