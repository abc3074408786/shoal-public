import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {resolve} from 'node:path';import {mkdirSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url)),out=resolve(root,'../artifacts/plugins-layout-fix-20261102');
const server=await createServer({root,configFile:false,plugins:[react()],server:{host:'127.0.0.1',port:0},logLevel:'error'});
let browser;const records=[];
try{
 mkdirSync(out,{recursive:true});await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});
 const page=await browser.newPage();page.setDefaultTimeout(30000);await page.goto(server.resolvedUrls.local[0]+'tests/plugins.html');await page.getByRole('textbox',{name:'项目草稿',exact:true}).waitFor();
 await page.getByRole('navigation',{name:'主导航'}).getByRole('button',{name:'插件',exact:true}).click();await page.getByRole('heading',{name:'插件',exact:true}).waitFor();
 for(const viewport of [{width:1442,height:964},{width:900,height:800},{width:480,height:800}]){
  await page.setViewportSize(viewport);
  const buttons=await page.locator('.plugins-actions button').evaluateAll(elements=>elements.map(button=>{
   const box=button.getBoundingClientRect(),style=getComputedStyle(button),range=document.createRange();range.selectNodeContents(button);const text=range.getBoundingClientRect();
   return {name:button.getAttribute('aria-label'),width:box.width,scroll:button.scrollWidth,client:button.clientWidth,x:box.x,right:box.right,textRight:text.right,headerRight:button.closest('.plugins-header').getBoundingClientRect().right,viewport:innerWidth,overflow:style.overflow,shrink:style.flexShrink};
  }));
  await page.screenshot({path:resolve(out,'page-'+viewport.width+'.png'),animations:'disabled'});
  await page.getByRole('button',{name:'添加插件',exact:true}).click();await page.screenshot({path:resolve(out,'dialog-'+viewport.width+'.png'),animations:'disabled'});const modal=await page.getByRole('dialog',{name:'添加插件',exact:true}).boundingBox();await page.keyboard.press('Escape');await page.locator('.ui-dialog').waitFor({state:'detached'});
  records.push({viewport,buttons,modal});
 }
 writeFileSync(resolve(out,'geometry.json'),JSON.stringify(records,null,2));
 const failures=[];for(const row of records){for(const b of row.buttons){if(b.scroll>b.client+1||b.textRight>b.right+1||b.right>Math.min(b.headerRight,row.viewport.width)+1)failures.push(row.viewport.width+' '+b.name+' clips/overflows: '+JSON.stringify(b));}if(row.modal&&(row.modal.x<0||row.modal.x+row.modal.width>row.viewport.width+1))failures.push('dialog exceeds viewport '+row.viewport.width)}
 console.log(JSON.stringify(records));assert.deepEqual(failures,[],'Header labels and dialogs must fit at each supported width');console.log('PASS layout labels and dialogs');
}finally{await browser?.close();await server.close()}
