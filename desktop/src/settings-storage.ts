// Commit before announcing changes. Roll back a partial storage failure and never
// overwrite a preference edited by another window while this draft was open.
export type SettingsWrite={key:string;before:string|null;after:string;event:string};
export function commitSettings(writes:SettingsWrite[]){
  const changes=writes.filter(write=>write.before!==write.after);
  for(const change of changes)if(localStorage.getItem(change.key)!==change.before)throw Error('设置已在其他窗口发生变化。请取消后重新打开，再保存。');
  const applied:SettingsWrite[]=[];
  try{for(const change of changes){localStorage.setItem(change.key,change.after);applied.push(change)}}catch(error){for(const change of applied.reverse()){try{if(change.before===null)localStorage.removeItem(change.key);else localStorage.setItem(change.key,change.before)}catch{/* Preserve the original failure. */}}throw error;}
  for(const event of new Set(changes.map(change=>change.event)))window.dispatchEvent(new Event(event));
}
