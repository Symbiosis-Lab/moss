/**
 * Attention-based background refresh scheduler.
 *
 * The policy: refresh while someone is plausibly looking, and treat a failed
 * background attempt as "no news" rather than an error. Concretely — refresh
 * when the window regains focus or the tab becomes visible again (if the last
 * success is stale), on a steady cadence while visible AND focused, and when
 * the network returns (if stale). A failure doubles the wait until the next
 * attempt, capped, and an explicit (`refreshNow`) call always resets that
 * backoff — the user just asked, so the next automatic retry starts fresh
 * too.
 *
 * Every time source and event source is injected so this runs under fake
 * timers with no real listeners attached (`__tests__/attention-refresh.test.ts`).
 */

export type RefreshReason = 'background' | 'explicit';

export interface AttentionRefreshOptions {
  /**
   * Perform one refresh attempt. Resolve `true` on success, `false` on a
   * handled failure. A thrown error or a rejected promise both count as a
   * failure — the caller never needs to catch.
   */
  refresh: (reason: RefreshReason) => Promise<boolean>;

  /**
   * Called after every attempt finishes, AFTER `lastSuccessAtMs` and
   * `lastAttemptFailed` have already been updated — so a caller reacting to
   * it (typically: repaint) sees the scheduler's own bookkeeping as current
   * rather than one step stale. `refresh` itself is the wrong place for that
   * reaction for exactly this reason.
   */
  onSettled?: (ok: boolean, reason: RefreshReason) => void;

  /** Injected clock, ms since epoch. Defaults to `Date.now`. */
  now?: () => number;
  /** Injected timer functions, so tests run with no real timers pending. */
  setTimeoutFn?: (fn: () => void, ms: number) => number;
  clearTimeoutFn?: (id: number) => void;

  /** Fires on every `visibilitychange`, in either direction. Defaults to a
   *  real `document` listener. */
  addVisibilityChangeListener?: (fn: () => void) => () => void;
  /** Fires on both window `focus` and `blur`. Defaults to a real `window`
   *  listener pair. */
  addFocusChangeListener?: (fn: () => void) => () => void;
  /** Fires on window `online`. Defaults to a real `window` listener. */
  addOnlineListener?: (fn: () => void) => () => void;
  /** Defaults to reading `document.hidden`. */
  isHidden?: () => boolean;
  /** Defaults to `document.hasFocus()`. */
  isFocused?: () => boolean;

  /** How old the last success may be before attention/online regain
   *  triggers a refresh. Default 30s. */
  staleMs?: number;
  /** The steady cadence while visible+focused, and the first backoff step
   *  after a failure. Default 60s. */
  cadenceMs?: number;
  /** Backoff ceiling. Default 15 minutes. */
  maxBackoffMs?: number;
}

function defaultVisibilityChangeListener(fn: () => void): () => void {
  document.addEventListener('visibilitychange', fn);
  return () => document.removeEventListener('visibilitychange', fn);
}

function defaultFocusChangeListener(fn: () => void): () => void {
  window.addEventListener('focus', fn);
  window.addEventListener('blur', fn);
  return () => {
    window.removeEventListener('focus', fn);
    window.removeEventListener('blur', fn);
  };
}

function defaultOnlineListener(fn: () => void): () => void {
  window.addEventListener('online', fn);
  return () => window.removeEventListener('online', fn);
}

const DEFAULT_STALE_MS = 30_000;
const DEFAULT_CADENCE_MS = 60_000;
const DEFAULT_MAX_BACKOFF_MS = 15 * 60_000;

interface ResolvedOptions {
  refresh: (reason: RefreshReason) => Promise<boolean>;
  onSettled: (ok: boolean, reason: RefreshReason) => void;
  now: () => number;
  setTimeoutFn: (fn: () => void, ms: number) => number;
  clearTimeoutFn: (id: number) => void;
  addVisibilityChangeListener: (fn: () => void) => () => void;
  addFocusChangeListener: (fn: () => void) => () => void;
  addOnlineListener: (fn: () => void) => () => void;
  isHidden: () => boolean;
  isFocused: () => boolean;
  staleMs: number;
  cadenceMs: number;
  maxBackoffMs: number;
}

export class AttentionRefreshScheduler {
  private readonly opts: ResolvedOptions;
  private lastSuccessAt: number | null = null;
  private _lastAttemptFailed = false;
  private intervalMs: number;
  private timer: number | null = null;
  private unsubs: Array<() => void> = [];
  /** The currently-running attempt, if any. A second caller (typically an
   *  explicit click racing the steady cadence) awaits THIS rather than firing
   *  a second `refresh()` — see `attempt()`. */
  private inFlightPromise: Promise<boolean> | null = null;
  private started = false;

