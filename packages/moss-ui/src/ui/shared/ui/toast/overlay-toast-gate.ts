/**
 * Single owner for "is a WAITING full-window overlay currently covering the
 * preview?" — and the queue that gates toast delivery on it.
 *
 * Without it, a plugin/background toast (e.g. a Matters "session expired")
 * can pop up WHILE the post-open waiting screen is still up. The user can't act on it — they're staring at "Opening
 * your site…", not the app chrome the toast renders into — so it reads as a
 * premature, out-of-context interruption. The fix isn't "never show it", it's
 * "show it once there's something to show it over": queue while the overlay
 * is up, flush in order the moment it comes down.
 *
 * Waiting only — never an error screen. `error-hard`/`error-soft` can hold
 * the window indefinitely (a failed build waits for the author to fix
 * something, not for a timer), and the author IS looking at the app in that
 * state, unlike "Opening your site…". Queuing behind an error would be
 * "never", not "not yet" — so `PreviewManager.applyUnshowableReason` calls
 * `setOverlayUp(true)` only for the waiting ranks of the unshowable-reason
 * ladder, and `setOverlayUp(false)` — which flushes — on every other
 * transition, including INTO an error rank. That's the real state source
 * (the ladder), not a DOM query, and the same edge that already drives
 * `progressPanel.setScreenUp` next to it. Nothing calls it on the launcher
 * surface (no waiting overlay there), so `isOverlayUp()` stays false and
 * every toast on that surface runs immediately.
 *
 * `queueOrRun` is generic over "the thing to do to show a toast" so it gates
 * BOTH the ShowToast MossEvent path and direct `toastManager.showToast`
 * calls through the same rule.
 */

let overlayUp = false;

interface QueuedToast {
  key: string;
  run: () => void;
}

let queue: QueuedToast[] = [];

/**
 * However many toasts a real wait plausibly produces before someone reads
 * them; past this a background source is misbehaving (a retry storm, say)
 * and holding every attempt would only make the eventual flush worse. The
 * oldest queued entry is evicted to make room — it's also the stalest.
 */
const MAX_QUEUED = 5;

/** The one writer: called from PreviewManager's unshowable-reason ladder. */
export function setOverlayUp(up: boolean): void {
  if (overlayUp === up) return;
  overlayUp = up;
  if (!up) flush();
}

export function isOverlayUp(): boolean {
  return overlayUp;
}

/**
 * Run `fn` now, or — while the overlay is up — queue it and run it (in
 * queued order) the moment the overlay comes down.
 *
 * `key` dedupes: a later call with the same key, still queued, REPLACES the
 * earlier one rather than adding a second entry, so a toast that fires
 * repeatedly while the overlay is up (a retried background task, say)
 * flushes as its latest state once, not once per attempt. A toast with no
 * natural id should be keyed on its own content (variant + text) rather than
 * a fresh key per call, for the same reason: two identical id-less toasts
 * queued back to back are the same repeated attempt, not two toasts.
 */
export function queueOrRun(key: string, fn: () => void): void {
  if (!overlayUp) {
    fn();
    return;
  }
  const existing = queue.findIndex((q) => q.key === key);
  const entry: QueuedToast = { key, run: fn };
  if (existing >= 0) {
    queue[existing] = entry;
    return;
  }
  queue.push(entry);
  if (queue.length > MAX_QUEUED) queue.shift();
}

/** Drop a still-queued entry, for a toast whose subject went away while it waited. */
export function cancelQueued(key: string): void {
  queue = queue.filter((q) => q.key !== key);
}

function flush(): void {
  const pending = queue;
  queue = [];
  for (const q of pending) q.run();
}

/** Test-only: reset module state between tests. */
export function __test__reset(): void {
  overlayUp = false;
  queue = [];
}
