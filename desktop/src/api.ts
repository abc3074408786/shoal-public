import type {ExecutionPolicy} from './sandbox-policy';
import { invoke, isTauri } from './desktop-platform';
import {checkedTarget,currentTarget,type ExecutionTarget} from './execution-target';
export { isTauri };
export type { ExecutionTarget };
const electronBridge=()=>!!(globalThis as {velellaDesktop?:{invoke(command:string,args?:unknown):Promise<unknown>}}).velellaDesktop;
async function bridged<T>(command:string,args:Record<string,unknown>):Promise<T>{
  const result:any=await invoke(command,args);
  if(result&&result.ok===false)throw new Error(result.error||'操作未完成');
  return result;
}
export async function rpc<T = any>(request: Record<string,unknown>,target:ExecutionTarget=currentTarget()):Promise<T> {
  const bound=checkedTarget(target);
  // A remote request must never reach the local daemon: the dev bridge and the
  // Tauri shell have no machine channel, so they refuse instead of falling back.
  if(bound.kind!=='local'){
    if(!electronBridge())throw new Error('远端执行主机仅在桌面正式入口可用；开发桥不会把请求转到本机守护进程');
    return bridged<T>('rpc',{request,target:bound});
  }
  const result:any=isTauri()?await invoke('rpc',{request}):await fetch('/_velella',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(request)}).then(r=>r.json());
  if (!result.ok) throw new Error(result.error || '操作未完成');
  return result;
}
export async function executionHosts<T = any>(action:Record<string,unknown>):Promise<T>{
  if(!electronBridge())throw new Error('执行主机管理仅在桌面正式入口可用');
  return bridged<T>('execution_hosts',action);
}
export interface Project {id:string;name:string;path:string;setup:string;default_base?:string|null;setup_default?:'run'|'skip';direct?:boolean}
export interface LaunchOptions {model?:string;effort?:string}
export interface Task {parent_workspace_id?:string|null;creation_id?:string|null;setup_snapshot?:{decision:'run'|'skip';script:string;digest:string;reviewed_at?:string|null};continuity?:{auto_recover:boolean;restarts:number;resume_pending:boolean};delivery?:{commit_oid:string|null;pushed:boolean;pr_attempted:boolean;pull_request:boolean}|null;direct?:boolean;id:string;project_id:string;phase:string;reason:string;verified:boolean;created_at:string;worktree:string;branch:string;native_id:string|null;agent:{id:string}|null;command:{id:string}|null;shell:{id:string}|null;pr_url:string|null;checks:{command:string;passed:boolean|null;output:string}[];status:{state:string;hooks_ready:boolean;human_control:boolean;input_protocol?:number;exit:number|null;turns?:number;bytes?:number;last_event?:Record<string,unknown>|null}|null;spec:{name:string;agent:string;instruction:string;base:string;launch?:LaunchOptions;policy:{execution?:ExecutionPolicy;budget:{wall_clock:string}}}}
export const phases:Record<string,string>={queued:'排队中',preparing:'准备环境',running:'执行中',verifying:'验证中',review:'待审查',needs_attention:'需要处理',needs_recovery:'待恢复',cancelled:'已停止',delivered:'已交付'};
