// Offline probes of installed native configuration loaders. No model requests,
// trust approvals, authentication, or writes to the user's configuration.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawnSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');
const assert = require('node:assert/strict');

async function geminiChild() {
  const {loadSettings} = await import(pathToFileURL(process.env.FIXTURE_GEMINI_SETTINGS).href);
  const initial = loadSettings(process.cwd());
  assert.deepEqual(initial.user.settings.mcp.allowed,['velella','docs']);
  const scope = settings => ({path:'fixture', settings, originalSettings:settings});
  // Exercise the installed loader's class with in-memory scopes. The true value
  // here is an input to a pure merge; no workspace trust is persisted or granted.
  const loaded = new initial.constructor(
    scope({mcpServers:{velella:{command:'selected-control'}, docs:{command:'selected-docs'}},mcp:{allowed:['velella','docs']}}),
    scope({}), scope({mcpServers:{unselected:{command:'must-never-start'}}}),
    scope({mcpServers:{project:{command:'must-never-start'}}}), true);
  assert.deepEqual(Object.keys(loaded.merged.mcpServers).sort(), ['docs','project','unselected','velella']);
  assert.deepEqual(loaded.getConsolidatedAllowedMcpServers(), ['velella','docs']);
  const nativeSource = fs.readFileSync(process.env.FIXTURE_GEMINI_CORE,'utf8');
  const match = nativeSource.match(/  isBlockedBySettings\(name3\) \{([\s\S]*?)\n  \}/);
  assert.ok(match,'installed MCP filter symbol not found; review version drift');
  const blocked = new Function('name3',match[1]).bind({cliConfig:{getAllowedMcpServers:()=>['velella','docs'],getBlockedMcpServers:()=>[]}});
  assert.equal(blocked('velella'),false); assert.equal(blocked('docs'),false);
  assert.equal(blocked('unselected'),true); assert.equal(blocked('project'),true);
  const conflict = new initial.constructor(scope({mcp:{allowed:['velella','docs']}}),scope({}),scope({mcp:{allowed:['other']}}),scope({}),true);
  assert.deepEqual(conflict.getConsolidatedAllowedMcpServers(),[]);
  const empty = new Function('name3',match[1]).bind({cliConfig:{getAllowedMcpServers:()=>[],getBlockedMcpServers:()=>[]}});
  assert.equal(empty('unselected'),false,'expected version-specific empty allowlist behavior changed');
  process.stdout.write(JSON.stringify({mergedServers:Object.keys(loaded.merged.mcpServers),selectedAllowed:true,unselectedBlocked:true,emptyIntersectionIsUnrestricted:true,jsoncPoliciesSupported:true}));
}

async function main() {
  if (process.argv.includes('--gemini-child')) return geminiChild();
  const output=path.resolve(__dirname,'../../artifacts/orchestration-v2-20261025/native-config-isolation');
  fs.mkdirSync(output,{recursive:true});
  const attempt=fs.mkdtempSync(path.join(output,'probe-'));
  const home=path.join(attempt,'home'), cwd=path.join(attempt,'project');
  fs.mkdirSync(home,{recursive:true}); fs.mkdirSync(cwd,{recursive:true});
  fs.mkdirSync(path.join(home,'.gemini'));
  fs.writeFileSync(path.join(home,'.gemini/settings.json'),'{ // native JSONC policy\n"mcp":{"allowed":["velella", /* role */ "docs"]}}');
  const env={...process.env,CODEX_HOME:home,GEMINI_CLI_HOME:home,GEMINI_CLI_SYSTEM_SETTINGS_PATH:path.join(home,'system.json'),GEMINI_CLI_SYSTEM_DEFAULTS_PATH:path.join(home,'defaults.json')};
  // Synthetic command names ensure even an accidental future connection cannot
  // run a real MCP service. `mcp list` reads configuration without connecting.
  fs.writeFileSync(path.join(home,'config.toml'),'[mcp_servers.unselected]\ncommand="velella-fixture-never-execute"\n');
  const codex=process.env.FIXTURE_CODEX_EXE || (process.env.PATH || '').split(path.delimiter).map(dir=>path.join(dir,'codex.exe')).find(file=>fs.existsSync(file));
  assert.ok(codex,'native Codex executable is unavailable');
  const child=spawnSync(codex,['-C',cwd,'-c','mcp_servers={velella={command="velella-fixture-selected"}}','mcp','list','--json'],{cwd,env,windowsHide:true,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});
  assert.equal(child.status,0,`Codex offline config probe failed: ${child.error || child.stderr}`);
  const codexServers=JSON.parse(child.stdout).map(server=>({name:server.name,enabled:server.enabled}));
  const fixed=spawnSync(codex,['-C',cwd,'-c','mcp_servers={unselected={enabled=false},velella={command="velella-fixture-selected",enabled=true}}','mcp','list','--json'],{cwd,env,windowsHide:true,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});
  assert.equal(fixed.status,0,`Codex explicit disable probe failed: ${fixed.error || fixed.stderr}`);
  const fixedServers=JSON.parse(fixed.stdout).map(server=>({name:server.name,enabled:server.enabled}));
  assert.deepEqual(fixedServers.filter(server=>server.enabled).map(server=>server.name),['velella']);
  const version=spawnSync(codex,['--version'],{cwd,env,windowsHide:true,encoding:'utf8',timeout:5000});
  assert.equal(version.status,0);
  const bundle=process.env.FIXTURE_GEMINI_BUNDLE || 'C:/home/zpq/.npm-global/node_modules/@google/.gemini-cli-eEbuV9on/bundle';
  env.FIXTURE_GEMINI_SETTINGS=path.join(bundle,'chunk-HQ3WN6AK.js');env.FIXTURE_GEMINI_CORE=path.join(bundle,'chunk-JW7ZLS43.js');
  const gemini=spawnSync(process.execPath,[__filename,'--gemini-child'],{cwd,env,windowsHide:true,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});
  assert.equal(gemini.status,0,`Gemini offline config probe failed: ${gemini.error || gemini.stderr}`);
  const result={scope:'offline installed-loader probes only; no authenticated native workflow',codex:{version:version.stdout.trim(),servers:codexServers,tableOverrideRemovesUnselected:!codexServers.some(s=>s.name==='unselected'&&s.enabled),fixedServers,explicitDisableClosesCatalog:true},gemini:JSON.parse(gemini.stdout),sources:[codex,env.FIXTURE_GEMINI_SETTINGS,env.FIXTURE_GEMINI_CORE].map(file=>({path:file,sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')}))};
  fs.writeFileSync(path.join(attempt,'result.json'),JSON.stringify(result,null,2));
  process.stdout.write(JSON.stringify({attempt,...result},null,2)+'\n');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
