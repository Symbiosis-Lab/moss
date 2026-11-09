// Persistent context-adaptive editor toolbar — a glass pill at the bottom of
// the editor. Presence = focus, contents = caret context:
//
//   empty line             → insert page   (H · Media · List · Quote │ ⋯)
//   in text / selection    → format page   (h2 · quote · bold · italic · strike · code · link · comment)
//   ⋯                      → catalog page  (in-place morph to icon+label chips)
//
// The page depends only on whether the line is empty or text is selected, never
// on what sits under the caret: acting on a link, task, picture or comment stays
// on ⌘⏎, ⌘K, the hover and click cards, and a click on a task's brackets.
//
// Governing principle: stillness over motion. The pill's geometry never
// moves; contents cross-fade in place and the width glides. While prose is
// flowing the bar dims instead of leaving (video-player idle-fade).
//
// One catalog, two faces: chips render from SLASH_CATALOG and insert through
// the slash menu's machinery (zero-width trigger span at the caret), so the
// two surfaces can never drift. Format buttons call cm-format-keys'
// formatMark — the same engine as ⌘B.
//
// Asset-backed inserts (Media / Gallery / Hero) route through hooks.pickMedia
// — dialog + vault import stay in editor-main (file-first architecture); this
// module only dispatches the resulting text.

import { EditorView, ViewPlugin, keymap, type ViewUpdate } from '@codemirror/view';
import { type EditorState, Prec, type Extension } from '@codemirror/state';
import {
  SLASH_CATALOG,
  applyShortcodeTemplate,
  ts,
  type SlashCommandSpec,
} from './cm-shortcode-catalog';
import { slashMenuField } from './cm-slash-menu';
import { formatMark, toggleHeading, toggleBlockquote } from './cm-format-keys';
import type { FormatAction } from './selection-toolbar';
import { renderShortcodeAttrValue } from '@symbiosis-lab/moss-syntax';
import { embedFor, type DestinationFor } from './cm-drop';
import { baseName } from './path-domain';
import { t, onLocaleChange } from "../i18n/index";
import { editorBand } from './editor-band';
import type { MediaPickKind } from './media-pick-kind';

// ── Catalog split ─────────────────────────────────────────────────────────

/** Insert page. `media` is picker-driven (not a slash item — it replaces the
 * slash catalog's `img` with a file dialog). H is h2: the page title already
 * comes from the chip bar, so body headings start at "##". */
export const INSERT_BAR_ESSENTIALS: readonly string[] = ['h2', 'media', 'ul', 'bq'];

/** Catalog page, curated order: writing blocks first, then site blocks, then
 * technical. Excluded by design: `img` (Media covers it), `imath` (mid-line
 * construct), `h1`/`h3` and `task` (raw markdown / slash), `form` (slash only:
 * a request form posts nowhere until the site names its endpoint). */
export const INSERT_BAR_MORE: readonly string[] = [
  'ol', 'callout', 'hr', 'gallery', 'hero', 'grid', 'buttons',
  'subscribe', 'recent', 'code', 'math',
];

/** Chips that open the file picker instead of inserting a bare template. */
export type { MediaPickKind };
const PICKER_CMDS = new Set<string>(['media', 'gallery', 'hero']);

function catalogItem(name: string): SlashCommandSpec {
  const spec = SLASH_CATALOG.find((s) => s.name === name);
  if (!spec) throw new Error(`insert bar references unknown catalog item "${name}"`);
  return spec;
}

// ── Context classifier ────────────────────────────────────────────────────

export type InsertBarContext = 'insert' | 'format' | null;

/** Presence = focus (null hides the bar); contents = caret context. */
export function insertBarContext(state: EditorState, focused: boolean): InsertBarContext {
  if (!focused || state.readOnly) return null;
  if (state.field(slashMenuField, false)?.active) return null;
  const sel = state.selection.main;
  if (!sel.empty) return 'format';
  return state.doc.lineAt(sel.head).text.trim() === '' ? 'insert' : 'format';
}

// ── Insert helpers ────────────────────────────────────────────────────────

/** Insert through the slash-menu machinery with a zero-width trigger span:
 * there is no "/query" to clean up, so from === to === caret. */
export function insertAtCaret(view: EditorView, spec: SlashCommandSpec): void {
  const caret = view.state.selection.main.head;
  if (spec.kind === 'block') {
    spec.apply(view, caret, caret);
  } else {
    applyShortcodeTemplate(view, spec, caret, caret, { fencePrefix: true });
  }
  view.focus();
}

