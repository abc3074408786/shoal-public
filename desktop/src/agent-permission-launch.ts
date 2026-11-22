import {rpc} from './api';
import type {ExecutionTarget} from './execution-target';
import type {AgentPermissionMode} from './agent-preferences';

export async function permissionLaunchFields(mode:AgentPermissionMode,target:ExecutionTarget):Promise<{permission_mode?:AgentPermissionMode}>{
  if(mode==='inherit')return {};
  const protocol=await rpc({cmd:'protocol'},target);
  if(!protocol.capabilities?.includes('terminal-permissions-v1'))throw new Error('当前执行主机的后台尚不支持默认权限，请升级后台或选择“沿用 CLI 设置”。');
  return {permission_mode:mode};
}
