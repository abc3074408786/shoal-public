import {createContext,ReactNode,useContext,useEffect,useRef,useState} from 'react';
import {Button,NativeDialog} from './ui';
const Confirmation=createContext<(message:string)=>Promise<boolean>>(()=>Promise.resolve(false));
export function ConfirmationProvider({children}:{children:ReactNode}){
  const [message,setMessage]=useState<string|null>(null),resolve=useRef<((value:boolean)=>void)|null>(null),dialog=useRef<HTMLDialogElement>(null);
  function close(value:boolean){resolve.current?.(value);resolve.current=null;setMessage(null);dialog.current?.close()}
  useEffect(()=>{if(message!==null&&!dialog.current?.open)dialog.current?.showModal()},[message]);
  useEffect(()=>()=>{resolve.current?.(false)},[]);
  function ask(text:string){resolve.current?.(false);setMessage(text);return new Promise<boolean>(done=>{resolve.current=done})}
  return <Confirmation.Provider value={ask}>{children}<NativeDialog ref={dialog} aria-labelledby="confirm-action-title" onCancel={e=>{e.preventDefault();close(false)}}><div className="dialog-heading"><h2 id="confirm-action-title">确认操作</h2></div><div className="dialog-body"><p style={{whiteSpace:'pre-wrap'}}>{message}</p></div><div className="dialog-footer"><Button onClick={()=>close(false)}>取消</Button><Button className="primary" onClick={()=>close(true)}>确认</Button></div></NativeDialog></Confirmation.Provider>;
}
export const useConfirm=()=>useContext(Confirmation);
