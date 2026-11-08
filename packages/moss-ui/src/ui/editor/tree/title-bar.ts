/**
 * The breadcrumb strip as a navigation stack
 * The strip is the
 * app's title bar, and whatever screen is showing REPLACES its content
 * wholesale — a back link, then the subject, then that screen's own trailing
 * actions.
 *
 * `.nav-bc-strip` carries a second, absolutely-positioned content layer
 * (`.nav-bc-strip__mode`, built by `tree/render.ts` alongside the strip
 * itself in `mount()`, and rebuilt empty on every `render()`, which
 * otherwise wipes it along with the crumbs it `replaceChildren()`s — a
 * render while a title bar is active, e.g. a live locale switch mid-
 * Versions-mode, is a known gap: it reverts blank until the next
 * `setTitleBar` call). `setTitleBar`
 * mounts a caller-built `content` element into that layer (found by class,
 * not a stored reference — it's always there after `mount()`/`render()`)
 * and toggles `is-mode` on the strip, which fades the crumb children out/in
 * by CSS (file-tree.css: opacity+visibility on the strip's DIRECT `.seg.bc`
 * children only — the `>` combinator is what keeps this from also catching
 * `mkBackCrumb`'s own `.seg.bc`, one level deeper inside the mode layer).
 * Swapping content while already active crossfades the mode layer itself
 * (`crossfadeSwap`, shared/ui/morph.ts — the same fade-out/replace/fade-in
 * shape `crossfadeText` uses, generalized to an arbitrary mutation) rather
 * than replaying the CSS class transition, which only fires on a class
 * change.
 *
 * Content is built by whoever owns the screen (versions-surface.ts) from two
 * exported primitives: `mkBackCrumb` builds the leading back link from the
 * SAME markup a real crumb uses (`.seg.bc` + `.chev` + `.seg-name`), so it
 * lands where the root crumb lands with no new position rules; `mkTitleBarRow`
 * is the generic flex-row shell (back + subject + optional trailing actions)
 * that becomes the mode layer's one child.
 */

import { CHEV_SVG } from './render';
import { crossfadeSwap } from "../../shared/ui/morph";

/** Mounts `content` into `strip`'s mode layer (found by class — see the file
 *  header) and toggles `is-mode` on `strip`; `null` releases it (CSS
 *  reverses the crumb fade). Called from `FileTree.setTitleBar`. */
export function setTitleBar(strip: HTMLElement | null, content: HTMLElement | null): void {
  const modeLayer = strip?.querySelector<HTMLElement>('.nav-bc-strip__mode');
  if (!strip || !modeLayer) return;
  if (!content) {
    strip.classList.remove('is-mode');
    return;
  }
  if (strip.classList.contains('is-mode')) {
    // Already showing a title bar (e.g. list → detail): crossfade the swap
    // rather than replaying the CSS class transition, which only fires once,
    // on is-mode's OWN addition/removal.
    void crossfadeSwap(modeLayer, () => modeLayer.replaceChildren(content));
  } else {
    modeLayer.replaceChildren(content);
    strip.classList.add('is-mode');
  }
}

/** The leading back link, built from a real crumb's own markup so the "‹"
 *  disc and the label sit pixel-for-pixel where the root crumb's do —
 *  file-tree.css styles it with no new position rules, only `.chev--back`'s
 *  static 180° rotation (the "‹", where an ordinary crumb's chevron toggles
 *  open/closed). Not part of the tree's roving-tabindex system, so it carries
 *  its own minimal keyboard affordance (WAI-ARIA APG custom-button pattern). */
export function mkBackCrumb(label: string, onClick: () => void): HTMLElement {
  const el = document.createElement('div');
  el.className = 'seg bc';
  el.setAttribute('role', 'button');
  el.tabIndex = 0;
  el.setAttribute('aria-label', label);
  const chev = document.createElement('span');
  chev.className = 'chev chev--back';
  chev.innerHTML = CHEV_SVG;
  el.appendChild(chev);
  const name = document.createElement('span');
  name.className = 'seg-name';
  name.textContent = label;
  el.appendChild(name);
  el.addEventListener('click', onClick);
  el.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onClick(); }
  });
  return el;
}

/** The mode layer's one child: back link, then the subject, then whatever
 *  trailing actions the screen owns — a generic shell so the caller never
 *  hand-rolls `.nav-bc-strip__mode-row`'s own flex shape. */
export function mkTitleBarRow(...children: HTMLElement[]): HTMLElement {
  const row = document.createElement('div');
  row.className = 'nav-bc-strip__mode-row';
  for (const child of children) row.appendChild(child);
  return row;
}
