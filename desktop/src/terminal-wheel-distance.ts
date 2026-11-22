export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER=1;
export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MIN=1;
export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MAX=10;

export type TerminalTuiMouseWheelDistanceState={remainder:number;direction:number};
export const createTerminalTuiMouseWheelDistanceState=():TerminalTuiMouseWheelDistanceState=>({remainder:0,direction:0});
export function normalizeTerminalTuiMouseWheelMultiplier(value:number|undefined):number {
  return Number.isFinite(value)?Math.round(Math.max(1,Math.min(10,value!))):1;
}

/** Pixel input preserves physical distance. Line/page input uses the configured gain. */
export function resolveTerminalTuiMouseWheelReportCount(
  event:Pick<WheelEvent,'deltaY'> & Partial<Pick<WheelEvent,'deltaMode'>>,
  multiplier:number,state:TerminalTuiMouseWheelDistanceState,
  metrics:{cellHeight?:number;rows?:number}={},
):number {
  if(!Number.isFinite(event.deltaY)||!event.deltaY)return 0;
  const direction=Math.sign(event.deltaY);
  if(direction!==state.direction)state.remainder=0;
  state.direction=direction;
  const height=Number.isFinite(metrics.cellHeight)&&metrics.cellHeight!>0?metrics.cellHeight!:16;
  const rows=Number.isFinite(metrics.rows)&&metrics.rows!>0?metrics.rows!:1;
  const units=event.deltaMode===2?rows:event.deltaMode===1?1:1/height;
  const gain=event.deltaMode?normalizeTerminalTuiMouseWheelMultiplier(multiplier):1;
  const distance=Math.min(120,Math.abs(event.deltaY)*units*gain)+state.remainder;
  const count=Math.floor(distance);
  state.remainder=distance-count;
  return count;
}
