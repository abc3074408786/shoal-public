const fs = require('node:fs/promises');
const path = require('node:path');
const {randomUUID} = require('node:crypto');
const ontology = require('../plugins/ontology/service.cjs');
const skillCenter = require('../plugins/skill-center/service.cjs');
const {createComputerUseMcp} = require('./computer-use-mcp.cjs');

// Only trusted bundled modules register here. Renderer data never becomes a module path.
function createExtensions({profileRoot, ontologyCli, skillCli, runtime, selectDirectory, onCommand, plugins=[ontology,skillCenter], computerUse}) {
  const computerUseMcp = computerUse || createComputerUseMcp({profileRoot});
  const stateDir = path.join(profileRoot,'extensions');
  const configFile = path.join(stateDir,'state.json');
  const records = new Map(plugins.map(plugin => [plugin.manifest.id,{plugin, installed:true, enabled:false, accepting:false, active:new Set(), instance:null}]));
  let loaded, config, controls = Promise.resolve();
  const serial = fn => { const result=controls.then(fn); controls=result.catch(()=>{}); return result; };
  async function load() {
    return loaded ??= fs.readFile(configFile,'utf8').then(JSON.parse,error=>{
      if (error.code === 'ENOENT') return {};
      throw error;
    }).then(value => {
      if (!value || typeof value !== 'object' || Array.isArray(value)) throw Error('扩展配置格式错误，未更改配置。');
      config=value;
      for (const [id,record] of records) {
        const saved=config[id];
        if (saved && (typeof saved.enabled !== 'boolean' || saved.installed!==undefined && typeof saved.installed!=='boolean' || saved.dataDir && (typeof saved.dataDir !== 'string' || !path.isAbsolute(saved.dataDir)))) throw Error('插件配置格式错误，未更改配置。');
        record.installed=saved?.installed!==false;
        record.enabled=record.accepting=record.installed&&saved?.enabled===true;
        record.dataDir=saved?.dataDir || path.join(stateDir,id,'data');
      }
    });
  }
  async function persist() {
    await fs.mkdir(stateDir,{recursive:true,mode:0o700});
    const temporary=configFile+'.'+randomUUID()+'.tmp';
    try { await fs.writeFile(temporary,JSON.stringify(config,null,2),{flag:'wx',mode:0o600});await fs.rename(temporary,configFile); }
    finally { await fs.unlink(temporary).catch(error=>{if(error.code!=='ENOENT')throw error}); }
  }
  function info(record) { return {...record.plugin.manifest,installed:record.installed,enabled:record.enabled,dataDir:record.dataDir}; }
  function instance(record) { return record.instance ??= record.plugin.create({dataDir:record.dataDir,stateDir:path.join(stateDir,record.plugin.manifest.id),cliPath:record.plugin.manifest.id==='skill-center'?skillCli:ontologyCli,runtime,onCommand}); }
  return {
    async request(args) {
      try {
        if (args?.target?.kind !== 'local') throw Object.assign(new Error('内置插件仅支持本机，请切回本机执行主机。'),{code:'local_only'});
        if (args.action === 'prepareBundledMcp') {
          if (args.pluginId !== 'computer-use') throw Error('未注册的本地 MCP 插件');
          return {ok:true,result:await computerUseMcp.prepare()};
        }
        await load();
        if (args.action === 'list') return {ok:true,result:{plugins:[...records.values()].map(info)}};
        const record=records.get(args.pluginId);
        if (!record) throw Object.assign(new Error('扩展未注册'),{code:'unknown_extension'});
        if (args.action === 'install' || args.action === 'uninstall') return await serial(async()=>{
          if(args.action==='uninstall'&&args.confirmed!==true)throw Error('卸载插件需要明确确认');
          const installed=args.action==='install';
          if(record.installed===installed)return {ok:true,result:info(record)};
          const accepting=record.accepting;record.accepting=false;
          await Promise.allSettled([...record.active]);
          const previous=config[args.pluginId];
          config[args.pluginId]={installed,enabled:false,dataDir:record.dataDir};
          try {await persist();}catch(error){config[args.pluginId]=previous;record.accepting=accepting;throw error;}
          record.installed=installed;record.enabled=false;record.instance=null;
          return {ok:true,result:info(record)};
        });
        if (args.action === 'setEnabled') return await serial(async()=>{
          if(!record.installed)throw Error('插件已卸载，请先重新安装');
          if (typeof args.enabled !== 'boolean') throw Error('启停值必须为布尔值');
          record.accepting=false;
          await Promise.allSettled([...record.active]);
          const previous=config[args.pluginId];
          config[args.pluginId]={installed:true,enabled:args.enabled,dataDir:record.dataDir};
          try { await persist(); } catch(error) {config[args.pluginId]=previous;record.accepting=record.enabled;throw error;}
          record.enabled=record.accepting=args.enabled;record.instance=null;
          return {ok:true,result:info(record)};
        });
        if (args.action === 'selectDataDirectory') return await serial(async()=>{
          if (!record.installed || !record.enabled) throw Error('请先安装并启用插件');
          const chosen=await selectDirectory?.(record.dataDir);
          if (!chosen) return {ok:true,result:info(record)};
          if (!path.isAbsolute(chosen)) throw Error('请选择绝对目录');
          record.accepting=false;await Promise.allSettled([...record.active]);
          const previous=config[args.pluginId];
          config[args.pluginId]={installed:true,enabled:true,dataDir:chosen};
          try {await persist();} catch(error) {config[args.pluginId]=previous;record.accepting=true;throw error;}
          record.dataDir=chosen;record.instance=null;record.accepting=true;
          return {ok:true,result:info(record)};
        });
        if (args.action !== 'call') throw Error('未知扩展操作');
        if (!record.installed || !record.enabled || !record.accepting) throw Object.assign(new Error('插件已卸载、停用或正在切换，请重新打开。'),{code:'extension_disabled'});
        const pending=Promise.resolve().then(()=>instance(record).call(args.request));
        record.active.add(pending);
        try { return {ok:true,result:await pending}; } finally {record.active.delete(pending);}
      } catch(error) {return {ok:false,error:{code:error.code || 'extension_error',message:error.message}};}
    },
    async dispose() {for(const record of records.values())record.accepting=false;await Promise.allSettled([...records.values()].flatMap(record=>[...record.active]));},
  };
}
module.exports={createExtensions};
