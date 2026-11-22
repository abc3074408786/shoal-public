import type {Terminal} from '@xterm/xterm';
import type {TerminalSize} from './terminal-resize';

// Fit the content box, not the parent's border-box CSS height. The current
// FitAddon subtracts padding on xterm itself, but our padding belongs to its
// flex container. Counting it as cells hides the last native CLI footer rows.
export function measureTerminalViewport(term:Terminal):TerminalSize|undefined {
  const element=term.element,parent=element?.parentElement;
  if(!element||!parent||parent.clientWidth<=0||parent.clientHeight<=0)return;
  const cell=term.dimensions?.css.cell;
  if(!cell||cell.width<=0||cell.height<=0)return;
  const view=parent.ownerDocument.defaultView;
  if(!view)return;
  const containerStyle=view.getComputedStyle(parent),elementStyle=view.getComputedStyle(element);
  const padding=(name:string)=>(parseFloat(containerStyle.getPropertyValue(name))||0)+(parseFloat(elementStyle.getPropertyValue(name))||0);
  const scrollbar=term.options.scrollback===0||term.options.scrollbar?.showScrollbar===false?0:term.options.scrollbar?.width??14;
  const width=parent.clientWidth-padding('padding-left')-padding('padding-right')-scrollbar;
  const height=parent.clientHeight-padding('padding-top')-padding('padding-bottom');
  return {cols:Math.max(2,Math.floor(width/cell.width)),rows:Math.max(1,Math.floor(height/cell.height))};
}
