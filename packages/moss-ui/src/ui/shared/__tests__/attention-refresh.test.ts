/**
 * Tests for the attention-based background refresh scheduler.
 *
 * Everything is injected — clock, timers, event sources — so the whole
 * cadence/backoff/attention policy runs deterministically with no real
 * timers pending and no real DOM listeners attached.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AttentionRefreshScheduler } from '../attention-refresh';

interface Harness {
  now: () => number;
  advance: (ms: number) => void;
  setTimeoutFn: (fn: () => void, ms: number) => number;
  clearTimeoutFn: (id: number) => void;
  addVisibilityChangeListener: (fn: () => void) => () => void;
  addFocusChangeListener: (fn: () => void) => () => void;
  addOnlineListener: (fn: () => void) => () => void;
  isHidden: () => boolean;
  isFocused: () => boolean;
  setHidden: (v: boolean) => void;
  setFocused: (v: boolean) => void;
  fireVisibilityChange: () => void;
  fireFocusChange: () => void;
  fireOnline: () => void;
  pendingTimerCount: () => number;
  lastScheduledDelay: () => number | null;
}

function makeHarness(): Harness {
  let nowMs = 0;
  let hidden = false;
  let focused = true;
  let nextId = 1;
  const timers: Array<{ id: number; fn: () => void; at: number; delay: number }> = [];

  const visListeners: Array<() => void> = [];
  const focusListeners: Array<() => void> = [];
  const onlineListeners: Array<() => void> = [];

  const setTimeoutFn = (fn: () => void, ms: number): number => {
    const id = nextId++;
    timers.push({ id, fn, at: nowMs + ms, delay: ms });
    return id;
  };
  const clearTimeoutFn = (id: number): void => {
    const idx = timers.findIndex((t) => t.id === id);
    if (idx !== -1) timers.splice(idx, 1);
  };

  function advance(ms: number): void {
    nowMs += ms;
    for (;;) {
      const due = timers.filter((t) => t.at <= nowMs).sort((a, b) => a.at - b.at)[0];
      if (!due) break;
      clearTimeoutFn(due.id);
      due.fn();
    }
  }

  function makeListenerSource(bag: Array<() => void>) {
    return (fn: () => void): (() => void) => {
      bag.push(fn);
      return () => {
        const idx = bag.indexOf(fn);
        if (idx !== -1) bag.splice(idx, 1);
      };
    };
  }

  return {
    now: () => nowMs,
    advance,
    setTimeoutFn,
    clearTimeoutFn,
    addVisibilityChangeListener: makeListenerSource(visListeners),
    addFocusChangeListener: makeListenerSource(focusListeners),
    addOnlineListener: makeListenerSource(onlineListeners),
    isHidden: () => hidden,
    isFocused: () => focused,
    setHidden: (v) => { hidden = v; },
    setFocused: (v) => { focused = v; },
    fireVisibilityChange: () => { for (const fn of [...visListeners]) fn(); },
    fireFocusChange: () => { for (const fn of [...focusListeners]) fn(); },
    fireOnline: () => { for (const fn of [...onlineListeners]) fn(); },
    pendingTimerCount: () => timers.length,
    lastScheduledDelay: () => (timers.length === 0 ? null : timers[timers.length - 1]!.delay),
  };
}

function makeScheduler(h: Harness, refresh: (reason: string) => Promise<boolean>, overrides: Partial<{ staleMs: number; cadenceMs: number; maxBackoffMs: number }> = {}) {
  return new AttentionRefreshScheduler({
    refresh,
    now: h.now,
    setTimeoutFn: h.setTimeoutFn,
    clearTimeoutFn: h.clearTimeoutFn,
    addVisibilityChangeListener: h.addVisibilityChangeListener,
    addFocusChangeListener: h.addFocusChangeListener,
    addOnlineListener: h.addOnlineListener,
    isHidden: h.isHidden,
    isFocused: h.isFocused,
    staleMs: overrides.staleMs ?? 30_000,
    cadenceMs: overrides.cadenceMs ?? 60_000,
    maxBackoffMs: overrides.maxBackoffMs ?? 15 * 60_000,
  });
}

/** Flush pending promise microtasks (real, since the scheduler's own timers
 *  are all injected/fake — this just lets `await refresh()` resolve). */
async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('AttentionRefreshScheduler — cadence', () => {
  let h: Harness;
  beforeEach(() => { h = makeHarness(); });

  it('ticks every cadenceMs while visible and focused', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh);
    scheduler.start();

    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);

    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('stops polling the moment the window loses focus, and does not resume on its own', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh);
    scheduler.start();

    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(h.pendingTimerCount()).toBe(1); // rescheduled after the tick

    h.setFocused(false);
    h.fireFocusChange(); // blur
    expect(h.pendingTimerCount()).toBe(0); // torn down immediately, not left to fire later

    h.advance(10 * 60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1); // still just the one tick
  });

  it('stops polling the moment the document is hidden', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh);
    scheduler.start();

    h.setHidden(true);
    h.fireVisibilityChange();
    expect(h.pendingTimerCount()).toBe(0);

    h.advance(10 * 60_000);
    await flush();
    expect(refresh).not.toHaveBeenCalled();
  });
});

