import {useEffect,useState} from 'react';

/** Delay feedback only. Data and terminal readiness never wait for this timer. */
export function useLoadingReveal(){
  const [revealed,setRevealed]=useState(false);
  useEffect(()=>{const timer=setTimeout(()=>setRevealed(true),180);return()=>clearTimeout(timer)},[]);
  return revealed;
}
