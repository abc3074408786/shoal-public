// Project/workspace selection belongs to WorkspaceApp, independently of page history.
export type ShellLocation={page:'home'|'project'|'orchestration'|'automation'|'sources'|'settings'|'plugins'};
export type ShellHistory={entries:ShellLocation[];index:number};
export const sameLocation=(a:ShellLocation,b:ShellLocation)=>a.page===b.page;
export function recordLocation(history:ShellHistory,location:ShellLocation):ShellHistory {
  if(sameLocation(history.entries[history.index],location))return history;
  const entries=[...history.entries.slice(0,history.index+1),location].slice(-50);
  return {entries,index:entries.length-1};
}
export function moveLocation(history:ShellHistory,delta:number):ShellHistory {
  const index=Math.max(0,Math.min(history.entries.length-1,history.index+delta));
  return index===history.index?history:{...history,index};
}
