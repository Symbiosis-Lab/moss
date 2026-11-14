// The editor webview's two field instruments. Both exist for the same reason:
// the symptom the user reports — "it jumped", "it went dead" — arrives with no
// error attached to it, so unless something writes the fact down at the moment
// it happens moss.log is silent and the report is unactionable. They watch the
// same pane and the same anchor elements, which is why they share a file.
//
//   initLayoutDiag()            a layout-shift detector. Gated on diagEnabled().
//   installEditorFocusProbe()   a click that fails to place the caret. ALWAYS
//                               ON — it is the only instrument that would catch
//                               a covered editor in the field.
//
// ── Layout shift ────────────────────────────────────────────────────────
//
// WKWebView-safe layout-shift detector for the editor webview.
//
// WebKit (macOS WKWebView) does NOT implement the Chromium Layout Instability
// API (`layout-shift`) or Long Tasks API (`longtask`), so we cannot rely on
// PerformanceObserver for CLS. Instead we watch a few STRUCTURAL anchor
// elements with a ResizeObserver — which fires only on real size changes, no
// polling — and report when a stable anchor's top/left *position* jumps. That
// captures the visible "content jumped" shift the user reports.
//
// Each shift is tagged `recentInput` so expected typing/caret reflow (the
// live-preview marker reveal on cursor move) can be told apart from an
// unprompted jump — the latter is the real bug and is always logged; the former
// is rate-limited and counted.
//
// The native observers are attached too, feature-detected, as a zero-cost bonus
// if the webview ever supports them (Chromium does; WKWebView silently no-ops).

import type { EditorView } from '@codemirror/view';

import { diagWarn, diagRateLimited, rateGate, count, diagEnabled } from './diag';

interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

// Structural anchors whose *position* should be stable during editing. A jump
// in any of their top/left means something above/around them reflowed.
const ANCHOR_SELECTORS = [
  '.cm-editor',
  '.cm-scroller',
  '.cm-content',
  '#editor-heading',
  '#chip-track',
  '.chip-bar',
];

// A position jump under this many px is treated as noise (subpixel jitter).
const POSITION_EPS_PX = 1.5;
// Within this window after a keydown/pointerdown, a shift is "input-driven".
const INPUT_WINDOW_MS = 400;

let lastInput = -Infinity;
const lastRect = new Map<string, Rect>();
let observedEditor: Element | null = null;

function rectOf(el: Element): Rect {
  const r = el.getBoundingClientRect();
  return { top: r.top, left: r.left, width: r.width, height: r.height };
}

function anchors(): { name: string; el: Element }[] {
  const out: { name: string; el: Element }[] = [];
  for (const sel of ANCHOR_SELECTORS) {
    const el = document.querySelector(sel);
    if (el) out.push({ name: sel, el });
  }
  return out;
}

// Compare every anchor to its last-seen rect; log position jumps. Called from
// the ResizeObserver callback, where layout is already clean (reading rects
// there does not force an extra reflow).
function check(trigger: string): void {
  const now = performance.now();
  const recentInput = now - lastInput < INPUT_WINDOW_MS;
  for (const { name, el } of anchors()) {
    const cur = rectOf(el);
    const prev = lastRect.get(name);
    lastRect.set(name, cur);
    if (!prev) continue;
    const dTop = cur.top - prev.top;
    const dLeft = cur.left - prev.left;
    const dHeight = cur.height - prev.height;
    // Only POSITION jumps count as a shift. Pure height growth (a document
    // getting longer as you type) is not a shift of a stable anchor.
    if (Math.abs(dTop) <= POSITION_EPS_PX && Math.abs(dLeft) <= POSITION_EPS_PX) {
      continue;
    }
    count(recentInput ? 'layout.shift.typing' : 'layout.shift.unprompted');
    const line =
      `${name} moved Δtop=${dTop.toFixed(1)}px Δleft=${dLeft.toFixed(1)}px ` +
      `(Δheight=${dHeight.toFixed(1)}px) trigger=${trigger} recentInput=${recentInput}`;
    if (recentInput) {
      // Expected reflow — collapse the per-keystroke flood.
      diagRateLimited('layout', `typing:${name}`, line, 1500);
    } else {
      // Unprompted jump — the reported bug. Always log.
      diagWarn('layout', line);
    }
  }
}

