import type {Terminal,IBufferCell} from '@xterm/xterm';

type FrameRun={text:string;column:number;sgr:string};
export type TerminalFrame={offset:number;cols:number;rows:number;contentRows:number;ansi:string};
export type TerminalFrames={frames:TerminalFrame[];dropped:number};
function sgr(cell:IBufferCell){
  const values=[0];
  for(const [active,code] of [[cell.isBold(),1],[cell.isDim(),2],[cell.isItalic(),3],[cell.isUnderline(),4],[cell.isInverse(),7]] as const)if(active)values.push(code);
  for(const background of [false,true]){
    const value=background?cell.getBgColor():cell.getFgColor(),code=background?48:38;
    if(background?cell.isBgRGB():cell.isFgRGB())values.push(code,2,(value>>16)&255,(value>>8)&255,value&255);
    else if(background?cell.isBgPalette():cell.isFgPalette())values.push(code,5,value);
  }
  return `\x1b[${values.join(';')}m`;
}
// Capture the current screen before xterm reflows it for the next geometry.
// These immutable display copies never replace or alter the raw terminal log.
export function captureTerminalFrame(term:Pick<Terminal,'buffer'|'cols'|'rows'>,offset:number):TerminalFrame{
  const buffer=term.buffer.active,lines:FrameRun[][]=[],plain:string[]=[];
  for(let row=0;row<term.rows;row++){
    const line=buffer.getLine(buffer.baseY+row),runs:FrameRun[]=[];
    let key='';
    for(let column=0;column<term.cols;column++){
      const cell=line?.getCell(column);if(!cell||cell.getWidth()===0)continue;
      const text=cell.getChars()||' ';
      const escape=sgr(cell),nextKey=escape;
      if(runs.length&&key===nextKey)runs[runs.length-1].text+=text;
      else{runs.push({text,column,sgr:escape});key=nextKey}
    }
    lines.push(runs);plain.push(line?.translateToString(true,0,term.cols)||'');
  }
  // Empty rows below the last visible content do not add useful scroll distance.
  while(plain.length&&!plain[plain.length-1].trim()){plain.pop();lines.pop()}
  const ansi='\x1b[2J\x1b[H'+lines.map((runs,row)=>runs.map(run=>`\x1b[${row+1};${run.column+1}H${run.sgr}${run.text}`).join('')).join('')+'\x1b[0m\x1b[?25l';
  // ANSI already encodes characters, cell positions and styles. Retaining the
  // per-run object graph and a plain-text copy multiplies history heap usage.
  return {offset,cols:term.cols,rows:term.rows,contentRows:lines.length,ansi};
}
export function createTerminalFrameStore(maxCharacters=4*1024*1024){
  const frames:TerminalFrame[]=[];let characters=0,dropped=0;
  return {
    capture(term:Pick<Terminal,'buffer'|'cols'|'rows'>,offset:number){
      const frame=captureTerminalFrame(term,offset),last=frames[frames.length-1];
      if(!frame.contentRows||last?.cols===frame.cols&&last.rows===frame.rows&&last.ansi===frame.ansi)return;
      frames.push(frame);characters+=frame.ansi.length;
      while(frames.length>1&&(characters>maxCharacters||frames.length>200)){characters-=frames.shift()!.ansi.length;dropped++}
    },
    snapshot():TerminalFrames{return {frames:frames.slice(),dropped}},
    get count(){return frames.length},
  };
}

// Observe BEFORE xterm erases a screen or leaves the alternate buffer. Capturing
// after a transport page misses a whole conversation when exit and clear arrive
// in that same page. Returning false preserves xterm's normal ANSI handling.
// These frames can be rebuilt from terminal.bin whenever a viewer is reopened.
export function captureTerminalClears(term:Pick<Terminal,'parser'>,capture:()=>void){
  const handlers=[
    term.parser.registerCsiHandler({final:'J'},params=>{if(params[0]===2||params[0]===3)capture();return false}),
    term.parser.registerCsiHandler({prefix:'?',final:'l'},params=>{if(params.some(value=>value===47||value===1047||value===1049))capture();return false}),
    term.parser.registerEscHandler({final:'c'},()=>{capture();return false}),
  ];
  return {dispose(){for(const handler of handlers)handler.dispose()}};
}

// A new shell still needs a complete parser/mode reset. Rebuild the rendered
// normal history through public buffer APIs, then move its last screen into
// scrollback so the new process's cursor-home repaint cannot overwrite it.
// This is display-only: the writer must suppress protocol input, and nothing
// here is sent to the PTY or persisted over the original recording.
export async function resetTerminalWithHistory(term:Pick<Terminal,'buffer'|'cols'|'rows'|'reset'>,write:(bytes:Uint8Array,history:boolean)=>Promise<void>){
  const buffers=[term.buffer.normal];
  if(term.buffer.active!==term.buffer.normal)buffers.push(term.buffer.active);
  const output:string[]=[];
  for(const buffer of buffers){
    let end=buffer.length;
    while(end&&!buffer.getLine(end-1)?.translateToString(true).trim())end--;
    for(let row=0;row<end;row++){
      const line=buffer.getLine(row);if(!line)continue;
      let style='';
      for(let column=0;column<term.cols;column++){
        const cell=line.getCell(column);if(!cell||cell.getWidth()===0)continue;
        const next=sgr(cell);if(next!==style){output.push(next);style=next}
        output.push(cell.getChars()||' ');
      }
      if(!buffer.getLine(row+1)?.isWrapped||row===end-1)output.push('\x1b[0m\r\n');
    }
  }
  term.reset();
  if(output.length)await write(new TextEncoder().encode(output.join('')+'\r\n'.repeat(term.rows)+'\x1b[H\x1b[0m'),true);
}
