/**
 * Latest-wins sender — coalesces a high-frequency value stream onto an async
 * channel with AT MOST ONE send in flight.
 *
 * Built for the divider's `set_divider_position` IPC: each Tauri invoke is an
 * independent async task, so a fire-and-forget 60/s burst can have dozens of
 * sends in flight that interleave (a stale position landing AFTER a newer
 * one). This sender serializes the channel: while a send is in flight, new
 * values overwrite a single pending slot; when the send settles, only the
 * latest pending value is dispatched (intermediates are dropped — they were
 * already superseded).
 *
 * Contract (unit-tested in __tests__/latest-wins-sender.test.ts):
 *  - Idle → request() calls send() SYNCHRONOUSLY in the same tick (callers
 *    pin a synchronous first invoke).
 *  - In flight → request() stores the value in the single pending slot.
 *  - On settle (resolve OR reject — a failed send must not wedge the sender):
 *    dispatch the pending value unless it equals the last sent value.
 *  - request() promises resolve at the next drain point (sender idle at-or-
 *    after the request); they never reject.
 *  - cancelPending() drops only the stored pending value (used by teardown
 *    paths where a stale geometry write must not trail in after a close).
 */

export interface LatestWinsSender<T> {
  /** Send value. If idle, calls send() SYNCHRONOUSLY in this tick; if a send is
   *  in flight, stores value in the single pending slot (replacing any previous
   *  pending). Returns a promise that resolves once this value — or a newer one
   *  that superseded it — has been delivered (send settled). Never rejects. */
  request(value: T): Promise<void>;
  /** Drop the stored pending value (the in-flight send is unaffected). Used when
   *  a teardown supersedes geometry (panel closing) so a stale position cannot
   *  trail in after the close. Outstanding request() promises still resolve. */
  cancelPending(): void;
}

export interface LatestWinsSenderOptions<T> {
  /** Value-equality for the skip-equal-pending check (contract #4). Defaults
   *  to Object.is — fine for primitive payloads, but STRUCTURED payloads
   *  (e.g. the divider's {position, commit}) must supply a comparator or two
   *  equal-valued literals will compare unequal by identity and re-send. */
  isEqual?: (a: T, b: T) => boolean;
}

export function createLatestWinsSender<T>(
  send: (value: T) => Promise<void>,
  options: LatestWinsSenderOptions<T> = {},
): LatestWinsSender<T> {
  const isEqual = options.isEqual ?? Object.is;
  let inFlight = false;
  let pending: { value: T } | null = null;
  let lastSent: { value: T } | null = null;
  let idleWaiters: (() => void)[] = [];

  function settle(): void {
    if (pending !== null && !(lastSent !== null && isEqual(pending.value, lastSent.value))) {
      const next = pending.value;
      pending = null;
      dispatch(next);
      return;
    }
    pending = null;
    inFlight = false;
    const waiters = idleWaiters;
    idleWaiters = [];
    for (const resolve of waiters) resolve();
  }

  function dispatch(value: T): void {
    inFlight = true;
    lastSent = { value };
    send(value).then(settle, settle);
  }

  return {
    request(value: T): Promise<void> {
      const drained = new Promise<void>((resolve) => {
        idleWaiters.push(resolve);
      });
      if (inFlight) {
        pending = { value };
      } else {
        dispatch(value);
      }
      return drained;
    },
    cancelPending(): void {
      pending = null;
    },
  };
}