export function initLayoutDiag(): void {
  if (!diagEnabled()) return;
  // Defense-in-depth: skip in any environment without the DOM observers this
  // relies on (unit tests, minimal webviews) rather than throwing at import.
  if (typeof ResizeObserver === 'undefined' || typeof MutationObserver === 'undefined') return;

  const ro = new ResizeObserver((entries) => {
    const which = entries
      .map((e) => {
        const t = e.target as Element;
        return t.className ? `.${String(t.className).split(' ')[0]}` : (t.id ? `#${t.id}` : t.tagName);
      })
      .join(',');
    check(`resize:${which}`);
  });

  const observeAnchors = () => {
    for (const { name, el } of anchors()) {
      lastRect.set(name, rectOf(el));
      ro.observe(el); // re-observing an already-observed element is a no-op
    }
  };

  // Prime + observe once the initial layout settles.
  setTimeout(observeAnchors, 600);

  // Track user input to classify shifts.
  const markInput = () => {
    lastInput = performance.now();
  };
  document.addEventListener('keydown', markInput, { capture: true, passive: true });
  document.addEventListener('pointerdown', markInput, { capture: true, passive: true });

  // CM6 destroys + recreates `.cm-editor` on file switch, which drops our
  // observations. Re-observe ONLY when the `.cm-editor` element identity
  // actually changes — the callback stays a cheap querySelector + ref compare
  // even under the heavy per-keystroke DOM churn, so it adds no measurable cost.
  const remount = new MutationObserver(() => {
    const cm = document.querySelector('.cm-editor');
    if (cm !== observedEditor) {
      observedEditor = cm;
      observeAnchors();
    }
  });
  // Guard: observing a null body throws and would break editor init. Module is a
  // deferred ES module so body normally exists, but stay defensive.
  const startRemountObserver = () => {
    if (document.body) remount.observe(document.body, { childList: true, subtree: true });
  };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', startRemountObserver, { once: true });
  } else {
    startRemountObserver();
  }

  // Bonus: native APIs if ever supported. WKWebView throws on the unknown
  // entry type and this silently no-ops.
  try {
    const po = new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as unknown as Array<{
        value?: number;
        hadRecentInput?: boolean;
        sources?: Array<{ node?: Node }>;
      }>) {
        if (entry.hadRecentInput) continue;
        const nodes = (entry.sources ?? [])
          .map((s) => (s.node as Element | undefined)?.nodeName ?? '?')
          .join(',');
        diagWarn('layout', `native layout-shift value=${entry.value?.toFixed?.(4)} sources=${nodes}`);
      }
    });
    po.observe({ type: 'layout-shift', buffered: true } as PerformanceObserverInit);
  } catch {
    /* Layout Instability API unsupported (WKWebView) — manual detector covers it */
  }
  try {
    const lt = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        count('longtask');
        if (entry.duration >= 100) diagWarn('energy', `longtask ${Math.round(entry.duration)}ms`);
      }
    });
    lt.observe({ type: 'longtask', buffered: true } as PerformanceObserverInit);
  } catch {
    /* Long Tasks API unsupported (WKWebView) */
  }
}

// ── Focus probe ─────────────────────────────────────────────────────────
//
// The instrument that catches "the editor looks dead and nothing is in the log":
// a click on the editor's text surface that does not place the cursor. The
// symptom has no error attached to it — the user clicks, nothing happens, and
// moss.log is silent — so the probe writes the one fact that tells the causes
// apart: which element actually received the click.
//
// WHY IT NEEDS A CLASSIFIER AND NOT JUST A `hasFocus` CHECK
//
// The editor pane is not only the document. `#editor-heading` (the inline
// title) is re-parented INTO `.cm-scroller` at mount, and the bottom band —
// undo, redo, the save badge, the insert-bar pill — is appended to the pane.
// Clicking any of those is a real, intended gesture that correctly leaves the
// CodeMirror view unfocused. A probe that only asks "did the view take focus"
// reports every one of them as a stuck editor.
//
// That is not hypothetical: the probe fired eight times in ninety seconds,
// naming `editor-heading`, `doc-state-btn` and a
// bare `<svg>` (the undo icon). All three were the user typing a title and
// pressing undo — the editor was fine.
//
// So the probe classifies the element under the pointer before it speaks.

/**
 * Chrome that lives inside the editor pane and legitimately absorbs a click
 * without moving the caret. Every entry is a real target the user meant to hit.
 *
 * Each entry is matched with `closest()`, which walks ALL the way to `<html>` —
 * so an unbounded entry silences the probe for the whole webview, not just for
 * the control it names. The two entries that could are therefore scoped: an
 * ancestor of the pane carrying `contenteditable="false"` (or an overlay that is
 * one, or sits inside a `button`) would otherwise turn the instrument off
 * wholesale, which is the failure mode a mute instrument cannot report.
 */
