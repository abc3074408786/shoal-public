// Offline proof that the installed OpenCode resolves a role-local catalog.
// It makes no model request, trust decision, login, or user-config write.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velella-opencode-native-'));
const cwd = path.join(root, 'workspace');
const configHome = path.join(root, 'config');
const dataHome = path.join(root, 'data');
const cacheHome = path.join(root, 'cache');
for (const dir of [cwd, configHome, dataHome, cacheHome]) fs.mkdirSync(dir, { recursive: true });
fs.mkdirSync(path.join(cwd, '.agents', 'skills', 'unselected'), { recursive: true });
fs.writeFileSync(path.join(cwd, '.agents', 'skills', 'unselected', 'SKILL.md'), '---\nname: unselected\ndescription: must not load\n---\n');
const configPath = path.join(root, 'opencode.json');
fs.writeFileSync(configPath, JSON.stringify({
  mcp: { velella: { type: 'local', command: ['velella-fixture-never-execute'], enabled: true } },
  agent: { velella: { mode: 'primary', prompt: 'fixture', permission: { skill: { '*': 'deny' } } } },
  default_agent: 'velella',
}));
const env = { ...process.env, OPENCODE_CONFIG: configPath, OPENCODE_CONFIG_CONTENT: '{}', XDG_CONFIG_HOME: configHome, XDG_DATA_HOME: dataHome, XDG_CACHE_HOME: cacheHome,
  OPENCODE_DISABLE_PROJECT_CONFIG: '1', OPENCODE_DISABLE_EXTERNAL_SKILLS: '1', OPENCODE_PURE: '1', OPENCODE_DISABLE_DEFAULT_PLUGINS: '1', OPENCODE_DISABLE_AUTOUPDATE: '1' };
function installedOpenCode() {
  if (process.env.FIXTURE_OPENCODE_EXE) return process.env.FIXTURE_OPENCODE_EXE;
  const lookup = spawnSync('where.exe', ['opencode'], { encoding: 'utf8', windowsHide: true });
  for (const shim of String(lookup.stdout || '').split(/\r?\n/).filter(Boolean)) {
    const candidate = path.join(path.dirname(shim), 'node_modules', 'opencode-ai', 'bin', 'opencode.exe');
    if (fs.existsSync(candidate)) return candidate;
  }
}
const binary = installedOpenCode();
assert.ok(binary, 'installed OpenCode executable is unavailable');
function probe(...args) {
  const child = spawnSync(binary, args, { cwd, env, encoding: 'utf8', windowsHide: true, timeout: 30000, maxBuffer: 1024 * 1024 });
  assert.equal(child.status, 0, `OpenCode offline probe failed: ${child.error || child.stderr}`);
  return child.stdout;
}
const config = probe('debug', 'config');
const skills = probe('debug', 'skill');
const agent = probe('debug', 'agent', 'velella');
assert.ok(config.trim().length > 0, 'OpenCode debug config produced no native output');
assert.doesNotMatch(skills, /unselected/);
assert.ok(agent.trim().length > 0, 'OpenCode debug agent produced no native output');
process.stdout.write(JSON.stringify({ scope: 'offline installed OpenCode parser probe only; it proves the isolated CLI can start and does not list the injected external skill. Role-generated MCP, skill-permission and lifecycle execution require the runtime integration fixture', root, sources: [{ path: configPath, sha256: crypto.createHash('sha256').update(fs.readFileSync(configPath)).digest('hex') }], unselectedExternalSkillHidden: true }, null, 2) + '\n');