/** Gallery block from embed lines (`mediaEmbeds`), one per picked file. */
export function buildGalleryInsert(embeds: string[]): string {
  return ':::gallery {per-line=3}\n' + embeds.join('\n') + '\n:::';
}

/**
 * One embed line per picked project path, in the standard `![](path)` form
 * (embedFor), `![[name]]` where the backend refuses a destination.
 */
export async function mediaEmbeds(
  paths: string[],
  destinationFor: DestinationFor | undefined,
): Promise<string[]> {
  return Promise.all(paths.map((p) => embedFor(p, destinationFor)));
}

/** Hero block from the picker. Plain dispatch (not a CM6 snippet — snippet
 * syntax has no escape for `${…}` in filenames); "Title" is selected
 * manually so typing replaces it immediately.
 *
 * One file keeps the `image=` attribute form, rendered by the shared
 * `renderShortcodeAttrValue` (the attr grammar's bareword set is ASCII-only,
 * so real picked names carrying spaces or CJK come back quoted, with `"` and
 * `\` escaped as `read_quoted` in `attrs.rs` expects).
 * Several files use the body-lines grammar — consecutive leading media
 * lines are the hero's crossfading slides (videos become ambient loops). */
export function insertHeroAtCaret(view: EditorView, names: string[], embeds: string[]): void {
  if (names.length === 0) return;
  const caret = view.state.selection.main.head;
  let prefix: string;
  if (names.length === 1) {
    prefix = `:::hero {image=${renderShortcodeAttrValue(names[0])}}\n# `;
  } else {
    prefix = `:::hero\n${embeds.join('\n')}\n# `;
  }
  const text = `${prefix}Title\nSubtitle\n:::`;
  view.dispatch({
    changes: { from: caret, to: caret, insert: text },
    selection: { anchor: caret + prefix.length, head: caret + prefix.length + 'Title'.length },
  });
  view.focus();
}

// ── Icons ─────────────────────────────────────────────────────────────────
// 24 viewBox, 2px stroke `currentColor` (render size set by editor.css) —
// provenance per icon checked against Gutenberg/Ghost source SVGs (design
// doc, icon studies).

const SV = (paths: string, sw = 2) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
const SF = (paths: string) =>
  `<svg viewBox="0 0 24 24" fill="currentColor" stroke="none">${paths}</svg>`;

const ICONS: Record<string, string> = {
  h2: SV('<path d="M6 12h12"/><path d="M6 20V4"/><path d="M18 20V4"/>'),
  media: SV('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="m3 17 5-5 4 4"/>')
    .replace('</svg>', '<path d="M14 7.5v5l4.5-2.5z" fill="currentColor" stroke="none"/></svg>'),
  ul: SV('<line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>'),
  bq: SF('<path d="M10.7 5.5C7.2 5.9 5 8.4 5 12v6.5h6.5V12H8.2c.2-2.1 1.3-3.3 3.1-3.7z"/><path d="M20.2 5.5c-3.5.4-5.7 2.9-5.7 6.5v6.5H21V12h-3.3c.2-2.1 1.3-3.3 3.1-3.7z"/>'),
  more: SF('<circle cx="12" cy="5" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="12" cy="19" r="1.6"/>'),
  back: SV('<polyline points="15 18 9 12 15 6"/>'),
  ol: SV('<line x1="10" y1="6" x2="21" y2="6"/><line x1="10" y1="12" x2="21" y2="12"/><line x1="10" y1="18" x2="21" y2="18"/><path d="M4 6h1v4"/><path d="M4 10h2"/><path d="M6 18H4c0-1 2-2 2-3s-1-1.5-2-1"/>'),
  hr: SV('<path d="M5 12h14"/>'),
  callout: SV('<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 9v4"/><path d="M7 15.5h.01"/><path d="M11 10h6"/><path d="M11 14h4"/>'),
  gallery: SV('<rect x="7" y="3" width="14" height="14" rx="2"/><path d="M3 7v12a2 2 0 0 0 2 2h12"/><circle cx="11.5" cy="7.5" r="1.25"/><path d="m21 13-3-3-6 6"/>'),
  hero: SV('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/>'),
  grid: SV('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M12 3v18"/>'),
  buttons: SV('<rect x="3" y="8.5" width="18" height="8" rx="4"/><path d="M8 12.5h8"/>'),
  subscribe: SV('<path d="M12 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v6"/><path d="m22 7-10 6L2 7"/><path d="M19 15v6"/><path d="M22 18h-6"/>'),
  recent: SV('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M7 8h.01"/><path d="M11 8h6"/><path d="M7 12h.01"/><path d="M11 12h6"/><path d="M7 16h.01"/><path d="M11 16h6"/>'),
  code: SV('<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>'),
  math: SV('<path d="M18 7V4H6l6 8-6 8h12v-3"/>'),
  // Format page — the retired ftbar's exact icons (editor.html), unchanged.
  bold: SV('<path d="M6 4h8a4 4 0 014 4 4 4 0 01-4 4H6z"/><path d="M6 12h9a4 4 0 014 4 4 4 0 01-4 4H6z"/>', 2.5),
  italic: SV('<line x1="19" y1="4" x2="10" y2="4"/><line x1="14" y1="20" x2="5" y2="20"/><line x1="15" y1="4" x2="9" y2="20"/>', 2.5),
  strike: SV('<path d="M16 4H9a3 3 0 000 6h6a3 3 0 010 6H8"/><line x1="4" y1="12" x2="20" y2="12"/>', 2.5),
  fmtcode: SV('<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>'),
  link: SV('<path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/>'),
  comment: SV('<path d="M21 11.5a8.38 8.38 0 0 1-8.5 8.5 8.5 8.5 0 0 1-3.8-.9L3 21l1.9-5.7A8.38 8.38 0 0 1 4 11.5 8.5 8.5 0 0 1 12.5 3 8.38 8.38 0 0 1 21 11.5z"/>'),
};

