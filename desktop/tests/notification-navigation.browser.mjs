import assert from 'node:assert/strict';import {createRequire} from 'node:module';
import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
const server=await createServer({root,configFile:false,plugins:[react(),{name:'deny-real-daemon',configureServer(s){s.middlewares.use('/_velella',(_req,res)=>{res.statusCode=403;res.end('Synthetic fixture only')})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
const remote={kind:'ssh',host_id:'11111111-1111-4111-8111-111111111111',server_id:'22222222-2222-4222-8222-222222222222'},checks=[];
let browser;
try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1280,height:800}}),errors=[];page.on('pageerror',e=>errors.push(String(e)));
  await page.goto(server.resolvedUrls.local[0]+'tests/settings-extension.html');await page.getByRole('heading',{name:'Agent',exact:true}).waitFor();await page.getByRole('button',{name:'取消',exact:true}).click();
  await page.waitForFunction(()=>!!window.__fixture.refresh);
  await page.evaluate(()=>{window.__fixture.delayNextSnapshot=true;window.__fixture.pendingRefresh=window.__fixture.refresh()});await page.waitForFunction(()=>!!window.__fixture.releaseSnapshot);
  await page.evaluate(remote=>window.__fixture.selectTarget(remote),remote);await page.getByRole('button',{name:'打开项目 远端设置验收项目',exact:true}).waitFor();
  await page.evaluate(async()=>{window.__fixture.releaseSnapshot();await window.__fixture.pendingRefresh});assert.equal(await page.getByRole('button',{name:'打开项目 远端设置验收项目',exact:true}).isVisible(),true);checks.push('late local manual refresh cannot overwrite the active remote snapshot');
  await page.evaluate(()=>window.__fixture.handlers['notification-action']({payload:{notification:{extra:{taskId:'task-a',target:{kind:'local'}}}}}));
  await page.waitForFunction(()=>window.__fixture.currentTarget().kind==='local'&&localStorage.getItem('velella:workspace')==='task:task-a');checks.push('clicking the local-host notification while remote switches back and opens its task');
  await page.evaluate(()=>window.dispatchEvent(new Event('velella:open-settings')));await page.getByRole('navigation',{name:'设置分类'}).getByRole('button',{name:'外观',exact:true}).click();await page.getByRole('combobox',{name:'界面主题'}).selectOption('dark');
  await page.evaluate(remote=>window.__fixture.handlers['notification-action']({payload:{notification:{extra:{taskId:'remote-task-a',target:remote}}}}),remote);
  await page.getByRole('alert').filter({hasText:'未连接'}).waitFor();assert.equal(await page.evaluate(()=>window.__fixture.currentTarget().kind),'local');assert.equal(await page.getByRole('combobox',{name:'界面主题'}).inputValue(),'dark');checks.push('disconnected notification host is reported without fallback or discarding a settings draft');
  await page.evaluate(remote=>{window.__fixture.connections=[{state:'connected',target:remote}];window.__fixture.handlers['notification-action']({payload:{notification:{extra:{taskId:'remote-task-a',target:remote}}}})},remote);
  await page.getByRole('dialog').filter({hasText:'放弃未保存的设置'}).waitFor();await page.getByRole('dialog').getByRole('button',{name:'取消',exact:true}).click();assert.equal(await page.evaluate(()=>window.__fixture.currentTarget().kind),'local');checks.push('dirty settings require confirmation before notification navigation');
  await page.getByRole('button',{name:'取消',exact:true}).click();
  await page.evaluate(remote=>window.__fixture.handlers['notification-action']({payload:{notification:{extra:{taskId:'remote-task-a',target:remote}}}}),remote);
  await page.waitForFunction(()=>window.__fixture.currentTarget().kind==='ssh'&&Object.entries(localStorage).some(([key,value])=>key.startsWith('ssh:')&&key.endsWith('velella:workspace')&&value==='task:remote-task-a'));checks.push('a connected remote notification opens the task on its exact host identity');
  assert.deepEqual(errors,[]);writeFileSync(new URL('../../artifacts/settings-extension-20261102/notification-navigation-results.json',import.meta.url),JSON.stringify({checks,pageErrors:errors,synthetic:true},null,2));for(const name of checks)console.log('PASS '+name);
}finally{await browser?.close();await server.close()}
