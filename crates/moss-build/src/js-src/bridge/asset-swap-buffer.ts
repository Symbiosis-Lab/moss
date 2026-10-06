/**
 * Buffer for preview asset swaps whose target element is not in the DOM yet.
 *
 * A `moss-asset-ready` / AssetsSettled signal can race ahead of the preview
 * reload's parse — the message arrives before the matching `<source srcset>` /
 * `<img>` / `<video>` exists, so the immediate swap matches nothing. This
 * buffer holds those paths and replays them (driven by a MutationObserver in
 * iframe-bridge) as elements appear. Extracted from the iframe-bridge IIFE so
 * the bookkeeping (dedup, bound, clear-on-navigation, drain-applies-and-evicts)
 * is unit-testable in isolation — it's the highest-risk new surface of the
 * 2026-07-02 image-drop preview-refresh fix.
 *
 * The DOM mutation itself is injected as `apply(assetType, eventAbsPath) =>
 * matchCount` so this module is pure w.r.t. the document.
 */
export interface AssetSwapBufferOptions {
  /** Apply a swap against the live DOM; returns how many elements matched. */
  apply: (assetType: string, eventAbsPath: string) => number;
  /** Current document URL — a change means buffered swaps are for a page we left. */
  currentUrl: () => string;
  /** Max buffered entries (overflow is dropped, not grown). Default 64. */
  maxPending?: number;
  /**
   * Max futile drain() attempts before a never-matching entry is evicted.
   * Bounds the MutationObserver's lifetime: an AssetsSettled path with no
   * swappable element on the current page (e.g. an OG-card image that lives
   * only in `<meta>`, or a variant for a different page) would otherwise keep
   * `size()` above zero forever, so the observer never disconnects and
   * re-scans the whole DOM on every mutation. After this many drains without a
   * match, give up on that entry. Default 20.
   */
  maxDrainAttempts?: number;
}

export interface AssetSwapBuffer {
  /**
   * Try to apply immediately; if nothing matches, buffer for later replay.
   * Returns "applied" when the swap hit ≥1 element now, else "buffered".
   */
  handle: (assetType: string, eventAbsPath: string) => "applied" | "buffered";
  /** Re-attempt all buffered swaps; evict any that now match. */
  drain: () => void;
  /** Number of currently-buffered swaps (for tests / observer teardown). */
  size: () => number;
}

export function createAssetSwapBuffer(opts: AssetSwapBufferOptions): AssetSwapBuffer {
  const max = opts.maxPending ?? 64;
  const maxAttempts = opts.maxDrainAttempts ?? 20;
  const pending = new Map<string, { assetType: string; eventAbsPath: string; attempts: number }>();
  let pendingUrl = opts.currentUrl();

  function clearIfNavigated(): void {
    const url = opts.currentUrl();
    if (url !== pendingUrl) {
      // A swap buffered for the old page must not apply to the new document.
      pending.clear();
      pendingUrl = url;
    }
  }

  return {
    handle(assetType, eventAbsPath) {
      if (opts.apply(assetType, eventAbsPath) > 0) return "applied";
      clearIfNavigated();
      if (pending.size < max) {
        pending.set(assetType + "\n" + eventAbsPath, { assetType, eventAbsPath, attempts: 0 });
      }
      return "buffered";
    },
    drain() {
      clearIfNavigated();
      for (const [key, entry] of [...pending]) {
        if (opts.apply(entry.assetType, entry.eventAbsPath) > 0) {
          pending.delete(key); // matched — done.
        } else if (++entry.attempts >= maxAttempts) {
          // Never-matching path (e.g. an OG-card image only in <meta>, or a
          // variant for another page) — give up so size() can reach 0 and the
          // caller's observer disconnects instead of scanning forever.
          pending.delete(key);
        }
      }
    },
    size() {
      return pending.size;
    },
  };
}
