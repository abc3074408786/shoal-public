export type Theme='dark'|'light';
export function parseTheme(value:unknown):Theme {return value==='light'?'light':'dark'}
export function clampPanelWidth(value:number,min:number,max:number){return Math.min(max,Math.max(min,Number.isFinite(value)?value:min))}
export function readPreference<T>(key:string,fallback:T):T {
  try {const raw=localStorage.getItem(key);if(raw===null)return fallback;const value=JSON.parse(raw);return typeof value===typeof fallback&&(typeof value!=='number'||Number.isFinite(value))?value:fallback} catch {return fallback}
}
export function writePreference(key:string,value:unknown){try{localStorage.setItem(key,JSON.stringify(value))}catch{/* Display preferences remain usable without storage. */}}
