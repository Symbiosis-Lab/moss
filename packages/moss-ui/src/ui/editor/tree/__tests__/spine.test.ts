/**
 * The spine's scheduling half — the debounce and the WKWebView stall watchdog.
 *
 * These used to be three tests reaching into FileTree's private `spineTimer`
 * through a cast. The record owns the timer now, so the rules can be proved
 * where they live and the tree's own suite keeps only the wiring assertion
 * (unmount stops a pending reallocation).
 *
 * Allocation itself is not tested here: it needs a laid-out panel, which jsdom
 * cannot give and which the render gates already drive.
 */
import { describe, test, expect, vi, afterEach } from 'vitest';
import { Spine } from '../spine';

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('Spine scheduling', () => {
  test('a burst of notifications coalesces into one timer-driven reallocation', () => {
    vi.useFakeTimers();
    const reallocate = vi.fn();
    const spine = new Spine(reallocate);

    spine.schedule();
    spine.schedule();
    spine.schedule();
    expect(reallocate).not.toHaveBeenCalled();

    // A TIMER drives it, never requestAnimationFrame — rAF never fires in the
    // idle action-panel webview, and an rAF-based version would not run here.
    vi.advanceTimersByTime(1);
    expect(reallocate).toHaveBeenCalledTimes(1);

    // The gate resets: the next burst arms a fresh timer.
    spine.schedule();
    vi.advanceTimersByTime(1);
    expect(reallocate).toHaveBeenCalledTimes(2);
  });

  test('a notification arriving after the pending timer stalled forces a fresh reallocation', () => {
    // The divider-drag freeze. WKWebView can
    // starve an ALREADY-ARMED setTimeout for an unbounded time — one captured
    // case took 42 SECONDS — while this webview is not OS-focused, which is
    // the normal case during a divider drag. ResizeObserver delivery is never
    // suspended in the same stall, so a notification arriving then is real and
    // on time. The old guard read "a handle is truthy" as "a fire is
    // imminent", and dropped every such notification for the whole stall.
    //
    // performance.now is stubbed rather than the clock warped, so the stall
    // can be modelled exactly: wall-clock time passes while the pending timer
    // callback is never invoked.
    vi.useFakeTimers();
    const reallocate = vi.fn();
    const spine = new Spine(reallocate);
    const now = vi.spyOn(performance, 'now');

    now.mockReturnValue(1_000);
    spine.schedule();
    expect(reallocate).not.toHaveBeenCalled();

    now.mockReturnValue(1_000 + 42_000);
    spine.schedule();
    expect(reallocate).toHaveBeenCalledTimes(1); // forced, not swallowed

    // The stalled handle must not fire again later and double the work.
    vi.advanceTimersByTime(50_000);
    expect(reallocate).toHaveBeenCalledTimes(1);
  });

  test('a pending timer younger than the stall window still coalesces', () => {
    vi.useFakeTimers();
    const reallocate = vi.fn();
    const spine = new Spine(reallocate);
    const now = vi.spyOn(performance, 'now');

    now.mockReturnValue(1_000);
    spine.schedule();
    now.mockReturnValue(1_050); // half the watchdog window
    spine.schedule();
    expect(reallocate).not.toHaveBeenCalled();
  });

  test('dispose cancels both the debounce and the post-mount settle', () => {
    vi.useFakeTimers();
    const reallocate = vi.fn();
    const spine = new Spine(reallocate);

    spine.schedule();
    spine.armInitialSettle();
    spine.dispose();

    vi.advanceTimersByTime(10_000);
    expect(reallocate).not.toHaveBeenCalled();
  });

  test('offsets fall back to a structural indent until an allocation lands', () => {
    const spine = new Spine(() => {});
    expect(spine.hasOffsets).toBe(false);
    expect(spine.offsetAt(0)).toBe(0);
    expect(spine.offsetAt(2)).toBe(56); // 2 × the 28px structural step
  });
});
