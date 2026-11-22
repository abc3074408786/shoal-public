export type ExecutionPolicy = {mode:'native'} | {mode:'isolated';network:'denied'|'host';read_only:string[];writable:string[];home?:string|null} | {mode:'restricted';network:'denied';read_only:string[];writable:string[];home?:string|null};
export type SandboxDraft = {mode:ExecutionPolicy['mode'];network:'denied'|'host';readOnly:string;writable:string};
export type RestrictedCapabilities = {platform:string;backend:'landlock-seccomp';available:boolean;reason:string|null;filesystem_allowlist:boolean;process_namespace:false;network_modes:('denied')[];experimental:true};
export type SandboxCapabilities = {platform:string;backend:string;available:boolean;reason:string|null;restricted?:RestrictedCapabilities};
export type SandboxProbe = {status:'loading'} | {status:'error';error:string} | {status:'ready';capabilities:SandboxCapabilities};
export const emptySandboxDraft = ():SandboxDraft => ({mode:'native',network:'denied',readOnly:'',writable:''});

export function readCapabilities(value:unknown):SandboxCapabilities {
  const v=value as Partial<SandboxCapabilities>|null;
  if(!v||typeof v.available!=='boolean'||typeof v.platform!=='string'||typeof v.backend!=='string'||(v.reason!=null&&typeof v.reason!=='string'))throw new Error('后台未返回有效的隔离能力，请更新或重新连接后台服务。');
  const restricted=v.restricted;
  const validRestricted=restricted&&typeof restricted.platform==='string'&&restricted.backend==='landlock-seccomp'&&typeof restricted.available==='boolean'&&(restricted.reason==null||typeof restricted.reason==='string')&&typeof restricted.filesystem_allowlist==='boolean'&&restricted.process_namespace===false&&Array.isArray(restricted.network_modes)&&restricted.network_modes.length<=1&&restricted.network_modes.every(mode=>mode==='denied')&&restricted.experimental===true;
  return {platform:v.platform,backend:v.backend,available:v.available,reason:v.reason??null,...(validRestricted?{restricted:{...restricted,reason:restricted.reason??null}}:{})};
}

export function isolationBlockReason(probe:SandboxProbe,direct=false):string {
  if(direct)return '隔离执行需要 Git 工作目录，当前项目是普通文件夹。';
  if(probe.status==='loading')return '正在检查当前环境的隔离能力…';
  if(probe.status==='error')return '隔离能力检查失败，请重试；也可以自行选择本机执行。';
  if(!probe.capabilities.available)return probe.capabilities.platform==='linux'?'当前 Linux 环境无法启动隔离执行。':'当前平台尚未实现隔离执行。';
  return '';
}

export function restrictedBlockReason(probe:SandboxProbe,direct=false):string {
  if(direct)return '受限执行需要 Git 工作目录，当前项目是普通文件夹。';
  if(probe.status==='loading')return '正在检查当前环境的文件与网络限制能力…';
  if(probe.status==='error')return '受限执行能力检查失败，请重新检查。';
  const capability=probe.capabilities.restricted;
  if(!capability)return '后台未返回有效的受限执行能力，请更新或重新连接后台服务。';
  if(capability.platform!=='linux')return '当前平台尚未实现受限执行。';
  if(!capability.available||!capability.filesystem_allowlist||!capability.network_modes.includes('denied'))return '当前 Linux 环境无法启动受限执行。';
  return '';
}

export function executionBlockReason(mode:SandboxDraft['mode'],probe:SandboxProbe,direct=false):string {
  return mode==='native'?'':mode==='restricted'?restrictedBlockReason(probe,direct):isolationBlockReason(probe,direct);
}

function directories(text:string):string[] {
  const values=text.split('\n').map(v=>v.trim()).filter(Boolean);
  if(values.some(v=>!v.startsWith('/')||v.includes('\0')))throw new Error('额外目录必须是绝对路径，每行填写一个目录。');
  return [...new Set(values)];
}

export function buildExecution(draft:SandboxDraft,probe:SandboxProbe,direct=false):ExecutionPolicy {
  if(draft.mode==='native')return {mode:'native'};
  const blocked=executionBlockReason(draft.mode,probe,direct);
  if(blocked)throw new Error(blocked);
  if(draft.mode==='restricted'&&draft.network!=='denied')throw new Error('受限执行仅支持关闭网络，不能连接远程模型或联网登录。');
  const read_only=directories(draft.readOnly),writable=directories(draft.writable);
  if(read_only.some(path=>writable.includes(path)))throw new Error('同一目录不能同时设置为只读和可写。');
  return draft.mode==='restricted'?{mode:'restricted',network:'denied',read_only,writable}:{mode:'isolated',network:draft.network,read_only,writable};
}

export function executionDescription(policy?:ExecutionPolicy,direct=false):string {
  if(policy?.mode==='restricted')return '受限执行（实验性）· 文件目录白名单、网络关闭、独立 HOME。未隔离进程命名空间。准备脚本、Agent、验证和任务 Shell 共用限制策略；无法连接远程模型或联网登录，浏览器工具暂不可用。启动失败不会转为本机执行。';
  if(policy?.mode==='isolated')return `隔离执行 · ${policy.network==='host'?'允许网络，包含本机服务':'网络关闭'}。准备脚本、Agent、验证和任务 Shell 共用隔离策略。使用独立登录资料；浏览器工具暂不可用。`;
  return direct?'本机执行，直接修改原文件夹，没有自动回滚。':'本机执行，worktree 隔离代码改动；系统权限由 Agent 原生机制处理。准备脚本与验证命令在本机运行。';
}
