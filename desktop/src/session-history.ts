import {agentCatalog} from './agent-preferences';
import type {Session} from './workbench-types';

export function isAgentSession(session:Session){
  return session.hybrid===true||session.kind==='agent'||session.native_chat===true||agentCatalog.some(agent=>agent.program===session.program||agent.id===session.program)||session.program==='zcode-api';
}

export function partitionSessionHistory(sessions:Session[],openSessionIds:string[]){
  const opened=new Set(openSessionIds);
  return {current:sessions.filter(session=>opened.has(session.id)),history:sessions.filter(session=>!opened.has(session.id))};
}

export type SessionHistoryFilter='agents'|'shells'|'all';
export function filterSessionHistory(sessions:Session[],filter:SessionHistoryFilter,query=''){
  const needle=query.trim().toLocaleLowerCase();
  return sessions.filter(session=>(filter==='all'||isAgentSession(session)===(filter==='agents'))&&(!needle||[session.title,session.program,session.id,session.native_id].some(value=>value?.toLocaleLowerCase().includes(needle)))).slice().reverse();
}
