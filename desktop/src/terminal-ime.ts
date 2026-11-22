import type {Terminal} from '@xterm/xterm';
import {resolveTerminalImeAnchor} from './terminal-ime-anchor';
import {installTerminalComposition} from './terminal-composition';
import {terminalModifiedEnter,terminalKeyboardFlags,type TerminalKeyboardEnvironment} from './terminal-keyboard';

// A TUI can redraw and move its output cursor while the user is choosing a
// character. Keep that cursor from dragging the native IME away from the input.
export function stabilizeTerminalComposition(term:Terminal,environment?:TerminalKeyboardEnvironment){
  const element=term.element,textarea=term.textarea;
  if(!element||!textarea)return {dispose(){}};
  let composing=false,pinned=false,column=0,row=0,cellWidth=0,cellHeight=0,screenWidth=0;
  const screen=element.querySelector<HTMLElement>('.xterm-screen');
  const preedit=document.createElement('div');
  preedit.className='terminal-ime-preedit';preedit.hidden=true;preedit.setAttribute('aria-hidden','true');
  screen?.append(preedit);
  const clear=()=>{
    composing=false;pinned=false;
    preedit.hidden=true;preedit.textContent='';
    delete element.dataset.terminalComposing;
    element.style.removeProperty('--terminal-ime-inline');
    element.style.removeProperty('--terminal-ime-block');
    element.style.removeProperty('--terminal-ime-view-inline');
    element.style.removeProperty('--terminal-ime-screen-width');
    element.style.removeProperty('--terminal-ime-view-width');
  };
  const place=()=>{
    if(!composing||!pinned)return;
    const x=column*cellWidth,width=Math.min(screenWidth,parseFloat(textarea.style.width)||1);
    element.style.setProperty('--terminal-ime-inline',`${Math.max(0,Math.min(x,screenWidth-width))}px`);
    element.style.setProperty('--terminal-ime-view-inline',`${x}px`);
    element.style.setProperty('--terminal-ime-block',`${row*cellHeight}px`);
    element.style.setProperty('--terminal-ime-screen-width',`${screenWidth}px`);
    element.style.setProperty('--terminal-ime-view-width',`${Math.max(cellWidth,screenWidth-x)}px`);
    preedit.style.fontFamily=term.options.fontFamily??'monospace';
    preedit.style.fontSize=`${term.options.fontSize??14}px`;
    preedit.style.lineHeight=`${cellHeight}px`;
  };
  // Some Windows IMEs read the native insertion point before sending any DOM
  // composition event. Keep a recognized TUI's painted caret ready beforehand.
  const prepare=()=>{
    if(composing){place();return}
    if(textarea.ownerDocument.activeElement!==textarea)return;
    const buffer=term.buffer.active;
    const anchor=resolveTerminalImeAnchor({buffer,rows:term.rows,cols:term.cols,cursorX:buffer.cursorX,cursorY:buffer.cursorY});
    const bounds=screen?.getBoundingClientRect();
    if(!anchor||!bounds||bounds.width<=0||bounds.height<=0)return;
    const left=`${anchor.column*bounds.width/term.cols}px`,top=`${anchor.row*bounds.height/term.rows}px`;
    // xterm also writes these styles on cursor movement/resize. Comparing first
    // lets the observer correct those writes without triggering itself forever.
    if(textarea.style.left!==left)textarea.style.left=left;
    if(textarea.style.top!==top)textarea.style.top=top;
  };
  const pin=()=>{
    if(!composing)return;
    if(pinned){place();return}
    const bounds=screen?.getBoundingClientRect();
    if(!bounds||bounds.width<=0||bounds.height<=0)return;
    cellWidth=bounds.width/term.cols;cellHeight=bounds.height/term.rows;screenWidth=bounds.width;
    const buffer=term.buffer.active;
    const anchor=resolveTerminalImeAnchor({buffer,rows:term.rows,cols:term.cols,cursorX:buffer.cursorX,cursorY:buffer.cursorY});
    column=anchor?.column??Math.min(buffer.cursorX,term.cols-1);row=anchor?.row??buffer.cursorY;
    element.dataset.terminalComposing='true';pinned=true;
    place();
  };
  const composition=installTerminalComposition(term,{
    start(){clear();composing=true;pin()},
    update(text){preedit.textContent=text;preedit.hidden=!text;pin()},
    end(){clear();prepare()},
  },environment?event=>terminalModifiedEnter(event,terminalKeyboardFlags(term),environment):undefined);
  const refresh=[term.onRender(prepare),term.onCursorMove(prepare),term.onResize(prepare)];
  const styles=new MutationObserver(prepare);styles.observe(textarea,{attributes:true,attributeFilter:['style']});
  const size=new ResizeObserver(()=>{
    if(!composing||!pinned){prepare();return}
    const bounds=screen?.getBoundingClientRect();
    if(bounds&&bounds.width>0&&bounds.height>0){cellWidth=bounds.width/term.cols;cellHeight=bounds.height/term.rows;screenWidth=bounds.width;column=Math.min(column,term.cols-1);row=Math.min(row,term.rows-1);place()}
  });
  if(screen)size.observe(screen);
  textarea.addEventListener('focus',prepare);
  prepare();
  return {dispose(){
    composition.dispose();preedit.remove();
    textarea.removeEventListener('focus',prepare);
    styles.disconnect();
    size.disconnect();
    for(const event of refresh)event.dispose();
    clear();
  }};
}
