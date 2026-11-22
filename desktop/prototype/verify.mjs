import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir, readdir, writeFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {join, resolve} from 'node:path';

const {chromium} = createRequire(import.meta.url)('playwright');
const bundle = resolve(process.argv[2]);
const evidence = resolve(process.argv[3]);
await mkdir(evidence, {recursive: true});
const launcher = spawn(join(bundle, 'Velella-Prototype.exe'), ['-no-browser'], {stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true});
const url = await new Promise((resolveUrl, reject) => {
  let output = '';
  launcher.once('error', reject);
  launcher.once('exit', code => reject(Error('Launcher exited: ' + code)));
  launcher.stdout.on('data', data => {
    output += data;
    const match = output.match(/http:\/\/127\.0\.0\.1:\d+\/prototype\//);
    if (match) resolveUrl(new URL(match[0]));
  });
});
const checks = [];
const faults = [];
const external = [];
let browser;

async function checked(name, test) {
  try {
    await test();
    checks.push({name, passed: true});
    console.log('PASS', name);
  } catch (error) {
    checks.push({name, passed: false, detail: String(error)});
    console.error('FAIL', name, String(error));
  }
}

try {
  browser = await chromium.launch({channel: 'msedge', headless: true});
  const context = await browser.newContext({viewport: {width: 1440, height: 960}, colorScheme: 'light'});
  context.setDefaultTimeout(7500);
  await context.route('**/*', route => {
    const request = route.request();
    if (!request.url().startsWith(url.origin + '/') || request.method() !== 'GET') {
      external.push({url: request.url(), method: request.method()});
      return route.abort();
    }
    return route.continue();
  });
  context.on('page', page => {
    page.on('pageerror', error => faults.push({path: page.url(), error: String(error)}));
    page.on('response', response => {
      if (response.status() >= 400) faults.push({path: page.url(), error: response.status() + ' ' + response.url()});
    });
  });
  const gallery = await context.newPage();
  await gallery.goto(url.href);
  const hrefs = await gallery.locator('nav a').evaluateAll(links => links.map(link => link.href));
  const entries = (await readdir(join(bundle, 'site', 'tests'), {recursive: true})).filter(path => path.endsWith('.html'));
  assert.equal(hrefs.length, new Set(hrefs).size);
  await gallery.screenshot({path: join(evidence, '01-gallery.png'), fullPage: true});
  const paths = [...new Set([...hrefs, ...entries.map(path => new URL('/tests/' + path.replaceAll('\\', '/'), url).href)])];
  for (const entry of paths) {
    await checked(new URL(entry).pathname + new URL(entry).search, async () => {
      const page = await context.newPage();
      try {
        assert.equal((await page.goto(entry)).status(), 200);
        if (entry.endsWith('/terminal-white-surface.html')) await page.locator('#report[data-passed]').waitFor();
        else await page.waitForFunction(() => document.querySelector('#root')?.childElementCount > 0);
      } finally {
        await page.close();
      }
    });
  }
  await gallery.close();
  const page = await context.newPage();
  const button = name => page.getByRole('button', {name, exact: true});
  const heading = name => page.getByRole('heading', {name, exact: true});
  const shell = name => page.locator('[data-shell-page="' + name + '"]').waitFor();
  const backHome = async () => {
    await button('返回项目首页').click();
    await shell('home');
  };
  const tab = title => page.getByRole('tab', {name: title, exact: true});
  const shot = name => page.screenshot({path: join(evidence, name + '.png')});
  await checked('首页 → 示例项目 → 模拟终端', async () => {
    await page.goto(new URL('app.html', url).href);
    await heading('项目工作台').waitFor();
    await button('打开项目 Velella 示例项目').click();
    await shell('project');
    await page.locator('.xterm-screen').waitFor();
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('这里是模拟终端'));
    await shot('02-workspace');
    await page.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('demo phrase');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('已接收模拟输入'));
  });
  await checked('文档、Monaco 编辑器、Git 与浏览器标签', async () => {
    await tab('README.md').click();
    await heading('Velella 示例项目').waitFor();
    await tab('main.ts').click();
    await page.locator('.monaco-editor').first().waitFor();
    await shot('03-editor');
    await tab('Git 变更').click();
    await page.locator('.git-diff-text').first().getByText(/export const workspaceName/).waitFor();
    await tab('浏览器').click();
    await page.getByRole('textbox', {name: '网页地址'}).waitFor();
    await page.waitForFunction(() => document.querySelector('input[aria-label="网页地址"]')?.value === 'https://example.test/workspace');
    await tab('Agent 输入').click();
    await page.getByRole('textbox', {name: '发送给 Agent'}).fill('核对页面');
    await button('发送').click();
    await page.getByText(/已提交到当前会话/).waitFor();
  });
  await checked('全部 10 个设置分类', async () => {
    await page.goto(new URL('app.html?view=project', url).href);
    await button('设置').click();
    await shell('settings');
    for (const name of ['Agent', '权限', '编排', 'AI 账号', '外观', '输入与编辑', '终端', '快捷键', '通知', '连接与工具']) {
      const category = page.getByRole('navigation', {name: '设置分类'}).getByRole('button', {name, exact: true});
      await category.click();
      assert.equal(await category.getAttribute('aria-current'), 'page');
      await page.locator('.app-settings-content h3').first().waitFor();
    }
    await shot('04-settings');
  });
  await checked('从空白首页创建项目和工作区', async () => {
    await page.goto(new URL('app.html?empty', url).href);
    await button('创建项目').click();
    const project = page.getByRole('dialog', {name: '创建项目文件夹'});
    await project.getByRole('textbox', {name: '项目名称'}).fill('浏览器评审项目');
    await project.getByRole('button', {name: '选择位置'}).click();
    await project.getByRole('button', {name: '创建项目', exact: true}).click();
    await shell('project');
    const menu = page.getByRole('navigation', {name: '应用菜单'});
    await menu.getByRole('button', {name: '文件', exact: true}).click();
    await page.getByRole('menuitem', {name: '新建工作区'}).click();
    const workspace = page.getByRole('dialog', {name: '新建工作树'});
    await workspace.getByRole('tab', {name: '名称', exact: true}).click();
    await workspace.getByRole('textbox', {name: '名称', exact: true}).fill('演示分支');
    await workspace.getByLabel('创建后').selectOption('none');
    await workspace.getByRole('button', {name: '创建并打开', exact: true}).click();
    await workspace.waitFor({state: 'hidden'});
    const created = button('打开工作区 演示分支');
    await created.waitFor();
    assert.equal(await created.getAttribute('aria-current'), 'page');
  });
  await checked('任务来源详情和自动化运行记录', async () => {
    await page.goto(new URL('app.html', url).href);
    await button('任务来源').click();
    await page.locator('.source-row').first().click();
    await heading('核对工作区导航').waitFor();
    await button('自动化').click();
    await button('运行记录').click();
    await heading('运行记录').waitFor();
    await page.locator('.automation-history .automation-row').first().waitFor();
  });
  await checked('Agent 编排新建协作界面', async () => {
    await page.goto(new URL('/tests/orchestration-v2.html', url).href);
    await backHome();
    await button('Agent 编排').click();
    await button('新建协作').click();
    await button('生成协作方案').waitFor();
    await shot('05-orchestration');
  });
  await checked('插件目录、技能中心和开发本体', async () => {
    await page.goto(new URL('app.html?view=plugins', url).href);
    await heading('插件').waitFor();
    await button('发现').click();
    await page.getByRole('button', {name: '查看 GitHub'}).waitFor();
    await page.locator('.plugin-icon img').first().evaluate(img => img.decode());
    await button('技能中心').click();
    await button('打开技能中心').click();
    await page.getByRole('dialog', {name: '技能中心'}).getByText('界面评审', {exact: true}).waitFor();
    await button('关闭插件').click();
    await page.getByRole('button', {name: /^已添加/}).click();
    await button('打开 开发本体').click();
    await page.getByRole('dialog', {name: '开发本体'}).getByText('核对工作区导航', {exact: true}).waitFor();
    await shot('06-ontology');
  });
  await checked('任务监视与断线、空态', async () => {
    for (const [path, label] of [
      ['hud.html', '整理工作区演示'],
      ['hud.html?empty', '暂无进行中的任务。'],
      ['hud.html?stale', '连接已断开，显示最后已知状态。'],
      ['app.html?disconnected', '暂时无法连接工作台'],
    ]) {
      await page.goto(new URL(path, url).href);
      await page.getByText(label, {exact: true}).waitFor();
    }
  });
  await checked('服务仅提供本机静态文件', async () => {
    const result = await fetch(new URL('/_velella', url), {method: 'POST', body: '{}'});
    assert.equal(result.status, 405);
    assert.deepEqual(external, []);
    assert.deepEqual(faults, []);
  });
} finally {
  await browser?.close();
  launcher.kill();
  await writeFile(join(evidence, 'browser-verification.json'), JSON.stringify({checks, faults, external}, null, 2));
}
if (checks.some(check => !check.passed)) process.exitCode = 1;
