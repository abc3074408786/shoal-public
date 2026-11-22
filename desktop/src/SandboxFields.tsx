import {useCallback,useEffect,useRef} from 'react';
import {rpc} from './api';
import {Button,Textarea} from './ui';
import {isolationBlockReason,restrictedBlockReason,readCapabilities,SandboxDraft,SandboxProbe} from './sandbox-policy';

export default function SandboxFields({value,onChange,probe,onProbe,direct,projectId}:{value:SandboxDraft;onChange:(draft:SandboxDraft)=>void;probe:SandboxProbe;onProbe:(probe:SandboxProbe)=>void;direct:boolean;projectId:string}){
  const generation=useRef(0);
  const refresh=useCallback(async()=>{
    const request=++generation.current;onProbe({status:'loading'});
    try{const result=await rpc({cmd:'sandbox.capabilities'});if(request===generation.current)onProbe({status:'ready',capabilities:readCapabilities(result.sandbox)})}
    catch(error){if(request===generation.current)onProbe({status:'error',error:String(error)})}
  },[onProbe]);
  useEffect(()=>{void refresh();return()=>{generation.current++}},[refresh,projectId]);
  const isolatedBlocked=isolationBlockReason(probe,direct),restrictedBlocked=restrictedBlockReason(probe,direct),isolated=value.mode==='isolated',restricted=value.mode==='restricted',limited=isolated||restricted;
  const status=restricted?restrictedBlocked||'当前环境支持文件与网络限制（实验性）':isolated?isolatedBlocked||'当前环境支持隔离执行':`隔离执行：${isolatedBlocked||'可用'} 受限执行：${restrictedBlocked||'可用（实验性）'}`;
  const detail=probe.status==='error'?probe.error:probe.status==='ready'?(restricted?probe.capabilities.restricted?.reason:probe.capabilities.reason):null;
  return <fieldset className="sandbox-fields"><legend>执行环境</legend>
    <label>执行模式<select aria-describedby="sandbox-status sandbox-scope" value={value.mode} onChange={e=>{const mode=e.target.value as SandboxDraft['mode'];onChange({...value,mode,network:mode==='restricted'?'denied':value.network})}}>
      <option value="native">本机执行</option><option value="isolated" disabled={Boolean(isolatedBlocked)}>隔离执行（Linux）</option><option value="restricted" disabled={Boolean(restrictedBlocked)}>文件与网络限制（实验性/Linux）</option>
    </select></label>
    <div className="health-check"><span id="sandbox-status" role="status">{status}</span><Button type="button" disabled={probe.status==='loading'} onClick={()=>void refresh()}>重新检查</Button></div>
    {detail&&<details className="sandbox-details"><summary>查看检查详情</summary><p>{detail}</p></details>}
    <p id="sandbox-scope" className="muted">{restricted?'仅提供文件目录白名单和网络关闭，未隔离进程命名空间。准备脚本、Agent、验证和任务 Shell 共用限制策略，使用独立 HOME；浏览器工具暂不可用。启动失败不会转为本机执行。':isolated?'准备脚本、Agent、验证和任务 Shell 都受隔离策略约束。使用独立登录资料，浏览器工具暂不可用；隔离失败时不会转为本机执行。':'使用本机账号和工具配置。Agent 沿用原生权限；准备脚本与验证命令直接在本机运行。'}</p>
    {limited&&<><label>网络访问<select value={value.network} disabled={restricted} onChange={e=>onChange({...value,network:e.target.value as SandboxDraft['network']})}><option value="denied">关闭网络</option>{!restricted&&<option value="host">允许网络（含本机服务）</option>}</select></label>
      <p className="muted">{restricted?'受限执行始终关闭网络，无法连接远程模型、下载依赖或联网登录。':value.network==='host'?'允许访问互联网、局域网和本机服务，不限制访问域名。':'无法连接远程模型 API 或下载依赖。需要联网时，请明确选择允许网络。'}</p>
      <details className="sandbox-details"><summary>额外目录（可选）</summary><p className="muted">任务目录已允许写入。只填写需要访问的其他目录，使用真实绝对路径，每行一个；不要填写主目录、凭据目录或 Velella 数据目录。</p>
        <label>额外只读目录<Textarea value={value.readOnly} onChange={e=>onChange({...value,readOnly:e.target.value})} rows={2} placeholder="例如 /opt/agent-tools"/></label>
        <label>额外可写目录<Textarea value={value.writable} onChange={e=>onChange({...value,writable:e.target.value})} rows={2} placeholder="例如 /opt/task-cache"/></label>
      </details></>}
  </fieldset>;
}
