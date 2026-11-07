/**
 * Subscribe to typed MossEvents, the engine contract's event union
 * (`@symbiosis-lab/moss-ui`).
 *
 * Usage:
 * ```ts
 * const unlisten = await onMossEvent("BackgroundProgress", (msg) => {
 *   // msg.kind === "BackgroundProgress"; msg.payload is BackgroundProgress
 *   panel.upsertTask({ id: `bg:${msg.payload.task}`, label: msg.payload.message });
 * });
 * // ...later
 * unlisten();
 * ```
 *
 * For unit variants (e.g., `BuildComplete`), `msg` has no `payload` field —
 * `MossEventOf<K>` correctly narrows. Don't try to read `msg.payload` on those.
 */
import { host, type Unlisten } from "../host";
import type { AppLanguage, MossEvent, PublishTarget, PublishVerdict } from "@symbiosis-lab/moss-ui";
// Re-exported for the callers that read them beside the events.
export type { AppLanguage, PublishTarget, PublishVerdict };

/** The channel the host sends engine events on; the desktop's Rust side names it the same. */
const MOSS_EVENT_CHANNEL = "moss-event";

type Kind = MossEvent["kind"];
type MossEventOf<K extends Kind> = Extract<MossEvent, { kind: K }>;

/** One event kind's payload. */
export type MossEventPayload<K extends Kind> = MossEventOf<K> extends { payload: infer P } ? P : never;

type AnyKindHandlers = { [K in Kind]?: (msg: MossEventOf<K>) => void };

/**
 * A handler per event kind; a kind without one is ignored. PublishVerdict is
 * not one of them: it reaches a listener only through subscribePublishVerdict,
 * which filters it to the listener's folder.
 */
export type MossEventHandlers = Omit<AnyKindHandlers, "PublishVerdict">;

/** One subscription that hands each event to the handler for its kind. */
export function onMossEvents(handlers: MossEventHandlers): Promise<Unlisten> {
  return subscribe(handlers);
}

async function subscribe(handlers: AnyKindHandlers): Promise<Unlisten> {
  const unlistenChannel = await host.engine.on<MossEvent>(MOSS_EVENT_CHANNEL, (event) => {
    (handlers[event.kind] as ((msg: MossEvent) => void) | undefined)?.(event);
  });
  // Optional call: a test's engine stub may resolve undefined.
  return () => unlistenChannel?.();
}

export function onMossEvent<K extends Kind>(
  kind: K,
  handler: (msg: MossEventOf<K>) => void,
): Promise<Unlisten> {
  return subscribe({ [kind]: handler });
}

/**
 * The one place that calls `onMossEvent("PublishVerdict", ...)`. Every
 * listener — the deploy tab, the email composer, the OnionPress modal's
 * adapter — routes through here instead of subscribing directly, so the
 * folder (and optional target) check lives in exactly one place rather than
 * being copied into each listener body, where a missed copy is a stray
 * verdict repainting the wrong folder's state.
 *
 * `getFolder` is read fresh on every event, not once at subscribe time: a
 * long-lived subscriber (the deploy tab) can switch folders without
 * re-subscribing, and the check must compare against whichever folder is
 * active *now*. `target` narrows to one wire target (`"onionpress"` for the
 * OnionPress modal, `"moss"` for the composer); `null` means the caller
 * dispatches on target itself (the deploy tab handles both).
 *
 * Returns a synchronous unsubscribe — registration with the underlying
 * `listen()` is still async, but a caller that unsubscribes before it
 * resolves is honored via a cancelled flag.
 */
export function subscribePublishVerdict(
  getFolder: () => string,
  target: PublishTarget | null,
  cb: (v: PublishVerdict) => void,
): Unlisten {
  let unlisten: Unlisten | null = null;
  let cancelled = false;
  void onMossEvent("PublishVerdict", (msg) => {
    const v = msg.payload;
    if (v.folder !== getFolder()) return;
    if (target !== null && v.target !== target) return;
    cb(v);
  }).then((un) => {
    if (cancelled) {
      un();
      return;
    }
    unlisten = un;
  });
  return () => {
    cancelled = true;
    unlisten?.();
    unlisten = null;
  };
}

/**
 * Subscribe to a signal the host emits with no payload. Signals
 * carry no data, so they stay off the typed `moss-event` channel — but they
 * come off the same wire, so the seam that owns it owns them too.
 */
export async function onMossSignal(name: string, handler: () => void): Promise<Unlisten> {
  const unlisten = await host.engine.on(name, () => handler());
  return () => unlisten?.();
}