/** `kbd` mirrors the live bindings in cm-format-keys.ts (formatKeysExtension)
 * and cm-comment.ts (obsidian comment keymap) — the buttons dispatch the same
 * engines, so tooltip chips teach the fast path. Rendered in macOS symbol form
 * (⌘/⇧/⌥): there is no shared platform helper in the codebase and macOS is
 * the primary target; CM6's "Mod-" maps to Ctrl elsewhere. Verify against the
 * keymaps before changing a chip — a wrong chip is worse than none. */
export const FORMAT_ACTIONS: readonly { fmt: string; icon: string; kbd: string; labelKey: 'toolbar.bold' | 'toolbar.italic' | 'toolbar.strikethrough' | 'toolbar.code' | 'toolbar.link' | 'toolbar.comment' | 'toolbar.insert_heading' | 'toolbar.insert_quote' }[] = [
  { fmt: 'heading', icon: 'h2', kbd: '⌘⌥2', labelKey: 'toolbar.insert_heading' },   // Mod-Alt-2
  { fmt: 'quote', icon: 'bq', kbd: '⌘⇧9', labelKey: 'toolbar.insert_quote' },       // Mod-Shift-9
  { fmt: 'bold', icon: 'bold', kbd: '⌘B', labelKey: 'toolbar.bold' },               // Mod-b
  { fmt: 'italic', icon: 'italic', kbd: '⌘I', labelKey: 'toolbar.italic' },         // Mod-i
  { fmt: 'strike', icon: 'strike', kbd: '⌘⇧S', labelKey: 'toolbar.strikethrough' }, // Mod-Shift-s
  { fmt: 'code', icon: 'fmtcode', kbd: '⌘~', labelKey: 'toolbar.code' },            // Mod-~
  { fmt: 'link', icon: 'link', kbd: '⌘K', labelKey: 'toolbar.link' },               // Mod-k
  { fmt: 'comment', icon: 'comment', kbd: '⌘⇧M', labelKey: 'toolbar.comment' },     // Mod-Shift-m
];

// ── Hooks (all I/O lives in the caller) ───────────────────────────────────

export interface InsertBarHooks {
  /** Open the system file picker (filters/multiplicity by kind), import the
   * files into the vault, and return their project-relative paths under the
   * collision-safe final names — or null/[] on cancel. */
  pickMedia(kind: MediaPickKind): Promise<string[] | null>;
  /** A picked file's destination from the page being edited (cm-drop.ts). */
  destinationFor?: DestinationFor;
  /** Comment action (selection-anchored) — same route the ftbar used. */
  onComment?(view: EditorView): void;
  /** Called after a picker-backed insert lands, so the caller can flush the
   * save immediately (one watcher rebuild sees asset + embed together). */
  afterMediaInsert?(): void | Promise<void>;
}

