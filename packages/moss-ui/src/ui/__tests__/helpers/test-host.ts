/**
 * The host record the UI's tests run against: what every host has, stubbed
 * (`../../host.ts`). The setup fills it before each test file; a test replaces
 * the members it drives.
 */
import { vi } from 'vitest';
import { fillHost, host, inPageBus, type Bus, type UiHost } from '../../host';

type Handler = (payload: never) => void;
const engineHandlers = new Map<string, Set<Handler>>();

/** Sends an engine event to the UI's subscribers, as a host delivers it. */
export function emitEngineEvent(name: string, payload?: unknown): void {
  for (const handler of [...(engineHandlers.get(name) ?? [])]) handler(payload as never);
}

/** Who is subscribed to an engine event. */
export function engineSubscribers(name: string): number {
  return engineHandlers.get(name)?.size ?? 0;
}

/** Empties the record and fills it with stubs; a member a test set before is gone. */
export function resetTestHost(): void {
  engineHandlers.clear();
  for (const key of Object.keys(host)) delete (host as unknown as Record<string, unknown>)[key];
  const record: UiHost = {
    engine: {
      // A test that reaches the engine says what it answers (a `vi.mock` of the engine module, or `host.engine.call`).
      call: async (cmd) => { throw new Error(`no engine stub for ${cmd}`); },
      on: async (name, handler) => {
        const set = engineHandlers.get(name) ?? new Set();
        engineHandlers.set(name, set);
        set.add(handler as Handler);
        return () => { set.delete(handler as Handler); };
      },
    },
    bus: inPageBus(),
    sourceAssetBase: 'moss-source://localhost',
    openExternal: vi.fn(async () => {}),
    pickFiles: vi.fn(async () => []),
  };
  fillHost(record);
}

/** An in-page bus that reports each message sent and each subscription made, for a test that asserts on them. */
export function recordingBus(
  onEmit: (name: string, payload?: unknown) => void,
  onSubscribe: (name: string, handler: (payload: never) => void) => void = () => {},
): Bus {
  const inner = inPageBus();
  return {
    emit: (...message: [string, unknown?]) => { onEmit(...message); inner.emit(...message); },
    on: (name, handler) => { onSubscribe(name, handler as (payload: never) => void); return inner.on(name, handler); },
  };
}
