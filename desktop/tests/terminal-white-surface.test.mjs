import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const code=ts.transpileModule(readFileSync(new URL('../src/terminal-white-surface.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {whiteDisplayCell,installWhiteTerminalSurface}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
const COLOR=0x3ffffff,RGB=0x3000000,INVERSE=0x4000000;
const write=(term,text)=>new Promise(resolve=>term.write(text,resolve));
function bridge(renderer){
 const osc=new Map();
 const term={_core:{_renderService:{_renderer:{value:renderer}}},parser:{registerOscHandler(id,handler){osc.set(id,handler);return {dispose(){osc.delete(id)}}}}};
 return {term,osc};
}

test('old dark RGB/indexed fills become light surfaces while inverse stays distinct and buffer stays original',async()=>{
 const term=new xterm.Terminal({allowProposedApi:true,cols:80,rows:12});
 try{
  await write(term,'\x1b[48;2;35;45;42mRGB\r\n\x1b[48:2::40:50:44mCOLON\r\n\x1b[48;5;238mINDEX\r\n\x1b[44mANSI\r\n\x1b[0;7mINVERSE\r\n\x1b[0;2;3;31;48;2;20;20;20mDIM RED 中文');
  for(let y=0;y<6;y++){
   const original=term.buffer.active.getLine(y).getCell(0);
   const before={fg:original.fg,bg:original.bg,text:original.getChars(),width:original.getWidth()};
   const projected=whiteDisplayCell(original);
   assert.equal(projected.bg&RGB,RGB);assert.equal(projected.fg&INVERSE,0);
   assert.equal(projected.isItalic(),original.isItalic());
   assert.equal(projected.getChars(),before.text);assert.equal(projected.getWidth(),before.width);
   if(original.isInverse())assert.equal(projected.bg&0xffffff,0xe8f0fc);
   assert.deepEqual({fg:original.fg,bg:original.bg,text:original.getChars(),width:original.getWidth()},before);
  }
 }finally{term.dispose()}
});

test('WebGL projection runs before color/cache resolution; dark mode restores original attrs',()=>{
 const raw={fg:0x3000000|0x3355aa|INVERSE,bg:0x3000000|0x232d2a};
 let seen;
 const resolver={result:{},resolve(cell){seen=cell;this.result={fg:cell.fg,bg:cell.bg,ext:123}}};
 const {term}=bridge({_cellColorResolver:resolver});const original=resolver.resolve;
 const surface=installWhiteTerminalSurface(term,true);assert.equal(surface.attach(),true);
 resolver.resolve(raw,0,0,10,20);assert.notEqual(seen,raw);assert.equal(resolver.result.bg&COLOR,RGB|0xe8f0fc);assert.equal(resolver.result.fg&INVERSE,0);assert.equal(resolver.result.ext,123);
 surface.setEnabled(false);resolver.resolve(raw);assert.equal(seen,raw);assert.deepEqual(resolver.result,{...raw,ext:123});
 surface.dispose();assert.equal(resolver.resolve,original);
});

test('DOM receives projected scratch cells but retained recording/buffer and later dark rows stay original',async()=>{
 const source=new xterm.Terminal({allowProposedApi:true});
 try{
  await write(source,'\x1b[38;2;250;250;250;48;2;35;45;42mHistory');
  const line=source._core._bufferService.buffer.lines.get(0),before=source.buffer.active.getLine(0).getCell(0).bg;
  let seen;
  const factory={createRow(line){const scratch={};line.loadCell(0,scratch);seen=scratch;return [{style:{backgroundColor:'rgb(172, 206, 247)'}}]}};
  const {term}=bridge({_rowFactory:factory});const original=factory.createRow;
  const surface=installWhiteTerminalSurface(term,true);surface.attach();
  const rows=factory.createRow(line);assert.equal(seen.bg&COLOR,RGB|0xe3e3e3);assert.equal(rows[0].style.backgroundColor,'rgb(172, 206, 247)');
  assert.equal(source.buffer.active.getLine(0).getCell(0).bg,before);
  surface.setEnabled(false);factory.createRow(line);assert.equal(seen.bg,before);
  surface.dispose();assert.equal(factory.createRow,original);
 }finally{source.dispose()}
});

test('canvas, prompt, selection and neutral fills retain separate roles',()=>{
 assert.equal(whiteDisplayCell({fg:0,bg:0}).bg,0);
 assert.equal(whiteDisplayCell({fg:0,bg:RGB|0x434e4a}).bg&COLOR,RGB|0xe3e3e3);
 assert.equal(whiteDisplayCell({fg:INVERSE,bg:0}).bg&COLOR,RGB|0xe8f0fc);
 assert.equal(whiteDisplayCell({fg:0,bg:RGB|0xe3e3e3}).bg&COLOR,RGB|0xe3e3e3);
 assert.equal(whiteDisplayCell({fg:RGB|0xdfe8e3,bg:0}).fg&COLOR,RGB|0x2e3434);
 assert.equal(whiteDisplayCell({fg:RGB|0xcc0000,bg:0}).fg&COLOR,RGB|0xcc0000);
});

test('WebGL keeps the renderer selection background instead of erasing it',()=>{
 const resolver={result:{},resolve(cell){this.result={fg:cell.fg,bg:RGB|0xaccef7,ext:0}}};
 const {term}=bridge({_cellColorResolver:resolver}),surface=installWhiteTerminalSurface(term,true);surface.attach();
 resolver.resolve({fg:0,bg:RGB|0x232d2a});assert.equal(resolver.result.bg&COLOR,RGB|0xaccef7);surface.dispose();
});

test('renderer replacements are rebound and OSC background writes are blocked only in white mode',()=>{
 const first={_cellColorResolver:{result:{},resolve(cell){this.result={...cell}}}},second={_rowFactory:{createRow(){return []}}};
 const oldResolve=first._cellColorResolver.resolve,oldCreate=second._rowFactory.createRow;
 const {term,osc}=bridge(first);const surface=installWhiteTerminalSurface(term,true);surface.attach();
 assert.equal(osc.get(11)('#000000'),true);assert.equal(osc.get(11)('?'),false);assert.equal(osc.get(111)(''),true);
 term._core._renderService._renderer.value=second;surface.attach();assert.equal(first._cellColorResolver.resolve,oldResolve);assert.notEqual(second._rowFactory.createRow,oldCreate);
 surface.setEnabled(false);assert.equal(osc.get(11)('#000000'),false);assert.equal(osc.get(111)(''),false);
 surface.dispose();assert.equal(second._rowFactory.createRow,oldCreate);assert.equal(osc.size,0);
});
