import {useMemo,useSyncExternalStore} from 'react';
import {WORKBENCH_PREFERENCES_KEY,WORKBENCH_PREFERENCES_EVENT,parseWorkbenchPreferences} from './workbench-preferences';
const subscribe=(notify:()=>void)=>{window.addEventListener('storage',notify);window.addEventListener(WORKBENCH_PREFERENCES_EVENT,notify);return()=>{window.removeEventListener('storage',notify);window.removeEventListener(WORKBENCH_PREFERENCES_EVENT,notify)}};
export function useWorkbenchPreferences(){
  const raw=useSyncExternalStore(subscribe,()=>{try{return localStorage.getItem(WORKBENCH_PREFERENCES_KEY)||''}catch{return ''}});
  return useMemo(()=>{try{return parseWorkbenchPreferences(JSON.parse(raw))}catch{return parseWorkbenchPreferences(null)}},[raw]);
}
