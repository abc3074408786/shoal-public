import type {Layout,Session,Tab} from './workbench-types';
import {tabSessions} from './split-layout';

/** Explicit terminal navigation must not be diverted into an annotation chat. */
export function sessionSurfaceTab(layout:Layout,session:Session,title:string,chat:boolean,forceTerminal=false):Tab {
  const existing=layout.panes.flatMap(p=>p.tabs).find(t=>(!forceTerminal||t.kind==='terminal')&&tabSessions(t).includes(session.id));
  return existing||{id:crypto.randomUUID(),kind:!forceTerminal&&chat?'agent':'terminal',target:session.id,title};
}
