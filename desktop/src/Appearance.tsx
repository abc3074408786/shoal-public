import {createContext,ReactNode,useContext,useEffect,useState} from 'react';
import {parseTheme,readPreference,Theme,writePreference} from './ui-preferences';
import {WORKBENCH_PREFERENCES_EVENT} from './workbench-preferences';
import {useWorkbenchPreferences} from './useWorkbenchPreferences';
const Appearance=createContext<{theme:Theme;setTheme:(theme:Theme)=>void}>({theme:'dark',setTheme:()=>{}});
export function AppearanceProvider({children}:{children:ReactNode}){
  const [theme,setTheme]=useState(()=>parseTheme(readPreference('velella:ui:theme','dark')));
  const preferences=useWorkbenchPreferences();
  useEffect(()=>{const refresh=()=>setTheme(parseTheme(readPreference('velella:ui:theme','dark')));window.addEventListener(WORKBENCH_PREFERENCES_EVENT,refresh);window.addEventListener('storage',refresh);return()=>{window.removeEventListener(WORKBENCH_PREFERENCES_EVENT,refresh);window.removeEventListener('storage',refresh)}},[]);
  useEffect(()=>{document.documentElement.style.zoom=String(preferences.uiScale/100)},[preferences.uiScale]);
  useEffect(()=>{document.documentElement.dataset.theme=theme;document.documentElement.style.colorScheme=theme;writePreference('velella:ui:theme',theme)},[theme]);
  return <Appearance.Provider value={{theme,setTheme}}>{children}</Appearance.Provider>;
}
export const useAppearance=()=>useContext(Appearance);
export function useViewportWidth(){const [width,setWidth]=useState(window.innerWidth);useEffect(()=>{const update=()=>setWidth(window.innerWidth);window.addEventListener('resize',update);return()=>window.removeEventListener('resize',update)},[]);return width}
export function usePreference<T>(key:string,fallback:T){
  const [value,setValue]=useState<T>(()=>readPreference(key,fallback));
  useEffect(()=>writePreference(key,value),[key,value]);
  return [value,setValue] as const;
}
