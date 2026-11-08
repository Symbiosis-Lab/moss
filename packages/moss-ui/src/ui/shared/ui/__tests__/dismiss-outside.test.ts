/**
 * Unit tests for the shared outside-dismissal helper (shared/ui/dismiss-outside.ts).
 *
 * The three cases that earned this module are the ones a plain
 * `document.addEventListener('mousedown', …)` cannot see, so they are what
 * these tests are mostly about:
 *   - a click on a Tauri window drag region, whose mousedown is killed with
 *     `stopImmediatePropagation()` before any app listener bubbles
 *   - a click in a DIFFERENT webview (shell titlebar, editor), which reaches
 *     this document only as a `window` blur
 *   - a press or Escape inside the preview iframe, a different document whose
 *     events reach the shell only as the bridge's postMessage
 * plus the ordinary bubble-phase behaviour they must not disturb.
 *
 * What jsdom CANNOT see here is the bug that earned the third case: it has no
 * second document holding real focus, so it cannot show that blur never fires
 * for a menu opened by a right-click already inside the iframe. The desktop
 * app's end-to-end preview context-menu scenario covers that.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { armOutsideDismiss, isSwallowedByDragRegion } from '../dismiss-outside';
import { initTooltips, destroyTooltips } from '../../../components/tooltip';

/** Stand-in for Tauri's injected drag script: a document-level BUBBLE listener,
 *  registered before any app listener, that kills mousedowns on drag regions. */
function installTauriDragScript(): () => void {
  const handler = (ev: MouseEvent): void => {
    if (isSwallowedByDragRegion(ev)) ev.stopImmediatePropagation();
  };
  document.addEventListener('mousedown', handler);
  return () => document.removeEventListener('mousedown', handler);
}

function el(tag = 'div', attrs: Record<string, string> = {}): HTMLElement {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  document.body.appendChild(node);
  return node;
}

function mousedown(node: Element, init: MouseEventInit = {}): void {
  node.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, composed: true, ...init }));
}

let disarm: (() => void) | null = null;
let uninstallDrag: (() => void) | null = null;

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  disarm?.();
  disarm = null;
  uninstallDrag?.();
  uninstallDrag = null;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

