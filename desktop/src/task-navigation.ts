import type {Project,Task} from './api';

export type TaskNavigation = {requestId:string;projectId:string;kind:'create'|'list'} | {requestId:string;projectId:string;kind:'open';taskId:string};

// A route is project-scoped; never resolve a same-named task from another project.
export function resolveTaskNavigation(route:TaskNavigation,projects:Project[],tasks:Task[]){
  if(!projects.some(project=>project.id===route.projectId))return null;
  if(route.kind!=='open')return {projectId:route.projectId,taskId:'',create:route.kind==='create'};
  const task=tasks.find(task=>task.id===route.taskId&&task.project_id===route.projectId);
  return task?{projectId:task.project_id,taskId:task.id,create:false}:null;
}

export function tasksForProject(tasks:Task[],projectId:string,query=''){
  const term=query.trim().toLowerCase();
  return tasks.filter(task=>task.project_id===projectId&&(!term||(task.spec.name+' '+task.spec.agent).toLowerCase().includes(term)))
    .sort((a,b)=>b.created_at.localeCompare(a.created_at)||a.id.localeCompare(b.id));
}

export function taskDisplayState(task:Task){
  return ['preparing','running','verifying'].includes(task.phase)&&task.status?.state==='waiting_permission'?'waiting_permission':task.phase;
}
