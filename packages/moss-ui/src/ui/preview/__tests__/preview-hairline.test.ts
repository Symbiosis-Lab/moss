// PreviewHairline lifecycle tests
//
// The hairline is the rebuild progress bar on the bottom edge of the nav pill.
// Its TIMING was iterated on carefully (the pill flash must fire RIGHT AFTER the
// bar reaches full, not during the grow), so these tests lock the lifecycle:
// start → grow → complete(onReachedFull) → dissolve, plus the cancelable
// no-change fallback. Pure DOM + timers; jsdom has no layout, so we assert on
// class transitions and callback ordering rather than pixel widths.

import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { PreviewHairline } from '../preview-hairline';

function mountChrome(): void {
  document.body.innerHTML = `
    <div class="moss-titlebar">
      <div class="moss-pill-cluster">
        <div class="moss-nav-pill"></div>
      </div>
    </div>`;
}

function bar(): HTMLElement | null {
  return document.querySelector<HTMLElement>('.moss-preview-hairline');
}

describe('PreviewHairline', () => {
  describe('determinate (ambient download) pass', () => {
    test('startDeterminate does NOT trickle — width tracks only real setUploadProgress', () => {
      hairline.startDeterminate();
      const w0 = bar()!.style.width;
      // Advancing fake time must NOT grow the bar (no trickle timer running).
      vi.advanceTimersByTime(2000);
      expect(bar()!.style.width).toBe(w0);
      // Only an explicit real fraction moves it.
      hairline.setUploadProgress(0.5);
      expect(bar()!.style.width).not.toBe(w0);
    });

    // The hover description moved to ProgressSeam (it belongs on the ONE hover
    // target, not on the paint layer). The track's inertness is pinned where it
    // is load-bearing: preview-manager.test.ts asserts no data-tooltip appears
    // on the real deploy path, and progress-seam.test.ts guards the CSS that
    // keeps the track pointer-events:none. PreviewHairline itself no longer has
    // a tooltip API at all.

    test('setUploadProgress on an idle bar opens a DETERMINATE pass (no trickle)', () => {
      // Regression: the fallback used to call start(), arming the NProgress
      // trickle. Because the fill is monotonic, the trickle outran the real byte
      // fraction and could never be pulled back — the bar shot up, then crawled.
      hairline.setUploadProgress(0.1);
      const w0 = bar()!.style.width;
      vi.advanceTimersByTime(2000);
      expect(bar()!.style.width).toBe(w0);
    });
  });

  let hairline: PreviewHairline;

  beforeEach(() => {
    vi.useFakeTimers();
    mountChrome();
    hairline = new PreviewHairline();
  });

  afterEach(() => {
    hairline.destroy();
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  test('start() mounts an active bar inside a track in the pill cluster', () => {
    expect(bar()).toBeNull();
    hairline.start();
    const el = bar();
    expect(el).not.toBeNull();
    // Track (CSS-anchored to the pill's straight run) → fill. The track is a
    // cluster child so a pill move repositions everything with zero JS.
    expect(el?.parentElement?.classList.contains('moss-preview-hairline-track')).toBe(true);
    expect(
      el?.parentElement?.parentElement?.classList.contains('moss-pill-cluster'),
    ).toBe(true);
    expect(el?.classList.contains('active')).toBe(true);
    expect(el?.classList.contains('dissolving')).toBe(false);
  });

  test('start() does not trickle — no interval is armed', () => {
    // A bare start() arms no timer: advancing fake time must not move the bar,
    // only a real setBuildProgress/setUploadProgress tick may.
    hairline.start();
    const w0 = bar()!.style.width;
    vi.advanceTimersByTime(5000);
    expect(bar()!.style.width).toBe(w0);
  });

  test('the fill width is a PERCENTAGE of the straight run, never measured px', () => {
    // No getBoundingClientRect-derived widths: the fraction maps to a % of
    // the CSS-anchored track (build fills 0→90%, so 0.5 → 45%). jsdom has no
    // layout, so a measured-px implementation would write "0px" here.
    hairline.start();
    hairline.setBuildProgress(0.5);
    expect(bar()!.style.width).toMatch(/45(\.\d+)?%/);
  });

  test('start() is a no-op for chrome that is not present yet', () => {
    document.body.innerHTML = ''; // no cluster / pill
    hairline.start();
    expect(bar()).toBeNull();
  });

  test('complete() fires onReachedFull AFTER the grow, then dissolves', () => {
    hairline.start();
    const onReachedFull = vi.fn();
    hairline.complete(onReachedFull);

    // The flash callback must NOT fire during the grow (the bug the user
    // flagged: "it flashes before the progress line finishes").
    expect(onReachedFull).not.toHaveBeenCalled();
    vi.advanceTimersByTime(299);
    expect(onReachedFull).not.toHaveBeenCalled();

    // Fires right after the 300ms width transition (GROW_MS).
    vi.advanceTimersByTime(1);
    expect(onReachedFull).toHaveBeenCalledTimes(1);

    // Still showing at full during the brief hold, then dissolves.
    expect(bar()?.classList.contains('active')).toBe(true);
    vi.advanceTimersByTime(220); // COMPLETE_HOLD_MS
    expect(bar()?.classList.contains('active')).toBe(false);
    expect(bar()?.classList.contains('dissolving')).toBe(true);
  });

  test('complete() works even when start() was never called (blink-fast build)', () => {
    const onReachedFull = vi.fn();
    hairline.complete(onReachedFull); // self-starts
    expect(bar()).not.toBeNull();
    expect(bar()?.classList.contains('active')).toBe(true);
    vi.advanceTimersByTime(300);
    expect(onReachedFull).toHaveBeenCalledTimes(1);
  });

  test('dissolveAfter() completes the bar on its own after the delay', () => {
    hairline.start();
    hairline.dissolveAfter(1500);
    vi.advanceTimersByTime(1499);
    expect(bar()?.classList.contains('dissolving')).toBe(false);
    // At the delay, complete() runs; +GROW_MS+HOLD reaches the dissolve.
    vi.advanceTimersByTime(1);
    vi.advanceTimersByTime(300 + 220);
    expect(bar()?.classList.contains('dissolving')).toBe(true);
  });

  test('cancelPendingDissolve() stops a scheduled dissolveAfter', () => {
    hairline.start();
    hairline.dissolveAfter(1500);
    hairline.cancelPendingDissolve();
    // Even well past the delay, the bar is still active (refresh will complete
    // it precisely when the morph lands).
    vi.advanceTimersByTime(5000);
    expect(bar()?.classList.contains('active')).toBe(true);
    expect(bar()?.classList.contains('dissolving')).toBe(false);
  });

  // The `setBuildProgress() self-starts and never moves backwards` case was
  // deleted: it conceded in its own comment that it could not assert the
  // monotonic guard ("we can't read pixels in jsdom"). The case below asserts
  // the SAME applyDeterminate guard on the SAME path with a real width
  // assertion, and `complete() works even when start() was never called`
  // covers the self-start half.
  test('setUploadProgress() is the on-screen position, uncapped, and monotonic (deploy upload)', () => {
    // The deploy's span table (progress/hairline-pacing.ts) already reserves its tail —
    // finalizing stops at 0.96 — so the hairline must NOT rescale it by
    // BUILD_CAP too: that put finalizing at 82.8% on screen and reserved the
    // finishing slice twice.
    hairline.setUploadProgress(0.5); // self-starts
    expect(bar()!.style.width).toMatch(/\b50(\.\d+)?%/);
    // Byte progress can report jitter; the bar must never shrink.
    hairline.setUploadProgress(0.2);
    expect(bar()!.style.width).toMatch(/\b50(\.\d+)?%/);
  });

  test('setBuildProgress() alone carries the build cap — the morph owns the last slice', () => {
    hairline.setBuildProgress(1); // build stream done; morph still to land
    expect(bar()!.style.width).toMatch(/\b90(\.\d+)?%/);
  });

  test('cancel() dissolves the bar WITHOUT filling to full (deploy failed)', () => {
    hairline.setUploadProgress(0.3);
    expect(bar()?.classList.contains('active')).toBe(true);
    hairline.cancel();
    // Unlike complete(), cancel does not fill to 100% or flash — it just fades.
    expect(bar()?.classList.contains('active')).toBe(false);
    expect(bar()?.classList.contains('dissolving')).toBe(true);
  });

  test('cancel() then a retry within DISSOLVE_MS is NOT wiped by the stale dissolve timer', () => {
    // Regression: dissolve()'s fade-out timer used to be untracked, so after a
    // failed deploy the 500ms cleanup fired into the retry and zeroed the bar.
    hairline.setUploadProgress(0.3);
    hairline.cancel(); // schedules the 500ms dissolve cleanup
    // Retry begins before the cleanup would fire — start() must cancel it.
    hairline.start();
    hairline.setUploadProgress(0.4);
    expect(bar()?.classList.contains('active')).toBe(true);
    // Advance well past the old cleanup window: it must have been cancelled.
    vi.advanceTimersByTime(600);
    expect(bar()?.classList.contains('active')).toBe(true);
    expect(bar()?.classList.contains('dissolving')).toBe(false);
  });

  test('destroy() removes the bar and clears timers', () => {
    hairline.start();
    hairline.dissolveAfter(1500);
    hairline.destroy();
    expect(bar()).toBeNull();
    // No pending timer should resurrect anything.
    expect(() => vi.advanceTimersByTime(5000)).not.toThrow();
    expect(bar()).toBeNull();
  });
});
