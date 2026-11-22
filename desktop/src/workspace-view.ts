import type {Project,Task} from './api';
import type {Workspace} from './workbench-types';
import type {ProjectLibrary,WorkspaceView} from './project-library';

const nameOrder=(a:{name:string},b:{name:string})=>a.name.localeCompare(b.name,'zh-CN',{numeric:true,sensitivity:'base'});
export function compareWorkspaceView(order:WorkspaceView['workspaceSort']){
  return (a:Workspace,b:Workspace)=>order==='manual'?Number(b.favorite)-Number(a.favorite)||a.position-b.position:(order==='name_desc'?-1:1)*nameOrder(a,b);
}
export function sortViewProjects(projects:Project[],order:WorkspaceView['projectSort']){return order==='manual'?[...projects]:[...projects].sort((a,b)=>(order==='name_desc'?-1:1)*nameOrder(a,b))}
/** Filter before building trees, so hidden ancestors cannot reappear as search context. */
export function viewWorkspaces(workspaces:Workspace[],projects:Project[],library:ProjectLibrary,view:WorkspaceView){
  const ids=new Set(projects.map(p=>p.id));
  return workspaces.filter(w=>!w.archived&&ids.has(w.project_id)&&(!view.project||w.project_id===view.project)&&(!view.hideSleeping||!library.workspaces?.[w.id]?.sleeping));
}
export function linkedWorkspacePR(workspace:Workspace,tasks:Task[]):string|undefined{
  return tasks.filter(t=>t.project_id===workspace.project_id&&(t.direct?`main:${t.project_id}`:`task:${t.id}`)===workspace.id)
    .sort((a,b)=>b.created_at.localeCompare(a.created_at)).map(t=>t.pr_url||'').find(url=>/^https?:\/\//i.test(url));
}
export function workspaceViewGroups(rows:Workspace[],view:WorkspaceView,library:ProjectLibrary,tasks:Task[]){
  if(view.groupBy==='none')return [{id:'all',label:'所有工作区',rows}];
  if(view.groupBy==='status')return [{id:'todo',label:'待处理'},{id:'in_progress',label:'进行中'},{id:'in_review',label:'待审核'},{id:'done',label:'已完成'}].map(group=>({...group,rows:rows.filter(w=>(library.workspaces?.[w.id]?.status||'todo')===group.id)})).filter(g=>g.rows.length);
  return [{id:'linked',label:'已关联 PR',rows:rows.filter(w=>linkedWorkspacePR(w,tasks))},{id:'unknown',label:'PR 未确认',rows:rows.filter(w=>!linkedWorkspacePR(w,tasks))}].filter(g=>g.rows.length);
}