describe('armOutsideDismiss', () => {
  it('dismisses on an outside mousedown, but not on the tick that armed it', () => {
    const onDismiss = vi.fn();
    const surface = el();
    disarm = armOutsideDismiss({ inside: [surface], covers: [], onDismiss });

    mousedown(document.body);
    expect(onDismiss).not.toHaveBeenCalled();

    vi.advanceTimersByTime(0);
    mousedown(document.body);
    expect(onDismiss).toHaveBeenCalledWith('outside-click');
  });

  it('ignores mousedowns inside the surface or any extra inside-anchor', () => {
    const onDismiss = vi.fn();
    const surface = el();
    const child = document.createElement('span');
    surface.appendChild(child);
    const anchor = el('button');
    disarm = armOutsideDismiss({ inside: [surface, anchor, null], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    mousedown(child);
    mousedown(anchor);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('dismisses on a drag-region click, which Tauri stops before it bubbles', () => {
    uninstallDrag = installTauriDragScript();
    const onDismiss = vi.fn();
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    const titlebar = el('div', { 'data-tauri-drag-region': '' });
    mousedown(titlebar);
    expect(onDismiss).toHaveBeenCalledWith('outside-click');
  });

  it('dismisses exactly once for a drag-region click that DOES bubble', () => {
    // No drag script installed: both listeners see the event. The one-shot
    // latch is what keeps that from being a double close.
    const onDismiss = vi.fn();
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    mousedown(el('div', { 'data-tauri-drag-region': '' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('dismisses on webview blur — the only signal a click in another webview sends', () => {
    const onDismiss = vi.fn();
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    window.dispatchEvent(new Event('blur'));
    expect(onDismiss).toHaveBeenCalledWith('webview-blur');
  });

  it('stops listening after disarm, and after it has fired', () => {
    const onDismiss = vi.fn();
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss });
    vi.advanceTimersByTime(0);
    disarm();

    mousedown(document.body);
    window.dispatchEvent(new Event('blur'));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('spends only the suppressed blur, and still dismisses on the next click', () => {
    // The system file dialog blurs this webview by existing. Suppressing that
    // blur must not leave the surface armed-but-fired — permanently
    // un-dismissable, which is what refusing inside `onDismiss` would do,
    // because `dismiss` disarms BEFORE calling back.
    const onDismiss = vi.fn();
    let dialogOpen = true;
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss, suppressBlur: () => dialogOpen });
    vi.advanceTimersByTime(0);

    window.dispatchEvent(new Event('blur'));
    expect(onDismiss).not.toHaveBeenCalled();

    dialogOpen = false;
    mousedown(document.body);
    expect(onDismiss).toHaveBeenCalledWith('outside-click');
  });

  it('never suppresses an outside CLICK, only a blur', () => {
    const onDismiss = vi.fn();
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss, suppressBlur: () => true });
    vi.advanceTimersByTime(0);

    mousedown(document.body);
    expect(onDismiss).toHaveBeenCalledWith('outside-click');
  });

  it('dismisses on a press inside the preview iframe, which reaches this document only as a message', () => {
    const onDismiss = vi.fn();
    const surface = el();
    disarm = armOutsideDismiss({ inside: [surface], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'moss-preview-pointerdown' } }));
    expect(onDismiss).toHaveBeenCalledWith('preview-interaction');
  });

  it('dismisses on Escape typed into the preview iframe, which the shell keydown listener never sees', () => {
    const onDismiss = vi.fn();
    const surface = el();
    disarm = armOutsideDismiss({ inside: [surface], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'moss-escape-key' } }));
    expect(onDismiss).toHaveBeenCalledWith('preview-interaction');
  });

  it('ignores the bridge\'s other traffic — the preview posts up constantly', () => {
    const onDismiss = vi.fn();
    const surface = el();
    disarm = armOutsideDismiss({ inside: [surface], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    for (const data of [{ type: 'moss-iframe-scroll' }, { type: 'moss-navigation' }, 'plain string', null]) {
      window.dispatchEvent(new MessageEvent('message', { data }));
    }
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('leaves a bubble-phase stopPropagation meaning "this click does not dismiss"', () => {
    // The editor's undo/redo buttons hold an uncommitted chip popover open
    // exactly this way. A blanket capture listener would silently void that
    // contract.
    const onDismiss = vi.fn();
    const keepOpen = el('button');
    keepOpen.addEventListener('mousedown', (e) => e.stopPropagation());
    disarm = armOutsideDismiss({ inside: [el()], covers: [], onDismiss });
    vi.advanceTimersByTime(0);

    mousedown(keepOpen);
    expect(onDismiss).not.toHaveBeenCalled();
  });
});

describe("isSwallowedByDragRegion — mirrors Tauri's drag script", () => {
  /** The verdict must be read DURING dispatch: `composedPath()` is empty once
   *  the event has finished propagating (which is also why the helper's own
   *  listener calls it inline). */
  function verdictFor(node: Element, init: MouseEventInit = {}): boolean {
    let verdict = false;
    document.addEventListener('mousedown', (e) => { verdict = isSwallowedByDragRegion(e as MouseEvent); }, { once: true });
    mousedown(node, init);
    return verdict;
  }

  it('matches a bare region only when it IS the target', () => {
    const region = el('div', { 'data-tauri-drag-region': '' });
    const child = document.createElement('span');
    region.appendChild(child);

    expect(verdictFor(region)).toBe(true);
    expect(verdictFor(child)).toBe(false);
  });

  it('matches anywhere inside a "deep" region, and never a "false" one, and never a right-click', () => {
    const deep = el('div', { 'data-tauri-drag-region': 'deep' });
    const child = document.createElement('span');
    deep.appendChild(child);
    const opted = document.createElement('div');
    opted.setAttribute('data-tauri-drag-region', 'false');
    deep.appendChild(opted);

    expect(verdictFor(child)).toBe(true);
    expect(verdictFor(opted)).toBe(false);
    expect(verdictFor(deep, { button: 2 })).toBe(false);
  });
});

/**
 * Every floating surface moss has arms its dismissal here, which makes this
 * the one place that knows something is covering the page. A `--z-popup`
 * tooltip loses to all of those tiers, so it is held off for the armed
 * lifetime — not merely at the opening press, since re-crossing the anchor
 * under an open menu would otherwise resurrect it underneath.
 */
describe('armOutsideDismiss — tooltips yield to whatever it is arming for', () => {
  let disarm: (() => void) | null = null;

  beforeEach(() => initTooltips());
  afterEach(() => {
    disarm?.();
    disarm = null;
    destroyTooltips();
  });

  function hoverTrigger(): void {
    const trigger = document.createElement('button');
    trigger.setAttribute('data-tooltip', 'New page in William Blake');
    document.body.appendChild(trigger);
    trigger.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  }

  const tooltipShown = (): boolean =>
    document.querySelector('.moss-tooltip')?.classList.contains('visible') ?? false;

  /** A floating surface with a tooltip trigger of its own inside it. */
  function surfaceEl(): { root: HTMLElement; inner: HTMLElement } {
    const root = document.createElement('div');
    const inner = document.createElement('button');
    inner.setAttribute('data-tooltip', 'Copied');
    root.appendChild(inner);
    document.body.appendChild(root);
    return { root, inner };
  }

  it('hides a shown tooltip on arm, holds it off, and restores it on disarm', () => {
    const { root } = surfaceEl();
    hoverTrigger();
    expect(tooltipShown()).toBe(true);

    disarm = armOutsideDismiss({ inside: [root], covers: [root], onDismiss: () => {} });
    expect(tooltipShown()).toBe(false);

    hoverTrigger();
    expect(tooltipShown()).toBe(false);

    disarm();
    disarm = null;
    hoverTrigger();
    expect(tooltipShown()).toBe(true);
    root.remove();
  });

  it('holds off the ANCHOR\'s own tooltip, which `inside` would have spared', () => {
    // The reported bug itself. `inside` includes the anchor so a press on it
    // does not dismiss the surface it just opened — but the anchor is exactly
    // the trigger the surface opens onto, so its hint is the one certainly
    // covered. `covers` is a separate list for this reason.
    const { root } = surfaceEl();
    const anchor = document.createElement('button');
    anchor.setAttribute('data-tooltip', 'New page in William Blake');
    document.body.appendChild(anchor);

    disarm = armOutsideDismiss({ inside: [root, anchor], covers: [root], onDismiss: () => {} });

    anchor.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));

    expect(tooltipShown()).toBe(false);
    anchor.remove();
    root.remove();
  });

  it('leaves tooltips alone for a surface that covers nothing', () => {
    // The chip bar's provisional ghost is in-flow and can stay pending for
    // minutes; holding every tooltip in the bar off for that long would be a
    // worse bug than the one this mechanism fixes.
    disarm = armOutsideDismiss({ inside: [], covers: [], onDismiss: () => {} });

    hoverTrigger();

    expect(tooltipShown()).toBe(true);
  });

  it('scopes the hold to what it paints: a trigger INSIDE that keeps its tooltip', () => {
    // The icon-only copy button ships inside two modals, and `data-tooltip` is
    // its only sighted name AND the carrier of its "Copied" ack.
    const { root, inner } = surfaceEl();

    disarm = armOutsideDismiss({ inside: [root], covers: [root], onDismiss: () => {} });

    hoverTrigger();
    expect(tooltipShown()).toBe(false);

    inner.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    expect(tooltipShown()).toBe(true);
    expect(document.querySelector('.moss-tooltip')?.textContent).toBe('Copied');

    root.remove();
  });

  it('every floating surface inherits the hold through this one seam', () => {
    // The seam is the point: `ctx-menu.ts`, `input-popover.ts`, the host's
    // overlays, its URL autocomplete and both deployment dropdowns arm here
    // and are covered without a line of their own.
    const { root } = surfaceEl();

    disarm = armOutsideDismiss({ inside: [root], covers: [root], onDismiss: () => {} });

    hoverTrigger();
    expect(tooltipShown()).toBe(false);

    disarm();
    disarm = null;
    hoverTrigger();
    expect(tooltipShown()).toBe(true);
    root.remove();
  });
});
