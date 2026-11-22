const {isAbsolute}=require('node:path');
const {stat:readStat}=require('node:fs/promises');
const {requireLocalTarget}=require('./execution-hosts.cjs');

// The renderer supplies an ID, never an executable or an arbitrary path.
async function openWorkspaceFolder(args,{runtime,shell,stat=readStat}){
  requireLocalTarget(args?.target);
  if(typeof args?.workspace_id!=='string'||!args.workspace_id)throw Error('工作区标识无效');
  const snapshot=await runtime.request({cmd:'workbench.snapshot'});
  if(snapshot?.ok!==true)throw Error(snapshot?.error||'无法读取工作区');
  const workspace=snapshot.workspaces?.find(w=>w.id===args.workspace_id);
  if(!workspace||typeof workspace.path!=='string'||!isAbsolute(workspace.path))throw Error('工作区路径不可用');
  if(!(await stat(workspace.path)).isDirectory())throw Error('工作区路径不是文件夹');
  const error=await shell.openPath(workspace.path);if(error)throw Error(error);
  return {ok:true};
}
module.exports={openWorkspaceFolder};
