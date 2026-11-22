import type {ExtensionInfo, OntologyEntity} from '../src/extensions/types';
import type {Workspace} from '../src/workbench-types';

export function createDemoPlugins(workspaces: Workspace[], agents: string[]) {
  const plugins: ExtensionInfo[] = [
    {id: 'ontology', name: '开发本体', version: '1.1.0', description: '按角色查看示例任务、交付证据和关系。',
      installed: true, enabled: true, dataDir: 'D:/Demo/ontology', engine: {version: '模拟数据'}},
    {id: 'skill-center', name: '技能中心', version: '1.0.0', description: '查看示例技能及其来源。',
      installed: true, enabled: true, dataDir: 'D:/Demo/skills'},
  ];
  const skill = {name: '界面评审', path: 'D:/Demo/skills/review/SKILL.md', digest: 'sha256:demo-review',
    scope: 'global', source: {kind: 'local', path: 'D:/Demo/skills/review'}, installations: []};
  const entity: OntologyEntity = {id: 'example-task', type: 'task', project: 'demo', state: '待评审', revision: 1,
    fields: {name: '核对工作区导航', description: '在浅色和深色主题下检查主要入口。'}, source: '示例项目', evidence: '模拟记录'};
  const definition = {name: 'task', name_cn: '任务', description: '工作区界面评审任务', initial_state: '待评审',
    states: ['待评审', '完成'], transitions: [['待评审', '完成']],
    fields: [{name: 'name', description: '任务名称', required: true, kind: {kind: 'text'}}]};
  const role = {id: 'reviewer', name_cn: '界面评审员'};
  const action = {kind: 'review', role: role.id, subject: entity.id, subject_name: entity.fields.name,
    detail: '核对例行导航和保存交互。', subject_snapshot: entity};

  return function extension(args: Record<string, any>) {
    if (args.action === 'list') return {plugins};
    const plugin = plugins.find(item => item.id === args.pluginId);
    if (!plugin) throw Error('原型中没有此插件。');
    if (args.action === 'setEnabled') {
      plugin.enabled = args.enabled;
      return plugin;
    }
    if (args.action !== 'call') throw Error('此插件操作需要桌面运行时。');
    const request = args.request;
    if (plugin.id === 'skill-center') {
      switch (request.operation) {
        case 'workspaces': return {workspaces};
        case 'agents': return {agents: agents.map(id => ({id, program: id, paths: [], nativeDirectory: false, verification: '模拟数据'}))};
        case 'list': return {skills: [skill], discovered: [], backups: [], pending: []};
        case 'read': return {content: '# 界面评审\n\n核对导航、表单和空状态。', digest: skill.digest, files: [], dependencies: []};
      }
    } else {
      switch (request.operation) {
        case 'describe': return {initialized: true, engineReady: true};
        case 'schema': return {definitions: {entity_types: [definition], roles: [role], relations: []}};
        case 'projects': return {projects: [{...entity, id: 'demo', type: 'project', fields: {name: 'Velella 示例项目'}}]};
        case 'entities': return {entities: [entity]};
        case 'queue': return {actions: [action]};
        case 'entity': return entity;
        case 'context': return {task: entity, artifacts: [], evidence: [], next: {actions: []}};
        case 'neighbors': return {incoming: [], outgoing: []};
        case 'check': return {project: 'demo', check_version: '原型', valid: true, summary: {errors: 0, warnings: 0},
          basis: 'recorded_facts_only', rules: [], findings: [], snapshot: {entities: [{id: entity.id, revision: 1}], relation_count: 0}};
      }
    }
    throw Error('此插件操作需要桌面运行时。');
  };
}
