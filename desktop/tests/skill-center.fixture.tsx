import React from 'react';
import {createRoot} from 'react-dom/client';
import SkillCenter from '../plugins/skill-center/ui';
import '../src/style.css';
import '../src/theme.css';
import '../src/extensions/extensions.css';
const info={id:'skill-center',name:'技能中心',version:'1.0.0',description:'隔离验收',enabled:true,dataDir:'隔离技能库'};
async function request<T>(operation:string,values:Record<string,unknown>={}):Promise<T>{const response=await fetch('/__skill_center',{method:'POST',body:JSON.stringify({operation,...values})});const value=await response.json();if(!response.ok)throw Error(value.error);return value;}
createRoot(document.getElementById('root')!).render(<section className="extension-surface"><header className="extension-heading"><h2>技能中心</h2><span>隔离候选 · 实际 Rust 引擎</span></header><SkillCenter info={info} request={request} onBusyChange={()=>{}} onDirtyChange={()=>{}}/></section>);
