import type {WorkbenchPreferences} from './workbench-preferences';
type NoticeTask={id:string;phase:string;spec:{name:string}};
export class TaskNotificationTracker{
  private scope='';private initialized=false;private states=new Map<string,string>();
  reset(){this.initialized=false;this.states.clear()}
  update(scope:string,tasks:NoticeTask[],preferences:WorkbenchPreferences['notifications'],background:boolean){
    if(this.scope!==scope){this.scope=scope;this.initialized=false;this.states.clear()}
    const notices:{taskId:string;title:string;body:string}[]=[];
    for(const task of tasks){const previous=this.states.get(task.id);this.states.set(task.id,task.phase);
      if(!this.initialized||previous===undefined||previous===task.phase||!preferences.enabled||preferences.backgroundOnly&&!background)continue;
      const title=task.phase==='review'&&preferences.review?'任务等待审查':(['needs_attention','needs_recovery'].includes(task.phase)&&preferences.attention?'任务需要处理':task.phase==='delivered'&&preferences.delivered?'任务已交付':null);
      if(title)notices.push({taskId:task.id,title,body:task.spec.name.slice(0,160)});
    }
    const ids=new Set(tasks.map(t=>t.id));for(const id of this.states.keys())if(!ids.has(id))this.states.delete(id);
    this.initialized=true;return notices;
  }
}
