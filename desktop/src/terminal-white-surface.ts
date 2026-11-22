import type {Terminal} from '@xterm/xterm';

// Pinned xterm 6.1 / WebGL 0.20: adapt old dark TUI fills to the light surface,
// without flattening input blocks or changing the buffer/recording.
const COLOR=0x3ffffff,RGB=0x3000000,INVERSE=0x4000000,DIM=0x8000000;
const TANGO=[0x2e3436,0xcc0000,0x4e9a06,0x8e7700,0x3465a4,0x75507b,0x05727e,0x6a6a6a,0x555753,0xef2929,0x1b7a1b,0x6d5a00,0x204a87,0xad7fa8,0x034b50,0x3d3d3d];
type Cell={fg:number;bg:number};
function colorRGB(attr:number){
  if((attr&RGB)===RGB)return attr&0xffffff;
  const index=attr&255;
  if(index<16)return TANGO[index];
  if(index>=232){const c=8+10*(index-232);return c<<16|c<<8|c}
  const n=index-16,levels=[0,95,135,175,215,255];
  return levels[Math.floor(n/36)]<<16|levels[Math.floor(n/6)%6]<<8|levels[n%6];
}
function lightFill(color:number){
  const channels=[color>>>16,(color>>>8)&255,color&255],max=Math.max(...channels),min=Math.min(...channels);
  // Prompt blocks remain darker than the gray-white canvas, including CLIs
  // that already emit pale neutral fills. Colored fills keep a subtle tint.
  if(max-min<42)return 0xe3e3e3;
  return channels.map(c=>Math.round(241*.9+c*.1)).reduce((rgb,c)=>(rgb<<8)|c,0);
}
export function whiteDisplayCell<T extends Cell>(source:T):T {
  const cell=Object.create(source) as T;
  cell.bg=source.bg&~(COLOR|DIM);
  cell.fg=source.fg&~INVERSE;
  if(source.fg&INVERSE){cell.bg|=RGB|0xe8f0fc;cell.fg=(cell.fg&~COLOR)|RGB|0x2e3434}
  else if(source.bg&RGB)cell.bg|=RGB|lightFill(colorRGB(source.bg));
  if(source.bg&DIM)cell.fg=(cell.fg&~COLOR)|RGB|0x737373;
  else if((source.fg&RGB)===RGB){
    const rgb=source.fg&0xffffff,values=[rgb>>>16,(rgb>>>8)&255,rgb&255];
    if(Math.min(...values)>155&&Math.max(...values)-Math.min(...values)<42)cell.fg=(cell.fg&~COLOR)|RGB|0x2e3434;
  }
  return cell;
}

export function installWhiteTerminalSurface(term:Terminal,initial=false){
  let enabled=initial;
  let patched:{owner:any;key:string;original:Function;wrapper:Function}|undefined;
  const detach=()=>{if(patched&&patched.owner[patched.key]===patched.wrapper)patched.owner[patched.key]=patched.original;patched=undefined};
  const attach=()=>{
    const renderer=(term as any)._core?._renderService?._renderer?.value;
    const owner=renderer?._cellColorResolver||renderer?._rowFactory;
    if(owner===patched?.owner)return;
    detach();
    if(!owner)return false;
    const key=renderer._cellColorResolver?'resolve':'createRow',original=owner[key];
    if(typeof original!=='function')return false;
    const wrapper=key==='resolve'?function(this:any,cell:Cell,...args:any[]){
      const result=original.call(this,enabled?whiteDisplayCell(cell):cell,...args);
      // Keep xterm's selection/search decorations and their contrast calculation.
      return result;
    }:function(this:any,line:any,...args:any[]){
      if(!enabled)return original.call(this,line,...args);
      const view=Object.create(line);
      view.loadCell=(index:number,scratch:any)=>{
        line.loadCell(index,scratch);
        const projected=whiteDisplayCell(scratch);
        scratch.bg=projected.bg;scratch.fg=projected.fg;
        return scratch;
      };
      return original.call(this,view,...args);
    };
    owner[key]=wrapper;patched={owner,key,original,wrapper};
    return true;
  };
  // Ignore program background assignments in white mode; queries still use
  // xterm's normal response path. No terminal input is synthesized here.
  const osc=[term.parser.registerOscHandler(11,value=>enabled&&value!=='?'),term.parser.registerOscHandler(111,()=>enabled)];
  return {attach,setEnabled(value:boolean){enabled=value;attach()},dispose(){detach();for(const handler of osc)handler.dispose()}};
}
