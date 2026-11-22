import type {Project,Task} from './api';
import {phases} from './api';
import {invoke} from './desktop-platform';

export interface HudTask{
  id:string;name:string;project:string;
  phase:string;phase_label:string;reason:string;
  state:string;hooks_ready:boolean;human_control:boolean;
  turns:number;bytes:number;tool:string;created_at:string;
  since_changed:number;
}
export interface HudDigest{ts:number;connected:boolean;tasks:HudTask[]}

// Ordered worst-first: attention states lead, then queued work trails.
const WATCHED_PHASES=['needs_attention','needs_recovery','review','verifying','running','preparing','queued'];
const WATCHED=new Set(WATCHED_PHASES);
export const HUD_STUCK_MS=180000;

// The digest travels over the Electron-only "hud" channel; other shells and the
// dev bridge have no such channel and simply never see these calls.
export function pushHud(digest:HudDigest){
  if(!window.velellaDesktop)return;
  void invoke('hud',{action:'update',digest}).catch(()=>{});
}

// Claude/Codex PreToolUse hooks carry tool_name; anything else stays unnamed.
export function toolSummary(event:unknown):string{
  if(!event||typeof event!=='object')return '';
  const tool=(event as Record<string,unknown>).tool_name;
  return typeof tool==='string'?tool:'';
}

// Tracks which observable fields changed and when, so the HUD can tell
// "still working" (bytes/turns/phase moved) from "no progress for minutes".
export class HudDigestBuilder{
  private fingerprint=new Map<string,string>();
  private changedAt=new Map<string,number>();
  private last:{tasks:Task[];projects:Project[]}={tasks:[],projects:[]};
  build(tasks:Task[],projects:Project[],connected:boolean):HudDigest{
    this.last={tasks,projects};
    const now=Date.now(),seen=new Set<string>(),out:HudTask[]=[];
    for(const task of tasks){
      if(!WATCHED.has(task.phase))continue;
      seen.add(task.id);
      const fingerprint=[task.phase,task.reason,task.status?.state??'',task.status?.turns??0,task.status?.bytes??0].join('|');
      if(this.fingerprint.get(task.id)!==fingerprint){this.fingerprint.set(task.id,fingerprint);this.changedAt.set(task.id,now)}
      out.push({
        id:task.id,name:task.spec.name,project:projects.find(p=>p.id===task.project_id)?.name||'',
        phase:task.phase,phase_label:phases[task.phase]||task.phase,reason:task.reason,
        state:task.status?.state??'',hooks_ready:!!task.status?.hooks_ready,human_control:!!task.status?.human_control,
        turns:task.status?.turns??0,bytes:task.status?.bytes??0,
        tool:toolSummary(task.status?.last_event),created_at:task.created_at,
        since_changed:now-(this.changedAt.get(task.id)??now),
      });
    }
    for(const id of [...this.fingerprint.keys()])if(!seen.has(id)){this.fingerprint.delete(id);this.changedAt.delete(id)}
    out.sort((a,b)=>WATCHED_PHASES.indexOf(a.phase)-WATCHED_PHASES.indexOf(b.phase)||a.created_at.localeCompare(b.created_at));
    return {ts:now,connected,tasks:out};
  }
  // Poll failures keep the last known tasks on screen but drop the live mark.
  disconnected():HudDigest{return this.build(this.last.tasks,this.last.projects,false)}
}
