import {Check,ChevronRight,GitBranch,GitCommitHorizontal,Monitor,Moon,SlidersHorizontal,Terminal,Timer} from 'lucide-react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import type {ReactNode} from 'react';
import type {Project} from './api';
import {Button} from './ui';
import {defaultWorkspaceView,workspaceProperties,type WorkspaceView} from './project-library';

const sorts=[{value:'manual',label:'手动顺序'},{value:'name',label:'名称 A → Z'},{value:'name_desc',label:'名称 Z → A'}];
const layouts=[{value:'compact',label:'简洁'},{value:'detailed',label:'详细'},{value:'selected',label:'选中时详细'}];
const groups=[{value:'none',label:'无分组'},{value:'status',label:'状态'},{value:'pr',label:'PR'},{value:'project',label:'项目'}];
function Choice({label,value,choices,onChange}:{label:string;value:string;choices:{value:string;label:string}[];onChange:(value:string)=>void}){
  return <Menu.Sub><Menu.SubTrigger className="workspace-option-row"><span>{label}</span><span className="workspace-option-value">{choices.find(c=>c.value===value)?.label||'项目已不可用'}</span><ChevronRight size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu workspace-options-sub" collisionPadding={8}><Menu.RadioGroup value={value} aria-label={label} onValueChange={onChange}>{choices.map(c=><Menu.RadioItem key={c.value} className="workspace-option-row" value={c.value} onSelect={e=>e.preventDefault()}><span className="project-toolbar-check"><Menu.ItemIndicator><Check size={13}/></Menu.ItemIndicator></span><span>{c.label}</span></Menu.RadioItem>)}</Menu.RadioGroup></Menu.SubContent></Menu.Portal></Menu.Sub>;
}
function Unavailable({icon,children,reason}:{icon:ReactNode;children:ReactNode;reason:string}){
  return <Menu.Item className="workspace-option-row" disabled title={reason} aria-description={reason}>{icon}<span>{children}</span><small className="workspace-option-value">未接入</small></Menu.Item>;
}
export default function WorkspaceOptions({view,projects,hostLabel,disabled,onChange,onHosts,onExpand,onCollapse,canCollapse}:{view:WorkspaceView;projects:Project[];hostLabel:string;disabled:boolean;onChange:(patch:Partial<WorkspaceView>)=>unknown;onHosts?:()=>void;onExpand:()=>void;onCollapse:()=>void;canCollapse:boolean}){
  return <Menu.Root modal={false}><Menu.Trigger asChild><Button type="button" aria-label="工作区选项" title="工作区选项"><SlidersHorizontal size={15}/></Button></Menu.Trigger><Menu.Portal><Menu.Content className="wb-menu-panel project-action-menu workspace-options-menu" aria-label="工作区选项" align="end" sideOffset={6} collisionPadding={8} onCloseAutoFocus={e=>{if(document.querySelector('[role=dialog],dialog[open]'))e.preventDefault()}}>
    <Menu.Label className="workspace-options-title">工作区选项</Menu.Label>
    <Menu.Label className="wb-menu-label">显示</Menu.Label>
    <Menu.Sub><Menu.SubTrigger className="workspace-option-row"><span>主机</span><span className="workspace-option-value">{hostLabel}</span><ChevronRight size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu workspace-options-sub" collisionPadding={8}>
      <Menu.RadioGroup value="current"><Menu.RadioItem className="workspace-option-row" value="current"><Check size={13}/>{hostLabel}</Menu.RadioItem></Menu.RadioGroup>
      <p className="workspace-options-note">列表按执行主机独立显示，尚不支持跨主机汇总。</p>
      {onHosts&&<Menu.Item className="workspace-option-row" onSelect={onHosts}>管理执行主机…</Menu.Item>}
    </Menu.SubContent></Menu.Portal></Menu.Sub>
    <Choice label="项目" value={view.project} choices={[{value:'',label:'所有项目'},...projects.map(p=>({value:p.id,label:p.name}))]} onChange={project=>onChange({project})}/>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Label className="wb-menu-label">分组依据</Menu.Label>
    <Menu.RadioGroup className="workspace-group-tabs" value={view.groupBy} aria-label="分组依据" onValueChange={groupBy=>onChange({groupBy:groupBy as WorkspaceView['groupBy']})}>{groups.map(g=><Menu.RadioItem key={g.value} className="workspace-group-tab" value={g.value} onSelect={e=>e.preventDefault()}>{g.label}</Menu.RadioItem>)}</Menu.RadioGroup>
    {view.groupBy==='pr'&&<p className="workspace-options-note">按任务已关联的 PR 链接分组，其余标为“未确认”。</p>}
    <Menu.Separator className="wb-menu-divider"/>
    <Choice label="排序方式" value={view.workspaceSort} choices={sorts} onChange={workspaceSort=>onChange({workspaceSort:workspaceSort as WorkspaceView['workspaceSort']})}/>
    <Choice label="项目排序" value={view.projectSort} choices={sorts} onChange={projectSort=>onChange({projectSort:projectSort as WorkspaceView['projectSort']})}/>
    <Choice label="卡片布局" value={view.layout} choices={layouts} onChange={layout=>onChange({layout:layout as WorkspaceView['layout']})}/>
    <Menu.Sub><Menu.SubTrigger className="workspace-option-row"><span>显示属性</span><span className="workspace-option-value">{view.properties.length}</span><ChevronRight size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu workspace-options-sub" collisionPadding={8} aria-label="显示属性">
      {workspaceProperties.map(p=><Menu.CheckboxItem key={p.id} className="workspace-option-row" checked={view.properties.includes(p.id)} onSelect={e=>e.preventDefault()} onCheckedChange={checked=>onChange({properties:checked?[...view.properties,p.id]:view.properties.filter(id=>id!==p.id)})}><span className="project-toolbar-check"><Menu.ItemIndicator><Check size={13}/></Menu.ItemIndicator></span>{p.label}</Menu.CheckboxItem>)}
    </Menu.SubContent></Menu.Portal></Menu.Sub>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Label className="wb-menu-label">筛选条件</Menu.Label>
    <Menu.CheckboxItem className="workspace-option-row" checked={view.hideSleeping} disabled={disabled} onSelect={e=>e.preventDefault()} onCheckedChange={checked=>onChange({hideSleeping:checked===true})}><Moon size={14}/><span>隐藏休眠项</span><span className="workspace-option-switch" data-checked={view.hideSleeping} aria-hidden="true"/></Menu.CheckboxItem>
    <Unavailable icon={<GitBranch size={14}/>} reason="当前列表尚未提供经确认的仓库默认分支">隐藏默认分支</Unavailable>
    <Unavailable icon={<Timer size={14}/>} reason="当前工作区尚未记录是否由自动化创建">隐藏自动化创建的工作区</Unavailable>
    <Unavailable icon={<Terminal size={14}/>} reason="当前工作区尚未记录 CLI 创建来源">隐藏 CLI 创建的工作区</Unavailable>
    <Unavailable icon={<Monitor size={14}/>} reason="当前工作区尚未记录创建客户端">隐藏其他客户端工作区</Unavailable>
    <Unavailable icon={<GitCommitHorizontal size={14}/>} reason="当前列表尚未提供明确的分离 HEAD 状态">隐藏分离 HEAD</Unavailable>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Item className="workspace-option-row" onSelect={onExpand}>展开全部</Menu.Item>
    <Menu.Item className="workspace-option-row" disabled={!canCollapse} onSelect={onCollapse}>折叠全部</Menu.Item>
    <Menu.Item className="workspace-option-row" onSelect={()=>onChange(defaultWorkspaceView())}>恢复默认显示</Menu.Item>
  </Menu.Content></Menu.Portal></Menu.Root>;
}
