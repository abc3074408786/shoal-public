const fs = require('node:fs/promises');
const path = require('node:path');
const {createHash, randomUUID} = require('node:crypto');
const {execFile} = require('node:child_process');
const manifest = require('./manifest.json');

function invalid(message) { return Object.assign(new Error(message), {code:'invalid_request'}); }
function identifier(value, name) {
  if (typeof value !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(value)) throw invalid(`无效的${name}`);
  return value;
}
function object(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw invalid('需要JSON对象');
  return value;
}

function create({dataDir, stateDir, cliPath, onCommand}) {
  const temporaryRoot = path.join(stateDir, 'requests');
  let verified;
  async function verifyEngine() {
    return verified ??= fs.readFile(cliPath).then(bytes => {
      if (createHash('sha256').update(bytes).digest('hex') !== manifest.engine.sha256)
        throw Object.assign(new Error('本体引擎指纹不符，请使用本插件随附的引擎。'), {code:'engine_mismatch'});
    }).catch(error => { throw Object.assign(new Error(error.code === 'ENOENT' ? '本体引擎尚未打包，请先准备插件引擎。' : error.message), {code:error.code === 'ENOENT' ? 'engine_missing' : error.code}); });
  }
  async function run(args, payload) {
    await verifyEngine();
    let file;
    if (payload !== undefined) {
      await fs.mkdir(temporaryRoot, {recursive:true, mode:0o700});
      file = path.join(temporaryRoot, randomUUID()+'.json');
      await fs.writeFile(file, JSON.stringify(payload), {flag:'wx', mode:0o600});
      args = [...args, '--file', file];
    }
    try {
      return await new Promise((resolve, reject) => {
        execFile(cliPath, ['--data-dir',dataDir,'ontology',...args,'--json'],
          {encoding:'utf8', windowsHide:true, shell:false, timeout:15000, maxBuffer:8*1024*1024},
          (error, stdout, stderr) => {
            onCommand?.({executable:cliPath,args:['--data-dir',dataDir,'ontology',...args,'--json'],payload,exitCode:error?(typeof error.code==='number'?error.code:null):0,signal:error?.signal,stdout,stderr});
            let response;
            try { response = JSON.parse(stdout); } catch {
              reject(Object.assign(new Error(error?.killed ? (args[0]==='check'?'语义检查超时；没有修改本体数据，可重新检查。':'本体请求超时，写入结果未知；请刷新核对，不要重复提交。') : (stderr.trim() || error?.message || '本体返回无效响应')), {code:'engine_unavailable'})); return;
            }
            if (error || !response.ok) {
              reject(Object.assign(new Error(response.error?.message || '本体操作失败'), {code:response.error?.code || 'engine_error'})); return;
            }
            resolve(response.result);
          });
      });
    } finally { if (file) await fs.unlink(file); }
  }
  async function belongs(id, project) {
    const row = await run(['entity','get',identifier(id,'实体ID')]);
    // entity get includes history but retains the stored row's project field.
    if (row.project !== project) throw Object.assign(new Error('实体不属于当前项目，请重新选择。'), {code:'cross_project_link'});
    return row;
  }
  return {
    async call(request) {
      object(request);
      const {operation} = request;
      if (operation === 'describe') {
        let engineError;
        try { await verifyEngine(); } catch (error) { engineError = error.message; }
        const initialized = await fs.access(path.join(dataDir,'ontology.db')).then(()=>true,()=>false);
        return {manifest, dataDir, initialized, engineReady:!engineError, engineError};
      }
      if (operation === 'schema') return run(['schema']);
      if (operation === 'initialize') return run(['init']);
      if (operation === 'projects') return run(['project','list']);
      if (operation === 'createProject') {
        const id = identifier(request.id,'项目ID');
        if (typeof request.name !== 'string' || !request.name.trim()) throw invalid('项目名称不能为空');
        return run(['project','create',id,'--name',request.name,'--description',String(request.description || '')]);
      }
      const project = identifier(request.project,'项目ID');
      if (operation === 'check') return run(['check','--project',project]);
      if (operation === 'queue') return run(['next','project',project,...(request.role ? ['--role',identifier(request.role,'角色')] : [])]);
      if (operation === 'entities') return run(['entity','list','--project',project]);
      if (operation === 'export') return run(['export','--project',project]);
      if (['entity','context','neighbors'].includes(operation)) {
        await belongs(request.id, project);
        if (operation === 'context') return run(['context','task',request.id]);
        if (operation === 'neighbors') return run(['relation','neighbors',request.id,'--direction','both']);
        return run(['entity','get',request.id]);
      }
      if (operation === 'saveEntity') {
        const input = object(request.input);
        const allowed = ['type','id','fields','state','source','evidence','expected_revision','operation_id'];
        if (Object.keys(input).some(key => !allowed.includes(key))) throw invalid('不支持的实体字段');
        if (request.id) {
          await belongs(request.id, project);
          if (!Number.isInteger(input.expected_revision) || input.expected_revision < 1) throw invalid('更新需要当前整数版本');
          if ('id' in input || 'type' in input) throw invalid('不能修改实体身份');
          return run(['entity','update',request.id], input);
        }
        return run(['entity','create'], {...input,project});
      }
      if (operation === 'link') {
        const input = object(request.input);
        if (Object.keys(input).some(key=>!['kind','from','to','source','evidence','operation_id'].includes(key))) throw invalid('不支持的关系字段');
        await belongs(input.from, project); await belongs(input.to, project);
        return run(['relation','create'], input);
      }
      throw invalid('未知本体操作');
    },
  };
}
module.exports = {manifest, create};
