import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';

const require=createRequire(import.meta.url);
const {chromium}=require(process.env.VELELLA_TEST_PLAYWRIGHT||'playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const output=resolve(root,'../artifacts/cli-task-progress-20261105');
const html='<html><body><div id="chrome"></div><div id="root"></div><script type="module" src="/__cli-monitor.tsx"></script></body></html>';
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';
import Workbench from '/src/Workbench';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import {ChromeTarget} from '/src/WindowChrome';
import TaskProgress from '/src/TaskProgress';
import '/src/workspace.css';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
document.documentElement.dataset.theme='light';
const workspace={id:'monitor',project_id:'p',name:'知识蒸馏',path:'fixture',kind:'folder',archived:false,favorite:false,position:0};
createRoot(document.getElementById('root')).render(<TooltipProvider><ConfirmationProvider><ChromeTarget.Provider value={document.getElementById('chrome')}><div className="ide-shell" style={{display:'flex',flexDirection:'column',height:'100vh'}}>{location.search.includes('progress-only')?<TaskProgress workspaceId="monitor" sessionId="codex" taskName="旧受管任务名称"/>:<Workbench workspace={workspace} workspaceActions={null}/>}</div></ChromeTarget.Provider></ConfirmationProvider></TooltipProvider>);`;

test('right task tab shows whole-task progress, live updates, session isolation and truthful empty states',async()=>{
  const server=await createServer({root,configFile:false,plugins:[react(),{
    name:'cli-monitor-fixture',
    configureServer(s){s.middlewares.use((req,res,next)=>{
      if(req.url?.split('?')[0]==='/__cli-monitor'){void s.transformIndexHtml(req.url,html).then(x=>{res.setHeader('Content-Type','text/html');res.end(x)});return}
      if(req.url==='/__cli-monitor.tsx'){void s.transformRequest(req.url).then(x=>{res.setHeader('Content-Type','text/javascript');res.end(x.code)});return}
      next();
    })},
    resolveId(id){if(id==='/__cli-monitor.tsx')return id},load(id){if(id==='/__cli-monitor.tsx')return entry},
  }],server:{host:'127.0.0.1',port:0},logLevel:'error'});
  let browser;
  mkdirSync(output,{recursive:true});
  const requests=[],errors=[];
  const event=(seq,event,extra={})=>({seq,event,time:'2026-11-05T02:00:00Z',native_id:'native',agent_id:'',turn_id:'turn',call_id:'read',tool:'Read',input:'{"file_path":"知识蒸馏.md"}',output:'',failed:false,...extra});
  const events={codex:[event(0,'PreToolUse')],claude:[event(0,'PermissionRequest',{call_id:'bash',tool:'Bash',input:'{"command":"npm test","description":"运行项目测试"}'})],shell:[]};
  const steps=[{id:'1',title:'复现失败',status:'completed',blocked_by:[]},{id:'2',title:'修改登录处理',status:'in_progress',active_form:'正在修改登录处理',blocked_by:[]},{id:'3',title:'运行回归测试 <img src=x onerror=alert(1)>',status:'pending',blocked_by:['2']}];
  const progress={codex:{state:'unavailable',steps:[],reason:'等待此会话创建原生记录。'},claude:{state:'ready',steps:structuredClone(steps),task_label:'整理任务资料',reported_at:'2026-11-05T06:00:00Z'},zcode:{state:'ready',steps:structuredClone(steps),task_label:'ZCode 任务',plan_scope:'session'},shell:{state:'unsupported',steps:[],reason:'此 CLI 或执行模式尚未接入可信的任务计划。'}};
  const swarmRuns=[{id:'swarm',goal:'多 CLI 协作交付',state:'running',confirmed:true,steps:[{id:'a',title:'编写代码',status:'completed',provider:'codex',blocked_by:[]},{id:'b',title:'探索问题',status:'awaiting_review',provider:'omp',blocked_by:[]},{id:'c',title:'集成成果',status:'blocked',provider:'opencode',blocked_by:['b']},{id:'d',title:'独立验证',status:'pending',provider:'claude-code',blocked_by:['c']}]}];
  const sessions=[
    {id:'codex',title:'Codex',kind:'terminal',program:'codex',task_id:null,native_id:null,status:{exit:null,state:'running',input_protocol:1},recoverable:false},
    {id:'claude',title:'Claude',kind:'terminal',program:'claude',task_id:null,native_id:null,status:{exit:null,state:'waiting_permission',input_protocol:1},recoverable:false},
    {id:'shell',title:'PowerShell',kind:'terminal',program:'powershell',task_id:null,native_id:null,status:{exit:0,state:'exited',input_protocol:1},recoverable:false},
    {id:'zcode',title:'ZCode',kind:'terminal',program:'zcode-free',task_id:null,native_id:null,status:{exit:null,state:'running',input_protocol:1},recoverable:false},
  ];
  let layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
  try{
    await server.listen();
    browser=await chromium.launch({channel:'msedge',headless:true});
    const page=await browser.newPage({viewport:{width:1280,height:880}});page.setDefaultTimeout(10000);
    page.on('pageerror',e=>errors.push(e.message));
    await page.addInitScript(()=>{localStorage.setItem('velella:ui:right-open','true');localStorage.setItem('velella:ui:right-tab','"files"');localStorage.setItem('velella:ui:right-width','360');localStorage.setItem('velella:terminal-preferences:v1','{"renderer":"dom","cursorBlink":false}')});
    await page.route('**/_velella',async route=>{
      const q=route.request().postDataJSON();requests.push(q);let r={ok:true};
      if(q.cmd==='workbench.layout')r={...r,layout,split_layout_version:1};
      else if(q.cmd==='workbench.sessions')r.sessions=sessions;
      else if(q.cmd==='workbench.files')r.files=['.claude/settings.json','知识蒸馏.md'];
      else if(q.cmd==='workbench.git')r={...r,branch:'',status:'',diff:''};
      else if(q.cmd==='workbench.layout.save'){layout={...q.layout,revision:layout.revision+1};r.layout=layout}
      else if(q.cmd==='workbench.terminal.activity'){const all=events[q.session_id]||[];r={...r,enabled:q.session_id!=='shell',events:all.slice(q.since||0),next:all.length,total:all.length}}
      else if(q.cmd==='workbench.terminal.progress')r={...r,session_id:q.session_id,...progress[q.session_id]};
      else if(q.cmd==='orchestration.progress')r={...r,workspace_id:q.workspace_id,runs:swarmRuns};
      else if(q.cmd.endsWith('.output'))r={...r,from:0,end:0,total:0,data:'',geometry:[]};
      await route.fulfill({json:r});
    });
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__cli-monitor`);
    const tabs=page.getByRole('tablist',{name:'工作区工具视图'}),monitor=page.getByRole('region',{name:'CLI 任务监控'});
    await tabs.getByRole('tab',{name:'任务',exact:true}).click();
    await monitor.getByText('4 个会话',{exact:true}).waitFor();
    const plan=monitor.getByRole('region',{name:'整项任务进度'});
    await plan.getByText('暂时无法读取任务计划',{exact:true}).waitFor();
    progress.codex={state:'ready',steps:structuredClone(steps),task_label:'修复登录问题',reported_at:'2026-11-05T06:00:00Z'};
    await plan.getByText('修复登录问题',{exact:true}).waitFor();
    await plan.getByText('正在修改登录处理',{exact:true}).waitFor();
    assert.equal(await plan.locator('progress').getAttribute('value'),'1');
    assert.equal(await plan.locator('progress').getAttribute('max'),'3');
    assert.equal(await plan.locator('img').count(),0,'step titles render as text');
    assert.ok(!requests.some(q=>q.cmd==='workbench.terminal.activity'),'logs are auxiliary and initially collapsed');
    progress.codex.steps[1].status='completed';progress.codex.steps[2].status='in_progress';
    await page.waitForFunction(()=>document.querySelector('.task-progress progress')?.value===2);
    progress.codex.steps[2].status='completed';
    await plan.getByText('所有计划步骤已标记完成，实际结果仍需核验。',{exact:true}).waitFor();
    progress.codex.steps=[{id:'bad',title:'invalid',status:'invented',blocked_by:[]}];
    await plan.getByText('进度读取中断，以下为上次计划。可重新读取。',{exact:true}).waitFor();
    assert.equal(await plan.locator('progress').getAttribute('value'),'3','malformed response cannot overwrite last known plan');
    progress.codex={state:'no_plan',steps:[],reason:'请先上报本轮计划。'};
    await plan.getByText('CLI 尚未上报本轮计划',{exact:true}).waitFor();
    assert.equal(await plan.locator('progress').count(),0,'next task cannot inherit the completed plan');
    await monitor.locator('.cli-monitor-activity>summary').click();
    await monitor.getByText('读取文件 · 知识蒸馏.md',{exact:true}).last().waitFor();
    assert.equal(await tabs.getByRole('tab',{name:'任务',exact:true}).getAttribute('aria-selected'),'true');
    assert.ok(requests.some(q=>q.cmd==='workbench.terminal.activity'&&q.session_id==='codex'));
    events.codex.push(event(1,'PostToolUse',{output:'<img src=x onerror=alert(1)>\n已读取文档'}));
    await monitor.locator('.activity-item.returned').waitFor();
    await monitor.locator('.activity-item summary').click();
    await monitor.getByText('已读取文档',{exact:false}).waitFor();
    assert.equal(await monitor.locator('.activity-detail img').count(),0);
    await monitor.getByRole('button',{name:'Claude 等待授权',exact:true}).click();
    await plan.getByText('整理任务资料',{exact:true}).waitFor();
    assert.equal(await plan.getByText('修复登录问题',{exact:true}).count(),0);
    assert.equal(await plan.locator('progress').getAttribute('value'),'1');
    await monitor.locator('.activity-item.waiting').waitFor();
    assert.equal(await monitor.getByText('读取文件 · 知识蒸馏.md',{exact:true}).count(),0,'session changes discard the previous log');
    assert.ok(requests.some(q=>q.cmd==='workbench.terminal.activity'&&q.session_id==='claude'&&q.since===0));
    assert.ok(await page.locator('.wb-files').evaluate(el=>Math.abs(el.getBoundingClientRect().width-360)<2),'monitor occupies the right sidebar');
    const swarm=monitor.getByRole('region',{name:'鱼群任务进度'});
    await swarm.getByText('多 CLI 协作交付',{exact:true}).waitFor();
    assert.equal(await swarm.locator('progress').getAttribute('value'),'1','reported work is not accepted work');
    await swarm.getByText('已回报，待审查',{exact:true}).waitFor();
    await swarm.getByText('OMP',{exact:true}).waitFor();await swarm.getByText('OpenCode · 等待 b',{exact:true}).waitFor();
    swarmRuns[0].steps[1].status='completed';
    await page.waitForFunction(()=>document.querySelector('.swarm-progress progress')?.value===2);
    await page.screenshot({path:resolve(output,'right-task-panel.png')});
    await monitor.getByRole('button',{name:'打开会话',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('.ide-tab.selected')?.textContent.includes('Claude'));
    assert.ok(layout.panes.some(p=>p.tabs.some(t=>t.target==='claude')&&p.active));
    await monitor.getByRole('button',{name:'PowerShell 已结束',exact:true}).click();
    await plan.getByText('此 CLI 的计划尚未接入',{exact:true}).waitFor();
    assert.equal(await plan.locator('progress').count(),0);
    await monitor.getByRole('button',{name:'ZCode 运行中',exact:true}).click();
    await plan.getByRole('heading',{name:'ZCode 任务',exact:true}).waitFor();await plan.getByText('ZCode 会话任务清单',{exact:true}).waitFor();
    assert.equal(await plan.locator('progress').getAttribute('value'),'1');
    await tabs.getByRole('tab',{name:'文件',exact:true}).click();
    await page.locator('.wb-files-heading').getByText('知识蒸馏',{exact:true}).waitFor();
    const shellReads=requests.filter(q=>q.cmd.endsWith('.activity')&&q.session_id==='shell').length;
    await page.waitForTimeout(1200);
    assert.equal(requests.filter(q=>q.cmd.endsWith('.activity')&&q.session_id==='shell').length,shellReads,'hidden monitor stops reading');
    await tabs.getByRole('tab',{name:'任务',exact:true}).click();
    await monitor.getByText('4 个会话',{exact:true}).waitFor();
    await monitor.getByRole('button',{name:'Claude 等待授权',exact:true}).click();
    await plan.getByText('整理任务资料',{exact:true}).waitFor();
    await page.setViewportSize({width:900,height:760});
    await page.evaluate(()=>{const body=document.querySelector('.wb-body');body.style.gridTemplateColumns='minmax(0,1fr) 220px'});
    assert.ok(await page.locator('.wb-files').evaluate(el=>Math.abs(el.getBoundingClientRect().width-220)<2),'minimum sidebar width is exercised');
    assert.equal(await page.locator('.wb-files').evaluate(el=>el.scrollWidth>el.clientWidth),false,'minimum-width panel must not overflow');
    assert.equal(await tabs.evaluate(el=>el.scrollWidth>el.clientWidth),false,'four tabs fit at minimum panel width');
    await page.screenshot({path:resolve(output,'right-task-panel-narrow.png')});
    sessions.splice(0);
    await monitor.getByText('当前工作区还没有 CLI 会话。从“新建标签”启动后，可在这里查看状态和操作记录。',{exact:true}).waitFor();
    progress.codex={state:'ready',steps:structuredClone(steps),task_label:'当前轮任务'};
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__cli-monitor?progress-only`);
    const isolatedPlan=page.getByRole('region',{name:'整项任务进度'});
    await isolatedPlan.getByRole('heading',{name:'当前轮任务',exact:true}).waitFor();
    assert.equal(await isolatedPlan.getByText('旧受管任务名称',{exact:true}).count(),0,'native turn label takes precedence over managed task name');
    const reads=()=>requests.filter(q=>q.cmd==='workbench.terminal.progress'&&q.session_id==='codex').length;
    const beforeErrors=reads();
    progress.codex.steps=[{id:'bad',title:'invalid',status:'invented',blocked_by:[]}];
    await isolatedPlan.getByText('进度读取中断，以下为上次计划。可重新读取。',{exact:true}).waitFor();
    await page.waitForTimeout(3400);
    assert.equal(reads()-beforeErrors,2,'one initial error and exactly one retry');
    const stoppedReads=reads();await page.waitForTimeout(3400);
    assert.equal(reads(),stoppedReads,'two consecutive errors stop automatic retries');
    progress.codex={state:'ready',steps:structuredClone(steps),task_label:'恢复读取后的任务'};
    await isolatedPlan.getByRole('button',{name:'重新读取任务进度'}).click();
    await isolatedPlan.getByRole('heading',{name:'恢复读取后的任务',exact:true}).waitFor();
    assert.deepEqual(errors,[]);
    writeFileSync(resolve(output,'expanded-browser-result.json'),JSON.stringify({passed:true,synthetic:true,checks:['whole-task steps','initial unavailable recovers automatically','1/3 to 2/3 to 3/3 polling','native completion not verification','ZCode session todos','fish swarm multi-provider steps','reported is not accepted','swarm accepted-count live polling','malformed response preserves labeled last plan','transient error recovers automatically','two consecutive errors stop retries','manual reread resumes','native title preferred over managed task name','new task clears previous plan','escaped titles and tool output','session isolation','auxiliary logs collapsed','open existing terminal','unsupported CLI honest state','file tab preserved','inactive polling stopped','220px panel','empty state'],errors},null,2));
  }finally{await browser?.close();await server.close()}
});
