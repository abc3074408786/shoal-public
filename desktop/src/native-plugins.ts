import {invoke} from './desktop-platform';
import {currentTarget,type ExecutionTarget} from './execution-target';
import type {ExtensionInfo} from './extensions/types';

export async function nativePluginRequest<T>(request:Record<string,unknown>,target:ExecutionTarget=currentTarget()):Promise<T>{
  if(target.kind!=='local')throw Error('本机插件只能在本机执行主机管理。');
  const response=await invoke<{ok:boolean;result:T;error?:{code:string;message:string}}>('desktop_extensions',{...request,target});
  if(!response.ok)throw Error(response.error?.message||'插件操作未完成');
  return response.result;
}
export async function listNativePlugins(target:ExecutionTarget){
  const result=await nativePluginRequest<{plugins:ExtensionInfo[]}>({action:'list'},target);
  return result.plugins.map(p=>({...p,installed:p.installed!==false}));
}
