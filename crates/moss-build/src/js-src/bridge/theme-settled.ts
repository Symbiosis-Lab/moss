/**
 * One `data-theme` observer for the bridge's three reporters (page colour,
 * chrome ambient, site accent), each of which must re-read the page after a
 * theme flip.
 *
 * Watching the attribute rather than exposing a hook is the three-writer
 * story: the shell's push, the site's own moon toggle, and cold start all set
 * `data-theme`, and observing it catches all three without any of them having
 * to remember to call us. `moss-page-color` learned this the hard way: its
 * re-send for the moon case once lived in link-preview.ts, which only ships
 * when `[site].link_preview` is true.
 *
 * Every fire runs twice: now, and again after `THEME_SETTLE_MS`. site.css
 * applies a global `* { transition: background-color 0.2s }`, so a value read
 * the instant the attribute flips is partway through the fade; the second read
 * gets the committed colour at the cost of one late repaint.
 */

/** Delay before the confirming re-read after a theme flip, in ms. */
export const THEME_SETTLE_MS = 260;

export interface ThemeSettled {
  /** Run the callback now, and again once the background transition settles.
   *  Callers use this for changes that repaint the page without touching
   *  `data-theme` — a morph swaps content in place, so nothing else fires. */
  fire: () => void;
  /** Detach the observer and cancel any pending settle timer. */
  stop: () => void;
}

export function onThemeSettled(
  win: Window & typeof globalThis,
  callback: () => void,
): ThemeSettled {
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  const fire = (): void => {
    callback();
    if (settleTimer !== null) clearTimeout(settleTimer);
    settleTimer = setTimeout(() => {
      settleTimer = null;
      callback();
    }, THEME_SETTLE_MS);
  };

  let observer: MutationObserver | null = null;
  if (typeof win.MutationObserver === "function") {
    observer = new win.MutationObserver(fire);
    observer.observe(win.document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
  }

  return {
    fire,
    stop: () => {
      observer?.disconnect();
      if (settleTimer !== null) clearTimeout(settleTimer);
    },
  };
}