// ── DOM controller ────────────────────────────────────────────────────────

/** `.insert-bar-page` sits at `left: 5px` inside the pill (editor.css). It is
 * reserved TWICE: the page is inset from the left, and the pill has to end the
 * same distance past the page's right edge or the page hangs out of a box whose
 * `overflow: hidden` then scissors it. */
const PAGE_INSET = 5;
/** `.insert-bar-page`'s flex `gap`. The catalog page is lead + chips, so one. */
const PAGE_GAP = 1;
/** Room the chip list keeps before the trailing group starts shedding. The list
 * scrolls, so it can be narrow — but a catalog whose chips are a sliver is not a
 * catalog, and the hint that would be shed to widen it is worth less. */
const CHIPS_MIN = 96;
/** Exit fade is 160ms (editor.css); the hidden-page reset waits it out. */
const HIDE_RESET_MS = 240;

export type InsertBarPage = 'insert' | 'catalog' | 'format';

/** Exported for unit tests; production reaches it via insertBarExtension. */
export class InsertBar {
  readonly dom: HTMLElement;
  /** Resolves when an in-flight picker-backed insert settles (tests await it). */
  pendingInsert: Promise<void> | null = null;
  activePage: InsertBarPage = 'insert';

  private pill: HTMLElement;
  private pages!: Record<InsertBarPage, HTMLElement>;
  private visible = false;
  private destroyed = false;
  private hideResetTimer: number | null = null;
  private offLocale: () => void;

  constructor(private view: EditorView, private hooks: InsertBarHooks) {
    this.dom = document.createElement('div');
    this.dom.className = 'insert-bar';
    this.dom.setAttribute('aria-hidden', 'true');
    this.pill = document.createElement('div');
    this.pill.className = 'insert-bar-pill';
    this.dom.appendChild(this.pill);
    this.renderPages();

    // Keep editor focus and caret: no bar interaction may steal them — an
    // insert must land exactly where the caret sits.
    this.dom.addEventListener('mousedown', (e) => e.preventDefault());
    this.dom.addEventListener('click', (e) => this.onClick(e));

    this.offLocale = onLocaleChange(() => this.renderPages());
    // Anchored inside the CM pane, not on document.body. #moss-editor-cm is a
    // flex child of the 100vh editor column and .create-status is the column's
    // last in-flow child, so the pane ALREADY shrinks by exactly the status
    // line's height when a create session starts. Riding that costs no measuring
    // and no CSS var, and stays right when the status line wraps to two lines —
    // which a fixed viewport-anchored bar could not (it overlapped the status
    // line outright). Falls back to body so jsdom and any future host still work.
    const band = editorBand();
    band.appendChild(this.dom);
    // Constructor-time renderPages measured a detached DOM (width 0, bailed);
    // measure for real now that the bar is attached.
    this.fitWidth(false);
    window.addEventListener('resize', this.onResize);
    // The grid divides the row, but the pill's own width is an inline pixel
    // value — so it has to be re-measured whenever the row's OTHER occupant
    // arrives or leaves. The cluster mounts lazily, on the session's first
    // document edit, and nothing about that reaches this class: the wide
    // catalog page kept the width it had computed when the left column was
    // empty and hung 72px (one `--band-cluster-min`) past the pane.
    //
    // Watching the band, rather than being told, is what keeps the two boxes
    // from negotiating again: this asks the DOM a question about the row it is
    // in, instead of reinstating a protocol between two siblings.
    this.bandObserver = new MutationObserver(() => this.fitWidth(false));
    this.bandObserver.observe(band, { childList: true });
  }

  private bandObserver: MutationObserver | null = null;

  private onResize = () => this.fitWidth(false);

