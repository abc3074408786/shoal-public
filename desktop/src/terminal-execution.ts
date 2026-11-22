import {restrictedBlockReason,type SandboxProbe} from './sandbox-policy';
import type {Session} from './workbench-types';

export type TerminalExecutionMode='native'|'restricted';

export function terminalExecutionBlockReason(mode:TerminalExecutionMode,probe:SandboxProbe,worktree:boolean,supervised:boolean):string {
  if(mode==='native')return '';
  if(supervised)return '受限工具模式仅支持交互式 Codex 终端。请清空工作目标，或切换为本机执行后创建受管任务。';
  if(!worktree)return '受限工具模式仅支持 Git 工作树，当前工作区不可用。';
  return restrictedBlockReason(probe);
}

export function terminalExecutionRequest(mode:TerminalExecutionMode,probe:SandboxProbe,worktree:boolean,supervised:boolean):{mode:'restricted'}|undefined {
  const blocked=terminalExecutionBlockReason(mode,probe,worktree,supervised);
  if(blocked)throw new Error(blocked);
  return mode==='restricted'?{mode:'restricted'}:undefined;
}

export function terminalSessionTitle(session:Session):string {
  return session.program==='codex'&&session.kind==='terminal'&&session.execution_mode==='restricted'?'Codex · 工具受限':session.title;
}

export function terminalSessionStatus(session:Session):string {
  if(session.native_chat)return '已停用';
  if(session.status?.exit!=null)return '已结束';
  if(!session.status)return '尚未就绪';
  if(session.status.state==='host_lost')return '已失联';
  if(session.status.state==='waiting_permission')return '等待授权';
  if(session.status.state==='waiting_input')return '等待输入';
  return '运行中';
}

export const terminalSessionIsLive=(session:Session)=>!session.native_chat&&session.status?.exit===null&&session.status.state!=='host_lost';

export const restrictedTerminalDescription='使用原有 Codex 终端界面。模型连接由本机 CLI 处理；命令和文件工具使用目录白名单、独立 HOME，并关闭网络。未隔离进程命名空间，浏览器和 MCP 工具不可用，启动失败不会回退到本机工具执行。支持本机已有的 ChatGPT 登录，认证仅用于模型连接，沙箱工具无法读取；也可使用已配置的模型 API 连接。当前仅支持 Codex CLI 0.153.4。支持 !命令，并将结果写入当前会话；可按 Esc 中断。每条命令最多运行 120 秒、输出 256 KiB，每个会话最多 64 条，达到上限后请新建终端。';
