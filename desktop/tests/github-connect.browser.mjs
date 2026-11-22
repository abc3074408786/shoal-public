import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import {fileURLToPath} from 'node:url';
import {mkdirSync,readFileSync,writeFileSync,realpathSync} from 'node:fs';
import {resolve} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const evidence=resolve(root,`../artifacts/github-qr-20261019/browser-${Date.now()}`);mkdirSync(evidence,{recursive:true});
const qr=readFileSync(resolve(root,'../artifacts/github-qr-20261019/qr-fixture.svg'),'utf8');
const server=await createServer({root,server:{host:'127.0.0.1',port:0,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
let browser,page;const errors=[],checks=[],requests=[];
try {
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(10000);
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(()=>{Object.defineProperty(navigator,'clipboard',{value:{writeText:async text=>{window.__copied=text}}});window.open=url=>{window.__opened=url};});
  let authenticated=false,available=true,authState='waiting',statusError=false,repoFailure=false,saveFailure=false,startDelay=0,sources=[];
  const view=id=>({request_id:id,state:authState,...(authState==='waiting'?{user_code:'ABCD-1234',verification_uri:'https://github.com/login/device',qr_svg:qr}:authState==='complete'?{login:'fixture-user'}:{message:'授权未完成或已过期，请重新生成二维码。'})});
  await page.route('**/_velella',async route=>{
    const r=route.request().postDataJSON();requests.push(r);let value={ok:true};
    switch(r.cmd) {
      case 'workbench.snapshot':value={ok:true,projects:[],workspaces:[],tasks:[],backups:[],source_links:[]};break;
      case 'sources.sync':case 'sources.list':value={ok:true,sources,links:[],repositories:[{project_id:'p',project_name:'开发项目',provider:'github',host:'github.com',scope:'team/local'}]};break;
      case 'sources.github.account':value={ok:true,available,authenticated,login:authenticated?'fixture-user':null};break;
      case 'sources.github.auth.start':if(startDelay)await new Promise(resolve=>setTimeout(resolve,startDelay));value={ok:true,attempt:view(r.request_id)};break;
      case 'sources.github.auth.status':value=statusError?{ok:false,error:'fixture connection lost'}:{ok:true,attempt:view(r.request_id)};break;
      case 'sources.github.auth.cancel':value={ok:true,attempt:{request_id:r.request_id,state:'cancelled',message:'已停止等待授权。'}};break;
      case 'sources.github.repositories':value=repoFailure?{ok:false,error:'仓库暂不可用，请重试'}:{ok:true,repositories:r.page===2?[{full_name:'team/local',private:true,description:'中文开发仓库'}]:[{full_name:'fixture-user/example',private:false,description:'An accessible repository'},...Array.from({length:8},(_,i)=>({full_name:`fixture-user/long-development-repository-${i}`,private:true,description:'包含权限、预算、工作流、知识库和多语言交互的企业开发项目。'.repeat(3)}))],next_page:r.page===2?null:2};break;
      case 'sources.save':if(saveFailure){value={ok:false,error:'来源连接暂不可用'};break;}sources=[{...r.source,id:'fixture-source'}];value={ok:true,source:sources[0]};break;
      case 'sources.items':value={ok:true,items:[],next:null};break;
      default:value={ok:true,files:[],sessions:[],events:[],annotations:[]};
    }
    await route.fulfill({json:value});
  });
  const count=cmd=>requests.filter(r=>r.cmd===cmd).length;
  const dialog=()=>page.getByRole('dialog',{name:'连接 GitHub',exact:true});
  async function open(){await page.getByRole('button',{name:'连接来源',exact:true}).click();await dialog().waitFor();}
  async function generate(){await dialog().getByRole('button',{name:/生成.*二维码/}).click();await dialog().getByRole('img',{name:/GitHub 设备授权二维码/}).waitFor();}
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/`);await page.getByRole('button',{name:'任务',exact:true}).click();await open();
  await dialog().getByRole('button',{name:'生成登录二维码'}).waitFor();assert.equal(count('sources.github.auth.start'),0);assert.equal(await dialog().getByLabel('站点地址').count(),0);
  checks.push('unauthenticated connection is a QR flow; mount never starts authentication');
  await generate();assert.equal(count('sources.github.auth.start'),1);await dialog().getByText('ABCD-1234',{exact:true}).waitFor();
  await dialog().getByRole('button',{name:'复制设备码'}).click();assert.equal(await page.evaluate(()=>window.__copied),'ABCD-1234');
  await dialog().getByRole('button',{name:'在浏览器打开'}).click();assert.equal(await page.evaluate(()=>window.__opened),'https://github.com/login/device');
  await dialog().screenshot({path:resolve(evidence,'github-qr-dark.png')});await dialog().locator('img.github-qr').screenshot({path:resolve(evidence,'qr-decode.png')});
  checks.push('local SVG QR, device code copy and canonical browser alternative shown');
  authenticated=true;authState='complete';repoFailure=true;
  await dialog().getByText('仓库暂不可用，请重试',{exact:false}).waitFor();assert.equal(count('sources.github.auth.start'),1);
  repoFailure=false;await dialog().getByRole('button',{name:'刷新仓库'}).click();await dialog().getByRole('radio',{name:'fixture-user/example',exact:true}).waitFor();
  assert.equal(await dialog().locator('.github-repository').evaluateAll(rows=>rows.some(row=>row.scrollHeight>row.clientHeight+1)),false,'long repository descriptions must not be compressed or clipped');
  await dialog().getByRole('button',{name:'加载更多仓库'}).click();await dialog().getByLabel('搜索仓库').fill('local');await dialog().getByRole('radio',{name:'team/local',exact:true}).check();
  saveFailure=true;await dialog().getByRole('button',{name:'连接仓库',exact:true}).click();await dialog().getByText('来源连接暂不可用',{exact:false}).waitFor();assert.equal(count('sources.github.auth.start'),1);
  saveFailure=false;await dialog().screenshot({path:resolve(evidence,'github-repository.png')});await dialog().getByRole('button',{name:'连接仓库',exact:true}).click();await dialog().waitFor({state:'hidden'});
  const saved=requests.filter(r=>r.cmd==='sources.save').at(-1);assert.equal(saved.source.scope,'team/local');assert.equal(saved.source.project_id,'p');assert.equal(saved.source.auth_mode,'cli');assert.equal(saved.token,'');
  checks.push('confirmed login opens repository selector; pagination, filtering and retry preserve login and bind local project');
  await open();await dialog().getByRole('radio',{name:'fixture-user/example',exact:true}).waitFor();assert.equal(count('sources.github.auth.start'),1);await dialog().getByRole('button',{name:'取消',exact:true}).click();
  checks.push('existing login skips device authorization');
  authenticated=false;authState='waiting';await open();await generate();const cancelled=requests.filter(r=>r.cmd==='sources.github.auth.start').at(-1).request_id;
  await page.keyboard.press('Escape');await dialog().waitFor({state:'hidden'});await page.waitForTimeout(100);assert(requests.some(r=>r.cmd==='sources.github.auth.cancel'&&r.request_id===cancelled));
  checks.push('Escape closes the dialog and cancels only its login request');
  await page.getByRole('button',{name:'切换浅色主题',exact:true}).click();await page.setViewportSize({width:960,height:720});await open();await generate();
  await dialog().screenshot({path:resolve(evidence,'github-qr-light.png')});assert.equal(await dialog().evaluate(el=>el.scrollWidth>el.clientWidth),false);
  statusError=true;await dialog().getByRole('button',{name:'检查授权结果'}).waitFor();assert.equal(await dialog().locator('img.github-qr').count(),0);const started=count('sources.github.auth.start');
  statusError=false;authState='failed';await dialog().getByRole('button',{name:'检查授权结果'}).click();await dialog().getByRole('button',{name:'重新生成二维码'}).waitFor();assert.equal(count('sources.github.auth.start'),started);
  assert.equal(await dialog().getByText('ABCD-1234',{exact:true}).count(),0);authState='waiting';await generate();assert.equal(count('sources.github.auth.start'),started+1);
  await dialog().getByRole('button',{name:'停止等待',exact:true}).click();await dialog().getByRole('button',{name:'重新生成二维码'}).waitFor();await dialog().getByRole('button',{name:'取消',exact:true}).click();
  checks.push('uncertain and expired attempts hide stale QR; retry requires explicit action; light/narrow dialog fits');
  available=false;await open();await dialog().getByText(/扫码连接需要 GitHub CLI/).waitFor();assert.equal(await dialog().getByRole('button',{name:'生成登录二维码'}).count(),0);
  await dialog().getByRole('button',{name:'手动连接 / 企业站点'}).click();await dialog().getByLabel('站点地址').waitFor();await dialog().getByLabel('登录方式').selectOption('token');await dialog().getByLabel('个人访问 Token').waitFor();await dialog().getByRole('button',{name:'取消',exact:true}).click();
  checks.push('missing CLI has a manual/enterprise/token fallback');
  available=true;startDelay=300;await open();await dialog().getByRole('button',{name:'生成登录二维码'}).click();const late=requests.filter(r=>r.cmd==='sources.github.auth.start').at(-1).request_id;await page.keyboard.press('Escape');await page.waitForTimeout(450);
  assert(requests.some(r=>r.cmd==='sources.github.auth.cancel'&&r.request_id===late));assert.equal(await dialog().count(),0);assert.deepEqual(errors,[]);
  checks.push('closing during authorization startup cancels the late response');
  writeFileSync(resolve(evidence,'result.json'),JSON.stringify({status:'passed',checks,errors,realAuthentication:false,externalWrites:0},null,2));console.log(JSON.stringify({status:'passed',evidence,checks}));
} catch(error) {
  if(page)await page.screenshot({path:resolve(evidence,'failure.png')}).catch(()=>{});
  writeFileSync(resolve(evidence,'result.json'),JSON.stringify({status:'failed',checks,errors,error:String(error)},null,2));throw error;
} finally {await browser?.close();await server.close();}
