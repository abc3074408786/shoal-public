import type {IBuffer} from '@xterm/xterm';
export type TerminalImeAnchor={row:number;column:number};
type Screen={buffer:IBuffer;rows:number;cols:number;cursorX:number;cursorY:number;knownCursorAgent?:boolean};

/** A recognized input box may supply a painted caret when the VT cursor is parked elsewhere. */
export function resolveTerminalImeAnchor(screen:Screen):TerminalImeAnchor|null {
  const {buffer,rows,cols,cursorX,cursorY}=screen;
  if(rows<1||cols<1)return null;
  const lines=Array.from({length:rows},(_,row)=>buffer.getLine(buffer.baseY+row));
  const text=lines.map(line=>line?.translateToString(true)??'');
  const cursor=lines[cursorY]?.getCell(Math.min(cursorX,cols-1));
  if((cursor?.getChars()??'').trim())return null;
  const claude=text.slice(0,6).some(line=>/Claude Code v\d/.test(line));
  const rules=text.flatMap((line,row)=>line.trim().length>=Math.min(cols,8)&&/^─+$/.test(line.trim())?[row]:[]);
  for(let index=rules.length-2;index>=0;index--){
    const start=rules[index]+1,end=rules[index+1];
    if(!/^❯\s/.test(text[start]??'')||(cursorY>=start&&cursorY<end))continue;
    if(!claude&&!text.slice(end+1,end+3).some(line=>/\? for shortcuts|manual mode on/.test(line)))continue;
    const candidates:TerminalImeAnchor[]=[];
    for(let row=start;row<end;row++){
      for(let column=row===start?2:0;column<cols;column++){
        const cell=lines[row]?.getCell(column);
        if(cell?.getWidth()&&cell.isInverse())candidates.push({row,column});
      }
    }
    if(candidates.length)return candidates.length===1?candidates[0]:null;
  }
  if(cursorX!==0||text[cursorY]?.trim())return null;
  const known=screen.knownCursorAgent||text.slice(0,6).some(line=>line.trim()==='Cursor Agent');
  for(let row=rows-1;row>=0;row--){
    const line=lines[row];if(!line)continue;
    for(let column=0;column<cols-2;column++){
      if(line.getCell(column)?.getChars()!=='→'||line.getCell(column+1)?.getChars()!==' ')continue;
      const start=column+2,value=line.translateToString(true,start);
      if(!value.trim()||/^(Plan, search, build anything|Add a follow-up)/.test(value))return {row,column:start};
      if(!known)continue;
      let end=start;
      for(let cellIndex=start;cellIndex<cols;cellIndex++){
        const cell=line.getCell(cellIndex);
        if(cell?.getWidth()&&cell.getChars().trim())end=cellIndex+cell.getWidth();
      }
      return {row,column:Math.min(end,cols-1)};
    }
  }
  return null;
}
