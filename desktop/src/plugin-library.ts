import {currentTarget,namespacedKey,type ExecutionTarget} from './execution-target';
import type {McpServer} from './McpCatalogPicker';

// Library entries are references, never executable configuration or credentials.
export type PluginReference={id:string;path:string;digest:string;enabled?:boolean};
const key='velella:plugins:v1';
export function readPluginLibrary(target:ExecutionTarget=currentTarget()):PluginReference[]{
  const raw=localStorage.getItem(namespacedKey(target,key));
  if(!raw)return [];
  const value:unknown=JSON.parse(raw);
  if(!Array.isArray(value)||value.some(p=>!p||typeof p.id!=='string'||typeof p.path!=='string'||typeof p.digest!=='string'||p.enabled!==undefined&&typeof p.enabled!=='boolean'))throw Error('插件清单无法读取，请检查本机保存的数据。');
  return value.map(({id,path,digest,enabled})=>({id,path,digest,enabled:enabled!==false}));
}
export function addPluginReference(server:McpServer,target:ExecutionTarget=currentTarget()){
  if(!server.source)throw Error('请先预览并确认插件来源。');
  const {path,digest,entry_id:id}=server.source;
  const entries=readPluginLibrary(target).filter(p=>p.path!==path||p.id!==id);
  localStorage.setItem(namespacedKey(target,key),JSON.stringify([...entries,{id,path,digest,enabled:true}]));
  window.dispatchEvent(new Event('velella:plugins-changed'));
}
export function removePluginReference(plugin:PluginReference,target:ExecutionTarget=currentTarget()){
  const entries=readPluginLibrary(target).filter(p=>p.path!==plugin.path||p.id!==plugin.id);
  localStorage.setItem(namespacedKey(target,key),JSON.stringify(entries));
  window.dispatchEvent(new Event('velella:plugins-changed'));
}
export function setPluginReferenceEnabled(plugin:PluginReference,enabled:boolean,target:ExecutionTarget=currentTarget()){
  const entries=readPluginLibrary(target);if(!entries.some(p=>p.path===plugin.path&&p.id===plugin.id))throw Error('插件已从清单移除，请刷新。');
  localStorage.setItem(namespacedKey(target,key),JSON.stringify(entries.map(p=>p.path===plugin.path&&p.id===plugin.id?{...p,enabled}:p)));
  window.dispatchEvent(new Event('velella:plugins-changed'));
}
