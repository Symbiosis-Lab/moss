/**
 * The engine over HTTP, from a page on the UI origin that `moss edit --serve`
 * serves. The browser sends the session cookie with every same-origin request,
 * so no request carries a credential of its own.
 */
import { engineTier, type EngineTier } from '@symbiosis-lab/moss-ui';

const ROUTE: Record<EngineTier, string> = {
  public_read: '/__moss/invoke/',
  session_read: '/__moss/read/',
  write: '/__moss/mutate/',
};

/**
 * A command the carrier cannot reach. Thrown as a string, not an Error: the
 * engine client turns a string into `{ status: "error" }`, the branch call
 * sites handle, while an Error is a transport fault and propagates.
 */
function unavailable(cmd: string, why: string): never {
  const msg = `moss command '${cmd}' is not available over the carrier (${why})`;
  console.error(msg);
  throw msg;
}

/** The browser holds no live session: it signed out, or its session expired or was revoked. */
export class SignedOut extends Error {
  constructor(cmd: string) {
    super(`moss: signed out; cannot call ${cmd}. Open a new sign-in link.`);
  }
}

/** Sends `cmd` on the route for its tier; rejects as the engine's own call would. */
export async function call(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const tier = engineTier(cmd);
  if (!tier) unavailable(cmd, 'the engine does not serve it');
  const res = await fetch(ROUTE[tier] + cmd, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify(args ?? {}),
  });
  if (res.ok) {
    // A command that returns nothing answers an empty body.
    const text = await res.text();
    return text ? JSON.parse(text) : undefined;
  }
  if (res.status === 401) throw new SignedOut(cmd);
  let detail = '';
  try {
    const body = await res.json();
    detail = typeof body?.error === 'string' ? body.error : JSON.stringify(body);
  } catch {
    detail = await res.text().catch(() => '');
  }
  // The command's own error is a 500 with its string, thrown as given.
  if (res.status === 500 && detail) throw detail;
  // The bundled contract names a command this server does not: they are different versions.
  if (res.status === 404) unavailable(cmd, '404');
  throw new Error(`moss command '${cmd}' failed (${res.status}): ${detail}`);
}

type Handler = (payload: unknown) => void;

/**
 * Handlers by event name. One stream carries every name, as `{name, payload}`
 * frames; with no handler left the stream is closed.
 */
const handlers = new Map<string, Set<Handler>>();
let connection: AbortController | null = null;

/** Subscribes to the engine's event `name`; resolves once subscribed. */
export function on<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  const h = handler as Handler;
  const set = handlers.get(name) ?? new Set();
  handlers.set(name, set);
  set.add(h);
  if (!connection) {
    connection = new AbortController();
    void hold(connection);
  }
  let released = false;
  return Promise.resolve(() => {
    if (released) return;
    released = true;
    set.delete(h);
    if (set.size === 0) handlers.delete(name);
    if (handlers.size === 0) {
      connection?.abort();
      connection = null;
    }
  });
}

function dispatch(name: string, payload: unknown): void {
  // Copied first: a handler may unsubscribe itself.
  for (const h of [...(handlers.get(name) ?? [])]) {
    try {
      h(payload);
    } catch (e) {
      console.error(`[carrier] handler for '${name}' threw`, e);
    }
  }
}

/**
 * Holds the stream open, reconnecting with a backoff until the last handler
 * is gone: the server ends the stream on a restart or when the session ends.
 * Read with `fetch`, not `EventSource`, for a real abort on teardown.
 */
async function hold(controller: AbortController): Promise<void> {
  let backoffMs = 250;
  while (!controller.signal.aborted) {
    try {
      const res = await fetch('/__moss/events', {
        headers: { Accept: 'text/event-stream' },
        credentials: 'same-origin',
        signal: controller.signal,
      });
      if (!res.ok || !res.body) throw new Error(`moss event stream refused (${res.status})`);
      backoffMs = 250;
      await readFrames(res.body.getReader());
    } catch (e) {
      if (controller.signal.aborted) return;
      console.warn('[carrier] event stream dropped; retrying', e);
    }
    if (controller.signal.aborted) return;
    await new Promise((r) => setTimeout(r, backoffMs));
    backoffMs = Math.min(backoffMs * 2, 5_000);
  }
}

/**
 * SSE framing: records end at a blank line, and a record's `data:` lines join
 * with newlines. `event:`, `id:`, `retry:` and `:` keep-alives are ignored.
 */
async function readFrames(reader: ReadableStreamDefaultReader<Uint8Array>): Promise<void> {
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return;
    buffer += decoder.decode(value, { stream: true });
    let split: number;
    while ((split = buffer.indexOf('\n\n')) !== -1) {
      const record = buffer.slice(0, split);
      buffer = buffer.slice(split + 2);
      const data = record
        .split('\n')
        .filter((line) => line.startsWith('data:'))
        .map((line) => line.slice(5).replace(/^ /, ''))
        .join('\n');
      if (!data) continue;
      try {
        const frame = JSON.parse(data) as { name?: string; payload?: unknown };
        if (typeof frame.name === 'string') dispatch(frame.name, frame.payload);
      } catch (e) {
        console.error('[carrier] unparseable event frame', e);
      }
    }
  }
}
