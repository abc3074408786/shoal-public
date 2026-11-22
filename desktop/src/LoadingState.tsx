import type {CSSProperties} from 'react';
import logo from './assets/velella-logo-v2.png';
import {useLoadingReveal} from './loading-reveal';
import './loading-state.css';

/** Visual feedback only: readiness, retries and terminal input stay with callers. */
export default function LoadingState({label='正在加载',compact=false,className=''}:{label?:string;compact?:boolean;className?:string}){
  const revealed=useLoadingReveal();
  return <div className={`loading-state ${compact?'loading-state--compact':'loading-state--panel'} ${className}`} role="status" data-loading-revealed={revealed}>
    <div className="loading-school" aria-hidden="true" style={{'--fish-mark':`url(${logo})`} as CSSProperties}>
      <i className="loading-fish loading-fish--lead"/>
      <i className="loading-fish loading-fish--middle"/>
      <i className="loading-fish loading-fish--rear"/>
      <span className="loading-current loading-current--one"/>
      <span className="loading-current loading-current--two"/>
    </div>
    <span>{label}</span>
  </div>;
}
