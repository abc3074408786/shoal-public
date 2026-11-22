// Offline installed-loader verification. Synthetic home only; no login,
// workspace trust approval, model request, real MCP command, or user settings.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const {spawnSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');

async function child() {
  const bundle = process.env.FIXTURE_GEMINI_BUNDLE;
  const {loadSettings} = await import(pathToFileURL(path.join(bundle,'chunk-HQ3WN6AK.js')).href);
  const {Config, SkillManager, DefaultHookOutput} = await import(pathToFileURL(path.join(bundle,'chunk-JW7ZLS43.js')).href);
  const initial = loadSettings(process.cwd());
  const scope = settings => ({path:'fixture',settings,originalSettings:settings});
  const merged = new initial.constructor(
    scope({skills:{enabled:false},tools:{exclude:['activate_skill']}}),
    scope({skills:{enabled:true}}),scope({skills:{enabled:true}}),scope({skills:{enabled:true}}),true).merged;
  assert.equal(merged.skills.enabled,false,'role system policy must win over default/user/workspace');
  assert.ok(merged.tools.exclude.includes('activate_skill'));
  const config = new Config({sessionId:crypto.randomUUID(),targetDir:process.cwd(),cwd:process.cwd(),
    model:'gemini-2.5-pro',debugMode:false,skillsSupport:merged.skills.enabled,excludeTools:merged.tools.exclude,
    coreTools:[],mcpEnabled:false,extensionsEnabled:false,enableAgents:false,enableHooks:false,
    checkpointing:false,plan:false,usageStatisticsEnabled:false,telemetry:{enabled:false},
    fileFiltering:{enableFileWatcher:false},useRipgrep:false});
  try {
    const control = new SkillManager();
    await control.discoverSkills(config.storage,[],true);
    assert.ok(control.getSkills().some(skill=>skill.name==='fixture-unselected'),'positive control must discover the synthetic inherited skill');
    await config.initialize();
    assert.equal(config.isSkillsSupportEnabled(),false);
    assert.equal(config.getSkillManager().getSkills().length,0);
    await config.reloadSkills();
    assert.equal(config.getSkillManager().getSkills().length,0,'reload must preserve disabled discovery');
    const output = new DefaultHookOutput({hookSpecificOutput:{hookEventName:'SessionStart',additionalContext:'Selected snapshot body; references/rules.txt'}});
    assert.equal(output.getAdditionalContext(),'Selected snapshot body; references/rules.txt');
    fs.writeFileSync(process.env.FIXTURE_RESULT,JSON.stringify({scope:'offline native Gemini settings/core initialization; no authenticated workflow',
      systemPolicyWins:true,inheritedPositiveControl:true,initialCatalogEmpty:true,reloadCatalogEmpty:true,hookContextAccepted:true,
      sources:['chunk-HQ3WN6AK.js','chunk-JW7ZLS43.js'].map(name=>({path:path.join(bundle,name),sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(bundle,name))).digest('hex')}))}));
  } finally { await config.dispose(); }
}

async function main() {
  if(process.argv.includes('--child')) return child();
  const output=path.resolve(__dirname,'../../artifacts/orchestration-v2-20261025/native-skill-context');fs.mkdirSync(output,{recursive:true});
  const attempt=fs.mkdtempSync(path.join(output,'probe-'));
  const home=path.join(attempt,'home'),cwd=path.join(attempt,'project');
  const skill=path.join(home,'.agents/skills/unselected');fs.mkdirSync(skill,{recursive:true});fs.mkdirSync(cwd,{recursive:true});
  fs.writeFileSync(path.join(skill,'SKILL.md'),'---\nname: fixture-unselected\ndescription: Must not enter a controlled role\n---\nUnselected body');
  const env={...process.env,HOME:home,USERPROFILE:home,GEMINI_CLI_HOME:home,
    GEMINI_CLI_SYSTEM_SETTINGS_PATH:path.join(home,'system.json'),GEMINI_CLI_SYSTEM_DEFAULTS_PATH:path.join(home,'defaults.json'),
    FIXTURE_RESULT:path.join(attempt,'child-result.json'),
    FIXTURE_GEMINI_BUNDLE:process.env.FIXTURE_GEMINI_BUNDLE||'C:/home/zpq/.npm-global/node_modules/@google/.gemini-cli-eEbuV9on/bundle'};
  const run=spawnSync(process.execPath,[__filename,'--child'],{cwd,env,windowsHide:true,encoding:'utf8',timeout:30000,maxBuffer:1024*1024});
  fs.writeFileSync(path.join(attempt,'stderr.log'),run.stderr||'');
  fs.writeFileSync(path.join(attempt,'stdout.log'),run.stdout||'');
  assert.equal(run.status,0,`offline Gemini fixture failed: ${run.error||run.stderr}`);
  const result=JSON.parse(fs.readFileSync(env.FIXTURE_RESULT,'utf8'));fs.writeFileSync(path.join(attempt,'result.json'),JSON.stringify(result,null,2));
  process.stdout.write(JSON.stringify({attempt,...result},null,2)+'\n');
}
main().catch(error=>{console.error(error);process.exitCode=1;});
