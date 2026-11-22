import type {Project, Task} from '../src/api';
import {emptyLayout, type Layout, type Session, type Workspace} from '../src/workbench-types';
import type {HudTask} from '../src/hud-digest';
import {createDemoPlugins} from './plugin-data';

type BrowserPage = {id: string; url: string; open: boolean; ready: boolean; history: string[]};
type Request = Record<string, any>;
const success = (value = {}) => ({ok: true, ...value});

export function installDemo(query: URLSearchParams) {
  const projects: Project[] = [
    {id: 'demo', name: 'Velella 示例项目', path: 'D:/Demo/Velella', setup: '', default_base: 'main'},
    {id: 'docs', name: '产品文档', path: 'D:/Demo/Docs', setup: '', default_base: 'main'},
  ];
  const workspaces: Workspace[] = projects.map(project => ({
    id: 'main:' + project.id, project_id: project.id, name: project.name, path: project.path,
    kind: 'primary', archived: false, favorite: false, position: 0,
  }));
  const main = workspaces[0].id;
  workspaces.push(
    {...workspaces[0], id: 'task:review-task', name: '界面评审', path: 'D:/Demo/velella-review', kind: 'worktree', position: 1},
    {...workspaces[0], id: 'demo-archive', name: '已归档示例', path: 'D:/Demo/velella-archive', kind: 'worktree', archived: true, position: 2},
  );
  const demoSpec = {
    name: '整理工作区演示', agent: 'codex', base: 'main', instruction: '核对导航与常用操作。',
    done: [{run: 'npm test'}], policy: {budget: {wall_clock: '1h'}},
  };
  const tasks: Task[] = [{
    id: 'review-task', project_id: 'demo', phase: 'review', reason: '界面已整理，等待人工审查。',
    verified: true, created_at: '2026-11-21T08:00:00+08:00', worktree: workspaces[2].path, branch: 'demo/review',
    native_id: null, agent: null, command: null, shell: null, pr_url: null, status: null,
    checks: [{command: 'npm test', passed: true, output: '示例：界面检查通过'}], spec: demoSpec,
  }];
  const layouts = new Map<string, Layout>();
  const sessions = new Map<string, Session[]>();
  const output = new Map<string, string>();
  const pendingInput = new Map<string, string>();
  const feedback = new Map<string, object>();
  const creations = new Map<string, Request>();
  const listeners = new Map<string, Set<(event: {payload: any}) => void>>();
  const files = new Map([
    ['README.md', '# Velella 示例项目\n\n这是交互原型的示例文档。\n\n## 浏览工作区\n\n- 在终端标签查看模拟输出。\n- 在文件列表打开源代码。\n- 在 Git 标签查看示例变更。\n'],
    ['src/main.ts', 'export const workspaceName = "Velella 示例项目";\n\nconsole.log(workspaceName);\n'],
    ['docs/评审记录.md', '# 界面评审记录\n\n- [x] 项目导航\n- [x] 设置与主题\n- [ ] 业务验收\n'],
  ]);
  const diff = 'diff --git a/src/main.ts b/src/main.ts\n--- a/src/main.ts\n+++ b/src/main.ts\n@@ -1 +1 @@\n-export const workspaceName = "示例";\n+export const workspaceName = "Velella 示例项目";\n';
  const git = {git: true, branch: 'main', status: ' M src/main.ts\n', diff, staged: ''};
  const profiles = [{id: 'demo-ssh', name: '示例 SSH 主机', host: 'dev.example.test', port: 22, user: 'demo'}];
  const agents = ['codex', 'claude-code', 'opencode', 'omp', 'minimax'];
  const extension = createDemoPlugins(workspaces, agents);
  const sources: Request[] = ['github', 'gitlab', 'linear', 'jira'].map(provider => ({
    id: provider, provider, project_id: 'demo', name: provider === 'github' ? 'Velella 示例仓库' : provider,
    scope: provider === 'github' ? 'demo/velella' : 'DEMO', base_url: 'https://example.test',
    login: '', jira_kind: 'cloud', auth_mode: 'cli',
  }));
  const items: Request[] = sources.flatMap(source => ['核对工作区导航', '整理项目使用说明'].map((title, index) => ({
    key: source.id + ':' + (index + 1), source_id: source.id, project_id: 'demo', provider: source.provider,
    remote_id: String(index + 1), identifier: '#' + (index + 1), title, url: 'https://example.test/issues/' + (index + 1),
    description: '## 示例需求\n\n检查主要操作、空状态及中文路径的展示。\n\n- [ ] 项目和工作区导航\n- [ ] 保存交互与提示',
    state: '进行中', closed: false, updated_at: '2026-11-21T00:00:00Z',
    metadata: {assignees: ['示例评审员'], priority: '普通', labels: ['界面', '待验收']},
  })));
  const links: Request[] = [];
  const automations: Request[] = [
    {id: 'daily', project_id: 'demo', enabled: true, next_run: 1795312800, interval_minutes: 1440, spec: {...demoSpec, name: '每日项目检查'}},
    {id: 'weekly', project_id: 'demo', enabled: false, next_run: 1795485600, interval_minutes: 10080, spec: {...demoSpec, name: '每周开发进展'}},
  ];
  const runs = [{id: 'run-1', automation_id: 'daily', task_id: 'review-task', started_at: 1795226400, state: 'review'}];
  const pages: BrowserPage[] = [{id: 'demo-page', url: 'https://example.test/workspace', open: true, ready: true, history: ['https://example.test/workspace']}];
  const skill = {name: '界面评审', path: 'D:/Demo/skills/review/SKILL.md', digest: 'sha256:demo-review'};
  const catalog = {digest: 'sha256:demo-tools', entries: [{id: 'demo-docs', command: 'demo-docs', args: [], catalog: ['search', 'read']}]};

  function layoutFor(id: string) {
    if (!layouts.has(id)) layouts.set(id, emptyLayout());
    return layouts.get(id)!;
  }

  function addTerminal(workspaceId: string, program = 'powershell', id: string = crypto.randomUUID()) {
    const list = sessions.get(workspaceId) ?? [];
    if (list.some(session => session.id === id)) return id;
    const title = program === 'terminal' || program === 'powershell' ? 'PowerShell' : program === 'codex' ? 'Codex' : program;
    list.push({
      id, title, kind: 'terminal', program: title === 'PowerShell' ? 'powershell' : program,
      task_id: null, native_id: null, recoverable: false, execution_mode: 'native',
      status: {pid: 8400 + list.length, exit: null, state: 'waiting_input', human_control: false, input_protocol: 1},
    });
    sessions.set(workspaceId, list);
    output.set(id, `${title} · Velella 交互原型\r\n\r\n这里是模拟终端。输入只在当前浏览器回显。\r\n\r\n> `);
    return id;
  }

  addTerminal(main, 'codex', 'demo-terminal');
  addTerminal(main, 'codex', 'demo-agent');
  layoutFor(main).panes[0] = {
    id: 'main', active: 'terminal',
    tabs: [
      {id: 'terminal', kind: 'terminal', target: 'demo-terminal', title: 'Codex · 示例会话'},
      {id: 'readme', kind: 'files', target: 'README.md', title: 'README.md'},
      {id: 'source', kind: 'files', target: 'src/main.ts', title: 'main.ts'},
      {id: 'diff', kind: 'diff', target: '', title: 'Git 变更'},
      {id: 'browser', kind: 'browser', target: 'demo-page', title: '浏览器'},
      {id: 'agent', kind: 'agent', target: 'demo-agent', title: 'Agent 输入'},
    ],
  };

  function createProject(name: string, path: string) {
    const project = {id: crypto.randomUUID(), name, path, setup: '', default_base: 'main', direct: false};
    projects.push(project);
    workspaces.push({id: 'main:' + project.id, project_id: project.id, name, path, kind: 'primary', archived: false, favorite: false, position: 0});
    return project;
  }

  function createWorkspace(request: Request) {
    const project = projects.find(project => project.id === request.project_id);
    if (!project) throw Error('示例项目不存在。');
    const id = request.request_id ?? crypto.randomUUID();
    const existing = workspaces.find(workspace => workspace.id === id);
    if (existing) return {workspace: existing, creation: creations.get(id), existing: true};
    const workspace: Workspace = {
      id, project_id: project.id, name: request.name, path: request.path ?? `D:/Demo/worktrees/${request.name}`,
      kind: 'worktree', archived: false, favorite: false, position: workspaces.length,
      parent_workspace_id: request.parent_workspace_id,
    };
    workspaces.push(workspace);
    const session_id = request.launch ? addTerminal(id, request.launch) : undefined;
    const creation = {
      request_id: id,
      frozen: {workspace, setup: {decision: 'skip', script: ''}},
      steps: [
        {kind: 'checkout', state: 'succeeded'},
        {kind: 'setup', state: 'skipped'},
        {kind: 'launch', state: session_id ? 'succeeded' : 'skipped', descriptor: session_id ? {id: session_id} : null},
      ],
    };
    creations.set(id, creation);
    return {workspace, creation};
  }

  function rpc(request: Request): object {
    const id = request.workspace_id ?? main;
    const list = sessions.get(id) ?? [];
    const session = list.find(item => item.id === request.session_id);
    switch (request.cmd) {
      case 'workbench.snapshot':
        if (query.has('disconnected')) throw Error('示例状态：后台连接已断开。');
        if (query.has('empty')) {
          const visible = projects.slice(2);
          return {projects: visible, workspaces: workspaces.filter(item => visible.some(project => project.id === item.project_id)), tasks: [], source_links: []};
        }
        return {projects, workspaces, tasks, source_links: links};
      case 'protocol': return {capabilities: ['terminal-permissions-v1', 'terminal-accounts-v1']};
      case 'cli.inspect': return {found: true, path: 'D:/Demo/velella-cli.exe', source: 'path'};
      case 'agent.check': return {available: true, authenticated: true, version: '模拟版本'};
      case 'sandbox.capabilities': return {sandbox: {platform: 'windows', backend: 'unimplemented', available: false, reason: '原型使用模拟终端'}};
      case 'project.add': return {project: createProject(request.path.split(/[\\/]/).filter(Boolean).at(-1) || '示例项目', request.path)};
      case 'project.create': return {project: createProject(request.name, request.parent + '/' + request.name)};
      case 'project.update': Object.assign(projects.find(project => project.id === request.project_id) ?? {}, request); return {};
      case 'workbench.create':
      case 'worktree.import': return createWorkspace(request);
      case 'workbench.creation.status': return {creation: creations.get(request.request_id)};
      case 'workbench.update': {
        const workspace = workspaces.find(item => item.id === id);
        for (const key of ['name', 'parent_workspace_id', 'favorite', 'archived'] as const) {
          if (workspace && key in request) Object.assign(workspace, {[key]: request[key]});
        }
        return {};
      }
      case 'workbench.sessions': return {sessions: list};
      case 'workbench.layout': return {layout: layoutFor(id), split_layout_version: 1};
      case 'workbench.layout.save': {
        const layout = {...request.layout, revision: layoutFor(id).revision + 1};
        layouts.set(id, layout);
        return {layout, split_layout_version: 1};
      }
      case 'workbench.files':
      case 'task.files': return {files: [...files.keys()]};
      case 'workbench.search': return {matches: [...files].flatMap(([path, content]) => content.split('\n').flatMap((text, index) =>
        text.toLowerCase().includes(String(request.query).toLowerCase()) ? [{path, line: index + 1, excerpt: text}] : [])), truncated: false};
      case 'workbench.file.read':
      case 'file.read': return {content: files.get(request.path) ?? ''};
      case 'workbench.file.write':
      case 'file.write': files.set(request.path, request.content); return {};
      case 'workbench.file.create': files.set(request.path, ''); return {};
      case 'workbench.file.rename': files.set(request.new_path, files.get(request.path) ?? ''); files.delete(request.path); return {};
      case 'workbench.file.delete': files.delete(request.path); return {};
      case 'task.diff':
      case 'workbench.git': return git;
      case 'workbench.git.stage': Object.assign(git, {status: 'M  src/main.ts\n', staged: git.diff, diff: ''}); return {};
      case 'workbench.git.unstage': Object.assign(git, {status: ' M src/main.ts\n', diff: git.staged, staged: ''}); return {};
      case 'workbench.git.commit': Object.assign(git, {status: '', staged: ''}); return {};
      case 'task.events': return {events: [{seq: 1, ts: '2026-11-21T00:00:00Z', reason: '示例评审记录'}]};
      case 'workbench.terminal.open': return {session_id: addTerminal(id, request.launch, request.request_id)};
      case 'workbench.terminal.output': {
        const bytes = new TextEncoder().encode(output.get(request.session_id) ?? '');
        const from = Math.min(bytes.length, Number(request.since) || 0);
        const end = Math.min(bytes.length, from + 32768);
        return {from, end, total: bytes.length, data: btoa(String.fromCharCode(...bytes.slice(from, end))),
          geometry: [{offset: 0, rows: 24, cols: 100, sequence: 1}]};
      }
      case 'workbench.terminal.input': {
        const data = request.data ?? request.text;
        if (typeof data === 'string') {
          const previous = pendingInput.get(request.session_id) ?? '';
          const typed = data.replace(/[\u001b\u0000]/g, '').replace(/\r/g, '\n');
          const rows = (previous + typed).split('\n');
          pendingInput.set(request.session_id, rows.pop() ?? '');
          for (const row of rows) {
            const transcript = (output.get(request.session_id) ?? '') + row + '\r\n[原型] 已接收模拟输入。\r\n> ';
            output.set(request.session_id, transcript);
          }
        }
        return {};
      }
      case 'workbench.terminal.activity': return {enabled: true, events: [], next: 0, total: 0};
      case 'workbench.terminal.resize': return {};
      case 'workbench.terminal.control': if (session?.status) session.status.human_control = request.human; return {};
      case 'workbench.terminal.stop': if (session?.status) session.status = {...session.status, exit: 0, state: 'exited'}; return {};
      case 'workbench.feedback.send': {
        const receipt = {state: request.draft ? 'drafted' : 'submitted'};
        feedback.set(request.request_id, receipt);
        if (!request.draft) output.set(request.session_id, (output.get(request.session_id) ?? '') + request.text + '\r\n[原型] 已接收模拟消息。\r\n> ');
        return {receipt};
      }
      case 'workbench.feedback.status': return {receipt: feedback.get(request.request_id)};
      case 'workbench.source.attach': {
        const item = items.find(item => item.key === request.source_key);
        if (item) links.push({...item, workspace_id: id});
        return {};
      }
      case 'ssh.list': return {profiles};
      case 'worktree.list': return {branches: ['main', 'demo/review'], checkouts: workspaces.map(workspace => ({path: workspace.path, branch: 'main', locked: false})), workspaces};
      case 'orchestration.capabilities': return {providers: agents.map(id => ({id, installed: true, tools: ['read'], native_validation: 'not_verified'})), skills: [skill]};
      case 'orchestration.preflight': return {workspace_id: id, ready: true, issues: []};
      case 'orchestration.list': return {runs: []};
      case 'orchestration.templates.list': return {templates: []};
      case 'orchestration.mcp.preview': return {catalog: {...catalog, path: request.path}};
      case 'orchestration.mcp.confirm': return {server: {...catalog.entries[0], tools: request.tools, source: {digest: catalog.digest, path: request.path}}};
      case 'automation.list': return {automations, runs};
      case 'automation.save': {
        const automation = {...request.automation, id: request.automation.id || crypto.randomUUID()};
        const index = automations.findIndex(item => item.id === automation.id);
        if (index < 0) automations.push(automation); else automations[index] = automation;
        return {automation};
      }
      case 'automation.pause': {
        const automation = automations.find(item => item.id === request.id);
        if (automation) automation.enabled = false;
        return {};
      }
      case 'automation.run': {
        runs.push({id: crypto.randomUUID(), automation_id: request.id, task_id: 'review-task', started_at: Math.floor(Date.now() / 1000), state: 'queued'});
        return {task_id: 'review-task'};
      }
      case 'sources.list':
      case 'sources.sync': return {sources, repositories: [], links};
      case 'sources.items': return {items: items.filter(item => item.source_id === request.source_id && (request.state !== 'closed' || item.closed) && item.title.includes(request.search ?? '')), next: null};
      case 'sources.cached':
      case 'sources.detail': return {item: items.find(item => item.key === request.key)};
      case 'sources.comments': return {comments: [{id: 'comment', author: '示例评审员', created: '2026-11-21', body: '请通过示例工作区查看界面。'}], next: null};
      case 'sources.states': return {states: [{id: 'closed', name: '已完成'}]};
      case 'sources.scopes': return {choices: [{id: 'DEMO', name: '示例研发组'}]};
      case 'sources.create_options': return {options: {types: [], fields: []}};
      case 'sources.save': {
        const source = {...request.source, id: request.source.id || crypto.randomUUID()};
        const index = sources.findIndex(item => item.id === source.id);
        if (index < 0) sources.push(source); else sources[index] = source;
        return {source};
      }
      case 'sources.remove': {
        const index = sources.findIndex(item => item.id === request.source_id);
        if (index >= 0) sources.splice(index, 1);
        return {};
      }
      default: throw Error('原型未模拟此操作：' + request.cmd);
    }
  }

  function browser(request: Request) {
    const page = pages.find(page => page.id === request.id);
    switch (request.op) {
      case 'list': return {pages: pages.filter(page => page.open)};
      case 'create': {
        const page = {id: crypto.randomUUID(), url: String(request.url), open: true, ready: true, history: [String(request.url)]};
        pages.push(page);
        return {page};
      }
      case 'open': if (page) page.open = true; return {page};
      case 'close': if (page) page.open = false; return {};
      case 'navigate': if (page) { page.url = String(request.url); page.history.push(page.url); } return {};
      case 'snapshot': return {title: '原型浏览器', url: page?.url, text: '示例页面快照', elements: []};
      case 'reload':
      case 'back':
      case 'forward': return {};
      default: throw Error('此操作需要 Velella 桌面版的浏览器。');
    }
  }

  function hud(action: string) {
    if (action === 'toggle') window.open('./hud.html', 'velella-demo-hud', 'popup,width=420,height=480');
    if (action === 'hide') window.close();
    if (action === 'open_main' || action === 'open_task') location.assign('./app.html?view=project');
    if (action === 'ready') {
      const task = {name: '整理工作区演示', phase: 'running', phase_label: '执行中', project: projects[0].name,
        reason: '正在等待确认一条模拟输入。', state: 'waiting_input', id: 'demo-task', human_control: false,
        tool: '', turns: 2, since_changed: 0, hooks_ready: true, bytes: 64, created_at: tasks[0].created_at} satisfies HudTask;
      const state = {connected: !query.has('stale'), ts: Date.now(), tasks: query.has('empty') ? [] : [task]};
      for (const handler of listeners.get('velella:hud-state') ?? []) handler({payload: state});
    }
  }

  window.velellaDesktop = {
    listen(event, callback) {
      const handlers = listeners.get(event) ?? new Set();
      handlers.add(callback);
      listeners.set(event, handlers);
      return () => {handlers.delete(callback);};
    },
    async invoke<T>(command: string, raw?: unknown): Promise<T> {
      const args = (raw ?? {}) as Request;
      let value: unknown;
      switch (command) {
        case 'rpc': value = success(structuredClone(rpc(args.request))); break;
        case 'execution_hosts': value = success({hosts: [], profiles: [], connections: []}); break;
        case 'desktop_extensions': value = success({result: structuredClone(extension(args))}); break;
        case 'window': value = false; break;
        case 'dialog_open': value = 'D:/Demo/待打开示例'; break;
        case 'dialog_confirm': value = window.confirm(args.message); break;
        case 'browser_command': value = structuredClone(browser(args.request)); break;
        case 'desktop_resources': value = {privateBytes: 73400320, processes: [{type: 'renderer', pid: 8400, privateBytes: 73400320}], sampledAt: Date.now()}; break;
        case 'pr_checks': value = {state: 'ready', branch: 'main', checked_at: new Date().toISOString(),
          pull_requests: [{number: 7, title: '示例工作区界面调整', url: 'https://example.test/pull/7', state: 'OPEN',
            draft: false, head_sha: '1e0d3e1b0000', head_branch: 'demo/review', base_branch: 'main',
            checks: [{id: 'build', name: '前端构建', status: 'success', label: '示例通过', workflow: '界面检查'}],
            summary: {success: 1, pending: 0, failure: 0, cancelled: 0, skipped: 0, unknown: 0}}]}; break;
        case 'resource_manager': {
          const metrics = {cpu: 0.5, workingSetBytes: 28311552, privateBytes: 16777216};
          value = {...metrics, projects: [{...metrics, ...projects[0], workspaces: [{...metrics, ...workspaces[0], sessions: []}]}]};
          break;
        }
        case 'resource_space_scan': value = {token: 'demo-scan', rows: []}; break;
        case 'hud': hud(args.action); break;
        case 'browser_layout':
        case 'notify': break;
        default: throw Error('原型未模拟此桌面操作：' + command);
      }
      return value as T;
    },
  };
  if (query.get('view') === 'project') localStorage.setItem('velella:workspace', main);
  else localStorage.removeItem('velella:workspace');
  localStorage.setItem('velella:ui:theme', JSON.stringify(query.get('theme') === 'dark' ? 'dark' : 'light'));
  localStorage.setItem('velella:terminal-preferences:v1', JSON.stringify({renderer: 'dom'}));
}
