import {useEffect,useState} from 'react';
import {rpc} from './api';
import {Button,Input,Textarea} from './ui';
import {currentTarget} from './execution-target';
import {readPluginLibrary,type PluginReference} from './plugin-library';

export type McpSource={path:string;digest:string;entry_id:string;confirmation_id:string};
export type McpServer={id:string;command:string;args:string[];catalog:string[];tools:string[];source?:McpSource};
type Catalog={path:string;digest:string;entries:Omit<McpServer,'tools'|'source'>[]};

export default function McpCatalogPicker({onAdd,showLibrary=true,initialPath=''}:{onAdd:(server:McpServer)=>void;showLibrary?:boolean;initialPath?:string}){
  const [target]=useState(currentTarget);
  const [library,setLibrary]=useState(()=>{try{return showLibrary?readPluginLibrary(target).filter(p=>p.enabled!==false):[]}catch{return []}});
  const [path,setPath]=useState(initialPath),[catalog,setCatalog]=useState<Catalog|null>(null),[entryId,setEntryId]=useState('');
  const [selected,setSelected]=useState<string[]>([]),[busy,setBusy]=useState(false),[error,setError]=useState('');
  useEffect(()=>{if(!showLibrary){setLibrary([]);return}const read=()=>{try{setLibrary(readPluginLibrary(target).filter(p=>p.enabled!==false))}catch(e){setLibrary([]);setError(String(e))}};read();window.addEventListener('velella:plugins-changed',read);window.addEventListener('storage',read);return()=>{window.removeEventListener('velella:plugins-changed',read);window.removeEventListener('storage',read)}},[target,showLibrary]);
  const entry=catalog?.entries.find(e=>e.id===entryId);
  async function load(reference?:PluginReference){
    setBusy(true);setError('');setCatalog(null);setSelected([]);setEntryId('');
    try{const result=await rpc<{catalog:Catalog}>({cmd:'orchestration.mcp.preview',path:reference?.path||path},target);setCatalog(result.catalog);setEntryId(reference?result.catalog.entries.find(e=>e.id===reference.id)?.id||'':result.catalog.entries[0]?.id||'');if(reference){setPath(reference.path);if(result.catalog.digest!==reference.digest)setError('来源目录已变化，请重新核对命令和工具。');else if(!result.catalog.entries.some(e=>e.id===reference.id))setError('目录中已找不到此插件，请重新选择条目。');}}
    catch(e){setError(String(e));}finally{setBusy(false);}
  }
  async function add(){
    if(!entry||!catalog)return;
    setBusy(true);setError('');
    try{
      const result=await rpc<{server:McpServer}>({cmd:'orchestration.mcp.confirm',path:catalog.path,digest:catalog.digest,entry_id:entry.id,tools:selected,confirmed:true,request_id:crypto.randomUUID()},target);
      onAdd(result.server);setCatalog(null);setSelected([]);
    }catch(e){setError(String(e));}finally{setBusy(false);}
  }
  return <div className="orch-mcp-source">
    {library.length>0&&<div className="plugin-library-choices"><span>已添加插件</span>{library.map(p=><Button type="button" key={p.path+'#'+p.id} disabled={busy} onClick={()=>void load(p)}>{p.id}</Button>)}</div>}
    <label>目录文件绝对路径<Input aria-label="MCP 目录文件路径" value={path} disabled={busy} placeholder="D:\\项目\\mcp-catalog.json" onChange={e=>{setPath(e.target.value);setCatalog(null);setSelected([]);}}/></label>
    <Button disabled={busy||!path.trim()} onClick={()=>void load()}>载入目录预览</Button>
    <details><summary>目录格式</summary><p className="orch-muted">本地 JSON，最多 64 个条目、1 MiB。载入只读取此文件；参数中不要包含凭据。</p><pre>{JSON.stringify({version:1,entries:[{id:'docs',command:'docs-mcp',args:['--stdio'],catalog:['search','read']}]},null,2)}</pre></details>
    {error&&<p role="alert">{error}</p>}
    {catalog&&<div className="orch-report">
      <p className="orch-fingerprint">{catalog.path}<br/>{catalog.digest}</p>
      {!catalog.entries.length?<p>目录为空，可先添加条目再重新载入。</p>:<>
        <label>目录条目<select aria-label="MCP 目录条目" value={entryId} disabled={busy} onChange={e=>{setEntryId(e.target.value);setSelected([]);}}>{catalog.entries.map(e=><option key={e.id} value={e.id}>{e.id}</option>)}</select></label>
        {entry&&<><label>将执行的命令<Input value={entry.command} readOnly/></label><label>参数（JSON 数组）<Textarea value={JSON.stringify(entry.args)} readOnly/></label>
          <div className="orch-dependencies">{entry.catalog.map(tool=><label key={tool}><input type="checkbox" disabled={busy} checked={selected.includes(tool)} onChange={e=>setSelected(e.target.checked?[...selected,tool]:selected.filter(t=>t!==tool))}/>{tool}</label>)}</div>
          <p className="orch-muted">确认表示你认可此命令及勾选的工具。保存、启动和恢复时会复核来源；文件内容改变或增加工具后须重新确认。</p>
          <Button disabled={busy||!selected.length} onClick={()=>void add()}>确认来源并添加所选工具</Button>
        </>}
      </>}
    </div>}
  </div>;
}
