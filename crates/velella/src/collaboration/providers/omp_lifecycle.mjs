import {spawnSync} from 'node:child_process';

export default function velella(pi) {
  const selected = new Set(JSON.parse(process.env.VELELLA_OMP_TOOLS));
  const allowed = tool => tool.sourceInfo?.source === 'mcp' ||
    (tool.sourceInfo?.source === 'builtin' && selected.has(tool.name));
  const clamp = async () => pi.setActiveTools(pi.getAllTools().filter(allowed).map(tool => tool.name));
  const emit = (name, ctx) => {
    const id = ctx.sessionManager.getSessionId();
    if (!id) throw new Error('Velella lifecycle session ID is missing');
    const child = spawnSync(process.env.VELELLA_RUNTIME_EXE, ['orchestration-hook', name], {
      input: JSON.stringify({session_id: id, context_usage: ctx.getContextUsage?.() ?? null}), env: process.env, windowsHide: true,
      encoding: 'utf8', timeout: name === 'Interrupt' ? 1500 : 10000, maxBuffer: 1024 * 1024,
    });
    if (child.status !== 0) throw new Error('Velella lifecycle receipt failed');
  };
  pi.on('session_start', async (_event, ctx) => { await clamp(); emit('SessionStart', ctx); });
  // OMP catches before_agent_start errors. Input's handled result is its native
  // cancellation contract; do not rely on an exception to block a prompt.
  pi.on('input', async (_event, ctx) => {
    try { await clamp(); emit('UserPromptSubmit', ctx); }
    catch { ctx.ui.notify('Velella 无法验证角色上下文或任务连接，已停止本次输入。', 'error'); return {handled: true}; }
  });
  pi.on('before_agent_start', async () => { await clamp(); });
  pi.on('tool_call', async event => {
    const tools = pi.getAllTools();
    // OMP projects MCP tools through its write xd:// device. A read-only role
    // still needs that exact route; it must not thereby gain file writes.
    if (event.toolName === 'write' && typeof event.input?.path === 'string') {
      const route = tools.find(tool => tool.sourceInfo?.source === 'mcp' && event.input.path === `xd://${tool.name}`);
      if (route) return;
    }
    const tool = tools.find(tool => tool.name === event.toolName);
    if (!tool || !allowed(tool)) return {block: true, reason: 'Tool is outside this Velella role selection'};
  });
  pi.on('tool_approval_requested', async (_event, ctx) => { emit('PermissionRequest', ctx); });
  pi.on('agent_end', async (_event, ctx) => { emit('Stop', ctx); });
  pi.on('turn_end', async (_event, ctx) => { emit('ContextUsage', ctx); });
  pi.on('session_shutdown', async (_event, ctx) => { emit('Interrupt', ctx); });
}
