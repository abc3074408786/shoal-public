import type {Session} from './workbench-types';
export type DesktopResources={sampledAt:number;scope:'electron-private';privateBytes:number|null;processes:Array<{pid:number;type:string;privateBytes:number|null}>};
export function formatResourceBytes(bytes:number|null|undefined){
 if(bytes===null||bytes===undefined||!Number.isFinite(bytes)||bytes<0)return '—';
 return bytes>=1e9?`${(bytes/1e9).toFixed(2)} GB`:`${Math.round(bytes/1e6)} MB`;
}
export function runningTerminals(sessions:Session[]){
 return sessions.filter(s=>s.kind==='terminal'&&!s.native_chat&&s.status?.exit===null&&typeof s.status.pid==='number'&&s.status.pid>0&&!['host_lost','failed'].includes(s.status.state));
}
export function connectionState(connected:boolean,failures:number){return connected?'connected':failures>=3?'failed':failures>0?'reconnecting':'connecting'}
export const connectionLabels={connected:'已连接',connecting:'正在连接',reconnecting:'重连中',failed:'连接失败'};
export const processLabels:Record<string,string>={Browser:'主进程',Tab:'界面进程',GPU:'GPU 进程',Utility:'辅助进程',Zygote:'启动辅助', 'Sandbox helper':'沙箱辅助',Unknown:'其他进程'};
