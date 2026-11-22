// Every desktop request carries the execution host it was issued against. A
// component must never read a "current host" that changed while its callback
// was queued, so callers snapshot the target and pass it explicitly.
export type ExecutionTarget =
  | { kind: 'local' }
  | { kind: 'ssh'; host_id: string; server_id: string };

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export const LOCAL_TARGET: ExecutionTarget = Object.freeze({ kind: 'local' });

export function checkedTarget(value?: ExecutionTarget | null): ExecutionTarget {
  if (value === undefined || value === null) return LOCAL_TARGET;
  if (typeof value !== 'object' || Array.isArray(value)) throw Error('execution target required');
  const keys = Object.keys(value);
  if (value.kind === 'local' && keys.length === 1) return LOCAL_TARGET;
  if (value.kind !== 'ssh' || keys.length !== 3) throw Error('invalid execution target');
  const { host_id, server_id } = value as Extract<ExecutionTarget, { kind: 'ssh' }>;
  if (!UUID.test(host_id) || !UUID.test(server_id)) throw Error('invalid execution target identity');
  return Object.freeze({ kind: 'ssh', host_id, server_id });
}

let active: ExecutionTarget = LOCAL_TARGET;
const listeners = new Set<(target: ExecutionTarget) => void>();

export function currentTarget(): ExecutionTarget {
  return active;
}
export function selectTarget(target: ExecutionTarget): ExecutionTarget {
  const next = checkedTarget(target);
  if (sameTarget(next, active)) return active;
  active = next;
  for (const listener of [...listeners]) listener(next);
  return active;
}
export function subscribeTarget(listener: (target: ExecutionTarget) => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
// localStorage and drafts must not bleed across hosts. Local keys keep their historical
// spelling so existing drafts and selections survive the arrival of remote hosts; only
// remote state is isolated behind the host and server identity it was read from.
export function keyPrefix(target: ExecutionTarget): string {
  const bound = checkedTarget(target);
  return bound.kind === 'local' ? '' : `ssh:${bound.host_id}:${bound.server_id}:`;
}
export function namespacedKey(target: ExecutionTarget, key: string): string {
  return keyPrefix(target) + key;
}
export function sameTarget(a?: ExecutionTarget | null, b?: ExecutionTarget | null): boolean {
  const left = checkedTarget(a), right = checkedTarget(b);
  if (left.kind !== right.kind) return false;
  return left.kind === 'local' || (right.kind === 'ssh' && left.host_id === right.host_id && left.server_id === right.server_id);
}
