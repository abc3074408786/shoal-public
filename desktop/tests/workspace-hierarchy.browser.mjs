// Browser integration against the REAL Vite/React UI and an explicitly MOCKED /_velella runtime.
// No Rust runtime, Git operations, setup scripts, terminals, agents, or real project writes run.
// Git Bash (from any directory):
// NODE_PATH=C:/Users/developer/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules node D:/orcas/desktop/tests/workspace-hierarchy.browser.mjs
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {mkdirSync, realpathSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';

const {chromium} = createRequire(import.meta.url)('playwright');
const root = fileURLToPath(new URL('..', import.meta.url));
const evidence = resolve(root, '../artifacts', `workspace-hierarchy-browser-${Date.now()}`);
mkdirSync(evidence, {recursive: true});
const report = {
  runtime: 'MOCK /_velella; real Vite application and installed Microsoft Edge; no production runtime',
  evidence, startedAt: new Date().toISOString(), tests: [], console: [], applicationErrors: [],
  hmrLoopbackWarnings: [], networkErrors: [], blockedRequests: [], requests: [], layouts: [], screenshots: [],
};
const server = await createServer({
  root, configFile: false, cacheDir: resolve(evidence, 'vite-cache'), logLevel: 'error',
  plugins: [react(), {
    name: 'workspace-hierarchy-mock-runtime-guard',
    configureServer(vite) {
      // Defense in depth: even if browser interception fails, there is no runtime proxy.
      vite.middlewares.use('/_velella', (req, res) => {
        report.blockedRequests.push({source: 'Vite guard', method: req.method, url: req.url});
        res.statusCode = 500;
        res.end('MOCK ONLY: browser must intercept every runtime request');
      });
    },
  }],
  server: {host: '127.0.0.1', port: 0, strictPort: false,
    fs: {allow: [root, realpathSync(resolve(root, 'node_modules')), evidence]}},
});
let browser, origin;
const seedScript = '# MOCK ONLY: stored text, never executed\n  seed preparation  \n';
function fixture() {
  const projects = [
    {id: 'p', name: 'Atlas [MOCK]', path: 'D:/MOCK-ONLY/atlas', setup: seedScript, default_base: 'origin/release', setup_default: 'skip', direct: false},
    {id: 'q', name: 'Boreal [MOCK]', path: 'D:/MOCK-ONLY/boreal', setup: '', default_base: 'origin/main', setup_default: 'skip', direct: false},
  ];
  const w = (id, project_id, name, parent_workspace_id = null, extra = {}) => ({
    id, project_id, name, parent_workspace_id, path: `D:/MOCK-ONLY/${id}`, kind: 'worktree',
    position: 0, archived: false, favorite: false, ...extra,
  });
  // Deliberately put Boreal first: snapshot removal of the active workspace switches projects.
  const workspaces = [
    w('main:q', 'q', 'Boreal primary', null, {kind: 'primary'}),
    w('main:p', 'p', 'Atlas primary', null, {kind: 'primary'}),
    w('p-trunk', 'p', 'Platform'), w('p-child', 'p', 'Feature', 'p-trunk'),
    w('p-grand', 'p', 'Needle descendant', 'p-child'),
    w('p-old', 'p', 'Retired ancestor', null, {archived: true}),
    w('p-active', 'p', 'Living child', 'p-old'),
    w('p-cross', 'p', 'Cross project child', 'q-old'),
    w('q-old', 'q', 'Archived upstream', null, {archived: true}),
    w('q-other', 'q', 'Other workspace'),
    w('q-desc', 'q', 'Cross-project descendant', 'p-child'),
    w('p-remote', 'p', 'Different host', null, {host_id: 'another-mock-host'}),
  ];
  return {projects, workspaces, journals: {}, expectedWrites: [], writes: [], requests: [], hidden: new Set()};
}
const byId = (f, id) => f.workspaces.find(w => w.id === id);
function arm(f, request) {
  f.expectedWrites.push(actual => assert.deepEqual(actual, request, 'write must be the exact partial payload'));
}
function armCreate(f, projectId, parentId, name) {
  f.expectedWrites.push(actual => {
    assert.match(actual.request_id, /^[0-9a-f-]{36}$/i);
    assert.deepEqual(actual, {
      cmd: 'workbench.create', project_id: projectId, request_id: actual.request_id,
      parent_workspace_id: parentId, setup: 'inherit', name, execution: {mode: 'native'},
    }, 'creation must not inherit the selected workspace Git base or launch anything');
  });
}
function mockRpc(f, request) {
  const workspace = request.workspace_id && byId(f, request.workspace_id);
  if (request.workspace_id) assert(workspace, `unknown mock workspace ${request.workspace_id}`);
  switch (request.cmd) {
    case 'workbench.snapshot': return {projects: f.projects, workspaces: f.workspaces.filter(w => !f.hidden.has(w.id)), tasks: [], source_links: []};
    case 'workbench.git': return {branch: workspace.project_id === 'p' ? 'mock/selected-workspace-branch' : 'main', status: '', diff: '', staged: ''};
    case 'workbench.sessions': return {sessions: []};
    case 'workbench.creation.status': {
      assert.deepEqual(request, {cmd: 'workbench.creation.status', request_id: request.request_id});
      assert(f.journals[request.request_id], 'only a created workspace UUID may be observed');
      return {creation: structuredClone(f.journals[request.request_id]), retry_ready: false};
    }
    case 'workbench.files': return {files: []};
    case 'workbench.layout': return {layout: {revision: 0, axis: 'horizontal', ratio: 0.5, panes: [{id: 'main', active: null, tabs: []}], closed: []}, split_layout_version: 1};
    case 'sources.list': return {sources: [], links: [], repositories: []};
    case 'worktree.list': {
      const project = f.projects.find(p => p.id === request.project_id);
      assert(project, 'unknown mock project');
      return {branches: ['main', 'origin/main', 'origin/release'], default_base: {resolved: {
        reference: project.default_base || 'origin/main', oid: 'a'.repeat(40), inferred: !project.default_base,
      }}};
    }
    case 'project.update':
    case 'workbench.update':
    case 'workbench.create': {
      const validate = f.expectedWrites.shift();
      assert(validate, `unexpected write BLOCKED: ${request.cmd}`);
      validate(request); // Validate before mutating ONLY the in-memory fixture.
      f.writes.push(structuredClone(request));
      if (request.cmd === 'project.update') {
        const project = f.projects.find(p => p.id === request.project_id);
        assert(project);
        const {cmd, project_id, ...patch} = request;
        Object.assign(project, patch);
        return {project};
      }
      if (request.cmd === 'workbench.update') {
        const {cmd, workspace_id, ...patch} = request;
        Object.assign(workspace, patch);
        return {workspace};
      }
      assert(!byId(f, request.request_id), 'duplicate creation is not expected');
      const created = {id: request.request_id, project_id: request.project_id, name: request.name,
        parent_workspace_id: request.parent_workspace_id, path: `D:/MOCK-ONLY/${request.request_id}`,
        kind: 'worktree', position: 0, archived: false, favorite: false};
      f.workspaces.push(created);
      // MOCK server journal only: no checkout, setup, launch, or prompt actually executes.
      const creation = {request_id: request.request_id, request: structuredClone(request),
        frozen: {workspace: structuredClone(created), setup: {decision: 'skip', script: f.projects.find(p => p.id === request.project_id).setup}},
        steps: [{kind: 'checkout', state: 'succeeded'}, {kind: 'setup', state: 'skipped'},
          {kind: 'launch', state: 'skipped'}, {kind: 'prompt', state: 'skipped'}]};
      f.journals[request.request_id] = creation;
      return {workspace: created, creation};
    }
    default: throw new Error(`unallowlisted RPC BLOCKED (including all unexpected writes): ${request.cmd}`);
  }
}
function isHmrLoopback(text) {
  return /WebSocket/i.test(text) && /(?:127\.0\.0\.1|localhost|\[::1\])/.test(text)
    || /\[vite\].*(?:websocket|direct websocket connection|server connection lost)/i.test(text);
}
function button(page, f, id) {
  return page.locator(`.ide-workspace[title="${byId(f, id).path}"]`);
}
const dialog = page => page.getByRole('dialog');
async function selected(page, f, id) {
  await page.waitForFunction(id => localStorage.getItem('velella:workspace') === id, id);
  if (!byId(f, id).archived) await page.getByRole('button', {name: '工作区选项', exact: true}).waitFor();
}
async function select(page, f, id) {
  await button(page, f, id).click();
  await selected(page, f, id);
  assert.equal(await button(page, f, id).getAttribute('aria-current'), 'page');
}
async function menu(page, name) {
  await page.getByRole('button', {name: '工作区选项', exact: true}).click();
  await page.getByRole('menuitem', {name, exact: true}).click();
  await page.getByRole('menu', {includeHidden: true}).waitFor({state: 'detached'});
  await dialog(page).waitFor();
}
async function closeDialog(page) {
  await page.keyboard.press('Escape');
  await dialog(page).waitFor({state: 'hidden'});
}
async function saveDialog(page, f) {
  await dialog(page).getByRole('button', {name: '确定', exact: true}).click();
  await dialog(page).waitFor({state: 'hidden'});
  assert.equal(f.expectedWrites.length, 0, 'expected write was consumed');
}
async function capture(page, name) {
  const path = resolve(evidence, `${name}.png`);
  await page.screenshot({path, fullPage: true, animations: 'disabled'});
  report.screenshots.push(path);
}
async function layout(page, name) {
  const sample = await page.evaluate(() => {
    const doc = document.documentElement, body = document.body;
    const modal = document.querySelector('dialog[open]');
    const rect = modal?.getBoundingClientRect();
    return {theme: doc.dataset.theme, width: innerWidth, height: innerHeight,
      scrollWidth: Math.max(doc.scrollWidth, body.scrollWidth), scrollHeight: Math.max(doc.scrollHeight, body.scrollHeight),
      shellBackground: getComputedStyle(document.querySelector('.ide-shell')).backgroundColor,
      modal: rect ? {x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom,
        scrollWidth: modal.scrollWidth, clientWidth: modal.clientWidth} : null};
  });
  report.layouts.push({name, ...sample});
  assert(sample.scrollWidth <= sample.width + 1, `document horizontal overflow: ${JSON.stringify(sample)}`);
  assert(sample.scrollHeight <= sample.height + 1, `document vertical overflow: ${JSON.stringify(sample)}`);
  if (sample.modal) {
    assert(sample.modal.x >= -1 && sample.modal.y >= -1 && sample.modal.right <= sample.width + 1
      && sample.modal.bottom <= sample.height + 1, `dialog outside viewport: ${JSON.stringify(sample)}`);
    assert(sample.modal.scrollWidth <= sample.modal.clientWidth + 1, 'dialog has horizontal overflow');
  }
  return sample;
}
async function tabContained(page, count = 12) {
  for (let i = 0; i < count; i++) {
    await page.keyboard.press(i % 3 === 0 ? 'Shift+Tab' : 'Tab');
    assert(await page.evaluate(() => !!document.activeElement?.closest('dialog[open]')), 'keyboard focus escaped the native modal');
  }
}
async function scenario(name, body, {theme = 'dark', width = 1440} = {}) {
  const result = {name, theme, viewport: {width, height: 900}, status: 'running'};
  report.tests.push(result);
  const context = await browser.newContext({viewport: result.viewport, serviceWorkers: 'block'});
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const f = fixture();
  const errorStart = report.applicationErrors.length, blockedStart = report.blockedRequests.length;
  const networkStart = report.networkErrors.length;
  page.on('console', message => {
    const entry = {test: name, type: message.type(), text: message.text(), location: message.location()};
    report.console.push(entry);
    if (isHmrLoopback(entry.text)) report.hmrLoopbackWarnings.push(entry);
    else if (entry.type === 'error') report.applicationErrors.push(entry);
  });
  page.on('pageerror', error => report.applicationErrors.push({test: name, type: 'pageerror', text: error.stack || error.message}));
  page.on('requestfailed', request => {
    // Requests cancelled by navigation/teardown are not application failures.
    const error = request.failure()?.errorText || '';
    const entry = {test: name, url: request.url(), error};
    if (isHmrLoopback(`${entry.url} ${error}`)) report.hmrLoopbackWarnings.push(entry);
    else if (!/ERR_ABORTED/.test(error)) report.networkErrors.push(entry);
  });
  page.on('response', response => {
    if (response.status() >= 400) report.networkErrors.push({test: name, url: response.url(), status: response.status()});
  });
  await context.route('**/*', async route => {
    const request = route.request(), url = new URL(request.url());
    if (url.origin === origin && url.pathname === '/_velella') {
      let payload;
      try {
        assert.equal(request.method(), 'POST');
        payload = request.postDataJSON();
        const entry = {test: name, ...payload};
        report.requests.push(entry); f.requests.push(structuredClone(payload));
        const response = mockRpc(f, payload);
        await route.fulfill({json: {ok: true, ...response}});
      } catch (error) {
        report.blockedRequests.push({test: name, request: payload, error: String(error)});
        await route.fulfill({json: {ok: false, error: `MOCK BLOCKED: ${error.message}`}});
      }
      return;
    }
    if (url.origin === origin && request.method() === 'GET') return route.continue();
    report.blockedRequests.push({test: name, url: url.href, method: request.method(), error: 'unexpected network request'});
    await route.abort('blockedbyclient');
  });
  await page.addInitScript(({theme}) => {
    localStorage.setItem('velella:workspace', 'p-trunk');
    localStorage.setItem('velella:ui:theme', JSON.stringify(theme));
    localStorage.setItem('velella:ui:left-open', 'true');
    localStorage.setItem('velella:ui:right-open', 'false');
    localStorage.setItem('velella:terminal-preferences:v1', JSON.stringify({renderer: 'dom'}));
  }, {theme});
  try {
    await page.goto(origin, {waitUntil: 'domcontentloaded', timeout: 90000});
    await button(page, f, 'p-trunk').waitFor();
    await page.getByRole('button', {name: '工作区选项', exact: true}).waitFor();
    await body(page, f);
    for (const id of Object.keys(f.journals)) {
      if (!f.requests.some(r => r.cmd === 'workbench.creation.status' && r.request_id === id)) {
        await page.waitForResponse(response => response.request().postDataJSON()?.cmd === 'workbench.creation.status'
          && response.request().postDataJSON()?.request_id === id);
      }
      assert(!Object.hasOwn(f.journals[id].request, 'launch'), 'no-launch payload must omit launch, not send none');
    }
    assert(!f.requests.some(r => ['workbench.terminal.open', 'workbench.feedback.send', 'workbench.creation.retry'].includes(r.cmd)), 'hierarchy creation only observes journals; no launch or prompt writes');
    assert.equal(f.expectedWrites.length, 0, 'unfulfilled expected writes');
    assert.deepEqual(report.blockedRequests.slice(blockedStart), [], 'mock safety guard rejected a request');
    assert.deepEqual(report.applicationErrors.slice(errorStart), [], 'application console/page errors');
    assert.deepEqual(report.networkErrors.slice(networkStart), [], 'application network errors');
    result.status = 'passed';
  } catch (error) {
    result.status = 'failed'; result.error = error.stack || String(error);
    await capture(page, `${name}-failure`).catch(() => {});
    await page.locator('body').innerText().then(text => writeFileSync(resolve(evidence, `${name}-failure.txt`), text)).catch(() => {});
  } finally {
    result.mockWrites = f.writes;
    await context.close();
    console.log(`${result.status.toUpperCase()} ${name}${result.error ? `: ${result.error.split('\n')[0]}` : ''}`);
    writeFileSync(resolve(evidence, 'result.json'), JSON.stringify(report, null, 2));
  }
}

try {
  await server.listen();
  origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  // No security-bypass flags, executable changes, dependency installation, or persistent profile.
  browser = await chromium.launch({channel: 'msedge', headless: true});
  report.browser = {channel: 'msedge', version: browser.version(), origin};

  await scenario('settings-partial-save-and-clear', async (page, f) => {
    const other = structuredClone(f.projects[1]);
    const identity = {id: f.projects[0].id, name: f.projects[0].name, path: f.projects[0].path, direct: f.projects[0].direct};
    await menu(page, '项目设置');
    assert.equal(await dialog(page).getByLabel('默认 Git 基准', {exact: true}).inputValue(), 'origin/release');
    assert(await dialog(page).getByRole('radio', {name: '跳过准备脚本', exact: true}).isChecked());
    assert.equal(await dialog(page).getByRole('textbox', {name: '准备脚本', exact: true}).inputValue(), seedScript);
    const script = '# MOCK ONLY: never executed\n  preserve leading and trailing script whitespace  \n';
    await dialog(page).getByLabel('默认 Git 基准', {exact: true}).fill('  refs/heads/browser-default  ');
    await dialog(page).getByRole('radio', {name: '运行准备脚本', exact: true}).check();
    await dialog(page).getByRole('textbox', {name: '准备脚本', exact: true}).fill(script);
    await capture(page, 'settings-partial-save-dark');
    arm(f, {cmd: 'project.update', project_id: 'p', default_base: 'refs/heads/browser-default', setup_default: 'run', setup: script});
    await saveDialog(page, f);
    assert.deepEqual(f.projects[1], other);
    assert.deepEqual(Object.fromEntries(Object.keys(identity).map(k => [k, f.projects[0][k]])), identity);
    await menu(page, '项目设置');
    assert.equal(await dialog(page).getByLabel('默认 Git 基准', {exact: true}).inputValue(), 'refs/heads/browser-default');
    assert(await dialog(page).getByRole('radio', {name: '运行准备脚本', exact: true}).isChecked());
    assert.equal(await dialog(page).getByRole('textbox', {name: '准备脚本', exact: true}).inputValue(), script);
    await dialog(page).getByLabel('默认 Git 基准', {exact: true}).fill('   ');
    await dialog(page).getByRole('radio', {name: '跳过准备脚本', exact: true}).check();
    await dialog(page).getByRole('textbox', {name: '准备脚本', exact: true}).fill('');
    arm(f, {cmd: 'project.update', project_id: 'p', default_base: null, setup_default: 'skip', setup: ''});
    await saveDialog(page, f);
    await menu(page, '项目设置');
    assert.equal(await dialog(page).getByLabel('默认 Git 基准', {exact: true}).inputValue(), '');
    assert.equal(await dialog(page).getByRole('textbox', {name: '准备脚本', exact: true}).inputValue(), '');
    assert(await dialog(page).getByRole('radio', {name: '跳过准备脚本', exact: true}).isChecked());
    await closeDialog(page);
    assert.equal(f.writes.length, 2);
  });

  await scenario('settings-target-frozen-across-snapshot', async (page, f) => {
    await menu(page, '项目设置');
    // Simulate a background snapshot removing the selected workspace, not synthetic UI clicks.
    f.hidden.add('p-trunk');
    await selected(page, f, 'main:q');
    await dialog(page).getByLabel('默认 Git 基准', {exact: true}).fill('frozen-atlas-base');
    arm(f, {cmd: 'project.update', project_id: 'p', default_base: 'frozen-atlas-base', setup_default: 'skip', setup: seedScript});
    await capture(page, 'settings-frozen-target');
    await saveDialog(page, f);
    assert.equal(f.projects[1].default_base, 'origin/main');
    assert.equal(f.writes[0].project_id, 'p');
  });

  await scenario('hierarchy-and-ancestor-search', async (page, f) => {
    for (const [id, depth] of [['p-trunk', 0], ['p-child', 1], ['p-grand', 2]]) {
      assert.equal(await button(page, f, id).locator('..').getAttribute('data-depth'), String(depth));
      assert.match(await button(page, f, id).getAttribute('aria-description'), new RegExp(`第 ${depth + 1} 层`));
    }
    const positions = await Promise.all(['p-trunk', 'p-child', 'p-grand'].map(id => button(page, f, id).boundingBox()));
    assert(positions[0].y < positions[1].y && positions[1].y < positions[2].y);
    assert(positions[0].x < positions[1].x && positions[1].x < positions[2].x);
    await page.getByRole('button', {name: /^Atlas \[MOCK\]/}).click();
    await button(page, f, 'p-grand').waitFor({state: 'hidden'});
    await page.getByRole('textbox', {name: '搜索工作区', exact: true}).fill('Needle');
    await button(page, f, 'p-grand').waitFor();
    assert.equal(await page.locator('.workspace-tree-row').count(), 3);
    for (const id of ['p-trunk', 'p-child']) assert.match(await button(page, f, id).locator('..').getAttribute('class'), /workspace-tree-context/);
    assert.doesNotMatch(await button(page, f, 'p-grand').locator('..').getAttribute('class'), /workspace-tree-context/);
    await capture(page, 'ancestor-search-dark');
    await layout(page, 'ancestor-search-dark');
    await select(page, f, 'p-grand');
    assert.equal(f.writes.length, 0);
  });

  await scenario('archived-ancestor-and-cross-project-placeholder', async (page, f) => {
    assert.equal(await button(page, f, 'p-old').locator('..').getAttribute('data-depth'), '0');
    assert.equal(await button(page, f, 'p-active').locator('..').getAttribute('data-depth'), '1');
    assert.match(await button(page, f, 'p-old').getAttribute('aria-description'), /已归档父级/);
    await select(page, f, 'p-active');
    await capture(page, 'archived-parent-active-child');
    assert.equal(await button(page, f, 'q-old').count(), 0, 'unrelated archived rows remain hidden');
    await page.getByRole('button', {name: /^Boreal \[MOCK\]/}).click();
    await page.getByRole('textbox', {name: '搜索工作区', exact: true}).fill('Cross project child');
    const link = page.getByRole('button', {name: '打开父工作区：Archived upstream（已归档）', exact: true});
    assert.equal(await button(page, f, 'p-cross').locator('..').getAttribute('data-depth'), '0');
    await link.click();
    await page.getByRole('heading', {name: '工作区已归档', exact: true}).waitFor();
    await selected(page, f, 'q-old');
    assert.equal(await page.getByRole('textbox', {name: '搜索工作区', exact: true}).inputValue(), '');
    assert.equal(await page.getByRole('button', {name: /^Boreal \[MOCK\]/}).getAttribute('aria-expanded'), 'true');
    assert.equal(await page.getByRole('button', {name: '工作区选项', exact: true}).count(), 0);
    assert.equal(await page.locator('.ide-status > span').getAttribute('title'), byId(f, 'q-old').path);
    assert.equal(f.requests.filter(r => r.workspace_id === 'q-old').length, 0, 'archived placeholder must not load a live workbench');
    await capture(page, 'cross-project-archived-placeholder');
    await page.getByRole('button', {name: '项目列表选项', exact: true}).click();
    await page.getByRole('menuitem', {name: '显示已归档', exact: true}).click();
    await page.getByRole('menu', {includeHidden: true}).waitFor({state: 'detached'});
    await button(page, f, 'q-old').waitFor();
    assert.equal(await button(page, f, 'q-old').getAttribute('aria-current'), 'page');
    await page.getByRole('button', {name: '项目列表选项', exact: true}).click();
    await page.getByRole('menuitem', {name: '隐藏已归档', exact: true}).click();
    await page.getByRole('menu', {includeHidden: true}).waitFor({state: 'detached'});
    await button(page, f, 'q-old').waitFor({state: 'hidden'});
    assert(await button(page, f, 'p-active').isVisible());
    assert(await button(page, f, 'p-old').isVisible());
    assert.equal(f.writes.length, 0);
  });

  await scenario('reparent-candidates-and-remove-parent', async (page, f) => {
    const original = structuredClone(byId(f, 'p-trunk'));
    await menu(page, '更改父工作区');
    const parent = dialog(page).getByRole('combobox', {name: '父工作区', exact: true});
    const candidates = await parent.locator('option').evaluateAll(options => options.map(o => o.value));
    for (const id of ['p-trunk', 'p-child', 'p-grand', 'q-desc', 'p-remote']) assert(!candidates.includes(id), `must exclude self/descendant/other host: ${id}`);
    for (const id of ['', 'main:p', 'main:q', 'q-old', 'p-old']) assert(candidates.includes(id), `valid candidate missing: ${id}`);
    await parent.selectOption('q-old');
    await capture(page, 'parent-candidates-dark');
    arm(f, {cmd: 'workbench.update', workspace_id: 'p-trunk', parent_workspace_id: 'q-old'});
    await saveDialog(page, f);
    assert.deepEqual(byId(f, 'p-trunk'), {...original, parent_workspace_id: 'q-old'});
    await button(page, f, 'p-trunk').locator('..').getByRole('button', {name: '打开父工作区：Archived upstream（已归档）', exact: true}).waitFor();
    assert.equal(await button(page, f, 'p-child').locator('..').getAttribute('data-depth'), '1');
    await menu(page, '更改父工作区');
    assert.equal(await dialog(page).getByRole('combobox', {name: '父工作区', exact: true}).inputValue(), 'q-old');
    await dialog(page).getByRole('combobox', {name: '父工作区', exact: true}).selectOption('');
    arm(f, {cmd: 'workbench.update', workspace_id: 'p-trunk', parent_workspace_id: null});
    await saveDialog(page, f);
    assert.deepEqual(byId(f, 'p-trunk'), original);
    assert.equal(await button(page, f, 'p-trunk').locator('..').locator('.workspace-parent-link').count(), 0);
    assert.equal(await button(page, f, 'p-trunk').locator('..').getAttribute('data-depth'), '0');
  });

  await scenario('parent-target-frozen-across-snapshot', async (page, f) => {
    await menu(page, '更改父工作区');
    f.hidden.add('p-trunk');
    await selected(page, f, 'main:q');
    f.hidden.delete('p-trunk');
    await button(page, f, 'p-trunk').waitFor();
    await dialog(page).getByRole('combobox', {name: '父工作区', exact: true}).selectOption('q-other');
    arm(f, {cmd: 'workbench.update', workspace_id: 'p-trunk', parent_workspace_id: 'q-other'});
    await saveDialog(page, f);
    assert.equal(byId(f, 'main:q').parent_workspace_id, null);
    assert.equal(byId(f, 'p-trunk').parent_workspace_id, 'q-other');
    await selected(page, f, 'main:q');
  });

  await scenario('keyboard-escape-and-root-parent-guard', async (page, f) => {
    await select(page, f, 'main:p');
    const trigger = page.getByRole('button', {name: '工作区选项', exact: true});
    await trigger.focus(); await page.keyboard.press('Enter');
    const rootGuard = page.getByRole('menuitem', {name: '主工作区固定顶级', exact: true});
    assert(await rootGuard.isDisabled());
    await page.keyboard.press('Escape');
    await page.getByRole('menu', {includeHidden: true}).waitFor({state: 'detached'});
    assert(await trigger.evaluate(el => el === document.activeElement), 'menu Escape restores trigger focus');
    await trigger.press('Enter');
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menuitem');
    for (let i = 0; i < 9; i++) {
      if (await page.getByRole('menuitem', {name: '项目设置', exact: true}).evaluate(el => el === document.activeElement)) break;
      const previous = await page.evaluate(() => document.activeElement.textContent);
      await page.keyboard.press('ArrowDown');
      await page.waitForFunction(previous => document.activeElement?.textContent !== previous, previous);
    }
    assert(await page.getByRole('menuitem', {name: '项目设置', exact: true}).evaluate(el => el === document.activeElement));
    await page.keyboard.press('Enter');
    await page.getByRole('menu', {includeHidden: true}).waitFor({state: 'detached'});
    await dialog(page).waitFor();
    await dialog(page).getByLabel('默认 Git 基准', {exact: true}).fill('discard-this-mock-draft');
    await tabContained(page);
    await closeDialog(page);
    assert.equal(f.writes.length, 0);
    await menu(page, '项目设置');
    assert.equal(await dialog(page).getByLabel('默认 Git 基准', {exact: true}).inputValue(), 'origin/release');
    await dialog(page).getByRole('button', {name: '取消', exact: true}).click();
    await dialog(page).waitFor({state: 'hidden'});
  });

  await scenario('project-plus-creates-top-level', async (page, f) => {
    await page.getByRole('button', {name: '在 Boreal [MOCK] 中新建工作区', exact: true}).click();
    await page.getByRole('dialog', {name: '新建工作树', exact: true}).waitFor();
    assert.equal(await dialog(page).getByLabel('开发项目', {exact: true}).inputValue(), 'q');
    assert.equal(await dialog(page).getByLabel('创建后', {exact: true}).inputValue(), 'none');
    await dialog(page).locator('summary').click();
    assert.equal(await dialog(page).getByLabel('父级工作区', {exact: true}).inputValue(), '');
    assert.equal(await dialog(page).getByLabel('自定义基准引用', {exact: true}).inputValue(), '');
    await dialog(page).getByLabel('工作区名称', {exact: true}).fill('Top-level mock creation');
    await capture(page, 'project-plus-top-level');
    armCreate(f, 'q', null, 'Top-level mock creation');
    await page.keyboard.press('Control+Enter');
    await dialog(page).waitFor({state: 'hidden'});
    const created = f.writes.at(-1).request_id;
    await selected(page, f, created);
    assert.equal(await button(page, f, created).locator('..').getAttribute('data-depth'), '0');
    assert.equal(f.writes.length, 1);
  });

  await scenario('workspace-menu-creates-child-and-resets-global-plus', async (page, f) => {
    await select(page, f, 'p-child');
    await menu(page, '新建子工作区');
    assert.equal(await dialog(page).getByLabel('开发项目', {exact: true}).inputValue(), 'p');
    assert.equal(await dialog(page).getByLabel('创建后', {exact: true}).inputValue(), 'none');
    await dialog(page).locator('summary').click();
    assert.equal(await dialog(page).getByLabel('父级工作区', {exact: true}).inputValue(), 'p-child');
    assert.equal(await dialog(page).getByLabel('自定义基准引用', {exact: true}).inputValue(), '');
    await dialog(page).getByLabel('工作区名称', {exact: true}).fill('Nested mock creation');
    await capture(page, 'child-creation-850');
    await layout(page, 'child-creation-850');
    armCreate(f, 'p', 'p-child', 'Nested mock creation');
    await dialog(page).getByRole('button', {name: '创建并打开', exact: true}).click();
    await dialog(page).waitFor({state: 'hidden'});
    const created = f.writes.at(-1).request_id;
    await selected(page, f, created);
    assert.equal(await button(page, f, created).locator('..').getAttribute('data-depth'), '2');
    await page.locator('.ide-project-heading').getByRole('button', {name: '新建工作区', exact: true}).click();
    await dialog(page).waitFor(); await dialog(page).locator('summary').click();
    assert.equal(await dialog(page).getByLabel('父级工作区', {exact: true}).inputValue(), '', 'global plus must not retain prior child context');
    await closeDialog(page);
    assert.equal(f.writes.length, 1);
  }, {width: 850});

  for (const theme of ['dark', 'light']) for (const width of [1440, 850]) {
    await scenario(`layout-${theme}-${width}`, async (page, f) => {
      assert.equal(await page.locator('html').getAttribute('data-theme'), theme);
      await capture(page, `hierarchy-${theme}-${width}`);
      const initial = await layout(page, `hierarchy-${theme}-${width}`);
      await menu(page, '项目设置');
      await capture(page, `settings-${theme}-${width}`);
      await layout(page, `settings-${theme}-${width}`);
      await tabContained(page, 5); await closeDialog(page);
      await menu(page, '更改父工作区');
      await capture(page, `parent-${theme}-${width}`);
      await layout(page, `parent-${theme}-${width}`);
      await tabContained(page, 5); await closeDialog(page);
      await page.getByRole('button', {name: theme === 'dark' ? '切换浅色主题' : '切换深色主题', exact: true}).click();
      await page.waitForFunction(theme => document.documentElement.dataset.theme !== theme, theme);
      const switched = await layout(page, `switched-${theme}-${width}`);
      assert.notEqual(switched.shellBackground, initial.shellBackground, 'theme switch must change computed shell color');
      assert.equal(f.writes.length, 0);
    }, {theme, width});
  }
} catch (error) {
  report.fatal = error.stack || String(error);
  console.error(report.fatal);
} finally {
  await browser?.close();
  await server.close();
  report.finishedAt = new Date().toISOString();
  report.passed = report.tests.filter(t => t.status === 'passed').length;
  report.failed = report.tests.filter(t => t.status === 'failed').length;
  report.ok = !report.fatal && report.failed === 0 && report.tests.length === 13
    && report.applicationErrors.length === 0 && report.networkErrors.length === 0 && report.blockedRequests.length === 0;
  writeFileSync(resolve(evidence, 'result.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ok: report.ok, passed: report.passed, failed: report.failed,
    applicationErrors: report.applicationErrors.length, hmrLoopbackWarnings: report.hmrLoopbackWarnings.length,
    blockedRequests: report.blockedRequests.length, evidence}, null, 2));
  if (!report.ok) process.exitCode = 1;
}
