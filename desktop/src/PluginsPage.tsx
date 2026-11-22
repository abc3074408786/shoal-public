import {useEffect,useRef,useState} from 'react';
import {ArrowLeft,ArrowRight,Blocks,BookOpen,Calendar,Dumbbell,ExternalLink,Luggage,Mail,Plus,RefreshCw,Search,Settings2,Trash2,Power} from 'lucide-react';
import {Button,Input,Modal} from './ui';
import {currentTarget,type ExecutionTarget} from './execution-target';
import McpCatalogPicker from './McpCatalogPicker';
import ComputerUsePlugin from './ComputerUsePlugin';
import {addPluginReference,readPluginLibrary,removePluginReference,setPluginReferenceEnabled,type PluginReference} from './plugin-library';
import {publicPlugins,pluginCategories,type PublicPlugin} from './plugin-catalog';
import logo from './assets/velella-logo-v2.png';
import DesktopExtensions from './extensions/DesktopExtensions';
import {listNativePlugins,nativePluginRequest} from './native-plugins';
import type {ExtensionInfo} from './extensions/types';
import {useConfirm} from './ConfirmAction';
import './plugins.css';

function PluginIcon({plugin}:{plugin:PublicPlugin}){
  const [failed,setFailed]=useState(false);
  const Icon=({mail:Mail,calendar:Calendar,luggage:Luggage,dumbbell:Dumbbell} as Record<string,typeof Mail>)[plugin.icon];
  return <span className="plugin-icon" style={{color:plugin.color}} aria-hidden="true">{Icon?<Icon size={24}/>:failed?<Blocks size={24}/>:<img src={import.meta.env.BASE_URL+'plugin-logos/'+plugin.icon+'.svg'} alt="" onError={()=>setFailed(true)}/>}</span>;
}
export default function PluginsPage({onBack,onSettings}:{onBack:()=>void;onSettings:()=>void}){
  const [target]=useState<ExecutionTarget>(currentTarget);
  const ask=useConfirm();
  const [tab,setTab]=useState<'personal'|'discover'|'skills'>('personal');
  const [native,setNative]=useState<ExtensionInfo[]>([]),[nativeLoading,setNativeLoading]=useState(false),[nativeError,setNativeError]=useState(''),[nativeBusy,setNativeBusy]=useState('');
  const [opened,setOpened]=useState<ExtensionInfo|null>(null),[nativeDetails,setNativeDetails]=useState<ExtensionInfo|null>(null);
  const pending=useRef(false),alive=useRef(true);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
  const [search,setSearch]=useState(''),[category,setCategory]=useState(''),[library,setLibrary]=useState<PluginReference[]>([]);
  const [error,setError]=useState(''),[message,setMessage]=useState(''),[refresh,setRefresh]=useState(0);
  const loading=nativeLoading;
  const [importing,setImporting]=useState(false),[selected,setSelected]=useState('github'),[installed,setInstalled]=useState<PluginReference|null>(null);
  const [importPath,setImportPath]=useState('');
  function openImport(path=''){setImportPath(path);setImporting(true)}
  const detailHeading=useRef<HTMLHeadingElement>(null);
  useEffect(()=>{const read=()=>{try{setLibrary(readPluginLibrary(target));setError('')}catch(e){setError(String(e))}};read();window.addEventListener('velella:plugins-changed',read);window.addEventListener('storage',read);return()=>{window.removeEventListener('velella:plugins-changed',read);window.removeEventListener('storage',read)}},[target,refresh]);
  useEffect(()=>{if(target.kind!=='local')return;let live=true;setNativeLoading(true);setNativeError('');void listNativePlugins(target).then(items=>{if(live)setNative(items)}).catch(e=>{if(live)setNativeError('插件状态读取失败：'+String(e))}).finally(()=>{if(live)setNativeLoading(false)});return()=>{live=false}},[target,refresh]);
  async function nativeAction(plugin:ExtensionInfo,action:'setEnabled'|'install'|'uninstall'|'selectDataDirectory'){
    if(pending.current)return;
    pending.current=true;
    try{
      if(action==='uninstall'){setNativeDetails(null);if(!await ask('卸载「'+plugin.name+'」？将停用并移除安装登记，保留已有数据和应用内置的安装文件，可从“发现”重新安装。'))return;}
      if(!alive.current)return;setNativeBusy(plugin.id);setError('');
      const next=await nativePluginRequest<ExtensionInfo>({action,pluginId:plugin.id,...(action==='setEnabled'?{enabled:!plugin.enabled}:{}),...(action==='uninstall'?{confirmed:true}:{})},target);
      if(!alive.current)return;setNative(items=>items.map(p=>p.id===next.id?next:p));setNativeDetails(old=>old?.id===next.id?(next.installed===false?null:next):old);
      setMessage(action==='uninstall'?'已卸载 '+next.name+'，已有数据保留。':action==='install'?'已安装 '+next.name+'，启用后可以打开。':action==='selectDataDirectory'?'插件数据目录已更新。':next.name+(next.enabled?' 已启用。':' 已停用。'));
    }catch(e){if(alive.current)setError(String(e))}finally{pending.current=false;if(alive.current)setNativeBusy('')}
  }
  function toggleReference(plugin:PluginReference){try{setPluginReferenceEnabled(plugin,plugin.enabled===false,target);setMessage(plugin.id+(plugin.enabled===false?' 已启用，可供新配置选用。':' 已停用，不再出现在新配置的选择清单。'))}catch(e){setError(String(e))}}
  async function uninstallReference(plugin:PluginReference){setInstalled(null);if(!await ask('从个人清单移除「'+plugin.id+'」？保留来源文件和已有会话配置。'))return;try{removePluginReference(plugin,target);setMessage('已移除 '+plugin.id+' 的 MCP 配置。')}catch(e){setError(String(e))}}
  const query=search.trim().toLocaleLowerCase(),matches=(...values:string[])=>values.join(' ').toLocaleLowerCase().includes(query);
  const services=publicPlugins.filter(p=>(!category||p.category===category)&&matches(p.name,p.description));
  const showComputerUse=tab==='discover'&&(!category||category==='开发协作')&&matches('Computer Use 电脑操作 截图 鼠标 键盘 本机 Windows');
  const active=services.find(p=>p.id===selected)||services[0];
  const personal=library.filter(p=>matches(p.id,p.path));
  const skillCenter=native.find(p=>p.id==='skill-center');
  const installedNative=native.filter(p=>p.installed!==false),visibleNative=installedNative.filter(p=>matches(p.name,p.id,p.description,p.dataDir));
  const availableNative=native.filter(p=>matches(p.name,p.id,p.description));
  function changeTab(next:typeof tab){setTab(next);setSearch('');setCategory('')}
  function choose(plugin:PublicPlugin){setSelected(plugin.id);requestAnimationFrame(()=>detailHeading.current?.focus({preventScroll:window.innerWidth>1000}))}
  return <section className="plugins-page" aria-label="插件管理">
    <div className="plugins-content">
      <header className="plugins-header"><div className="plugins-title"><img src={logo} alt=""/><div><h1>插件</h1><p>为项目接入外部工具，管理已添加的连接。</p></div></div><div className="plugins-actions">
        <Button aria-label="刷新插件与技能" title="刷新插件与技能" disabled={loading||nativeLoading||!!nativeBusy} onClick={()=>{setMessage('');setRefresh(v=>v+1)}}><RefreshCw size={16} className={loading?'loading-icon':undefined}/></Button>
        <Button aria-label="插件设置" title="插件设置" onClick={onSettings}><Settings2 size={16}/></Button>
        <Button className="plugins-create" aria-label="添加插件" onClick={()=>openImport()}><Plus size={16}/>添加插件</Button>
      </div></header>
      <nav className="plugins-tabs" aria-label="插件视图"><Button aria-pressed={tab==='personal'} onClick={()=>changeTab('personal')}>已添加 <small>{library.length+installedNative.length}</small></Button><Button aria-pressed={tab==='discover'} onClick={()=>changeTab('discover')}>发现</Button><Button aria-pressed={tab==='skills'} onClick={()=>changeTab('skills')}>技能中心</Button><Button className="plugins-back" onClick={onBack}><ArrowLeft size={14}/>返回工作台</Button></nav>
      {error&&<p className="plugins-error" role="alert">{error}</p>}{message&&<p className="plugins-message" role="status">{message}</p>}
      {nativeError&&<p className="plugins-error" role="alert">{nativeError}</p>}
      <div className="plugins-tools"><label className="plugins-search"><Search size={16}/><Input type="search" aria-label={tab==='skills'?'搜索技能':'搜索插件'} placeholder={tab==='skills'?'搜索技能名称或路径':'搜索名称或用途'} value={search} onChange={e=>setSearch(e.target.value)}/></label>
        {tab==='discover'&&<select aria-label="服务类别" value={category} onChange={e=>setCategory(e.target.value)}><option value="">全部类别</option>{pluginCategories.map(c=><option key={c}>{c}</option>)}</select>}
        <span>{tab==='discover'?services.length+(showComputerUse?1:0)+' 项插件 / 服务参考':tab==='personal'?personal.length+visibleNative.length+' 个插件 / 配置':'统一管理项目与全局技能'}</span>
      </div>
      {showComputerUse&&<ComputerUsePlugin target={target} onImport={openImport}/>}
      {tab==='discover'&&availableNative.length>0&&<section className="plugins-bundled" aria-label="内置插件"><h2>内置插件</h2>{availableNative.map(p=><div className="plugin-installed-row" key={p.id}><span className="plugin-icon"><Blocks size={24}/></span><div className="plugin-identity"><strong>{p.name}</strong><span>{p.description}</span></div><Button disabled={nativeLoading||!!nativeBusy||p.installed!==false} onClick={()=>void nativeAction(p,'install')}>{p.installed!==false?'已安装':'安装'}</Button></div>)}</section>}
      {tab==='discover'&&services.length>0&&<p className="plugins-catalog-disclosure">以下为第三方服务资料，尚不提供账号连接或一键安装。已有 MCP 目录配置可从右上角添加。</p>}
      {tab==='discover'&&(services.length>0||!showComputerUse)&&<div className="plugins-workspace"><section className="plugins-directory" aria-label="服务目录">
        <div className="plugins-list-label"><span>服务参考 / 用途</span><span>类别</span></div>
        <div className="plugins-list">{services.map(plugin=><article className="plugin-row" key={plugin.id}><Button className="plugin-info" aria-label={'查看 '+plugin.name} aria-pressed={active?.id===plugin.id} onClick={()=>choose(plugin)}><PluginIcon plugin={plugin}/><span className="plugin-identity"><strong>{plugin.name}</strong><span>{plugin.description}</span></span><span className="plugin-category">{plugin.category}</span><ArrowRight size={16}/></Button></article>)}
          {!services.length&&!showComputerUse&&<div className="plugins-empty"><Search size={26}/><h2>没有找到插件</h2><p>试试其他名称或类别。</p><Button onClick={()=>{setSearch('');setCategory('')}}>清除搜索</Button></div>}
        </div>
      </section>{active&&<aside className="plugin-inspector" aria-label="插件详情"><div className="plugin-inspector-top"><PluginIcon key={active.id} plugin={active}/><span>服务详情</span></div><h2 ref={detailHeading} tabIndex={-1}>{active.name}</h2><p>{active.description}</p><dl><div><dt>类别</dt><dd>{active.category}</dd></div><div><dt>接入方式</dt><dd>MCP 目录</dd></div><div><dt>连接状态</dt><dd>尚未核验</dd></div></dl><Button className="plugins-create" onClick={()=>openImport()}><Plus size={16}/>添加配置</Button><a href={active.url} target="_blank" rel="noreferrer">查看服务官网 <ExternalLink size={14}/></a><div className="plugin-inspector-note"><h3>接入后怎么用</h3><p>在 Agent 编排中选用已添加的插件，按任务选择需要的工具。</p><p>添加前需要可用的目录配置。账号授权由服务自身处理。</p></div></aside>}</div>}
      {tab==='personal'&&<section className="plugins-personal" aria-label="已添加插件">
        {nativeLoading&&<p className="plugins-loading" role="status">正在读取已安装插件…</p>}
        {target.kind!=='local'&&<p className="plugins-loading">本机插件请切回本机执行主机管理。</p>}
        {(visibleNative.length>0||personal.length>0)?<div className="plugins-list">
          {visibleNative.map(p=><article className="plugin-installed-row" key={p.id}>
            <Button className="plugin-installed-info" aria-label={'查看 '+p.name+' 详情'} onClick={()=>setNativeDetails(p)}><span className="plugin-icon"><Blocks size={24}/></span><span className="plugin-identity"><strong>{p.name} <small>v{p.version}</small></strong><span>{p.description}</span></span></Button>
            <span className="plugin-state" data-enabled={p.enabled}>{p.enabled?'已启用':'已停用'}</span>
            <div className="plugin-installed-actions"><Button disabled={nativeLoading||!!nativeBusy||!p.enabled} aria-label={'打开 '+p.name} onClick={()=>setOpened(p)}>打开</Button><Button disabled={nativeLoading||!!nativeBusy} aria-label={(p.enabled?'停用 ':'启用 ')+p.name} onClick={()=>void nativeAction(p,'setEnabled')}><Power size={14}/>{p.enabled?'停用':'启用'}</Button><Button disabled={nativeLoading||!!nativeBusy} aria-label={'卸载 '+p.name} onClick={()=>void nativeAction(p,'uninstall')}><Trash2 size={14}/>卸载</Button></div>
          </article>)}
          {personal.map(p=><article className="plugin-installed-row" key={p.path+'#'+p.id}>
            <Button className="plugin-installed-info" aria-label={'管理 '+p.id} onClick={()=>setInstalled(p)}><span className="plugin-icon"><Blocks size={24}/></span><span className="plugin-identity"><strong>{p.id} <small>MCP 配置</small></strong><span>{p.path}</span></span></Button>
            <span className="plugin-state" data-enabled={p.enabled!==false}>{p.enabled===false?'已停用':'已启用'}</span>
            <div className="plugin-installed-actions"><Button onClick={()=>setInstalled(p)}>配置</Button><Button aria-label={(p.enabled===false?'启用 ':'停用 ')+p.id} onClick={()=>toggleReference(p)}><Power size={14}/>{p.enabled===false?'启用':'停用'}</Button><Button aria-label={'卸载 '+p.id} onClick={()=>void uninstallReference(p)}><Trash2 size={14}/>移除</Button></div>
          </article>)}
        </div>:!nativeLoading&&!nativeError&&<div className="plugins-empty"><Blocks size={30}/><h2>{query?'没有匹配的个人插件':'从你的第一个工具开始'}</h2><p>从“发现”安装内置插件，或添加已有 MCP 配置。</p><Button className="plugins-create" onClick={()=>changeTab('discover')}>发现插件</Button></div>}
        {library.length>0&&<p className="plugins-catalog-disclosure">MCP 配置的启停控制后续选用；已有运行会话保留原配置。</p>}
      </section>}
      {tab==='skills'&&<section className="plugins-personal" aria-label="技能中心入口"><div className="plugins-empty"><BookOpen size={28}/><h2>技能中心</h2><p>导入本地或 Git 技能，按项目或全局分发给 Agent，查看更新并恢复历史版本。</p>{skillCenter?<Button disabled={nativeLoading||!!nativeBusy} onClick={()=>{if(skillCenter.installed===false)void nativeAction(skillCenter,'install');else if(!skillCenter.enabled)void nativeAction(skillCenter,'setEnabled');else setOpened(skillCenter)}}>{skillCenter.installed===false?'安装技能中心':!skillCenter.enabled?'启用技能中心':'打开技能中心'}</Button>:<p>{nativeLoading?'正在读取插件状态…':'当前执行主机尚未提供技能中心，请使用本机候选版本。'}</p>}</div></section>}
    </div>
    <Modal open={importing} onOpenChange={setImporting} title="添加插件"><div className="dialog-body plugin-import"><p>预览目录、选择工具并确认来源后，插件会加入个人清单。</p><McpCatalogPicker key={importPath} initialPath={importPath} showLibrary={false} onAdd={server=>{try{addPluginReference(server,target);setImporting(false);setTab('personal');setSearch('');setMessage('已添加 '+server.id+'，可在 Agent 编排的外部 MCP 中选用。')}catch(e){setError(String(e))}}}/></div></Modal>
    <Modal open={!!installed} onOpenChange={v=>{if(!v)setInstalled(null)}} title={installed?.id||'管理配置'}>{installed&&<div className="dialog-body plugin-detail"><p>此配置可在 Agent 编排的外部 MCP 中选用。使用前会重新读取目录并确认工具。</p><label>来源目录<code>{installed.path}</code></label><Button onClick={()=>{openImport(installed.path);setInstalled(null)}}>重新导入配置</Button><Button className="danger" onClick={()=>void uninstallReference(installed)}><Trash2 size={16}/>从个人清单移除</Button></div>}</Modal>
    <Modal open={!!nativeDetails} onOpenChange={v=>{if(!v)setNativeDetails(null)}} title={nativeDetails?.name||'插件详情'}>{nativeDetails&&<div className="dialog-body plugin-detail"><p>{nativeDetails.description}</p><dl className="plugin-detail-facts"><div><dt>版本</dt><dd>{nativeDetails.version}</dd></div><div><dt>状态</dt><dd>{nativeDetails.enabled?'已启用':'已停用'}</dd></div><div><dt>数据目录</dt><dd><code>{nativeDetails.dataDir}</code></dd></div></dl><div className="plugin-installed-actions"><Button disabled={nativeLoading||!!nativeBusy||!nativeDetails.enabled} onClick={()=>{setOpened(nativeDetails);setNativeDetails(null)}}>打开插件</Button><Button disabled={nativeLoading||!!nativeBusy} onClick={()=>void nativeAction(nativeDetails,'setEnabled')}>{nativeDetails.enabled?'停用':'启用'}</Button><Button disabled={nativeLoading||!!nativeBusy||!nativeDetails.enabled} onClick={()=>void nativeAction(nativeDetails,'selectDataDirectory')}>选择数据目录</Button><Button disabled={nativeLoading||!!nativeBusy} className="danger" onClick={()=>void nativeAction(nativeDetails,'uninstall')}>卸载插件</Button></div><p>卸载保留数据。内置安装文件随应用保留，可从“发现”重新安装。</p></div>}</Modal>
    {opened&&<DesktopExtensions plugin={opened} onClose={()=>setOpened(null)}/>}

  </section>;
}
