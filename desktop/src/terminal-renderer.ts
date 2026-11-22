import type {Terminal} from '@xterm/xterm';
import {WebglAddon} from '@xterm/addon-webgl';
import type {TerminalPreferences} from './terminal-preferences';
import {repairTerminalCanvasDpr} from './terminal-canvas-dpr';
import {installWhiteTerminalSurface} from './terminal-white-surface';
import {subscribeTerminalWindowVisibility} from './terminal-window-visibility';

export type RendererStatus={kind:'dom'|'webgl';reason:string};
export function manageTerminalRenderer(term:Terminal,resize:()=>void,status:(value:RendererStatus)=>void,white=false){
  const surface=installWhiteTerminalSurface(term,white);
  let addon:WebglAddon|undefined,loss:{dispose():void}|undefined,frame=0,disposed=false,visible=true;
  let paneVisible=true,windowVisible=true;
  let selected:TerminalPreferences['renderer']='auto',failed=false;
  let repairing=false;
  const repair=()=>{
    if(disposed||!visible||!addon||repairing)return;
    repairing=true;try{repairTerminalCanvasDpr(term)}finally{repairing=false}
  };
  const rendered=term.onRender(repair);
  const refresh=()=>{
    surface.attach();
    cancelAnimationFrame(frame);
    if(visible)frame=requestAnimationFrame(()=>{if(!disposed&&visible){resize();repair();term.refresh(0,term.rows-1)}});
  };
  const release=()=>{
    loss?.dispose();loss=undefined;
    if(!addon)return;
    const current=addon;addon=undefined;
    // Release ANGLE resources as well as the canvas when switching panes/renderers.
    try{
      const renderer=(current as unknown as {_renderer?:{_gl?:WebGL2RenderingContext;_canvas?:HTMLCanvasElement}})._renderer;
      renderer?._gl?.getExtension('WEBGL_lose_context')?.loseContext();
      if(renderer?._canvas){renderer._canvas.width=0;renderer._canvas.height=0}
    }catch{/* Losing a driver context must not block DOM fallback. */}
    try{current.dispose()}catch{/* A failed constructor can leave a partial renderer. */}
  };
  const update=(mode:TerminalPreferences['renderer'])=>{
    if(disposed)return;
    if(mode!==selected)failed=false;
    selected=mode;
    release();
    if(!visible)return;
    if(failed){status({kind:'dom',reason:'GPU 上下文丢失，已回退'});refresh();return}
    if(mode==='dom'){status({kind:'dom',reason:'已选择 DOM'});refresh();return}
    try{
      addon=new WebglAddon();
      loss=addon.onContextLoss(()=>{failed=true;release();status({kind:'dom',reason:'GPU 上下文丢失，已回退'});refresh()});
      term.loadAddon(addon);
      if(mode==='auto'){
        const gl=(addon as unknown as {_renderer?:{_gl?:WebGL2RenderingContext}})._renderer?._gl;
        const extension=gl?.getExtension('WEBGL_debug_renderer_info');
        const gpu=extension?String(gl?.getParameter(extension.UNMASKED_RENDERER_WEBGL)):'';
        if(!gpu||/swiftshader|llvmpipe|software|microsoft basic render/i.test(gpu)){
          release();status({kind:'dom',reason:gpu?'软件 GPU，自动使用 DOM':'GPU 信息不可用，自动使用 DOM'});refresh();return;
        }
      }
      status({kind:'webgl',reason:mode==='auto'?'自动选择 WebGL':'已选择 WebGL'});
    }catch{release();status({kind:'dom',reason:'WebGL 不可用，已回退'})}
    refresh();
  };
  const syncVisibility=()=>{const value=paneVisible&&windowVisible;if(disposed||visible===value)return;visible=value;cancelAnimationFrame(frame);if(value)update(selected);else release()};
  const stopWindowVisibility=subscribeTerminalWindowVisibility(value=>{windowVisible=value;syncVisibility()});
  return {update,setWhiteSurface(value:boolean){surface.setEnabled(value);refresh()},setVisible(value:boolean){paneVisible=value;syncVisibility()},dispose(){disposed=true;stopWindowVisibility();rendered.dispose();cancelAnimationFrame(frame);release();surface.dispose()}};
}
