/** xterm selection uses display columns, not UTF-16 string offsets. */
export function findTerminalText(cells:{chars:string;width:number}[],query:string){
  if(!query)return null;
  let text='';const spans:{from:number;to:number;column:number;width:number}[]=[];
  cells.forEach((cell,column)=>{if(!cell.width)return;const chars=cell.chars||' ';spans.push({from:text.length,to:text.length+chars.length,column,width:cell.width});text+=chars});
  const start=text.toLowerCase().indexOf(query.toLowerCase());if(start<0)return null;
  const end=start+query.length,first=spans.find(cell=>cell.to>start),last=spans.find(cell=>cell.to>=end);
  return first&&last?{column:first.column,length:last.column+last.width-first.column}:null;
}