  constructor(options: AttentionRefreshOptions) {
    this.opts = {
      refresh: options.refresh,
      onSettled: options.onSettled ?? (() => {}),
      now: options.now ?? (() => Date.now()),
      setTimeoutFn: options.setTimeoutFn ?? ((fn, ms) => window.setTimeout(fn, ms)),
      clearTimeoutFn: options.clearTimeoutFn ?? ((id) => window.clearTimeout(id)),
      addVisibilityChangeListener: options.addVisibilityChangeListener ?? defaultVisibilityChangeListener,
      addFocusChangeListener: options.addFocusChangeListener ?? defaultFocusChangeListener,
      addOnlineListener: options.addOnlineListener ?? defaultOnlineListener,
      isHidden: options.isHidden ?? (() => document.hidden),
      isFocused: options.isFocused ?? (() => document.hasFocus()),
      staleMs: options.staleMs ?? DEFAULT_STALE_MS,
      cadenceMs: options.cadenceMs ?? DEFAULT_CADENCE_MS,
      maxBackoffMs: options.maxBackoffMs ?? DEFAULT_MAX_BACKOFF_MS,
    };
    this.intervalMs = this.opts.cadenceMs;
  }

  /** ms since the last successful refresh, or `null` if none has happened
   *  this session yet. */
  get lastSuccessAtMs(): number | null {
    return this.lastSuccessAt;
  }

  /** Whether the most recent attempt (background or explicit) failed. */
  get lastAttemptFailed(): boolean {
    return this._lastAttemptFailed;
  }

  /** Begin listening for attention/online events and, if currently visible
   *  and focused, schedule the steady cadence. Idempotent. */
  start(): void {
    if (this.started) return;
    this.started = true;
    this.unsubs.push(this.opts.addVisibilityChangeListener(() => this.onVisibilityOrFocusChange()));
    this.unsubs.push(this.opts.addFocusChangeListener(() => this.onVisibilityOrFocusChange()));
    this.unsubs.push(this.opts.addOnlineListener(() => this.onOnlineEvent()));
    this.scheduleIfAttentive();
  }

  /** Stop listening and clear any pending timer. */
  stop(): void {
    this.started = false;
    for (const unsub of this.unsubs.splice(0)) unsub();
    this.clearTimer();
  }

  /**
   * Run one refresh attempt outside the normal cadence — used for the
   * mount-time first sync and for point-in-time triggers. Same bookkeeping
   * as a cadence tick: updates `lastSuccessAtMs`/backoff and reschedules.
   */
  async triggerBackground(): Promise<boolean> {
    const ok = await this.attempt('background');
    this.scheduleIfAttentive();
    return ok;
  }

  /**
   * Explicit user-initiated refresh (e.g. clicking the freshness stamp).
   * Resets the backoff to the base cadence regardless of the outcome — the
   * user just asked, so automatic retries should not still be paying for an
   * earlier run of failures.
   */
  async refreshNow(): Promise<boolean> {
    this.clearTimer();
    const ok = await this.attempt('explicit');
    // Reset AFTER the attempt, overriding whatever `attempt()` computed —
    // an explicit failure must land back at the base cadence, not at double
    // whatever the backoff already was.
    this.intervalMs = this.opts.cadenceMs;
    this.scheduleIfAttentive();
    return ok;
  }

  private isAttentive(): boolean {
    return !this.opts.isHidden() && this.opts.isFocused();
  }

  private msSinceLastSuccess(): number | null {
    return this.lastSuccessAt === null ? null : this.opts.now() - this.lastSuccessAt;
  }

  private onVisibilityOrFocusChange(): void {
    if (!this.started) return;
    if (!this.isAttentive()) {
      // Nothing polls while hidden or unfocused — stop immediately rather
      // than waiting for an already-scheduled tick to fire.
      this.clearTimer();
      return;
    }
    const stale = this.msSinceLastSuccess();
    if (stale === null || stale > this.opts.staleMs) {
      void this.triggerBackground();
    } else {
      this.scheduleIfAttentive();
    }
  }

  private onOnlineEvent(): void {
    if (!this.started) return;
    const stale = this.msSinceLastSuccess();
    if (stale === null || stale > this.opts.staleMs) {
      void this.triggerBackground();
    }
  }

  private scheduleIfAttentive(): void {
    this.clearTimer();
    if (!this.started || !this.isAttentive()) return;
    this.timer = this.opts.setTimeoutFn(() => {
      void this.triggerBackground();
    }, this.intervalMs);
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      this.opts.clearTimeoutFn(this.timer);
      this.timer = null;
    }
  }

  /**
   * A caller that arrives while an attempt is already running gets THAT
   * attempt's real result, not a synthetic failure: a click on the freshness
   * stamp firing the instant before the steady cadence's own tick must not
   * report "couldn't refresh" for a refresh that, a moment later, succeeds.
   * It also means at most one `refresh()` call is ever in flight.
   */
  private attempt(reason: RefreshReason): Promise<boolean> {
    if (this.inFlightPromise) return this.inFlightPromise;
    const run = (async () => {
      let ok: boolean;
      try {
        ok = await this.opts.refresh(reason);
      } catch {
        ok = false;
      }
      this._lastAttemptFailed = !ok;
      if (ok) {
        this.lastSuccessAt = this.opts.now();
        this.intervalMs = this.opts.cadenceMs;
      } else {
        this.intervalMs = Math.min(this.intervalMs * 2, this.opts.maxBackoffMs);
      }
      this.opts.onSettled(ok, reason);
      // Cleared synchronously here, before returning/resolving — a trailing
      // `.finally()` would add a microtask hop after every attempt, for
      // every caller, forever, just to serve the rare concurrent-caller case.
      this.inFlightPromise = null;
      return ok;
    })();
    this.inFlightPromise = run;
    return run;
  }
}
