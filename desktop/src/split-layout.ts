import type {Layout,SplitNode,SplitAxis,Tab} from './workbench-types';

export type SplitEdge='left'|'right'|'top'|'bottom';
export const MAX_SPLIT_LEAVES=16;
export const leaf=(id:string):SplitNode=>({kind:'leaf',id});
export const leafIds=(tree:SplitNode):string[]=>tree.kind==='leaf'?[tree.id]:[...leafIds(tree.first),...leafIds(tree.second)];
export const tabSessions=(tab:Tab):string[]=>tab.kind==='terminal'?leafIds(tab.terminalTree||leaf(tab.target)):tab.kind==='agent'?[tab.target]:[];
export const terminalTree=(tab:Tab)=>tab.terminalTree||leaf(tab.target);
export function splitLeaf(tree:SplitNode,id:string,added:SplitNode,edge:SplitEdge):SplitNode {
  if(tree.kind==='leaf')return tree.id===id?{kind:'split',id:crypto.randomUUID(),axis:edge==='left'||edge==='right'?'horizontal':'vertical',ratio:.5,first:edge==='left'||edge==='top'?added:tree,second:edge==='left'||edge==='top'?tree:added}:tree;
  return {...tree,first:splitLeaf(tree.first,id,added,edge),second:splitLeaf(tree.second,id,added,edge)};
}
export function removeLeaf(tree:SplitNode,id:string):SplitNode|undefined {
  if(tree.kind==='leaf')return tree.id===id?undefined:tree;
  const first=removeLeaf(tree.first,id),second=removeLeaf(tree.second,id);
  return first&&second?{...tree,first,second}:first||second;
}
export function moveLeaf(tree:SplitNode,id:string,to:string,edge:SplitEdge):SplitNode {
  if(id===to||!leafIds(tree).includes(id)||!leafIds(tree).includes(to))return tree;
  return splitLeaf(removeLeaf(tree,id)!,to,leaf(id),edge);
}
export function resizeSplit(tree:SplitNode,id:string,ratio:number,axis?:SplitAxis):SplitNode {
  if(tree.kind==='leaf')return tree;
  if(tree.id===id)return {...tree,ratio:Math.max(.15,Math.min(.85,ratio)),axis:axis||tree.axis};
  return {...tree,first:resizeSplit(tree.first,id,ratio,axis),second:resizeSplit(tree.second,id,ratio,axis)};
}
export function workspaceTree(layout:Layout):SplitNode {
  if(layout.tree)return layout.tree;
  return layout.panes.slice(1).reduce<SplitNode>((first,p)=>({kind:'split',id:`legacy-${p.id}`,axis:layout.axis,ratio:layout.ratio,first,second:leaf(p.id)}),leaf(layout.panes[0].id));
}
// Legacy producers may append/remove a pane without knowing about trees.
// Preserve every existing boundary, prune missing panes, then append new ones.
export function normalizeLayout(layout:Layout):Layout {
  const ids=new Set(layout.panes.map(p=>p.id));
  let tree:SplitNode|undefined=workspaceTree(layout);
  for(const id of leafIds(tree))if(tree&&!ids.has(id))tree=removeLeaf(tree,id);
  for(const p of layout.panes)if(!tree)tree=leaf(p.id);else if(!leafIds(tree).includes(p.id))tree=splitLeaf(tree,leafIds(tree).at(-1)!,leaf(p.id),'right');
  return {...layout,tree,axis:tree?.kind==='split'?tree.axis:layout.axis,ratio:tree?.kind==='split'?tree.ratio:layout.ratio};
}
export function pruneEmptyPanes(layout:Layout):Layout {
  if(layout.panes.some(p=>p.tabs.length))layout.panes=layout.panes.filter(p=>p.tabs.length);
  else layout.panes=layout.panes.slice(0,1);
  return normalizeLayout(layout);
}
export function moveTab(layout:Layout,tabId:string,to:string,edge?:SplitEdge):{layout:Layout;paneId:string}|undefined {
  const next=normalizeLayout(structuredClone(layout)),from=next.panes.find(p=>p.tabs.some(t=>t.id===tabId)),dest=next.panes.find(p=>p.id===to);
  if(!from||!dest||from===dest&&(!edge||from.tabs.length===1))return;
  if(edge&&next.panes.length>=MAX_SPLIT_LEAVES)return;
  const tab=from.tabs.find(t=>t.id===tabId)!;
  from.tabs=from.tabs.filter(t=>t.id!==tabId);if(from.active===tabId)from.active=from.tabs[0]?.id||null;
  let pane=dest;
  if(edge){pane={id:crypto.randomUUID(),active:tab.id,tabs:[]};next.panes.push(pane);next.tree=splitLeaf(next.tree!,dest.id,leaf(pane.id),edge);}
  pane.tabs.push(tab);pane.active=tab.id;
  return {layout:pruneEmptyPanes(next),paneId:pane.id};
}
export function dropEdge(x:number,y:number,width:number,height:number):SplitEdge|undefined {
  const edges:[SplitEdge,number][]=[['left',x/width],['right',(width-x)/width],['top',y/height],['bottom',(height-y)/height]];
  edges.sort((a,b)=>a[1]-b[1]);return edges[0][1]<=.24?edges[0][0]:undefined;
}
