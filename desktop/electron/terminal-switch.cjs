const { randomUUID } = require('node:crypto');

// One in-flight navigation per window. A renderer acknowledgement, not send(),
// establishes success. No terminal input or process lifecycle operations here.
function createTerminalSwitcher({ send, activate, now = Date.now }) {
  let pending;
  const reject = (code, error) => ({ ok: false, code, error });
  function request(value) {
    if (pending) return Promise.resolve(reject('desktop_busy', 'Another terminal switch is pending'));
    for (const key of ['workspace_id', 'session_id']) {
      if (typeof value[key] !== 'string' || !value[key].length || value[key].length > 256 || /[\x00-\x1f]/.test(value[key]))
        return Promise.resolve(reject('invalid_target', 'Exact workspace and terminal IDs are required'));
    }
    const expires_at_ms = Math.min(Number(value.expires_at_ms), now() + 8000);
    if (!Number.isFinite(expires_at_ms) || expires_at_ms <= now())
      return Promise.resolve(reject('desktop_switch_timeout', 'Terminal switch deadline has expired'));
    return new Promise(resolve => {
      const request = { request_id: randomUUID(), workspace_id: value.workspace_id, session_id: value.session_id, expires_at_ms };
      const finish = result => { clearTimeout(pending.timer); pending = undefined; resolve(result); };
      pending = { request, finish, timer: setTimeout(() => finish(reject('desktop_switch_timeout', 'Renderer did not confirm terminal focus before the deadline; selection may have changed')), expires_at_ms - now()) };
      try { activate(); send(request); }
      catch (error) { finish(reject('desktop_unavailable', error.message)); }
    });
  }
  function complete(result) {
    if (!pending || result?.request_id !== pending.request.request_id) return false;
    const request = pending.request;
    if (result.workspace_id !== request.workspace_id || result.session_id !== request.session_id) return false;
    if (now() >= request.expires_at_ms) return false;
    pending.finish(result.ok === true && result.focused === true
      ? { ok: true, protocol: 'velella-terminal-switch-v1', workspace_id: request.workspace_id, session_id: request.session_id, focused: true }
      : reject('desktop_switch_rejected', typeof result.error === 'string' ? result.error.slice(0, 500) : 'Renderer did not confirm terminal focus'));
    return true;
  }
  function dispose() { if (pending) pending.finish(reject('desktop_unavailable', 'Desktop window closed')); }
  return { request, complete, dispose };
}
module.exports = { createTerminalSwitcher };
