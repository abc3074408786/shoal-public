import {namespacedKey,type ExecutionTarget} from './execution-target';
import {rpc} from './api';
export const ACCOUNT_PREFERENCES_KEY='velella:account-profiles:v1';
export const ACCOUNT_PREFERENCES_EVENT='velella:account-profiles';
export type AccountProvider='codex'|'claude-code';
export type AccountProfile={id:string;provider:AccountProvider;name:string;home:string};
export type AccountPreferences={profiles:AccountProfile[];defaults:Partial<Record<AccountProvider,string>>};
export function parseAccountPreferences(value:unknown):AccountPreferences{
  const v=(value&&typeof value==='object'?value:{}) as Partial<AccountPreferences>;
  const seen=new Set<string>(),profiles:AccountProfile[]=[];
  if(Array.isArray(v.profiles))for(const p of v.profiles.slice(0,32))if(p&&typeof p.id==='string'&&/^[a-zA-Z0-9-]{1,80}$/.test(p.id)&&!seen.has(p.id)&&(p.provider==='codex'||p.provider==='claude-code')&&typeof p.name==='string'&&p.name.trim()&&typeof p.home==='string'&&p.home.trim()&&!/[\x00-\x1f]/.test(p.home)){profiles.push({id:p.id,provider:p.provider,name:p.name.trim().slice(0,60),home:p.home.trim().slice(0,4096)});seen.add(p.id);}
  const defaults:AccountPreferences['defaults']={};
  for(const provider of ['codex','claude-code'] as const){const id=v.defaults?.[provider];if(typeof id==='string'&&id)defaults[provider]=id.slice(0,80);}
  return {profiles,defaults};
}
export function readAccountPreferences(target:ExecutionTarget){try{return parseAccountPreferences(JSON.parse(localStorage.getItem(namespacedKey(target,ACCOUNT_PREFERENCES_KEY))||'null'))}catch{return parseAccountPreferences(null)}}
export function defaultAccountHome(provider:string,target:ExecutionTarget):string|undefined{
  const prefs=readAccountPreferences(target),id=prefs.defaults[provider as AccountProvider];if(!id)return undefined;
  const profile=prefs.profiles.find(profile=>profile.provider===provider&&profile.id===id);if(!profile)throw Error('所选账号目录已不可用，请在设置中重新选择账号。');return profile.home;
}
export async function accountLaunchFields(home:string|undefined,target:ExecutionTarget){
  if(!home)return {};
  const protocol=await rpc({cmd:'protocol'},target);
  if(!protocol.capabilities?.includes('terminal-accounts-v1'))throw Error('当前执行主机尚不支持账号目录。请升级后台，或将账号选择恢复为“沿用 CLI 配置”。');
  return {account_home:home};
}
