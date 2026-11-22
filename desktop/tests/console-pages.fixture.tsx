// Interactive review of real components. Every RPC is handled in memory.
import React from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/motion.css';
const params=new URLSearchParams(location.search);
const projects=[{id:'a-project',name:'Velella',path:'D:\\示例项目\\Velella',setup:'',direct:false}];
const workspaces=[{id:'a-workspace',project_id:'a-project',name:'Velella 工作区',path:'D:\\示例项目\\Velella',kind:'primary',position:0,archived:false,favorite:false}];
const layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]} as any;
const providers=['github','linear','gitlab','jira'];
const sources=providers.map(provider=>({id:provider,provider,project_id:'a-project',name:provider==='github'?'Velella 产品开发':provider==='linear'?'产品研发团队':provider,scope:provider==='linear'?'team-a':provider==='jira'?'DEV':'velella/desktop',auth_mode:'cli',base_url:'https://example.test',login:'',jira_kind:'cloud'}));
const items=sources.flatMap(source=>[
  {key:source.id+':7',source_id:source.id,project_id:'a-project',provider:source.provider,remote_id:'7',identifier:'#7',title:'统一工作台的界面与交互',url:'https://example.test/7',description:'## 交付目标\n让工作区、任务与工具沿用同一套视觉。\n\n- [x] 清晰的页面层次\n- [ ] 核对窄窗口与键盘操作\n\n保留已有功能、任务详情和评论。',state:'进行中',closed:false,updated_at:'2026-10-24T03:20:00Z',metadata:{assignees:['产品研发'],creator:'示例用户',priority:'高',labels:['界面优化','待验收'],project:'Velella',milestone:'十月迭代',created_at:'2026-10-20T02:00:00Z',due_at:'2026-10-31'}},
  {key:source.id+':8',source_id:source.id,project_id:'a-project',provider:source.provider,remote_id:'8',identifier:'#8',title:'补齐工作区的使用说明',url:'https://example.test/8',description:'整理打开项目、终端会话和文件浏览的常用操作。',state:'待处理',closed:false,updated_at:'2026-10-23T07:10:00Z',metadata:{assignees:['文档维护'],priority:'普通',labels:['文档']}},
  {key:source.id+':9',source_id:source.id,project_id:'a-project',provider:source.provider,remote_id:'9',identifier:'#9',title:'核对长标题与任务详情的显示',url:'https://example.test/9',description:'检查长路径、中文标题、负责人、优先级、标签和讨论信息。',state:'待审查',closed:false,updated_at:'2026-10-22T08:45:00Z',metadata:{assignees:['测试'],priority:'普通',labels:['可用性']}},
]);
const comments=[{id:'comment-1',author:'产品研发',created:'2026-10-24T04:00:00Z',body:'界面按选定的 A 版统一；真实操作仍然保留。'}];
const requests:any[]=[];
function respond(r:any):any {
  requests.push(r);
  switch(r.cmd) {
    case 'workbench.snapshot':return {projects:params.has('empty')?[]:projects,workspaces:params.has('empty')?[]:workspaces,tasks:[],source_links:[]};
    case 'workbench.layout':return {layout};
    case 'workbench.layout.save':Object.assign(layout,r.layout);return {layout};
    case 'workbench.sessions':return {sessions:[]};
    case 'workbench.files':return {files:['README.md','src/WorkspaceApp.tsx','src/TaskSourcesPage.tsx','docs/产品界面与验收说明.md']};
    case 'workbench.git':return {branch:'main',status:'',diff:'',staged:''};
    case 'workbench.file':case 'workbench.read':return {content:'# Velella\n\n这是用于界面评审的示例文件。',text:'# Velella\n\n示例文件',binary:false};
    case 'sources.list':case 'sources.sync':return {sources,repositories:[],links:[]};
    case 'sources.scopes':return {choices:[{id:'team-a',name:'产品研发团队'}]};
    case 'sources.items':return {items:items.filter(item=>item.source_id===r.source_id&&(r.state!=='closed')&&(!r.search||(item.title+item.description).includes(r.search))),next:null};
    case 'sources.cached':case 'sources.detail':return {item:items.find(item=>item.key===r.key)};
    case 'sources.comments':return {comments,next:null};
    case 'sources.states':return {states:[{id:'closed',name:'已完成'}]};
    case 'sources.create_options':return {options:{types:[],fields:[]}};
    case 'sources.mutate':return {};
    case 'sources.create':{const source=sources.find(source=>source.id===r.source_id)!;const item={...items.find(item=>item.source_id===source.id)!,key:source.id+':'+Date.now(),title:r.fields.title,description:r.fields.description};items.push(item);return {item}};
    case 'sources.save':return {source:{...r.source,id:r.source.id||'preview-source'}};
    case 'sandbox.capabilities':return {sandbox:{available:false,reason:'隔离预览不运行终端',restricted:{available:false,reason:'隔离预览不运行终端'}}};
    case 'agent.check':return {available:true,authenticated:true,version:'模拟数据'};
    case 'ssh.list':return {profiles:[{id:'preview-host',name:'开发主机（示例）',host:'dev.example.test',user:'developer',port:22}]};
    case 'worktree.list':return {branches:['main'],checkouts:[{path:workspaces[0].path,branch:'main',locked:false}],workspaces};
    case 'automation.list':return {automations:[],runs:[]};
    default:throw Error('交互预览未执行实际操作：'+r.cmd);
  }
}
if('__TAURI_INTERNALS__' in window)throw Error('Do not run fixtures in a native host');
localStorage.setItem('velella:workspace','a-workspace');
localStorage.setItem('velella:ui:theme',JSON.stringify(params.get('theme')||'dark'));
localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));
window.fetch=async(_input,init)=>{try{return new Response(JSON.stringify({ok:true,...respond(JSON.parse(String(init?.body)))}))}catch(error){return new Response(JSON.stringify({ok:false,error:String(error)}))}};
Object.assign(window,{consolePreview:{requests}});
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
