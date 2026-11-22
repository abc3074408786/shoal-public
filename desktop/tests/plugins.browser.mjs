import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {resolve} from 'node:path';import {mkdirSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url)),out=resolve(root,'../artifacts/plugins-workbench-20261102');
const checks=[],check=name=>{checks.push(name);console.log('PASS '+name)};
const server=await createServer({root,configFile:false,plugins:[react(),{name:'deny-runtime',configureServer(s){s.middlewares.use('/_velella',(_q,r)=>{r.statusCode=403;r.end('Synthetic preview only')})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
let browser;
try{
  mkdirSync(out,{recursive:true});await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});
  const context=await browser.newContext({viewport:{width:1600,height:1000}}),page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(String(e)));
  page.setDefaultTimeout(10000);
  await page.goto(server.resolvedUrls.local[0]+'tests/plugins.html');
  const open=async()=>{await page.getByRole('navigation',{name:'主导航'}).getByRole('button',{name:'插件',exact:true}).click();await page.getByRole('heading',{name:'插件',exact:true}).waitFor();await page.getByRole('button',{name:'发现',exact:true}).click()};
  await page.getByRole('textbox',{name:'项目草稿'}).fill('草稿仍然保留');await open();
  await page.locator('.plugin-row').last().waitFor();assert.equal(await page.locator('.plugin-row').count(),18);
  await page.locator('.plugins-page img').evaluateAll(imgs=>Promise.all(imgs.map(i=>i.decode())));
  await page.screenshot({path:resolve(out,'plugins-desktop.png')});
  await page.getByRole('searchbox',{name:'搜索插件',exact:true}).fill('github');assert.equal(await page.locator('.plugin-row').count(),1);await page.getByRole('button',{name:'查看 GitHub',exact:true}).click();await page.getByRole('complementary',{name:'插件详情'}).getByRole('heading',{name:'GitHub',exact:true}).waitFor();assert.match(await page.getByRole('link',{name:'查看服务官网'}).getAttribute('href'),/^https:/);
  await page.getByRole('searchbox',{name:'搜索插件',exact:true}).fill('missing-query');await page.getByRole('heading',{name:'没有找到插件'}).waitFor();await page.getByRole('button',{name:'清除搜索'}).click();
  await page.getByRole('combobox',{name:'服务类别'}).selectOption('开发协作');assert.equal(await page.locator('.plugin-row').count(),2);await page.getByRole('combobox',{name:'服务类别'}).selectOption('');check('public search, service details, empty state and category filtering');
  await page.getByRole('button',{name:'添加插件',exact:true}).click();await page.getByRole('textbox',{name:'MCP 目录文件路径'}).fill('D:/synthetic/catalog.json');
  await page.evaluate(()=>window.__plugins.fail=true);await page.getByRole('button',{name:'载入目录预览'}).click();await page.getByRole('alert').filter({hasText:'目录文件不可用'}).waitFor();await page.evaluate(()=>window.__plugins.fail=false);await page.getByRole('button',{name:'载入目录预览'}).click();
  await page.getByRole('checkbox',{name:'search',exact:true}).check();await page.getByRole('button',{name:'确认来源并添加所选工具'}).click();await page.getByRole('status').filter({hasText:'已添加 docs'}).waitFor();
  assert.deepEqual(await page.evaluate(()=>JSON.parse(localStorage.getItem('velella:plugins:v1'))),[{id:'docs',path:'D:/synthetic/catalog.json',digest:'sha256:reviewed',enabled:true}]);
  assert.deepEqual(await page.evaluate(()=>window.__plugins.requests.find(r=>r.cmd==='orchestration.mcp.confirm').tools),['search']);check('preview failure recovers; confirmed tool subset stores a reference without command or credentials');
  await page.getByRole('button',{name:'返回工作台'}).click();await page.getByRole('navigation',{name:'主导航'}).getByRole('button',{name:'工作台',exact:true}).waitFor();await page.getByRole('button',{name:'返回',exact:true}).click();await page.getByRole('heading',{name:'插件',exact:true}).waitFor();await page.getByRole('button',{name:'返回',exact:true}).click();assert.equal(await page.getByRole('textbox',{name:'项目草稿'}).inputValue(),'草稿仍然保留');
  // Re-mount the role selector as a real user opening a plan would.
  await page.reload();await page.locator('.plugin-library-choices').getByRole('button',{name:'docs',exact:true}).click();await page.getByRole('checkbox',{name:'search',exact:true}).waitFor();assert.equal(await page.getByRole('checkbox',{name:'search',exact:true}).isChecked(),false);await page.getByRole('checkbox',{name:'read',exact:true}).check();await page.getByRole('button',{name:'确认来源并添加所选工具'}).click();assert.equal(await page.getByRole('status',{name:'所选插件'}).textContent(),'docs read');check('project remains mounted; history and persisted plugin selection require fresh source review');
  await open();await page.getByRole('button',{name:/^已添加/}).click();await page.getByRole('button',{name:'管理 docs'}).click();await page.getByRole('button',{name:'从个人清单移除'}).click();await page.getByRole('dialog',{name:'确认操作'}).getByRole('button',{name:'确认',exact:true}).click();await page.getByRole('button',{name:'已添加 1',exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'管理 docs'}).count(),0);check('personal management removes only the stored reference');
  await page.getByRole('navigation',{name:'插件视图'}).getByRole('button',{name:/^技能/}).click();await page.getByRole('heading',{name:'可用技能 2'}).waitFor();await page.getByRole('searchbox',{name:'搜索技能'}).fill('review');assert.equal(await page.locator('.plugin-skill').count(),1);await page.getByRole('searchbox',{name:'搜索技能'}).fill('');await page.evaluate(()=>window.__plugins.skillsFail=true);await page.getByRole('button',{name:'刷新插件与技能'}).click();await page.getByRole('alert').filter({hasText:'技能列表读取失败'}).waitFor();await page.evaluate(()=>window.__plugins.skillsFail=false);check('skills use real capability contract and expose discovery failures');
  await page.getByRole('button',{name:'发现',exact:true}).click();
  for(const viewport of [{width:900,height:800},{width:480,height:800}]){await page.setViewportSize(viewport);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1),false);await page.screenshot({path:resolve(out,`plugins-${viewport.width}.png`)});}
  await page.setViewportSize({width:1600,height:1000});await page.evaluate(()=>document.documentElement.dataset.theme='dark');await page.screenshot({path:resolve(out,'plugins-dark.png')});check('desktop, narrow and small-window layouts; light and dark surfaces');
  assert.deepEqual(errors,[]);writeFileSync(resolve(out,'browser-results.json'),JSON.stringify({checks,errors,synthetic:true,realServiceAuthorization:false},null,2));console.log('ALL '+checks.length+' SCENARIOS PASSED');
}finally{await browser?.close();await server.close()}