  private renderPages(): void {
    const label = (name: string): string =>
      name === 'h2' ? t('toolbar.insert_heading')
      : name === 'media' ? t('toolbar.insert_media')
      : name === 'ul' ? t('toolbar.insert_list')
      : t('toolbar.insert_quote');
    const essentials = INSERT_BAR_ESSENTIALS.map((name) =>
      `<button data-cmd="${name}" tabindex="-1" aria-label="${label(name)}" data-tooltip-position="top" data-tooltip="${label(name)}">${ICONS[name]}</button>`
    ).join('');
    const chips = INSERT_BAR_MORE.map((name) =>
      `<button class="insert-bar-chip" data-cmd="${name}" tabindex="-1" data-tooltip-position="top" data-tooltip="${ts(catalogItem(name).detailKey)}">${ICONS[name]}<span>${ts(catalogItem(name).labelKey)}</span></button>`
    ).join('');
    const formats = FORMAT_ACTIONS.map(({ fmt, icon, kbd, labelKey }) =>
      `<button data-fmt="${fmt}" tabindex="-1" aria-label="${t(labelKey)}" data-tooltip-position="top" data-tooltip="${t(labelKey)}" data-tooltip-kbd="${kbd}">${ICONS[icon]}</button>`
    ).join('');

    this.pill.innerHTML =
      `<div class="insert-bar-page insert-bar-page--insert">${essentials}` +
      `<div class="insert-bar-sep"></div>` +
      // The `/` chip is where the fast path is taught now: the catalog row is
      // for buttons, and a sentence sitting in it cost more room than the
      // lesson was worth. A tooltip at the moment of intent costs none.
      `<button class="insert-bar-more" tabindex="-1" aria-label="${t('toolbar.insert_more')}" data-tooltip-position="top" data-tooltip="${t('toolbar.insert_more')}" data-tooltip-kbd="/">${ICONS.more}</button></div>` +
      `<div class="insert-bar-page insert-bar-page--catalog">` +
      // The lead is a WRAPPER, not decoration: fitWidth measures it to work out
      // how much room the scrolling chip list may have. Summing the children
      // instead would undercount — .insert-bar-sep is 1px wide with 4px margins
      // either side, so its offsetWidth is 1 while it consumes 9, which is how
      // the old hardcoded reserve came to be ~10px short per separator and let
      // the row's end fall outside the pill's overflow:hidden (the half-drawn
      // divider bug).
      `<div class="insert-bar-lead">` +
      `<button class="insert-bar-back" tabindex="-1" aria-label="${t('toolbar.insert_back')}" data-tooltip-position="top" data-tooltip="${t('toolbar.insert_back')}">${ICONS.back}</button>` +
      `<div class="insert-bar-sep"></div>` +
      `</div>` +
      `<div class="insert-bar-chips">${chips}</div></div>` +
      `<div class="insert-bar-page insert-bar-page--format">${formats}</div>`;

    this.pages = {
      insert: this.pill.querySelector('.insert-bar-page--insert') as HTMLElement,
      catalog: this.pill.querySelector('.insert-bar-page--catalog') as HTMLElement,
      format: this.pill.querySelector('.insert-bar-page--format') as HTMLElement,
    };
    this.pages[this.activePage].classList.add('active');
    this.fitWidth(false);
  }

  private onClick(e: MouseEvent): void {
    const btn = (e.target as HTMLElement).closest('button');
    if (!btn || !this.dom.contains(btn)) return;
    if (btn.classList.contains('insert-bar-more')) return this.openMore();
    if (btn.classList.contains('insert-bar-back')) return this.closeMore();
    const fmt = btn.getAttribute('data-fmt');
    if (fmt) {
      if (fmt === 'comment') this.hooks.onComment?.(this.view);
      else if (fmt === 'heading') toggleHeading(this.view);
      else if (fmt === 'quote') toggleBlockquote(this.view);
      else formatMark(this.view, fmt as FormatAction);
      return;
    }
    const cmd = btn.getAttribute('data-cmd');
    if (!cmd) return;
    if (PICKER_CMDS.has(cmd)) {
      this.pendingInsert = this.pickAndInsert(cmd as MediaPickKind).catch((err) => {
        console.error('insert-bar media pick failed', err);
      });
      return;
    }
    insertAtCaret(this.view, catalogItem(cmd));
  }

  private async pickAndInsert(kind: MediaPickKind): Promise<void> {
    const paths = await this.hooks.pickMedia(kind);
    if (!paths || paths.length === 0) return;
    // Every media line is the standard form, written as the path from the page.
    const names = paths.map(baseName);
    const embeds = await mediaEmbeds(kind === 'media' ? paths.slice(0, 1) : paths, this.hooks.destinationFor);
    // The editor may have been remounted while the native dialog was open
    // (e.g. a CLI-routed folder switch): dispatching into a destroyed view is
    // a silent no-op, so bail before inserting against the wrong document.
    if (this.destroyed) return;
    const view = this.view;
    if (kind === 'hero') {
      insertHeroAtCaret(view, names, embeds);
    } else {
      const caret = view.state.selection.main.head;
      const text = kind === 'media' ? embeds[0] : buildGalleryInsert(embeds);
      view.dispatch({
        changes: { from: caret, to: caret, insert: text },
        selection: { anchor: caret + text.length },
      });
      view.focus();
    }
    await this.hooks.afterMediaInsert?.();
  }

