import type {Terminal} from '@xterm/xterm';

type CanvasRenderer={
  _canvas?:HTMLCanvasElement;_gl?:{canvas?:HTMLCanvasElement};_devicePixelRatio?:number;
  dimensions?:{device?:{canvas?:{width?:number;height?:number}}};
  handleDevicePixelRatioChange?:()=>void;handleResize?:(cols:number,rows:number)=>void;
};

// Use the same repair as Orca: re-run xterm's renderer sizing when its canvas
// backing store no longer matches the device-pixel character grid. No PTY resize.
export function repairTerminalCanvasDpr(term:Terminal):'current'|'deferred'|'repaired'{
  const renderer=(term as unknown as {_core?:{_renderService?:{_renderer?:{value?:CanvasRenderer}}}})._core?._renderService?._renderer?.value;
  const canvas=renderer?._canvas??renderer?._gl?.canvas;
  if(!renderer||!canvas)return 'current';
  const view=canvas.ownerDocument?.defaultView,expected=renderer.dimensions?.device?.canvas;
  if(!canvas.isConnected||!view||!expected?.width||!expected.height)return 'deferred';
  const tolerance=Math.max(1,Math.ceil(view.devicePixelRatio/2));
  const staleDpr=typeof renderer._devicePixelRatio==='number'&&renderer._devicePixelRatio!==view.devicePixelRatio;
  if(!staleDpr&&Math.abs(canvas.width-expected.width)<=tolerance&&Math.abs(canvas.height-expected.height)<=tolerance)return 'current';
  if(!renderer.handleDevicePixelRatioChange||!renderer.handleResize)return 'deferred';
  try{
    renderer.handleDevicePixelRatioChange();
    renderer.handleResize(term.cols,term.rows);
    term.refresh(0,term.rows-1);
    return 'repaired';
  }catch{return 'deferred'}
}
