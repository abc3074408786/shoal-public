import type {ReactNode} from 'react';
import type {Project} from './api';
import type {Workspace} from './workbench-types';
import './project-workspace.css';

/** Project chrome surrounds the existing live Workbench without owning its sessions. */
export default function ProjectWorkspace({projects,workspace,children}:{projects:Project[];workspaces:Workspace[];workspace:Workspace;onWorkspace:(workspace:Workspace)=>void;children:ReactNode}){
  const project=projects.find(p=>p.id===workspace.project_id);
  const projectName=project?.name||workspace.name;
  return <div className="project-layout-options project-workspace" data-layout="tabs">
    <div className="layout-project-board"><section className="layout-project-column" aria-label={projectName+' 项目工作区'}>
        {children}
    </section></div>
  </div>;
}