  /** Recompute presence + page from the view. `focusedOverride` exists for
   * tests (jsdom focus is unreliable); production passes nothing. */
  sync(focusedOverride?: boolean): void {
    const focused = focusedOverride ?? this.view.hasFocus;
    const ctx = insertBarContext(this.view.state, focused);
    if (ctx === null) {
      this.setVisible(false);
      return;
    }
    // A page correction at the moment of (re)appearing must SNAP — animating
    // it plays a through-morph under the entrance fade (the bar must appear
    // already wearing the right mode, per 刘果's report).
    const wasHidden = !this.visible;
    // The open catalog survives insert-context syncs while visible — the
    // author is deciding. It never survives a hide (any hide resets).
    if (!(ctx === 'insert' && this.activePage === 'catalog' && !wasHidden)) {
      this.setPage(ctx, { animate: !wasHidden });
    }
    this.setVisible(true);
  }

  private setVisible(v: boolean): void {
    if (v === this.visible) return;
    this.visible = v;
    this.dom.classList.toggle('visible', v);
    this.dom.setAttribute('aria-hidden', String(!v));
    if (v) {
      this.cancelHideReset();
      this.fitWidth(false);
    } else {
      // Defer the reset-to-insert until the exit fade has finished — an
      // immediate swap plays visibly under the fading bar.
      this.scheduleHideReset();
    }
  }

  private scheduleHideReset(): void {
    this.cancelHideReset();
    this.hideResetTimer = window.setTimeout(() => {
      this.hideResetTimer = null;
      if (!this.visible) this.setPage('insert', { animate: false });
    }, HIDE_RESET_MS);
  }

  private cancelHideReset(): void {
    if (this.hideResetTimer !== null) {
      window.clearTimeout(this.hideResetTimer);
      this.hideResetTimer = null;
    }
  }

  openMore(): void { this.setPage('catalog'); }
  closeMore(): void { this.setPage('insert'); }

  private setPage(name: InsertBarPage, opts: { animate?: boolean } = {}): void {
    if (name === this.activePage) return;
    const animate = opts.animate ?? true;
    this.activePage = name;
    const els = [this.pill, this.pages.insert, this.pages.catalog, this.pages.format];
    // Snap: suppress transitions across the swap (same forced-reflow pattern
    // as fitWidth's animate=false) so a hidden/entering bar changes pages
    // with no cross-fade or width glide.
    if (!animate) for (const el of els) el.style.transition = 'none';
    for (const k of ['insert', 'catalog', 'format'] as const) {
      this.pages[k].classList.toggle('active', k === name);
    }
    if (!animate) {
      void this.pill.offsetWidth;
      for (const el of els) el.style.transition = '';
    }
    this.fitWidth(animate);
  }