const CHROME_SELECTOR = [
  '#editor-heading', // the inline title — its own contenteditable
  '.editor-band', // undo / redo / save badge / insert-bar pill
  '.cm-gutters', // comment + fold gutter
  '.cm-panels', // find bar
  '.cm-tooltip', // completions, slash menu, lint
  '#chip-bar',
  // Controls, bounded to the editor webview's own root: a `<button>` ABOVE
  // `.moss-editor` is a page the editor was embedded into, not chrome the user
  // aimed at.
  '.moss-editor button, .moss-editor a, .moss-editor input, .moss-editor textarea, .moss-editor select',
  // CM6 marks atomic block widgets non-editable; clicking one selects the
  // widget rather than placing a caret, which is correct behaviour. Bounded to
  // the document itself — that is the only place those widgets live.
  '.cm-content [contenteditable="false"], .cm-widgetBuffer',
].join(',');

export type FocusMissVerdict =
  /** A real control took the click. Nothing is wrong; stay silent. */
  | 'chrome'
  /** The click landed on the text surface and the caret still did not move. */
  | 'stuck'
  /**
   * Something unrecognized is sitting over the editor and ate the click — a
   * plugin panel, a modal, an overlay left behind by a surface that closed.
   * The catch-all: by definition nothing in the app is known to produce it,
   * which is why it is worth a log line.
   */
  | 'covered'
  /** Nothing under the pointer (outside the viewport). Nothing to report. */
  | 'nothing';

/**
 * Decide what a focus miss means, given the element the engine says was under
 * the pointer. Pure: no layout, no timers — the hit-testing itself is the
 * browser's job and is exercised by the render gate.
 */
export function classifyFocusMiss(hit: Element | null, view: EditorView): FocusMissVerdict {
  if (!hit) return 'nothing';
  if (hit.closest(CHROME_SELECTOR)) return 'chrome';
  if (hit === view.contentDOM || view.contentDOM.contains(hit)) return 'stuck';
  // The scroller and the editor root are the text surface's own boxes: CM6
  // handles a click on either by placing the cursor, so a miss there is real.
  if (hit === view.scrollDOM || hit === view.dom) return 'stuck';
  return 'covered';
}

/** A stable, greppable name for the element in the log line. */
export function describeElement(el: Element): string {
  const id = el.id ? `#${el.id}` : '';
  // SVG elements carry an SVGAnimatedString here, not a string.
  const raw = typeof el.className === 'string' ? el.className.trim() : '';
  const cls = raw ? `.${raw.split(/\s+/).join('.')}` : '';
  return `${el.tagName.toLowerCase()}${id}${cls}`;
}

/**
 * A stuck editor is stuck for as long as the user keeps clicking it, so the
 * report is a flood by nature: the field upload this probe was rewritten for
 * held eight lines in ninety seconds, four of them in one second. One line per
 * verdict per window, carrying the suppressed count, says the same thing.
 */
const REPORT_EVERY_MS = 5000;

/**
 * Watch for clicks on `container` that fail to place the cursor, and log the
 * ones that mean something.
 *
 * Fires ONLY when a real CodeMirror document is mounted AND editable — a
 * legitimately read-only/sealed editor not focusing on click is expected.
 *
 * The listener is never removed: `container` is the editor webview's own pane,
 * which outlives everything that could tear it down.
 */
export function installEditorFocusProbe(
  container: HTMLElement,
  getView: () => EditorView | null | undefined,
  log: (message: string) => void,
): void {
  const onPointerDown = (ev: PointerEvent): void => {
    const view = getView();
    if (!view || view.state.readOnly) return; // no CM6 doc, or legitimately sealed
    const { clientX: x, clientY: y } = ev;
    // Let the click settle, then check whether the cursor actually landed.
    setTimeout(() => {
      if (view.hasFocus) return; // healthy — the editor took focus
      const hit = document.elementFromPoint(x, y);
      const verdict = classifyFocusMiss(hit, view);
      if (verdict === 'chrome' || verdict === 'nothing') return;
      const skipped = rateGate(`editor.focus.${verdict}`, REPORT_EVERY_MS);
      if (skipped === null) return;
      const extra = skipped > 0 ? ` (+${skipped} similar suppressed)` : '';
      const where = hit ? describeElement(hit) : '(none)';
      log(
        (verdict === 'covered'
          ? `[editor] click did NOT place the cursor — "${where}" is covering the editor ` +
            `and swallowed the click (editor is editable; an unrecognized overlay is on top of it)`
          : `[editor] click did NOT place the cursor — it landed on the text surface ` +
            `("${where}") and the editor is editable, so the caret is stuck`) + extra,
      );
    }, 60);
  };
  container.addEventListener('pointerdown', onPointerDown, true);
}
