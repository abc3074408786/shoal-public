import React from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';
const workspace={id:'fixture',project_id:'p',name:'研发测试工作区',path:'D:\\fixture',kind:'folder',position:0,archived:false,favorite:false};
const providers=['codex','claude-code','omp','opencode'];
const roles=['coordinate','explore','implement','review','integrate','verify'].map((kind,i)=>({id:kind,name:['主控','探索','实现','独立审查','集成','整体验证'][i],kind,instruction:'完成约定工作并报告证据',provider:providers[i%providers.length],model:'',skills:[],tools:[]}));
const nodes=['implement','other'].map((id,i)=>({id,title:i?'接口检查':'实现功能',role_id:i?'explore':'implement',instruction:'完成本步骤的已批准范围',depends_on:i?['implement']:[],scope:'fixture source',gap:'用户能完成完整流程',deliverables:['成果说明'],checks:['检查通过'],human_gate:i===0}));
const requests:any[]=[],templates:any[]=[];let run:any=null;
let catalogGeneration=0;
const sshProfile={id:'b0cfb946-d724-43d6-87e1-1530b16f50ae',name:'测试 Linux',host:'agent',user:'doctor'};
let preflightBlocked=false;
Object.assign(window,{setPreflightBlocked:(value:boolean)=>{preflightBlocked=value;}});
const catalog=()=>({path:'D:/fixture/mcp-catalog.json',digest:'sha256:'+(catalogGeneration?'b':'a').repeat(64),entries:[{id:'docs',command:'fixture-mcp',args:['--stdio'],catalog:['read','search']}]});
const plan=()=>({version:2,acceptance:'完整目标通过',parallelism:2,roles:structuredClone(roles),nodes:structuredClone(nodes),review_role:'review',integration_role:'integrate',verification_role:'verify'});
function response(r:any){requests.push(r);switch(r.cmd){
  case 'ssh.list':return {profiles:[sshProfile]};
  case 'ssh.hybrid.preview':if(r.id!==sshProfile.id)throw Error('SSH 配置不存在');return {profile:sshProfile,ssh_profile_id:sshProfile.id,ssh_profile_fingerprint:'sha256:'+'d'.repeat(64)};
  case 'ssh.hybrid.target.preview':if(r.id!==sshProfile.id)throw Error('SSH 配置不存在');return {profile:sshProfile,ssh_profile_id:sshProfile.id,ssh_profile_fingerprint:'sha256:'+'d'.repeat(64),target:{user:'doctor',hostname:'linux.example',ssh_target_fingerprint:'sha256:'+'e'.repeat(64)},provider:{provider:'custom',model:'deepseek-flash',provider_fingerprint:'sha256:'+'f'.repeat(64)}};
  case 'orchestration.preflight':return {workspace_id:r.workspace_id,ready:!preflightBlocked,issues:preflightBlocked?[{code:'git_baseline',message:'此项目还没有 Git 提交。请先初始化仓库并完成首次提交，再创建协作。'}]:[]};
  case 'orchestration.mcp.preview':return {catalog:catalog()};
  case 'orchestration.mcp.confirm':{if(!r.confirmed||r.digest!==catalog().digest)throw Error('预览后 MCP 来源已变化，请重新载入');return {server:{...catalog().entries[0],tools:r.tools,source:{path:catalog().path,digest:r.digest,entry_id:r.entry_id,confirmation_id:r.request_id}}};}
  case 'workbench.snapshot':return {projects:[{id:'p',name:'测试项目',path:'D:\\fixture',setup:''}],workspaces:[workspace],tasks:[],source_links:[]};
  case 'workbench.layout':return {layout:{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};
  case 'workbench.files':return {files:[]};case 'workbench.sessions':return {sessions:[]};case 'workbench.git':return {branch:'main',status:'',diff:'',staged:''};
  case 'orchestration.capabilities':return {providers:providers.map(id=>({id,installed:true,tools:['read','shell'],native_validation:'not_verified'})),skills:[{name:'fixture skill',path:'D:/fixture-skill/SKILL.md',digest:'sha256:fixture'}]};
  case 'orchestration.templates.list':return {templates:structuredClone(templates)};
  case 'orchestration.templates.save':templates.push({id:r.request_id,name:r.name,revision:1,plan:structuredClone(r.plan)});return {template:templates.at(-1)};
  case 'orchestration.list':return {runs:run?[structuredClone(run)]:[]};
  case 'orchestration.snapshot':return {run:structuredClone(run)};
  case 'orchestration.create':run={id:r.request_id,schema_version:2,agent:r.agent,goal:r.goal,workspace_id:r.workspace_id,state:'draft',revision:1,plan:r.plan||{...plan(),acceptance:r.acceptance},bindings:{},gates:{},stages:{implementation:'pending',development_validation:'pending',independent_testing:'pending',technical_acceptance:'pending',integration:'pending',deployment:'not_run',online:'not_verified',business_uat:'not_run'},events:[],status:{work:[],members:[],reviews:[],attempts:[]}};return {id:run.id};
  case 'orchestration.reopen':{const predecessor={id:run.id,candidate:run.final_candidate,reason_kind:r.reason_kind,evidence:r.evidence};run={id:r.request_id,schema_version:2,agent:r.agent,goal:r.goal,workspace_id:'fixture',state:'draft',revision:1,plan:{...plan(),acceptance:r.acceptance},predecessor,bindings:{},gates:{},stages:{implementation:'pending',development_validation:'pending',independent_testing:'pending',technical_acceptance:'pending',integration:'pending',deployment:'not_run',online:'not_verified',business_uat:'not_run'},events:[],status:{work:[],members:[],reviews:[],attempts:[]}};return {id:run.id};}
  case 'orchestration.plan.save':if(r.revision!==run.revision)throw Error('stale');run.plan=structuredClone(r.plan);run.revision++;return {id:run.id};
  case 'orchestration.plan.confirm':run.state='running';run.confirmed_revision=run.revision;run.nodes=[...run.plan.nodes,{...nodes[0],id:'__integrate',title:'集成固定候选',role_id:'integrate',depends_on:run.plan.nodes.map((n:any)=>n.id),human_gate:false},{...nodes[0],id:'__verify',title:'独立整体验证',role_id:'verify',depends_on:['__integrate'],human_gate:false}];return {id:run.id};
  case 'orchestration.revision.propose':if(r.revision!==run.revision)throw Error('stale');run.proposal={id:r.request_id,reason:r.reason,plan:structuredClone(r.plan)};return {id:run.id};
  case 'orchestration.revision.resolve':if(r.approved){run.plan=structuredClone(run.proposal.plan);run.revision++;run.confirmed_revision=run.revision;}run.proposal=null;return {id:run.id};
  case 'orchestration.gate.resolve':run.gates[r.node_id]={approved:r.approved,summary:r.summary};return {id:run.id};
  case 'orchestration.pause':run.state='paused';return {id:run.id};case 'orchestration.resume':run.state='running';return {id:run.id};case 'orchestration.stop':run.state='stopped';return {id:run.id};
  case 'orchestration.accept':run.state='accepted';return {id:run.id};
  default:throw Error('Unexpected v2 fixture RPC '+r.cmd);
}}
function reported(node:any){const evidence={version:1,checks:[{requirement_index:0,method:'fixture source inspection',result:'passed',detail:'fixture checked',artifact_ids:['log']}],artifacts:[{id:'log',path:'.velella/evidence/fixture.log',sha256:'a'.repeat(64),kind:'log'}],gaps:[]};return {work:{id:node.id,work_key:node.id,title:node.title,state:'accepted',attempt_id:'a-'+node.id,attempt_count:1,evidence_contract:{checks:node.checks}},attempt:{id:'a-'+node.id,report:{summary:'fixture evidence only',outcome:'completed',evidence}}};}
if('__TAURI_INTERNALS__' in window)throw Error('Never use a native profile for fixtures');
localStorage.clear();localStorage.setItem('velella:workspace','fixture');localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
window.fetch=async(_input,init)=>{try{return new Response(JSON.stringify({ok:true,...response(JSON.parse(String(init?.body)))}))}catch(error){return new Response(JSON.stringify({ok:false,error:String(error)}))}};
Object.assign(window,{orchestrationFixture:{requests,usage(){run.context_usage=[{session_id:'omp-session',workspace_id:'fixture',provider:'omp',state:'observed',receipt:{source:'omp.getContextUsage',basis:'native_estimate',tokens:80000,context_window:200000,percent:40,observed_at:'2026-10-26T00:00:00Z',handoff_required:true}},{session_id:'codex-session',workspace_id:'fixture',provider:'codex',state:'unknown'}]},driftSource(){catalogGeneration++;},stage(){const receipt=reported(run.nodes[0]);run.status.work=[receipt.work];run.status.attempts=[receipt.attempt]},finish(){run.state='awaiting_acceptance';run.integration_workspace='fixture';run.integration_branch='codex/orch/fixture/integration';run.final_candidate={revision:'fixture-source'};const receipts=run.nodes.map(reported);run.status.work=receipts.map((r:any)=>r.work);run.status.attempts=receipts.map((r:any)=>r.attempt);run.stages={...run.stages,implementation:'passed',integration:'passed',development_validation:'evidence_verified',independent_testing:'evidence_verified',technical_acceptance:'evidence_verified'};}}});
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