  /** Morph the pill width to the active page (FLIP-free: explicit px both
   * sides of a CSS width transition). Layout reads are routed through CM6's
   * requestMeasure — sync() runs inside plugin update(), where a raw
   * scrollWidth read would force a synchronous reflow mid-update (the same
   * discipline the retired selection toolbar documented). jsdom measures 0
   * and keeps width auto. The pages are absolutely positioned, so an unset
   * width collapses the pill — fitWidth therefore runs on attach and on
   * every show, not only on page changes. */
  private fitWidth(animate: boolean): void {
    this.view.requestMeasure({
      read: () => {
        // The band is the grid; the pill's column is what is left after the
        // cluster's track and the two column gaps. Reading the CSS rather than
        // restating it is the point of the move: `avail` is now ONE number
        // derived from the same declaration the browser is laying out with,
        // where it used to be four (`edge`, `symAvail`, `asymAvail`,
        // `leftStart`) computed here from constants mirroring that CSS by hand.
        //
        // The cluster is charged at `--band-cluster-min`, the floor its track
        // declares, not at its current width — and that is what keeps this
        // non-circular. The floor does not move when the badge text changes, so
        // the pill's width cannot feed back into it. Charging the current width
        // would make the two chase each other: a wider pill squeezes the
        // cluster, which frees room, which widens the pill.
        const band = this.dom.parentElement;
        const host = band?.clientWidth ?? 0;
        const page = this.pages[this.activePage];
        const cs = band ? getComputedStyle(band) : null;
        const gap = cs ? parseFloat(cs.columnGap) || 0 : 0;
        // Only when a cluster is actually there, which is also exactly when the
        // CSS floors that track — the floor is the cluster's own `min-width`,
        // read through the left track's `minmax(auto, …)`, so it exists if and
        // only if the element does. Reserving for an absent one is how the pill
        // ends up narrower than the room it has.
        const clusterMin =
          cs && band!.querySelector('.doc-state-cluster')
            ? parseFloat(cs.getPropertyValue('--band-cluster-min')) || 0
            : 0;
        // Both gaps, always: the counterweight track is empty but the grid
        // still puts a gap on each side of the pill's column.
        const avail = Math.max(0, host - 2 * gap - clusterMin);

        const cat = this.pages.catalog;
        const leadEl = cat.querySelector('.insert-bar-lead') as HTMLElement | null;
        const lead = leadEl?.getBoundingClientRect().width ?? 0;
        // lead reads 0 while detached (constructor) and in jsdom; writing a
        // max-width from a 0 reserve is what produced the over-wide catalog
        // page in the first place, so require a real measurement.
        if (!(host > 0 && lead > 0)) {
          return { host, avail, w: page.scrollWidth, budget: null };
        }
        // Everything on the row that is not the chip list: the page's inset at
        // BOTH ends, its one flex gap, and the lead.
        const budget = Math.max(0, avail - (2 * PAGE_INSET + PAGE_GAP + lead));
        // The catalog's width is COMPUTED, not measured. `budget` is applied to
        // the chip list in the write phase, so the page's `scrollWidth` here
        // still describes the previous one — and sizing the pill from it while
        // the chips get the new budget leaves the two permanently disagreeing,
        // with the list wider than the box whose `overflow: hidden` then
        // scissors it. Every term is already in hand, so ask for none of it
        // back from the layout.
        const chipsEl = cat.querySelector('.insert-bar-chips') as HTMLElement | null;
        const w =
          page === cat && chipsEl
            ? lead + PAGE_GAP + Math.min(chipsEl.scrollWidth, budget)
            : page.scrollWidth;
        return { host, avail, w, budget };
      },
      write: ({ host, avail, w, budget }) => {
        if (budget !== null) {
          const chips = this.pages.catalog.querySelector('.insert-bar-chips') as HTMLElement | null;
          if (chips) chips.style.maxWidth = `${budget}px`;
        }
        if (w <= 0) return;
        // The whole of the width rule. WHERE the pill then stands is the grid's
        // business — it was `--pill-dx`, a clamped distance from the pane's
        // centre to the centre of the room beside the cluster, and it is now
        // two `1fr` tracks splitting the leftover.
        const natural = w + 2 * PAGE_INSET;
        const target = host > 0 ? Math.min(natural, avail) : natural;
        if (!animate) {
          const prev = this.pill.style.transition;
          this.pill.style.transition = 'none';
          this.pill.style.width = `${target}px`;
          void this.pill.offsetWidth;
          this.pill.style.transition = prev;
        } else {
          this.pill.style.width = `${target}px`;
        }
      },
    });
  }

  /** Escape while the catalog is open returns to the insert page (bound at
   * high precedence in the extension keymap). */
  handleEscape(): boolean {
    if (this.activePage !== 'catalog') return false;
    this.closeMore();
    return true;
  }

  destroy(): void {
    this.destroyed = true;
    window.removeEventListener('resize', this.onResize);
    this.bandObserver?.disconnect();
    this.bandObserver = null;
    this.cancelHideReset();
    this.offLocale();
    this.dom.remove();
  }
}

// ── Extension ─────────────────────────────────────────────────────────────

export function insertBarExtension(hooks: InsertBarHooks): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      bar: InsertBar;
      constructor(view: EditorView) {
        this.bar = new InsertBar(view, hooks);
        this.bar.sync();
      }
      update(u: ViewUpdate) {
        // The classifier is O(1); recompute on every update (doc, selection,
        // focus, and slash-menu state changes all matter).
        this.bar.sync();
      }
      destroy() {
        this.bar.destroy();
      }
    },
  );
  return [
    plugin,
    Prec.high(
      keymap.of([
        {
          key: 'Escape',
          run: (view) => view.plugin(plugin)?.bar.handleEscape() ?? false,
        },
      ]),
    ),
  ];
}
