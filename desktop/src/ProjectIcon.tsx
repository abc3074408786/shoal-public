import {BookOpen,Code2,Folder,Globe,Layers,Package,Rocket,Terminal} from 'lucide-react';
import type {ProjectIconName} from './project-library';
const icons={folder:Folder,code:Code2,book:BookOpen,layers:Layers,rocket:Rocket,package:Package,terminal:Terminal,globe:Globe};
export const projectIconLabels={folder:'文件夹',code:'代码',book:'文档',layers:'图层',rocket:'火箭',package:'软件包',terminal:'终端',globe:'网站'};
export default function ProjectIcon({name='folder',size=15}:{name?:ProjectIconName;size?:number}){const Icon=icons[name]||Folder;return <Icon size={size} aria-hidden="true"/>}
