import type {Task} from './api';
import type {Workspace,Session,Layout} from './workbench-types';

export const taskWorkspaceId=(task:Task)=>task.direct?`main:${task.project_id}`:`task:${task.id}`;
export function workspaceTask(workspace:Workspace,tasks:Task[],session?:Session){
  if(session?.task_id)return tasks.find(t=>t.id===session.task_id&&t.project_id===workspace.project_id);
  return tasks.filter(t=>taskWorkspaceId(t)===workspace.id).sort((a,b)=>Number(['queued','preparing','running','verifying'].includes(b.phase))-Number(['queued','preparing','running','verifying'].includes(a.phase))||b.created_at.localeCompare(a.created_at))[0];
}
export function initialAgentSession(layout:Layout,sessions:Session[],task?:Task){
  // A deliberately closed last tab stays closed. Polling never steals a tab.
  if(layout.panes.some(p=>p.tabs.length)||layout.closed.length)return undefined;
  return sessions.find(s=>s.kind==='agent'&&s.id===task?.agent?.id);
}
export function workspaceRows(workspaces:Workspace[],tasks:Task[]):Workspace[]{
  const result=[...workspaces];
  for(const task of tasks){const id=taskWorkspaceId(task);if(!result.some(w=>w.id===id))result.push({id,project_id:task.project_id,name:task.spec.name,path:task.worktree,kind:'task',archived:false,favorite:false,position:0,parent_workspace_id:task.direct?null:task.parent_workspace_id??null})}
  return result;
}

// Current snapshots are local. An explicit host, if present, must never be
// joined to another host; these relationships organize rows only.
type HierarchyWorkspace=Workspace & {host_id?:string|null};
const workspaceHost=(workspace:HierarchyWorkspace)=>workspace.host_id??'local';
export const workspaceIsRoot=(workspace:Workspace)=>workspace.id.startsWith('main:')||workspace.kind==='primary'||workspace.kind==='folder';
const siblingOrder=(a:Workspace,b:Workspace)=>Number(b.favorite)-Number(a.favorite)||a.position-b.position;

function workspaceHierarchy(workspaces:HierarchyWorkspace[]){
  const byId=new Map(workspaces.map(workspace=>[workspace.id,workspace]));
  const parents=new Map<string,string>();
  for(const workspace of byId.values()){
    const parent=byId.get(workspace.parent_workspace_id||'');
    if(!workspaceIsRoot(workspace)&&parent&&parent.id!==workspace.id&&workspaceHost(parent)===workspaceHost(workspace))parents.set(workspace.id,parent.id);
  }
  // Break one display edge per cycle without mutating the persisted metadata.
  // Iterative walks also keep unusually deep hierarchies off the call stack.
  const visited=new Set<string>();
  for(const workspace of byId.values()){
    const path=new Set<string>();
    let id:string|undefined=workspace.id;
    while(id&&!visited.has(id)&&!path.has(id)){path.add(id);id=parents.get(id)}
    if(id&&path.has(id))parents.delete(id);
    for(const id of path)visited.add(id);
  }
  return {byId,parents};
}

export interface WorkspaceTreeRow {
  workspace:Workspace;
  depth:number;
  parent?:Workspace;
  contextOnly:boolean;
  archivedAncestor:boolean;
}
export function workspaceTreeRows(workspaces:HierarchyWorkspace[],options:{projectId:string;projectName?:string;search?:string;includeArchived?:boolean;compare?:(a:Workspace,b:Workspace)=>number}):WorkspaceTreeRow[]{
  const {byId,parents}=workspaceHierarchy(workspaces);
  const projectRows=[...byId.values()].filter(workspace=>workspace.project_id===options.projectId);
  const query=(options.search||'').trim().toLowerCase();
  const matches=new Set(projectRows.filter(workspace=>(options.includeArchived||!workspace.archived)&&`${options.projectName||''} ${workspace.name} ${workspace.path}`.toLowerCase().includes(query)).map(workspace=>workspace.id));
  const included=new Set(matches);
  const localParent=(id:string)=>{const parent=byId.get(parents.get(id)||'');return parent?.project_id===options.projectId?parent:undefined};
  for(const id of matches){
    let parent=localParent(id);
    while(parent&&!included.has(parent.id)){included.add(parent.id);parent=localParent(parent.id)}
  }
  const children=new Map<string,Workspace[]>();
  for(const workspace of projectRows){
    if(!included.has(workspace.id))continue;
    const parentId=localParent(workspace.id)?.id||'';
    const siblings=children.get(parentId)||[];
    siblings.push(workspace);children.set(parentId,siblings);
  }
  for(const siblings of children.values())siblings.sort(options.compare||siblingOrder);
  const result:WorkspaceTreeRow[]=[],pending=(children.get('')||[]).map(workspace=>({workspace,depth:0})).reverse();
  while(pending.length){
    const {workspace,depth}=pending.pop()!;
    result.push({workspace,depth,parent:byId.get(parents.get(workspace.id)||''),contextOnly:!matches.has(workspace.id),archivedAncestor:workspace.archived&&!options.includeArchived});
    const descendants=children.get(workspace.id)||[];
    for(let i=descendants.length-1;i>=0;i--)pending.push({workspace:descendants[i],depth:depth+1});
  }
  return result;
}

export function workspaceParentCandidates(workspace:HierarchyWorkspace,workspaces:HierarchyWorkspace[]):Workspace[]{
  if(workspaceIsRoot(workspace))return [];
  const byId=new Map(workspaces.map(row=>[row.id,row]));
  return workspaces.filter(candidate=>{
    if(workspaceHost(candidate)!==workspaceHost(workspace))return false;
    const visited=new Set([workspace.id]);
    let next:HierarchyWorkspace|undefined=candidate;
    while(next){
      if(visited.has(next.id)||workspaceHost(next)!==workspaceHost(workspace))return false;
      visited.add(next.id);
      if(!next.parent_workspace_id)return true;
      next=byId.get(next.parent_workspace_id);
    }
    return false;
  }).sort(siblingOrder);
}

export function workspaceTopPosition(workspace:HierarchyWorkspace,workspaces:HierarchyWorkspace[]):number{
  const {parents}=workspaceHierarchy(workspaces);
  const siblings=workspaces.filter(row=>row.project_id===workspace.project_id&&workspaceHost(row)===workspaceHost(workspace)&&parents.get(row.id)===parents.get(workspace.id));
  return siblings.reduce((position,row)=>Math.min(position,row.position),workspace.position)-1;
}
