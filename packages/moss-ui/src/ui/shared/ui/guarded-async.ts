/**
 * runGuarded — the one rule for an async UI action.
 *
 * THE INVARIANT: a surface that goes busy for the duration of an `await` MUST
 * come back, on every path — resolve, reject, or a `Result` carrying an error.
 * Whatever the caller disabled, hid, or replaced with a spinner is restored in
 * a `finally`, and a failure is turned into a message the caller can show.
 *
 * Why this exists as a shared primitive rather than a per-handler try/catch:
 * the same dead-end shipped three separate times in the OnionPress surfaces —
 * a badge stuck on "Stopping…" with every control dead, a Claim button left
 * disabled under a permanent "Checking…", a publish spinner that never
 * resolved. Each was one `await` whose rejection path skipped the line that
 * gave the UI back. Restating the rule per handler is how it kept being
 * forgotten, so it is stated once, here.
 *
 * The rejection path is NOT hypothetical: the generated bindings wrapper
 * resolves `{ status: 'error' }` when the backend error is a plain string but
 * RETHROWS when it is an `Error`. Every `commands.*` call therefore has two
 * failure shapes, and code that handles only the first is half-guarded.
 *
 * Pure and DOM-free — the caller supplies every effect as a closure.
 */

/** Human-readable text for anything a rejected promise can carry. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  if (err === null || err === undefined) return String(err);
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

export interface GuardedAction<T> {
  /** The async work. Usually one `commands.*` / injected-dep call. */
  action(): Promise<T>;
  /** Ran with the resolved value, BEFORE `restore`, so state the success path
   *  writes is already in place when the surface redraws. Skipped when stale. */
  onOk?(value: T): void;
  /** Ran when `action` REJECTS, with `errorMessage(err)`. A `Result`-shaped
   *  error is a resolved value — branch on it inside `onOk`. */
  onError(message: string, raw: unknown): void;
  /** Give the surface back: re-enable the control, clear the busy phase,
   *  redraw. Runs in a `finally` — success, failure and rejection alike. */
  restore?(): void;
  /** True when the surface is gone (destroyed / closed / superseded). Then
   *  nothing runs: there is nothing left to restore or tell. */
  stale?(): boolean;
}

/**
 * Run `spec.action` under the invariant above.
 *
 * Returns the resolved value, or `undefined` when the action rejected or the
 * surface went stale — so `if (!value) return;` is the caller's early-out.
 *
 * `onOk` runs inside the guarded region on purpose: a throw from the success
 * handler is just as capable of stranding a disabled control as a rejected
 * invoke, and it must not be the one path that escapes the rule.
 */
export async function runGuarded<T>(spec: GuardedAction<T>): Promise<T | undefined> {
  const stale = (): boolean => spec.stale?.() === true;
  try {
    const value = await spec.action();
    if (stale()) return undefined;
    spec.onOk?.(value);
    return value;
  } catch (err) {
    if (!stale()) spec.onError(errorMessage(err), err);
    return undefined;
  } finally {
    if (!stale()) spec.restore?.();
  }
}
