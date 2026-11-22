import {useMemo,useSyncExternalStore} from 'react';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {namespacedKey} from './execution-target';
import type {ExecutionTarget} from './execution-target';
import {AGENT_PREFERENCES_KEY,AGENT_PREFERENCES_EVENT,parseAgentPreferences,type AgentPreferences} from './agent-preferences';
function subscribe(notify:()=>void){window.addEventListener('storage',notify);window.addEventListener(AGENT_PREFERENCES_EVENT,notify);return()=>{window.removeEventListener('storage',notify);window.removeEventListener(AGENT_PREFERENCES_EVENT,notify)}}
export function readAgentPreferences(target:ExecutionTarget):AgentPreferences{
  try{return parseAgentPreferences(JSON.parse(localStorage.getItem(namespacedKey(target,AGENT_PREFERENCES_KEY))||'null'))}catch{return parseAgentPreferences(null)}
}
export function useAgentPreferences(target?:ExecutionTarget){
  const active=useExecutionTarget();
  const key=namespacedKey(target||active,AGENT_PREFERENCES_KEY);
  const raw=useSyncExternalStore(subscribe,()=>{try{return localStorage.getItem(key)||''}catch{return ''}});
  const preferences=useMemo(()=>{try{return parseAgentPreferences(JSON.parse(raw))}catch{return parseAgentPreferences(null)}},[raw]);
  function save(value:AgentPreferences){localStorage.setItem(key,JSON.stringify(parseAgentPreferences(value)));window.dispatchEvent(new Event(AGENT_PREFERENCES_EVENT))}
  return {preferences,save};
}
