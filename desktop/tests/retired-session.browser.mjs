import {test} from 'node:test';import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
test('retired chats are inert; both new-chat entries are absent; native Codex terminal still launches',async()=>{
 let server,browser;try{
  server=await createServer({root,configFile:false,plugins:[react()],server:{host:'127.0.0.1',port:0},logLevel:'error'});await server.listen();
  browser=await chromium.launch({channel:'msedge',headless:true});const p=await browser.newPage({viewport:{width:1100,height:800}});
  await p.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/retired-session.html`);
  await p.getByRole('button',{name:'新建 Codex 终端',exact:true}).waitFor();
  assert.equal(await p.getByRole('button',{name:'新建 Codex 聊天',exact:true}).count(),0);
  await p.getByRole('button',{name:'旧聊天 已停用',exact:true}).click();
  await p.getByText('独立 Codex 聊天已停用，历史数据保留在原会话目录。请新建 Codex 终端继续工作。',{exact:true}).waitFor();
  assert.equal(await p.locator('textarea,.xterm').count(),0);
  assert.equal(await p.evaluate(()=>retiredQA.requests.some(q=>q.cmd.startsWith('workbench.chat.')||q.cmd==='workbench.terminal.output'||q.cmd==='workbench.feedback.send')),false);
  await p.getByRole('button',{name:'关闭标签 旧聊天',exact:true}).click();
  await p.getByRole('button',{name:'新建 Codex 终端',exact:true}).click();
  await p.getByRole('heading',{name:'新建 Codex 终端',exact:true}).waitFor();await p.keyboard.press('Escape');
  await p.getByRole('button',{name:'新建标签',exact:true}).click();
  assert.equal(await p.getByRole('menuitem',{name:'新建 Codex 聊天',exact:true}).count(),0);
  await p.getByRole('menuitem',{name:'新建 Codex 终端',exact:true}).click();
  await p.getByRole('combobox').selectOption('native');
  await p.getByRole('button',{name:'创建并打开',exact:true}).click();await p.locator('.xterm').waitFor();
  assert.deepEqual(await p.evaluate(()=>retiredQA.requests.filter(q=>q.cmd==='workbench.terminal.open').map(q=>q.launch)),['codex']);
  assert.equal(await p.evaluate(()=>retiredQA.sessions.length),2);
 }finally{await browser?.close();await server?.close()}
});
