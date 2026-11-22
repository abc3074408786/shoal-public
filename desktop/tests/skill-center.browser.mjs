import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {resolve,join} from 'node:path';
import {mkdtemp,mkdir,writeFile,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
const require=createRequire(import.meta.url),{chromium}=require('playwright'),{create}=require('../plugins/skill-center/service.cjs');
const root=fileURLToPath(new URL('..',import.meta.url)),temp=await mkdtemp(join(tmpdir(),'velella-center-browser-')),out=resolve(root,'../artifacts/skill-center-20261109');
const project=join(temp,'project'),source=join(temp,'source'),calls=[],errors=[];
await mkdir(project);await mkdir(join(source,'references'),{recursive:true});
const body='---\nname: sample\ndescription: Skill Center fixture\n---\nRead references/rules.txt and return its marker.\n';
await writeFile(join(source,'SKILL.md'),body);await writeFile(join(source,'references/rules.txt'),'SKILL_CENTER_RESOURCE_20261109');
const runtime={request:async req=>{calls.push(req);if(req.cmd==='workbench.snapshot')return {ok:true,workspaces:[{id:'w',name:'隔离项目',path:project,archived:false}]};if(req.cmd==='workbench.terminal.open')return {ok:true,session_id:req.request_id};if(req.cmd==='workbench.sessions')return {ok:true,sessions:calls.filter(c=>c.cmd==='workbench.terminal.open').map(c=>({id:c.request_id,status:{state:'waiting_input',input_sequence:0,exit:null}}))};if(req.cmd==='workbench.feedback.send')return {ok:true,receipt:{state:'submitted'}};if(req.cmd==='workbench.creation.status')return {ok:true};throw Error('Unexpected runtime request '+req.cmd)}};
const service=create({dataDir:join(temp,'library'),stateDir:join(temp,'state'),cliPath:resolve(root,'../target/debug/velella-cli.exe'),runtime,locate:async p=>p==='zcode-free'?[]:[p]});
const server=await createServer({root,configFile:false,plugins:[react(),{name:'skill-center-fixture',configureServer(server){for(const route of ['/__skill_center','/_velella'])server.middlewares.use(route,(req,res)=>{let body='';req.on('data',chunk=>body+=chunk);req.on('end',async()=>{res.setHeader('Content-Type','application/json');try{const r=JSON.parse(body);res.end(JSON.stringify(await(route==='/_velella'?runtime.request(r):service.call(r))))}catch(error){res.statusCode=400;res.end(JSON.stringify({error:error.message}))}})})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
let browser;
try{
 await mkdir(out,{recursive:true});await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1440,height:1000}});page.setDefaultTimeout(20000);page.on('pageerror',e=>errors.push(String(e)));
 await page.goto(server.resolvedUrls.local[0]+'tests/skill-center.html');await page.getByRole('combobox',{name:'技能工作区'}).selectOption('w');
 await page.getByRole('button',{name:'导入技能',exact:true}).click();await page.getByRole('textbox',{name:'技能名称'}).fill('sample');await page.getByRole('textbox',{name:'包含 SKILL.md 的绝对目录'}).fill(source);await page.getByRole('combobox',{name:'范围',exact:true}).selectOption('project');await page.getByRole('button',{name:'预览来源与资源'}).click();await page.getByRole('button',{name:'确认导入到当前项目'}).click();
 await page.getByRole('button',{name:/sample.*当前项目/}).click();await page.getByRole('heading',{name:'sample',exact:true}).waitFor();assert.equal(await readFile(join(project,'.velella/skills/packages/sample/references/rules.txt'),'utf8'),'SKILL_CENTER_RESOURCE_20261109');
 await page.getByLabel('Claude Code',{exact:true}).check();await page.getByRole('button',{name:'预览分发目标'}).click();await page.getByRole('button',{name:'确认分发',exact:true}).click();await page.getByText('已逐目标处理。',{exact:false}).waitFor();
 assert.equal(await readFile(join(project,'.claude/skills/sample/SKILL.md'),'utf8'),body);assert.equal(await readFile(join(project,'.agents/skills/sample/references/rules.txt'),'utf8'),'SKILL_CENTER_RESOURCE_20261109');
 await page.getByRole('button',{name:'取消预览 / 刷新'}).click();await page.getByRole('button',{name:/sample.*当前项目/}).click();await page.getByRole('textbox',{name:'任务',exact:true}).fill('Return the resource marker.');await page.getByRole('button',{name:'启动技能任务',exact:true}).click();await page.getByText('已创建技能任务会话',{exact:false}).waitFor();assert(calls.find(c=>c.cmd==='workbench.terminal.open').initial_prompt.text.includes('Read references/rules.txt'));
 await writeFile(join(source,'references/rules.txt'),'UPDATED_RESOURCE');await page.getByRole('button',{name:'检查更新',exact:true}).click();await page.getByText(/modified · references[\\/]rules.txt/).waitFor();await page.getByRole('button',{name:'确认更新到当前项目'}).click();assert.equal(await readFile(join(project,'.claude/skills/sample/references/rules.txt'),'utf8'),'SKILL_CENTER_RESOURCE_20261109');
 await page.getByRole('button',{name:/sample.*当前项目/}).click();await page.getByRole('button',{name:'预览分发目标'}).click();await page.getByRole('button',{name:'确认分发',exact:true}).click();await page.getByText('已逐目标处理。',{exact:false}).waitFor();assert.equal(await readFile(join(project,'.claude/skills/sample/references/rules.txt'),'utf8'),'UPDATED_RESOURCE');await page.getByRole('button',{name:'取消预览 / 刷新'}).click();
 await page.getByRole('button',{name:/sample.*当前项目/}).click();
 await page.getByText('技能内容与来源',{exact:true}).waitFor();
 for(const [width,height] of [[1440,1000],[900,850],[480,900]]){await page.setViewportSize({width,height});await page.screenshot({path:join(out,`skill-center-${width}.png`)});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));}
 await page.evaluate(()=>document.documentElement.dataset.theme='dark');await page.screenshot({path:join(out,'skill-center-dark.png')});assert.deepEqual(errors,[]);
 await writeFile(join(out,'browser-result.json'),JSON.stringify({passed:true,engine:'actual Rust CLI',runtime:'synthetic; no model called',screenshots:[1440,900,480],errors},null,2));console.log('PASS: actual Rust import, scope isolation, resource distribution, update, frozen launch and responsive UI');
}catch(error){if(browser){for(const context of browser.contexts())for(const page of context.pages())await page.screenshot({path:join(out,'browser-failure.png')}).catch(()=>{});}throw error;}finally{if(browser)await browser.close();await server.close();await rm(temp,{recursive:true,force:true});}
