import {Terminal} from 'lucide-react';
import {agentCatalog,type AgentId} from './agent-preferences';
import codex from './assets/agents/codex.svg';
import claude from './assets/agents/claude-code.svg';
import gemini from './assets/agents/gemini.png';
import opencode from './assets/agents/opencode.svg';
import omp from './assets/agents/omp.svg';
import kimi from './assets/agents/kimi.svg';
import zcode from './assets/agents/zcode.png';
import './agent-logo.css';

const logos:Partial<Record<AgentId,string>>={codex,'claude-code':claude,gemini,opencode,omp,kimi,zcode};

export default function AgentLogo({agent,size=18}:{agent:AgentId;size?:number}){
  if(agent==='minimax')return <Terminal size={size} aria-hidden="true"/>;
  const image=agent==='codex'||agent==='claude-code'||agent==='gemini'||agent==='omp'||agent==='zcode';
  return <i className={`agent-logo agent-logo--${agent}${image?'':' is-mask'}`} aria-hidden="true" style={{width:size,height:size,...(!image?{maskImage:`url("${logos[agent]}")`}:{})}}>
    {image&&<img src={logos[agent]} alt="" draggable={false}/>}
  </i>;
}

export function AgentProgramIcon({program,size=16}:{program?:string;size?:number}){
  const name=program?.split(/[\\/]/).at(-1)?.replace(/\.(exe|cmd|bat|ps1)$/i,'').toLowerCase();
  const agent=agentCatalog.find(agent=>agent.program===name);
  return agent?<AgentLogo agent={agent.id} size={size}/>:<Terminal size={size} aria-hidden="true"/>;
}
