import {Terminal} from '@xterm/xterm';
import {WebglAddon} from '@xterm/addon-webgl';
import {installWhiteTerminalSurface} from '../src/terminal-white-surface';
import {surfaceThemes} from '../src/surface-themes';
import '@xterm/xterm/css/xterm.css';

document.body.style.cssText='margin:24px;background:#fff;color:#1f2224;font:14px system-ui';
const title=document.createElement('h1');title.textContent='Orca 参考浅色终端：真实 DOM / WebGL 隔离验证';document.body.append(title);
const report=document.createElement('pre');report.id='report';report.textContent='验证中';document.body.append(report);
const fixture='\x1b[2J\x1b[H\x1b[38;2;230;235;232;48;2;35;45;42m Codex RGB dark prompt                 \x1b[K\r\n\x1b[48:2::40:50:44m Colon RGB prompt                      \x1b[K\r\n\x1b[48;5;238m Indexed background                    \x1b[K\r\n\x1b[0;7m Inverse selection                     \x1b[K\r\n\x1b[0m\x1b[31m红色错误\x1b[32m 绿色状态\x1b[0m 中文历史\r\n\x1b]11;#000000\x07';
const records:any[]=[];
for(const mode of ['DOM','WebGL']){
  const heading=document.createElement('h2');heading.textContent=mode;document.body.append(heading);
  const container=document.createElement('div');container.style.cssText='padding:12px;border:1px solid #ddd;background:white';document.body.append(container);
  const term=new Terminal({cols:72,rows:7,fontSize:16,fontFamily:'Consolas',theme:surfaceThemes.light,allowProposedApi:true,minimumContrastRatio:4.5,cursorStyle:'bar',cursorBlink:false});term.open(container);
  let addon:WebglAddon|undefined;
  try{
    if(mode==='WebGL'){addon=new WebglAddon(true);term.loadAddon(addon)}
    const white=installWhiteTerminalSurface(term,true);if(!white.attach())throw Error('render adapter missing');
    await new Promise<void>(resolve=>term.write(fixture,resolve));
    term.refresh(0,term.rows-1);await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);
    const source=term.buffer.active.getLine(0)!.getCell(1)!;
    const bufferPreserved=source.getBgColor()===0x232d2a&&source.isBgRGB();
    let lightSurfaces=false;
    if(addon){
      const renderer=(addon as any)._renderer,gl:WebGL2RenderingContext=renderer._gl,cell=renderer.dimensions.device.cell;
      // Blank cells after each ANSI row still carry the original full-row fill.
      // Sample those fill pixels, away from glyphs and the cursor.
      const color=new Uint8Array(4);
      // EL clears inverse from erased tail cells: sample the explicit leading
      // space for the inverse row, and the filled blank tail for RGB/indexed.
      lightSurfaces=[0,1,2,3].every(row=>{const x=row===3?.5:60.5;gl.readPixels(Math.floor(cell.width*x),gl.drawingBufferHeight-Math.floor(cell.height*(row+.5)),1,1,gl.RGBA,gl.UNSIGNED_BYTE,color);const expected=row===3?[232,240,252]:[227,227,227];return expected.every((c,i)=>color[i]===c)});
    }else{
      const rows=[...container.querySelectorAll('.xterm-rows>div')];
      lightSurfaces=rows.slice(0,4).every((row,i)=>[...row.querySelectorAll('span')].some(span=>getComputedStyle(span).backgroundColor===(i===3?'rgb(232, 240, 252)':'rgb(227, 227, 227)')));
    }
    const passed=!!bufferPreserved&&lightSurfaces;
    records.push({mode,passed,lightSurfaces,bufferPreserved:!!bufferPreserved});
    const toggle=document.createElement('button');let light=true;toggle.textContent=mode+' 切换深浅色';toggle.onclick=()=>{light=!light;term.options.theme=surfaceThemes[light?'light':'dark'];white.setEnabled(light);term.refresh(0,term.rows-1)};document.body.append(toggle);
  }catch(error){records.push({mode,passed:false,error:String(error)})}
}
report.textContent=JSON.stringify(records,null,2);report.dataset.passed=String(records.every(row=>row.passed));
