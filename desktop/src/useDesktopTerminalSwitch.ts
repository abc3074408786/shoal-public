import {useEffect,useRef,useState} from 'react';
import {invoke,listen} from './desktop-platform';
import {rpc,isTauri} from './api';
import {currentTarget} from './execution-target';
import type {Workspace,Session} from './workbench-types';

export type DesktopTerminalSwitch={request_id:string;workspace_id:string;session_id:string;expires_at_ms:number};
const blocked=()=>!!document.querySelector('.app-settings-page,dialog[open],[role="dialog"],[role="alertdialog"],[role="menu"]');

export function useDesktopTerminalSwitch(navigate:(snapshot:any,request:DesktopTerminalSwitch)=>void,selected:(request:DesktopTerminalSwitch)=>boolean){
  const [request,setRequest]=useState<DesktopTerminalSwitch>();
  const current=useRef({navigate,selected});current.current={navigate,selected};
  useEffect(()=>{
    if(!isTauri()||!window.velellaDesktop)return;
    let disposed=false,handling=false;
    const check=(request:DesktopTerminalSwitch)=>{
      if(disposed||Date.now()>=request.expires_at_ms)throw Error('终端切换已超时');
      if(currentTarget().kind!=='local')throw Error('桌面当前正在使用 SSH 主机，请先切回本机');
      if(blocked())throw Error('请先返回工作界面或关闭当前对话框、菜单，再切换终端');
    };
    const subscription=listen<DesktopTerminalSwitch>('velella:terminal-switch',event=>{
      const request=event.payload;
      if(handling)return;
      handling=true;
      void (async()=>{
        let result:{ok:boolean;focused?:boolean;error?:string};
        try{
          check(request);
          const target=currentTarget();
          const snapshot=await rpc({cmd:'workbench.snapshot'},target);
          const workspace:Workspace|undefined=snapshot.workspaces.find((w:Workspace)=>w.id===request.workspace_id);
          if(!workspace||workspace.archived)throw Error('目标工作区不存在或已归档');
          const state=await rpc({cmd:'workbench.sessions',workspace_id:workspace.id},target);
          const session:Session|undefined=state.sessions.find((s:Session)=>s.id===request.session_id);
          if(!session||session.native_chat||session.kind==='agent')throw Error('目标不是此工作区的原生终端');
          check(request);
          current.current.navigate(snapshot,request);setRequest(request);
          let selectedOnce=false;
          for(;;){
            check(request);
            const selected=current.current.selected(request);
            if(selectedOnce&&!selected)throw Error('桌面选择已改变，终端切换已取消');
            selectedOnce=selectedOnce||selected;
            const textarea=[...document.querySelectorAll<HTMLTextAreaElement>(`[data-terminal-pane="${CSS.escape(request.session_id)}"] textarea`)].find(el=>el.getClientRects().length&&!el.disabled);
            if(selected&&textarea){
              textarea.focus({preventScroll:true});
              if(document.activeElement===textarea){result={ok:true,focused:true};break;}
            }
            await new Promise(resolve=>setTimeout(resolve,25));
          }
        }catch(error){result={ok:false,error:String(error)}}
        if(!disposed){setRequest(undefined);await invoke('terminal_switch_result',{...request,...result}).catch(()=>{})}
        handling=false;
      })();
    });
    return()=>{disposed=true;void subscription.then(off=>off())};
  },[]);
  return request;
}
