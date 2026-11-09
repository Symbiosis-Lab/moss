// ProgressSeam tests (pill-cluster composition).
//
// ProgressSeam owns the PERSISTENT, declarative state that used to ride the
// progress panel as a child: the quiet resting seam (a 1.5px line on the pill's
// straight underside) + the advisory dot (right origin). It mounts INTO the
// `.moss-pill-cluster` — the anchored container that owns the pill's centering
// (`--moss-safe-center-x`) — so the seam tracks the pill by pure CSS: no rect
// measuring, no observers. (The previous titlebar mount measured the pill and
// re-measured on an enumerated trigger list; a divider-var pill move fired none
// of them and stranded the seam — the misaligned-hairline bug.)
//
// Pure DOM, NO timers — jsdom has no layout, so we assert on the DECLARED state
// (descendancy, presence, declared position attrs/classes), never pixel widths.
// Pixel placement is the visual gate.

import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as path from 'node:path';
import { ProgressSeam } from '../progress-seam';

// __dirname is not defined in ESM/vitest — derive it for the source-CSS guards.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

function mountChrome(): void {
  document.body.innerHTML = `
    <div class="moss-titlebar">
      <div class="moss-pill-cluster">
        <div class="moss-nav-pill"></div>
      </div>
    </div>`;
}

function seamEl(): HTMLElement | null {
  return document.querySelector<HTMLElement>('.moss-progress-seam');
}
function dotEl(): HTMLElement | null {
  return document.querySelector<HTMLElement>('.moss-severity-dot');
}