describe('AttentionRefreshScheduler — attention regain', () => {
  let h: Harness;
  beforeEach(() => { h = makeHarness(); });

  it('refreshes on focus regain when the last success is older than staleMs', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh, { staleMs: 30_000 });
    scheduler.start();
    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1); // primes lastSuccessAt

    h.setFocused(false);
    h.fireFocusChange();
    h.advance(40_000); // > staleMs since the last success
    h.setFocused(true);
    h.fireFocusChange();
    await flush();

    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('does NOT refresh on focus regain when the last success is within staleMs', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh, { staleMs: 30_000 });
    scheduler.start();
    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);

    h.setFocused(false);
    h.fireFocusChange();
    h.advance(10_000); // < staleMs
    h.setFocused(true);
    h.fireFocusChange();
    await flush();

    expect(refresh).toHaveBeenCalledTimes(1); // no extra call
  });

  it('refreshes when the document becomes visible again and the cache is stale', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh, { staleMs: 30_000 });
    scheduler.start();
    h.advance(60_000);
    await flush();

    h.setHidden(true);
    h.fireVisibilityChange();
    h.advance(40_000);
    h.setHidden(false);
    h.fireVisibilityChange();
    await flush();

    expect(refresh).toHaveBeenCalledTimes(2);
  });
});

describe('AttentionRefreshScheduler — online', () => {
  let h: Harness;
  beforeEach(() => { h = makeHarness(); });

  it('refreshes when the network returns and the cache is stale', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh, { staleMs: 30_000 });
    scheduler.start();
    h.advance(60_000);
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);

    h.advance(40_000);
    h.fireOnline();
    await flush();

    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('does NOT refresh when the network returns and the cache is fresh', async () => {
    const refresh = vi.fn().mockResolvedValue(true);
    const scheduler = makeScheduler(h, refresh, { staleMs: 30_000 });
    scheduler.start();
    h.advance(60_000);
    await flush();

    h.advance(5_000);
    h.fireOnline();
    await flush();

    expect(refresh).toHaveBeenCalledTimes(1);
  });
});

describe('AttentionRefreshScheduler — backoff', () => {
  let h: Harness;
  beforeEach(() => { h = makeHarness(); });

  it('doubles the wait after each failure, caps it, and resets to cadence on the next success', async () => {
    let ok = false;
    const refresh = vi.fn(async () => ok);
    const scheduler = makeScheduler(h, refresh, { cadenceMs: 60_000, maxBackoffMs: 4 * 60_000 });
    scheduler.start();

    expect(h.lastScheduledDelay()).toBe(60_000);

    h.advance(60_000); // 1st attempt: fails
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(h.lastScheduledDelay()).toBe(120_000); // 60s -> 120s

    h.advance(120_000); // 2nd attempt: fails
    await flush();
    expect(h.lastScheduledDelay()).toBe(240_000); // 120s -> 240s

    h.advance(240_000); // 3rd attempt: fails, would double to 480s but capped at 240s
    await flush();
    expect(h.lastScheduledDelay()).toBe(4 * 60_000); // capped

    ok = true;
    h.advance(4 * 60_000); // 4th attempt: succeeds
    await flush();
    expect(h.lastScheduledDelay()).toBe(60_000); // reset to base cadence
  });

  it('refreshNow resets the backoff regardless of the outcome', async () => {
    const refresh = vi.fn().mockResolvedValue(false);
    const scheduler = makeScheduler(h, refresh, { cadenceMs: 60_000 });
    scheduler.start();

    h.advance(60_000); // fails, backoff -> 120s
    await flush();
    expect(h.lastScheduledDelay()).toBe(120_000);

    const ok = await scheduler.refreshNow(); // explicit; also fails
    expect(ok).toBe(false);
    // Backoff reset to the base cadence even though this attempt failed too —
    // the user just asked, so the next automatic retry starts fresh.
    expect(h.lastScheduledDelay()).toBe(60_000);
  });
});

describe('AttentionRefreshScheduler — a caller arriving mid-attempt', () => {
  let h: Harness;
  beforeEach(() => { h = makeHarness(); });

  it('an explicit click during an in-flight background refresh gets that refresh\'s real result, with only one network call', async () => {
    let resolveRefresh: (ok: boolean) => void = () => {};
    const refresh = vi.fn(() => new Promise<boolean>((resolve) => { resolveRefresh = resolve; }));
    const scheduler = makeScheduler(h, refresh, { cadenceMs: 60_000 });
    scheduler.start();

    h.advance(60_000); // fires the steady-cadence attempt; refresh() is now pending
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);

    // The explicit click arrives before that attempt resolves.
    const explicitResult = scheduler.refreshNow();

    // The in-flight (background) attempt now succeeds.
    resolveRefresh(true);
    const ok = await explicitResult;

    // Piggybacks on the real outcome — never a synthetic "couldn't refresh" —
    // and never triggers a second refresh() call of its own.
    expect(ok).toBe(true);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('an explicit click during an in-flight background refresh that rejects gets the failure, still resets the backoff, with only one network call', async () => {
    let rejectRefresh: (err: unknown) => void = () => {};
    const refresh = vi.fn(() => new Promise<boolean>((_resolve, reject) => { rejectRefresh = reject; }));
    const scheduler = makeScheduler(h, refresh, { cadenceMs: 60_000 });
    scheduler.start();

    h.advance(60_000); // fires the steady-cadence attempt; refresh() is now pending
    await flush();
    expect(refresh).toHaveBeenCalledTimes(1);

    // The explicit click arrives before that attempt resolves.
    const explicitResult = scheduler.refreshNow();

    // The in-flight (background) attempt now rejects. attempt()'s own
    // failure branch doubles the backoff (60s -> 120s) for the attempt it
    // settles — refreshNow()'s unconditional post-attempt reset is the only
    // thing that then pulls a JOINED caller's outcome back to the base
    // cadence, since (unlike a success) the failure branch never resets it.
    rejectRefresh(new Error('offline'));
    const ok = await explicitResult;

    // The joined caller gets the failure, not a hang or a thrown error, and
    // still never triggers a second refresh() call of its own.
    expect(ok).toBe(false);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(h.lastScheduledDelay()).toBe(60_000);
  });
});
