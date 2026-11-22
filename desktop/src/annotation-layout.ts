import {Layout,Session} from './workbench-types';
import {normalizeLayout,tabSessions} from './split-layout';

export function annotationChatLayout(layout:Layout,fromPane:string,session:Session):{layout:Layout;paneId:string}{
  const next=structuredClone(layout);
  let tab=next.panes.flatMap(p=>p.tabs).find(t=>tabSessions(t).includes(session.id));
  // A grouped terminal remains a terminal; opening it must not discard siblings.
  if(tab?.terminalTree){const pane=next.panes.find(p=>p.tabs.includes(tab!))!;pane.active=tab.id;return {layout:normalizeLayout(next),paneId:pane.id};}
  if(!tab)tab={id:crypto.randomUUID(),kind:'agent',target:session.id,title:session.title};
  tab.kind='agent';
  const source=next.panes.find(p=>p.id===fromPane)||next.panes[0];
  const browserActive=source.tabs.some(t=>t.id===source.active&&t.kind==='browser');
  let destination=browserActive?next.panes.find(p=>p.id!==source.id):source;
  if(!destination){destination={id:crypto.randomUUID(),tabs:[],active:null};next.panes.push(destination);next.axis='horizontal';next.ratio=.5;}
  for(const pane of next.panes){pane.tabs=pane.tabs.filter(t=>!(['agent','terminal'].includes(t.kind)&&t.target===session.id));if(!pane.tabs.some(t=>t.id===pane.active))pane.active=pane.tabs[0]?.id||null;}
  destination.tabs.push(tab);destination.active=tab.id;
  return {layout:normalizeLayout(next),paneId:destination.id};
}
