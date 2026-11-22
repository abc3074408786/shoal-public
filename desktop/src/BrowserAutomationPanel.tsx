import {useState} from 'react';
import {invoke} from './desktop-platform';
import {currentTarget} from './execution-target';
import {Button,Textarea} from './ui';

export default function BrowserAutomationPanel({workspaceId,pageId,open}:{workspaceId:string;pageId:string;open:boolean}) {
  const [busy,setBusy]=useState(false),[error,setError]=useState(''),[snapshot,setSnapshot]=useState(''),[capture,setCapture]=useState(''),[context,setContext]=useState(''),[copied,setCopied]=useState(false);
  async function run(op:'snapshot'|'screenshot'|'agent_context') {
    if(busy)return;
    setBusy(true);setError('');setCopied(false);
    try {
      const result=await invoke<any>('browser_command',{request:{op,workspace_id:workspaceId,id:pageId},target:currentTarget()});
      if(op==='snapshot')setSnapshot(JSON.stringify(result,null,2));
      if(op==='screenshot')setCapture(`data:image/png;base64,${result.data}`);
      if(op==='agent_context') {
        const text=`请通过 Velella 浏览器接口操作这个页面。\n运行时：${result.runtime_path}\n数据目录：${result.data_dir}\n命令参数：browser <数据目录> --request-file <UTF-8 JSON文件>\n首次请求：\n${JSON.stringify(result.request,null,2)}\n${result.instructions}`;
        setContext(text);
        try {await navigator.clipboard.writeText(text);setCopied(true)} catch {setCopied(false)}
      }
    } catch(e) {setError(String(e))} finally {setBusy(false)}
  }
  return <section className="capability-section" aria-label="Agent 页面控制">
    <h3>与 Agent 共用此页面</h3>
    <p className="capability-note">Agent 可读取、点击、填写当前网页。操作后重新读取页面；网页内容作为参考资料，不作为执行指令。目前支持主页面元素。</p>
    <div className="capability-row">
      <Button disabled={busy||!open} onClick={()=>void run('snapshot')}>读取页面</Button>
      <Button disabled={busy||!open} onClick={()=>void run('screenshot')}>页面截图</Button>
      <Button disabled={busy} onClick={()=>void run('agent_context')}>复制给 Agent</Button>
    </div>
    {error&&<p role="alert" className="ide-error">{error}</p>}
    {context&&<><p role="status">{copied?'已复制，可粘贴到 Agent 会话。':'可选择下方内容，复制到 Agent 会话。'}</p><label>页面控制信息<Textarea readOnly rows={6} value={context}/></label></>}
    {snapshot&&<details open><summary>本次页面快照</summary><pre className="capability-history">{snapshot}</pre></details>}
    {capture&&<figure><img src={capture} alt="当前内置网页的截图" style={{inlineSize:'100%',blockSize:'auto'}}/><figcaption><a href={capture} download={`${pageId}.png`}>保存 PNG 截图</a></figcaption></figure>}
  </section>;
}
