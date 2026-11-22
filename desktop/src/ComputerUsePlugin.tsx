import {useEffect,useRef,useState} from 'react';
import {Monitor} from 'lucide-react';
import {Button} from './ui';
import {nativePluginRequest} from './native-plugins';
import type {ExecutionTarget} from './execution-target';

export default function ComputerUsePlugin({target,onImport}:{target:ExecutionTarget;onImport:(path:string)=>void}){
  const [busy,setBusy]=useState(false),[error,setError]=useState('');
  const pending=useRef(false),alive=useRef(true);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
  async function prepare(){
    if(pending.current)return;
    pending.current=true;setBusy(true);setError('');
    try{
      const result=await nativePluginRequest<{path:string}>({action:'prepareBundledMcp',pluginId:'computer-use'},target);
      if(alive.current)onImport(result.path);
    }catch(e){if(alive.current)setError(String(e))}
    finally{pending.current=false;if(alive.current)setBusy(false)}
  }
  return <section className="plugins-bundled" aria-label="本地 MCP 插件">
    <h2>本地 MCP 插件</h2>
    <div className="plugin-installed-row">
      <span className="plugin-icon" aria-hidden="true"><Monitor size={24}/></span>
      <div className="plugin-identity"><strong>Computer Use · 电脑操作</strong><span>为鱼群提供截图、鼠标和键盘工具，仅支持本机 Windows。添加时选择只读观察或受控输入，并确认需要的工具。</span></div>
      <Button disabled={busy||target.kind!=='local'} onClick={()=>void prepare()}>{busy?'正在准备目录…':'添加 Computer Use'}</Button>
    </div>
    <p className="plugins-catalog-disclosure">添加不会启动控制服务。输入操作需要控制租约；可通过本机停止命令撤销控制。清单启停影响后续选用，已有会话保留原配置。</p>
    {target.kind!=='local'&&<p className="plugins-catalog-disclosure">请切回本机执行主机后添加 Computer Use。</p>}
    {error&&<p className="plugins-error" role="alert">{error}</p>}
  </section>;
}
