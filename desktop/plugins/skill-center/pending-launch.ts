import {currentTarget,namespacedKey} from '../../src/execution-target';
const storageKey=()=>namespacedKey(currentTarget(),'velella:skill-center:pending-launch');
export function pendingLaunch():Record<string,unknown>|undefined {
  const raw=localStorage.getItem(storageKey());if(!raw)return;
  const value=JSON.parse(raw);
  if(!value||typeof value.request_id!=='string'||typeof value.workspace_id!=='string'||typeof value.task!=='string')throw Error('未决技能启动记录损坏，请先核对原工作区终端；没有创建新任务。');
  return value;
}
export function rememberLaunch(request:Record<string,unknown>){
  const previous=pendingLaunch();if(previous&&previous.request_id!==request.request_id)throw Error('还有未决启动请求，请先核对原请求。');
  localStorage.setItem(storageKey(),JSON.stringify(request));
}
export function finishLaunch(request:Record<string,unknown>){
  if(pendingLaunch()?.request_id===request.request_id)localStorage.removeItem(storageKey());
}
