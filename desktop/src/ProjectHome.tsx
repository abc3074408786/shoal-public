import {useState} from 'react';
import {ChevronRight,Folder,FolderOpen,GitBranch,Plus,Search} from 'lucide-react';
import type {Project} from './api';
import {Button,Input} from './ui';
import logo from './assets/velella-logo-v2.png';
import './project-home.css';

export default function ProjectHome({projects,disabled,onOpen,onCreate,onAdd}:{projects:Project[];disabled:boolean;onOpen:(project:Project)=>void;onCreate:()=>void;onAdd:()=>void}){
  const [query,setQuery]=useState('');
  const search=query.trim().toLocaleLowerCase();
  const visible=projects.filter(project=>`${project.name}\n${project.path}`.toLocaleLowerCase().includes(search));
  return <section className="project-home" aria-label="项目工作台"><div className="project-home-inner">
    <header className="project-home-header">
      <div className="project-home-brand"><img src={logo} alt="鱼群" width="40" height="40"/><div><h1>项目工作台</h1><p>继续已有项目，或开始一个新项目。</p></div></div>
    </header>
    <div className="project-home-content">
    <section className="project-home-start" aria-labelledby="project-home-start-title">
      <h2 className="project-home-section-title" id="project-home-start-title">开始</h2>
      <div className="project-home-actions">
        <Button disabled={disabled} aria-label="打开项目" onClick={onAdd}><FolderOpen size={18} aria-hidden="true"/><span><strong>打开项目</strong><small>选择本地文件夹或仓库</small></span></Button>
        <Button disabled={disabled} aria-label="创建项目" onClick={onCreate}><Plus size={18} aria-hidden="true"/><span><strong>创建项目</strong><small>从一个新项目开始</small></span></Button>
      </div>
    </section>
    <section className="project-home-projects" aria-labelledby="project-home-list-title">
    <h2 className="project-home-section-title" id="project-home-list-title">已有项目 <span>{projects.length}</span></h2>
    <label className="project-home-search"><Search size={15} aria-hidden="true"/><Input type="search" aria-label="搜索项目" placeholder="搜索项目或路径" value={query} onChange={event=>setQuery(event.target.value)} onKeyDown={event=>{if(event.key==='Escape'){setQuery('');event.stopPropagation()}}}/></label>
    <ul className="project-home-grid" aria-labelledby="project-home-list-title">{visible.map(project=><li key={project.id}><Button className="project-home-card" disabled={disabled} aria-label={'打开项目 '+project.name} onClick={()=>onOpen(project)}>
      <span className="project-home-folder" aria-hidden="true">{project.direct===false?<GitBranch size={17}/>:<Folder size={17}/>}</span>
      <span className="project-home-info"><strong title={project.name}>{project.name}</strong><code title={project.path}>{project.path}</code></span>
      <ChevronRight className="project-home-open" size={16} aria-hidden="true"/>
    </Button></li>)}</ul>
    {!projects.length&&<div className="project-home-empty"><FolderOpen size={28} aria-hidden="true"/><h3>还没有项目</h3><p>打开已有目录，或创建一个新项目。</p></div>}
    {!!projects.length&&!visible.length&&<div className="project-home-empty" role="status"><Search size={26} aria-hidden="true"/><h3>没有匹配的项目</h3><p>试试项目名称或路径中的其他文字。</p><Button onClick={()=>setQuery('')}>清除搜索</Button></div>}
    </section>
    </div>
  </div></section>;
}
