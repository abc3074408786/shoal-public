// Only read-only settings inspections use this queue. Never enqueue launches,
// user input or other mutations. An active RPC retains the slot until it settles.
const tails = new Map<string, Promise<void>>();

function cancelled(): Error {
  const error = new Error('Inspection no longer requested');
  error.name = 'AbortError';
  return error;
}

export function scheduleAgentCheck<T>(scope: string, inspect: () => Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(cancelled());
  const previous = tails.get(scope) ?? Promise.resolve();
  const result = previous.then(() => {
    if (signal.aborted) throw cancelled();
    return inspect();
  });
  // A failed probe releases the queue; its original result/error still belongs
  // to the caller. Cancelling a page never releases a running backend RPC early.
  const tail = result.then(() => {}, () => {});
  tails.set(scope, tail);
  void tail.then(() => { if (tails.get(scope) === tail) tails.delete(scope); });
  return result;
}