describe('ProgressSeam', () => {
  let seam: ProgressSeam;

  beforeEach(() => {
    mountChrome();
    seam = new ProgressSeam();
  });

  afterEach(() => {
    seam.destroy();
    document.body.innerHTML = '';
  });

  test('mount() places the seam inside the pill cluster (CSS-anchored to the pill)', () => {
    expect(seamEl()).toBeNull();
    seam.mount();
    const el = seamEl();
    expect(el).not.toBeNull();
    expect(el?.parentElement?.classList.contains('moss-pill-cluster')).toBe(true);
  });

  test('the seam carries NO inline geometry — placement is pure CSS in the cluster', () => {
    // The contract that fixes the stranded-seam bug: a pill move (e.g. the
    // divider var changing --moss-safe-center-x) repositions the cluster and
    // everything in it by CSS alone. Any JS-written left/top/width would need
    // a re-measure trigger — and triggers are exactly what went stale.
    seam.mount();
    seam.setTopSeverity('broken');
    for (const el of [seamEl()!, dotEl()!, document.querySelector<HTMLElement>('.moss-seam-hit')!]) {
      expect(el.style.left).toBe('');
      expect(el.style.top).toBe('');
      expect(el.style.width).toBe('');
    }
  });

  test('the seam is NOT a descendant of .moss-progress-panel', () => {
    // Even with a panel present in the DOM, the seam lives on the titlebar layer,
    // not inside the panel — that is the whole point of the promotion.
    document.querySelector('.moss-titlebar')!.insertAdjacentHTML(
      'beforeend',
      '<div class="moss-progress-panel"></div>',
    );
    seam.mount();
    expect(seamEl()).not.toBeNull();
    expect(seamEl()!.closest('.moss-progress-panel')).toBeNull();
  });

  test('is hidden at rest; setVisible toggles the line + hit-region (no idle line)', () => {
    // The user-reported bug: a persistent green line on an idle pill reads as
    // stuck progress, and the reveal hover has no panel behind it. The seam is
    // shown ONLY when the panel has content (driven by setVisible()).
    seam.mount();
    const hit = () => document.querySelector('.moss-seam-hit');
    // At rest: both hidden — idle pill shows no line, no reveal handle.
    expect(seamEl()!.classList.contains('is-hidden')).toBe(true);
    expect(hit()!.classList.contains('is-hidden')).toBe(true);
    // Content present → shown.
    seam.setVisible(true);
    expect(seamEl()!.classList.contains('is-hidden')).toBe(false);
    expect(hit()!.classList.contains('is-hidden')).toBe(false);
    // Idle again → hidden.
    seam.setVisible(false);
    expect(seamEl()!.classList.contains('is-hidden')).toBe(true);
    expect(hit()!.classList.contains('is-hidden')).toBe(true);
  });

  test('mount() is idempotent (no duplicate seam)', () => {
    seam.mount();
    seam.mount();
    expect(document.querySelectorAll('.moss-progress-seam').length).toBe(1);
  });

  test('mount() is a no-op when the pill cluster is not present yet', () => {
    document.body.innerHTML = ''; // no chrome
    seam.mount();
    expect(seamEl()).toBeNull();
  });

  test('setTopSeverity renders the dot; "none" removes it', () => {
    seam.mount();
    expect(dotEl()).toBeNull();

    seam.setTopSeverity('broken');
    expect(dotEl()).not.toBeNull();
    // The dot is on the titlebar layer too, never inside the panel.
    expect(dotEl()!.closest('.moss-progress-panel')).toBeNull();

    seam.setTopSeverity('none');
    expect(dotEl()).toBeNull();
  });

  test('setTopSeverity self-mounts if not yet mounted', () => {
    expect(seamEl()).toBeNull();
    seam.setTopSeverity('broken');
    expect(seamEl()).not.toBeNull();
    expect(dotEl()).not.toBeNull();
  });

  test('setTopSeverity is idempotent — repeated call does not duplicate the dot', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    seam.setTopSeverity('broken');
    expect(document.querySelectorAll('.moss-severity-dot').length).toBe(1);
  });

  test('the dot declares the seam RIGHT origin (data-origin="right")', () => {
    // The L→R rebuild fill buries a LEFT dot; the dot lives at the seam's right
    // end. Pixel position is the visual gate — here we lock the DECLARED origin
    // so the placement intent can't silently flip to "left".
    seam.mount();
    seam.setTopSeverity('degraded');
    expect(dotEl()!.getAttribute('data-origin')).toBe('right');
  });

  test('destroy() removes the seam and the dot', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    seam.destroy();
    expect(seamEl()).toBeNull();
    expect(dotEl()).toBeNull();
  });

  // The seam emits hover/click inputs without mutating
  // visibility; the panel maps inputs to transitions (single owner). The seam
  // also owns the padded hit-region + the hover-only disclosure cue.

  function hitEl(): HTMLElement | null {
    return document.querySelector<HTMLElement>('.moss-seam-hit');
  }
  function cueEl(): HTMLElement | null {
    return document.querySelector<HTMLElement>('.moss-seam-cue');
  }

  test('mount() adds a padded, keyboard-focusable hit-region (the reveal target)', () => {
    seam.mount();
    const hit = hitEl();
    expect(hit).not.toBeNull();
    // Lives on the titlebar layer, never inside the panel.
    expect(hit!.closest('.moss-progress-panel')).toBeNull();
    // Keyboard-reachable + named for assistive tech (works without hover —
    // Windows/touch).
    expect(hit!.getAttribute('tabindex')).toBe('0');
    expect(hit!.getAttribute('role')).toBe('button');
  });


  test('No .moss-seam-cue element is created — the disclosure glyph is the handle button', () => {
    // The ⌄ U+2304 cue was the seam's disclosure glyph (opacity-faded, panel-driven).
    // The ▾/▴ handle INSIDE the panel glass replaces it. The seam
    // must NOT create a cue child — one disclosure glyph, in one place.
    seam.mount();
    expect(cueEl()).toBeNull();
  });

  test('No bare ⌄ U+2304 text node exists on the hit-region after mount', () => {
    // Guard that the old cue text ("⌄") was not simply left as a text node.
    seam.mount();
    const hit = hitEl()!;
    expect(hit.textContent?.includes('⌄')).toBe(false);
  });

  test('setCallbacks: mouseenter/leave/click emit the corresponding INPUTS (seam never mutates visibility)', () => {
    seam.mount();
    const onHoverEnter = vi.fn();
    const onHoverLeave = vi.fn();
    const onClick = vi.fn();
    seam.setCallbacks({ onHoverEnter, onHoverLeave, onClick });

    const hit = hitEl()!;
    hit.dispatchEvent(new MouseEvent('mouseenter'));
    expect(onHoverEnter).toHaveBeenCalledTimes(1);

    hit.dispatchEvent(new MouseEvent('mouseleave'));
    expect(onHoverLeave).toHaveBeenCalledTimes(1);

    hit.dispatchEvent(new MouseEvent('click'));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  test('Enter (and Space) on the focused hit-region emits onClick (keyboard parity)', () => {
    seam.mount();
    const onClick = vi.fn();
    seam.setCallbacks({ onClick });
    const hit = hitEl()!;
    hit.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onClick).toHaveBeenCalledTimes(1);
    hit.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    expect(onClick).toHaveBeenCalledTimes(2);
  });

  test('the hit-region/cue carry NO visibility state — emitting an input does not create or touch a panel', () => {
    // The whole point of the single-owner split: the seam can fire all day and
    // the panel DOM is unaffected (the panel decides).
    seam.mount();
    seam.setCallbacks({ onHoverEnter: () => {}, onClick: () => {} });
    const hit = hitEl()!;
    hit.dispatchEvent(new MouseEvent('mouseenter'));
    hit.dispatchEvent(new MouseEvent('click'));
    expect(document.querySelector('.moss-progress-panel')).toBeNull();
  });

  test('destroy() removes the hit-region and the cue too', () => {
    seam.mount();
    seam.destroy();
    expect(hitEl()).toBeNull();
    expect(cueEl()).toBeNull();
  });

  // ── setTopSeverity — SOLE owner of the seam dot ─────────────────

  function severityDotEl(): HTMLElement | null {
    return document.querySelector<HTMLElement>('.moss-severity-dot');
  }

  test('setTopSeverity("broken") creates the dot with --broken class', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    const dot = severityDotEl();
    expect(dot).not.toBeNull();
    expect(dot!.classList.contains('moss-severity-dot--broken')).toBe(true);
    expect(dot!.classList.contains('moss-severity-dot--degraded')).toBe(false);
  });

  test('setTopSeverity("degraded") creates the dot with --degraded class', () => {
    seam.mount();
    seam.setTopSeverity('degraded');
    const dot = severityDotEl();
    expect(dot).not.toBeNull();
    expect(dot!.classList.contains('moss-severity-dot--degraded')).toBe(true);
    expect(dot!.classList.contains('moss-severity-dot--broken')).toBe(false);
  });

  test('setTopSeverity("none") hides/removes the dot', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    expect(severityDotEl()).not.toBeNull();
    seam.setTopSeverity('none');
    expect(severityDotEl()).toBeNull();
  });

  test('setTopSeverity switches modifier class on transition (no duplicates)', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    seam.setTopSeverity('degraded');
    const dots = document.querySelectorAll('.moss-severity-dot');
    expect(dots.length).toBe(1);
    expect(dots[0].classList.contains('moss-severity-dot--degraded')).toBe(true);
    expect(dots[0].classList.contains('moss-severity-dot--broken')).toBe(false);
  });

  test('hit-region aria-label is "Show progress" at rest, changes per severity', () => {
    seam.mount();
    const hit = hitEl()!;
    expect(hit.getAttribute('aria-label')).toBe('Show progress');
    seam.setTopSeverity('broken');
    // aria-label differs (non-tautological: "broken" label ≠ rest label)
    const brokenLabel = hit.getAttribute('aria-label')!;
    expect(brokenLabel).not.toBe('Show progress');
    seam.setTopSeverity('degraded');
    const degradedLabel = hit.getAttribute('aria-label')!;
    expect(degradedLabel).not.toBe('Show progress');
    // The three labels must all differ from each other
    expect(brokenLabel).not.toBe(degradedLabel);
    seam.setTopSeverity('none');
    expect(hit.getAttribute('aria-label')).toBe('Show progress');
  });

  test('no .moss-advisory-dot class is created', () => {
    seam.mount();
    seam.setTopSeverity('broken');
    expect(document.querySelector('.moss-advisory-dot')).toBeNull();
  });

  // Source-CSS declared-value guards (jsdom has no layout — assert the source,
  // pixel correctness is the visual gate). These pin the two halves of the
  // flicker fix that live in CSS.
  test('no raw :hover rule drives the cue — the panel-asserted is-visible is the only pointer path', () => {
    const css = readFileSync(path.resolve(__dirname, '../../../style.css'), 'utf8');
    expect(css).not.toMatch(/\.moss-seam-hit:hover\s*\.moss-seam-cue/);
  });

  test('the hit band is 24px (WCAG 2.5.8) and reaches DOWNWARD from the seam, not centered', () => {
    // Centered-on-the-seam wasted the top half under the pill (same z, occluded)
    // and left ~8px of effective target. The panel opens downward — the band
    // extends where the revealed surface will be. 24px is the WCAG 2.2 SC 2.5.8
    // Target Size (Minimum) floor; the band was 20px, under it.
    const css = readFileSync(path.resolve(__dirname, '../../../style.css'), 'utf8');
    const start = css.indexOf('\n.moss-seam-hit {');
    expect(start).toBeGreaterThan(-1);
    const block = css.slice(start, css.indexOf('}', start));
    expect(block).toMatch(/height:\s*24px/);
    expect(block).toMatch(/translateY\(-2px\)/);
  });

  test('the fill track never takes the pointer — it would outrank the hit band', () => {
    // The track paints at z(--z-toast)+2 so the green fill covers the panel's
    // square top. Any `pointer-events:auto` on it therefore beats `.moss-seam-hit`
    // (z:2) and swallows the hover that opens the panel — which is exactly what a
    // `[data-tooltip]` pad on the track did during every publish.
    const css = readFileSync(path.resolve(__dirname, '../../../style.css'), 'utf8');
    expect(css).not.toMatch(/\.moss-preview-hairline-track\[data-tooltip\]/);
  });

  test('setTooltip puts the hover description on the hit band, and clears it', () => {
    // On the band (the one hover target), NOT the paint layer. Used only where
    // no panel tells the same story — the borrowed ambient-download pass.
    seam.mount();
    const hit = () => document.querySelector<HTMLElement>('.moss-seam-hit')!;
    seam.setTooltip('OnionPress (downloading… 42%)');
    expect(hit().getAttribute('data-tooltip')).toContain('42%');
    seam.setTooltip(null);
    expect(hit().hasAttribute('data-tooltip')).toBe(false);
  });

  // Reflow tracking (harness-surfaced bug). The seam measured the pill once and
  // only re-measured on `window resize`. A titlebar reflow — the progress panel
  // appearing, a sibling width change, the pill's `refreshing` class toggling —
  // moves the centered pill with NO window resize, stranding the seam to the
  // left of the pill. The fix observes the titlebar for childList + class
  // mutations and re-measures (jsdom has no layout, so we assert the WIRING:
  // a mutation triggers measure(); pixel placement is the visual gate).

  test('mount() wires NO observers or resize listeners (CSS owns placement)', () => {
    // The previous design enumerated "reasons the pill might move" (resize,
    // ResizeObserver, MutationObserver) and re-measured on each. The list went
    // stale the moment a NEW reason appeared (the divider CSS var), stranding
    // the seam. The cluster composition deletes the whole trigger class: there
    // is nothing to re-measure, so there must be nothing listening.
    const addListener = vi.spyOn(window, 'addEventListener');
    const observers: MutationObserver[] = [];
    const RealMO = globalThis.MutationObserver;
    const spyMO = class extends RealMO {
      constructor(cb: MutationCallback) {
        super(cb);
        observers.push(this);
      }
    };
    globalThis.MutationObserver = spyMO as typeof MutationObserver;
    try {
      seam.mount();
      expect(addListener).not.toHaveBeenCalledWith('resize', expect.anything(), expect.anything());
      expect(observers.length).toBe(0);
    } finally {
      globalThis.MutationObserver = RealMO;
      addListener.mockRestore();
    }
  });
});
