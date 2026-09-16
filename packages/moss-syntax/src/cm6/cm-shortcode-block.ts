/**
 * Block-decoration StateField for shortcode blocks. Reads the SYNCHRONOUS Lezer
 * syntax tree and pairs Open↔Close fence markers with an arity stack, so
 * positions are always current for tr.state — no stale offsets, no glitches.
 *
 * Resting (cursor outside block): open fence → micro-tag widget (one-line-tall,
 * click-to-focus); body lines tinted (live-preview renders their inline
 * markdown); close fence → faint rule. Active (selection head within the paired
 * range): all lines raw for editing. Editing a line ABOVE the block does not
 * change range-overlap, so the block never toggles.
 *
 * Large-doc caveat: syntaxTree(state) may be incomplete past the viewport for
 * very long docs. The parse worker extends the tree via effect-only
 * transactions; both fields rebuild on that advance (see `treeAdvanced`), so
 * late-parsed blocks decorate as soon as the parse reaches them. We do not
 * assert whole-doc completeness at any single point in time.
 */

import { EditorView, Decoration, WidgetType } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { EditorState, StateField, RangeSet, RangeSetBuilder, type Extension, type StateEffectType, type Transaction } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { SHORTCODE_OPEN_RE, isOpenMatch, parseAttrKvSpans, shortcodeAssetRef } from '../shortcode.js';
import { revealInputsChanged } from './cm-source-mode.js';
import { nodeTouchesSelection } from './cm-active-lines.js';

// ── Host strings seam ─────────────────────────────────────────────────────────
// This module used to call moss's `t()` directly; the package must not know
// about any host's i18n. Each string is a THUNK, not a value, because the old
// code called `t()` at widget-DOM time (`toDOM`) — locale-reactive: a locale
// switch re-renders with fresh text. Hosts that want that pass `() => t('…')`;
// hosts that don't pass nothing and get the English defaults.
export interface ShortcodeBlockStrings {
  /** Tooltip on a resting tag whose named asset does not resolve. */
  assetMissingHint: () => string;
  /** Inline label after a deprecated `---` cell divider. */
  legacyDividerLabel: () => string;
  /** Tooltip on that label. */
  legacyDividerTooltip: () => string;
}

const DEFAULT_STRINGS: ShortcodeBlockStrings = {
  assetMissingHint: () =>
    "This file isn't in your folder. moss won't publish the site until it is.",
  legacyDividerLabel: () => 'old divider — use +++',
  legacyDividerTooltip: () =>
    'The grid still splits here, but --- is the old way to write a cell divider. Replace it with +++.',
};

export interface ShortcodeBlockInfo {
  from: number;       // start of the open-fence line
  to: number;         // end of the close-fence line
  name: string;
  attrs: string;
  closeFrom: number;  // start of the close-fence line
  arity: number;      // colon count of the fence (3 for `:::`, 4 for `::::`, …)
  children: ShortcodeBlockInfo[]; // nested blocks (e.g. `::::buttons` in `:::grid`)
}

/**
 * Pair Open↔Close fence markers with an arity stack. Returns the TOP-LEVEL
 * blocks; each carries its nested `children` so the decoration layer can render
 * `::::buttons` inside `:::grid` instead of leaking the inner fences as raw
 * text. (Nesting parity with the Rust extractor — see cm-shortcode-scanner.)
 */
export function collectShortcodeBlocks(state: EditorState): ShortcodeBlockInfo[] {
  interface OpenMarker { from: number; arity: number; name: string; attrs: string; children: ShortcodeBlockInfo[]; }
  const stack: OpenMarker[] = [];
  const out: ShortcodeBlockInfo[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === 'ShortcodeOpenLine') {
        const lineFrom = state.doc.lineAt(node.from).from;
        const m = SHORTCODE_OPEN_RE.exec(state.doc.sliceString(lineFrom, node.to));
        const arity = m ? m[2].length : 3;
        let name = '', attrs = '';
        const c = node.node.cursor();
        if (c.firstChild()) {
          do {
            if (c.name === 'ShortcodeName') name = state.doc.sliceString(c.from, c.to);
            else if (c.name === 'ShortcodeAttrs') attrs = state.doc.sliceString(c.from, c.to).trim();
          } while (c.nextSibling());
        }
        stack.push({ from: lineFrom, arity, name, attrs, children: [] });
      } else if (node.name === 'ShortcodeCloseLine') {
        const arity = state.doc.sliceString(node.from, node.to).trim().length;
        for (let i = stack.length - 1; i >= 0; i--) {
          if (stack[i].arity === arity) {
            // splice from i: take the matched open and discard any unclosed inner
            // opens above it (malformed nesting → never half-rendered).
            const open = stack.splice(i)[0];
            const block: ShortcodeBlockInfo = {
              from: open.from, to: node.to, name: open.name, attrs: open.attrs,
              closeFrom: node.from, arity: open.arity, children: open.children,
            };
            if (stack.length > 0) stack[stack.length - 1].children.push(block);
            else out.push(block);
            break;
          }
        }
      }
    },
  });
  return out;
}

/** Flatten a block tree to a document-ordered list (parent before children). */
export function flattenBlocks(blocks: ShortcodeBlockInfo[]): ShortcodeBlockInfo[] {
  const out: ShortcodeBlockInfo[] = [];
  const walk = (b: ShortcodeBlockInfo) => { out.push(b); for (const c of b.children) walk(c); };
  for (const b of blocks) walk(b);
  return out;
}

/**
 * Char ranges covered by every `:::` fence BODY — the open and close fence
 * lines excluded, document order, non-overlapping.
 *
 * Only TOP-LEVEL blocks contribute a range. A nested `::::buttons` lives inside
 * its parent's body by construction, so adding it would produce a second range
 * covering ground the parent already covers — and a caller asking "is this
 * position in a fence body?" would then have to dedupe. One range per top-level
 * block answers that question in one pass.
 *
 * Exists so the editor can render a fence body at STRUCTURE density (media
 * become one-line tokens) while ordinary body text is untouched — see
 * docs/archive/2026-08-21-editor-container-media-density-design.md. Lives here
 * rather than in the editor so there stays exactly one reader of the block
 * tree; two readers of the same structure drift.
 */
export function shortcodeBodyRanges(state: EditorState): { from: number; to: number }[] {
  const out: { from: number; to: number }[] = [];
  for (const top of collectShortcodeBlocks(state)) {
    const bodyFrom = Math.min(state.doc.lineAt(top.from).to + 1, top.closeFrom);
    if (bodyFrom < top.closeFrom) out.push({ from: bodyFrom, to: top.closeFrom });
  }
  return out;
}

/** True when `pos` falls inside any fence body. Linear over the ranges, which
 *  number one per top-level block on the page — the caller that runs per
 *  syntax-tree node computes the ranges ONCE and passes them in. */
export function inShortcodeBody(ranges: { from: number; to: number }[], pos: number): boolean {
  return ranges.some((r) => pos >= r.from && pos < r.to);
}

/** Shortcodes whose body splits into cells on a `+++` line (grid, buttons). */
const CELL_DIVIDER_NAMES = new Set(['grid', 'buttons']);

/** A body line that is exactly `+++` — the cell divider (mirrors the Rust
 *  `split_grid_cells` / `cells::split_cells` rule; legacy `---` form omitted). */
export function isCellDividerLine(text: string): boolean {
  return text.trim() === '+++';
}

/**
 * A body line that is exactly `---` — the OLD spelling of a cell divider.
 * Still accepted by the build (`split_grid_cells`), still deprecated, and only
 * ever a divider inside `:::grid`: `split_cells`, which every other cell type
 * uses, does not know `---` at all. Mirrors `match_divider` in the Rust
 * `editor_scan`, which likewise flags `---` only at grid depth.
 */
export function isLegacyDividerLine(text: string): boolean {
  return text.trim() === '---';
}

/** The deepest block whose BODY contains `lineFrom`, or null. */
function innermostBlockAt(lineFrom: number, subtree: ShortcodeBlockInfo[]): ShortcodeBlockInfo | null {
  let innermost: ShortcodeBlockInfo | null = null;
  for (const b of subtree) {
    if (b.from < lineFrom && lineFrom < b.closeFrom) {
      if (!innermost || b.from > innermost.from) innermost = b;
    }
  }
  return innermost;
}

/**
 * True when the INNERMOST block containing `lineFrom` in its body is a
 * cell-dividing shortcode (grid or buttons). A `+++` inside a nested
 * `::::buttons` still divides (buttons is a cell type); it renders as literal
 * body text only when the innermost container is a NON-cell type (e.g. hero) —
 * matching the build, which splits cells per-block, not recursively.
 */
export function dividesCellsAt(lineFrom: number, subtree: ShortcodeBlockInfo[]): boolean {
  const innermost = innermostBlockAt(lineFrom, subtree);
  return innermost != null && CELL_DIVIDER_NAMES.has(innermost.name);
}

/**
 * True when a `---` on this line is a deprecated cell divider rather than
 * ordinary content — i.e. the innermost enclosing block is a `:::grid`.
 * Inside `:::buttons` (or anywhere else) `---` is just text, and hinting
 * there would be wrong.
 */
export function legacyDividesCellsAt(lineFrom: number, subtree: ShortcodeBlockInfo[]): boolean {
  return innermostBlockAt(lineFrom, subtree)?.name === 'grid';
}

/**
 * Count the legacy `---` dividers the Rust `editor_scan` would also count:
 * only those directly in a TOP-LEVEL `:::grid` body. Exists so the dev-only
 * divergence check can compare the two parsers on dividers, not just on block
 * names — the grid-only rule above is now written twice and would otherwise
 * drift silently.
 */
export function topLevelLegacyDividerCount(state: EditorState): number {
  let n = 0;
  for (const top of collectShortcodeBlocks(state)) {
    if (top.name !== 'grid') continue;
    const subtree = flattenBlocks([top]);
    let pos = state.doc.lineAt(top.from).from;
    while (pos <= top.to) {
      const ln = state.doc.lineAt(pos);
      if (isLegacyDividerLine(ln.text) && innermostBlockAt(ln.from, subtree) === top) n++;
      if (ln.to + 1 > top.to) break;
      pos = ln.to + 1;
    }
  }
  return n;
}

/**
 * Block-range activation — the editor-wide reveal contract: any selection
 * range TOUCHING [from, to] reveals the block's source. Matches the
 * selection-overlap activation tables and inline marks use (getActiveLines /
 * isNodeActive in cm-live-preview), so Cmd+A and multi-line drags reveal
 * shortcode fences like everything else. (Was head-only before, which made
 * shortcode blocks the one construct Cmd+A did not reveal.)
 */
export function isBlockActive(state: EditorState, b: ShortcodeBlockInfo): boolean {
  return nodeTouchesSelection(state, b.from, b.to);
}

const ICONS: Record<string, string> = {
  grid: '▦', hero: '◉', gallery: '⊞', buttons: '⊡', subscribe: '✉', recent: '◷',
};

/** Icon for a nameless `:::{.class}` block ("Pure-CSS region" in
 *  shortcode-grammar.md) — distinct from both the named-shortcode icons and
 *  the `‹/›` unknown-name fallback, since this isn't unknown, it just has no
 *  name to show. */
const CLASS_REGION_ICON = '▢';

const CLASS_TOKEN_RE = /\.[\w-]+/g;

/**
 * A nameless block has no name to label the tag with, so the class list
 * IS the name — it's the only thing the author typed that identifies the
 * block. `{.tagline}` → "tagline"; `{.subscribe-card .wide}` →
 * "subscribe-card wide". Returns null when there's no class to show (an
 * attrs-only block with no class, which shouldn't normally occur but must
 * not crash the tag into a blank label).
 */
export function classListLabel(attrs: string): string | null {
  const classes = attrs.match(CLASS_TOKEN_RE);
  if (!classes || classes.length === 0) return null;
  return classes.map((c) => c.slice(1)).join(' ');
}

const WIDTH_RE = /\b(wide|page|screen|full|body)\b/;

/** Params shown in the tag (on hover), for layout-ambiguous types. */
export function tagParams(attrs: string): string {
  const parts: string[] = [];
  const cols = /\bcols=([^\s"'{}]+)/.exec(attrs);
  if (cols) {
    parts.push(`cols ${cols[1]}`);
  } else {
    // Positional form (`:::grid 3`): echo the leading bare token verbatim
    // rather than label it `cols` — the author wrote it, and we don't want the
    // tag to assert a semantic the backend might interpret differently. A bare
    // `{…}` attr block is not a positional token and is skipped here, and a
    // width keyword is left to the dedicated branch below so it isn't doubled.
    const positional = /^([^\s{][^\s]*)/.exec(attrs.trim());
    if (positional && !WIDTH_RE.test(positional[1])) parts.push(positional[1]);
  }
  if (WIDTH_RE.test(attrs)) {
    parts.push(WIDTH_RE.exec(attrs)![1]);
  }
  // Read `image=` with the block grammar, not a regex over the raw text: the
  // old `/\bimage=["']?([^\s"'{}]+)/` cut a quoted path at its first space, so
  // `image="my photo.jpg"` hinted `my`. It is also the second reader of the
  // same attribute in this file, and two readers of one attribute drift.
  const brace = attrs.indexOf('{');
  const img = brace === -1
    ? null
    : parseAttrKvSpans(attrs.slice(brace))?.find((kv) => kv.key === 'image');
  if (img) {
    const path = img.value.split('|')[0].trim();
    if (path) parts.push(path.split('/').pop()!);
  }
  return parts.join('  ·  ');
}

/** Resolve the icon + label a resting block's tag shows. A named block
 *  (`:::grid`) shows its own icon and name; a nameless `:::{.class}` block
 *  (see `classListLabel`) shows the class-region icon and its class list. */
function tagIconAndLabel(name: string, attrs: string): { icon: string; label: string } {
  if (name === '') {
    const classLabel = classListLabel(attrs);
    if (classLabel) return { icon: CLASS_REGION_ICON, label: classLabel };
  }
  return { icon: ICONS[name] ?? '‹/›', label: name };
}

/**
 * What a resting block should draw where its icon goes.
 *
 * - `{ url }` — the shortcode names an image that resolved; draw it.
 * - `'missing'` — it names a path nothing resolves to. This is the ONLY place
 *   that can be said: a resting block's source line is replaced, so the
 *   `.cm-asset-unresolved` underline the same target earns elsewhere is not
 *   on screen to be seen. Without this the tag would look identical whether
 *   the file existed or not, right up until publish refuses.
 * - `null` — no asset named, not resolved yet, or resolved to something with
 *   no still frame (video). Falls back to the glyph, which is never wrong.
 */
export type ShortcodeAsset = { url: string } | 'missing' | null;

/** Resolve a shortcode's asset path to what its tag should draw. */
export type ResolveShortcodeAsset = (target: string) => ShortcodeAsset;

class MicroTagWidget extends WidgetType {
  constructor(
    private readonly icon: string,
    private readonly label: string,
    private readonly params: string,
    private readonly bodyPos: number,
    private readonly asset: ShortcodeAsset,
    private readonly strings: ShortcodeBlockStrings,
  ) { super(); }
  eq(o: MicroTagWidget) {
    return o.icon === this.icon && o.label === this.label && o.params === this.params
      && o.bodyPos === this.bodyPos && sameAsset(o.asset, this.asset);
  }
  toDOM(): HTMLElement {
    const el = document.createElement('span');
    el.className = 'cm-sc-tag';
    el.appendChild(this.renderLeading());
    const nm = document.createElement('span');
    nm.className = 'cm-sc-tag-nm';
    nm.textContent = this.label;
    el.appendChild(nm);
    if (this.params) {
      const at = document.createElement('span');
      at.className = 'cm-sc-tag-at';
      at.textContent = this.params;
      el.appendChild(at);
    }
    el.addEventListener('mousedown', (e) => {
      e.preventDefault();
      e.stopPropagation();
      const view = EditorView.findFromDOM(el);
      if (!view) return;
      view.dispatch({ selection: { anchor: this.bodyPos }, scrollIntoView: true });
      view.focus();
    });
    return el;
  }

  /**
   * The thumbnail, the missing-file marker, or the glyph — in that order of
   * preference. All three occupy the same slot and the same box, so the tag
   * stays exactly one line tall whichever one wins: a hero that resolves must
   * not reflow the document relative to one that hasn't resolved yet.
   */
  private renderLeading(): HTMLElement {
    if (this.asset && this.asset !== 'missing') {
      const img = document.createElement('img');
      img.className = 'cm-sc-tag-thumb';
      img.alt = '';
      img.src = this.asset.url;
      return img;
    }
    const ic = document.createElement('span');
    ic.className = 'cm-sc-tag-ic';
    if (this.asset === 'missing') {
      ic.classList.add('cm-sc-tag-ic--missing');
      ic.textContent = MISSING_ICON;
      // The tag is all the author can see of a resting block, so it has to
      // carry the explanation too. `data-tooltip` (portal), never `title=`.
      ic.setAttribute('data-tooltip', this.strings.assetMissingHint());
      return ic;
    }
    ic.textContent = this.icon;
    return ic;
  }

  ignoreEvent(e: Event) { return e.type !== 'mousedown'; }
}

/** Widget identity for the asset slot — drives `eq`, hence DOM reuse. */
function sameAsset(a: ShortcodeAsset, b: ShortcodeAsset): boolean {
  if (a === b) return true;
  if (!a || !b || a === 'missing' || b === 'missing') return false;
  return a.url === b.url;
}

/** Stands in for a thumbnail when the named file isn't there. */
const MISSING_ICON = '⊘';

/**
 * The note next to a `---` cell divider: it still works, but `+++` is the
 * spelling to use. Sits at the END of the divider line so the author reads
 * what they typed first and the correction second, and so it never competes
 * with a decoration over the `---` itself — live-preview may already have
 * turned that text into a rule or a setext heading marker.
 *
 * Deliberately not a replacement: hiding the `---` would leave the author
 * told to type `+++` with nothing visible to change.
 */
class LegacyDividerHintWidget extends WidgetType {
  constructor(
    private readonly linePos: number,
    private readonly strings: ShortcodeBlockStrings,
  ) { super(); }
  eq(o: LegacyDividerHintWidget) { return o.linePos === this.linePos; }
  toDOM(): HTMLElement {
    const el = document.createElement('span');
    el.className = 'cm-sc-legacy-hint';
    el.textContent = this.strings.legacyDividerLabel();
    el.setAttribute('data-tooltip', this.strings.legacyDividerTooltip());
    el.addEventListener('mousedown', (e) => {
      e.preventDefault();
      e.stopPropagation();
      const view = EditorView.findFromDOM(el);
      if (!view) return;
      view.dispatch({ selection: { anchor: this.linePos }, scrollIntoView: true });
      view.focus();
    });
    return el;
  }
  ignoreEvent(e: Event) { return e.type !== 'mousedown'; }
}

const REPLACE = Decoration.replace({});

// ── Revealed-fence token marks ───────────────────────────────────────────────
// A block whose range the selection touches shows its fences as RAW SOURCE
// (the reveal contract). Raw is not plain: the fence gets token-level syntax
// highlighting — delimiters muted, payload styled — mirroring Obsidian's
// formatting/payload class split. Resting fences never carry these marks;
// their lines are REPLACE'd (micro-tag / rule) so there is nothing to color.
// See docs/archive/2026-08-14-raw-markup-token-highlight.md.
const MARK_SC_DELIM = Decoration.mark({ class: 'cm-sc-delim' });      // ::: { }
const MARK_SC_NAME = Decoration.mark({ class: 'cm-sc-name' });        // grid
const MARK_SC_ATTR_KEY = Decoration.mark({ class: 'cm-sc-attr-key' }); // cols=

/**
 * Token marks for a revealed OPEN fence line (`:::name {k=v}`): colons and
 * braces muted, name in keyword weight, attr keys secondary, values plain.
 * Reuses the fence grammar's own regex and the attr parser — no second parser.
 * Marks are emitted left-to-right so the RangeSetBuilder stays sorted.
 */
function addOpenFenceTokenMarks(
  builder: RangeSetBuilder<Decoration>,
  ln: { from: number; text: string },
): void {
  const m = SHORTCODE_OPEN_RE.exec(ln.text);
  if (!isOpenMatch(m)) return;
  const colonsFrom = ln.from + m![1].length;
  const colonsTo = colonsFrom + m![2].length;
  builder.add(colonsFrom, colonsTo, MARK_SC_DELIM);
  const nameLen = m![3]?.length ?? 0;
  if (nameLen > 0) builder.add(colonsTo, colonsTo + nameLen, MARK_SC_NAME);

  const brace = ln.text.indexOf('{', m![1].length + m![2].length + nameLen);
  if (brace === -1) return;
  builder.add(ln.from + brace, ln.from + brace + 1, MARK_SC_DELIM); // {
  const kvs = parseAttrKvSpans(ln.text.slice(brace));
  if (kvs === null) return; // malformed attrs → the build reads no attrs; stay quiet
  for (const kv of kvs) {
    builder.add(ln.from + brace + kv.keyFrom, ln.from + brace + kv.keyTo, MARK_SC_ATTR_KEY);
  }
  const close = ln.text.lastIndexOf('}');
  if (close > brace) builder.add(ln.from + close, ln.from + close + 1, MARK_SC_DELIM); // }
}

/** Token marks for a revealed CLOSE fence line (`:::`): the colons, muted. */
function addCloseFenceTokenMarks(
  builder: RangeSetBuilder<Decoration>,
  ln: { from: number; text: string },
): void {
  const indent = ln.text.length - ln.text.trimStart().length;
  const colons = ln.text.trim().length;
  if (colons > 0) builder.add(ln.from + indent, ln.from + indent + colons, MARK_SC_DELIM);
}

/**
 * What a block's tag should draw in its icon slot. Null resolver (jsdom,
 * a read-only viewer, any editor built without `getFromFile`) → the glyph.
 */
function blockAsset(b: ShortcodeBlockInfo, resolve?: ResolveShortcodeAsset): ShortcodeAsset {
  if (!resolve) return null;
  const ref = shortcodeAssetRef(b.name, b.attrs);
  return ref ? resolve(ref.target) : null;
}

function buildBlockDecorations(
  state: EditorState,
  strings: ShortcodeBlockStrings,
  resolve?: ResolveShortcodeAsset,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const top of collectShortcodeBlocks(state)) {
    const subtree = flattenBlocks([top]);
    const openByStart = new Map<number, ShortcodeBlockInfo>();
    const closeByStart = new Map<number, ShortcodeBlockInfo>();
    for (const b of subtree) { openByStart.set(b.from, b); closeByStart.set(b.closeFrom, b); }

    if (isBlockActive(state, top)) {
      // active — tint every line (whole subtree revealed as raw source), and
      // token-highlight the raw fences: delimiters muted, name/keys styled.
      let pos = state.doc.lineAt(top.from).from;
      while (pos <= top.to) {
        const ln = state.doc.lineAt(pos);
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-active' }));
        if (openByStart.has(ln.from)) addOpenFenceTokenMarks(builder, ln);
        else if (closeByStart.has(ln.from)) addCloseFenceTokenMarks(builder, ln);
        // The hint follows the author into the block. Editing is exactly when
        // they can act on it, so an active block must not drop it.
        if (isLegacyDividerLine(ln.text) && legacyDividesCellsAt(ln.from, subtree)) {
          builder.add(ln.to, ln.to,
            Decoration.widget({ widget: new LegacyDividerHintWidget(ln.from, strings), side: 1 }));
        }
        if (ln.to + 1 > top.to) break;
        pos = ln.to + 1;
      }
      continue;
    }
    // resting — render the whole subtree: every block's open fence → micro-tag,
    // close fence → faint rule, `+++` cell divider → hidden literal + CSS rule.
    let pos = state.doc.lineAt(top.from).from;
    while (pos <= top.to) {
      const ln = state.doc.lineAt(pos);
      const openB = openByStart.get(ln.from);
      const closeB = closeByStart.get(ln.from);
      if (openB) {
        const bodyPos = Math.min(ln.to + 1, openB.to);
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-openrest' }));
        const { icon, label } = tagIconAndLabel(openB.name, openB.attrs);
        builder.add(ln.from, ln.from,
          Decoration.widget({
            widget: new MicroTagWidget(
              icon, label, tagParams(openB.attrs), bodyPos, blockAsset(openB, resolve), strings,
            ),
            side: -1,
          }));
        if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE); // hide raw open fence
      } else if (closeB) {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-closerest' }));
        if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE); // hide close fence; CSS draws a rule
      } else if (isCellDividerLine(ln.text) && dividesCellsAt(ln.from, subtree)) {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-divider' }));
        if (ln.to > ln.from) builder.add(ln.from, ln.to, REPLACE); // hide `+++`; CSS draws the divider
      } else if (isLegacyDividerLine(ln.text) && legacyDividesCellsAt(ln.from, subtree)) {
        // Deliberately NOT the `cm-sc-line-divider` treatment: an old divider
        // should not look as settled as a correct one, and hiding the literal
        // would take away the text the author has to edit.
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-legacy-divider' }));
        builder.add(ln.to, ln.to,
          Decoration.widget({ widget: new LegacyDividerHintWidget(ln.from, strings), side: 1 }));
      } else {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-body' }));
      }
      if (ln.to + 1 > top.to) break;
      pos = ln.to + 1;
    }
  }
  return builder.finish();
}

/**
 * True when the incremental parse extended the tree without a doc change —
 * the parse worker's effect-only transactions on large documents. Without
 * this trigger, blocks beyond the initially-parsed region never decorate
 * until the next edit or cursor move (scrolling produces no transaction at
 * all for StateFields).
 */
function treeAdvanced(tr: Transaction): boolean {
  return syntaxTree(tr.state) != syntaxTree(tr.startState);
}

function shortcodeBlockField(opts: ShortcodeBlockOptions): StateField<DecorationSet> {
  const { resolveAsset, refsResolvedEffect } = opts;
  const strings: ShortcodeBlockStrings = { ...DEFAULT_STRINGS, ...opts.strings };
  return StateField.define<DecorationSet>({
    create: (state) => buildBlockDecorations(state, strings, resolveAsset),
    update(value, tr) {
      // Asset resolution is asynchronous and lands as an effect on a
      // transaction that changes neither doc nor selection. Without this
      // clause a hero's thumbnail would appear only on the next keystroke —
      // which, for a file opened and read but not edited, is never.
      const resolved = refsResolvedEffect !== undefined
        && tr.effects.some((e) => e.is(refsResolvedEffect));
      if (!revealInputsChanged(tr) && !treeAdvanced(tr) && !resolved) return value;
      return buildBlockDecorations(tr.state, strings, resolveAsset);
    },
    provide: (f) => EditorView.decorations.from(f),
  });
}

// atomicRanges: cursor skips each hidden open-fence range of a resting block —
// including nested blocks, whose open fences are hidden too. flattenBlocks
// returns document order (parent before children), so the ranges are sorted.
function buildAtomicRanges(state: EditorState): RangeSet<Decoration> {
  const builder = new RangeSetBuilder<Decoration>();
  for (const top of collectShortcodeBlocks(state)) {
    if (isBlockActive(state, top)) continue;
    for (const b of flattenBlocks([top])) {
      const openLine = state.doc.lineAt(b.from);
      if (openLine.to > openLine.from) builder.add(openLine.from, openLine.to, REPLACE);
    }
  }
  return builder.finish();
}

const atomicField = StateField.define<RangeSet<Decoration>>({
  create: buildAtomicRanges,
  update(value, tr) {
    if (!revealInputsChanged(tr) && !treeAdvanced(tr)) return value;
    return buildAtomicRanges(tr.state);
  },
});

export interface ShortcodeBlockOptions {
  /**
   * Resolves a shortcode's asset path to a thumbnail. Omitted → every tag
   * keeps its glyph, which is the pre-thumbnail behaviour exactly.
   */
  resolveAsset?: ResolveShortcodeAsset;
  /**
   * `cm-reference-resolver`'s "a batch of references resolved" effect, passed
   * in rather than imported: that module reaches `../bindings`, and this one
   * is kept off that import graph (same reason `asset-preview` declares its
   * own envelope type, and the same shape `assetLintExtension` takes).
   */
  refsResolvedEffect?: StateEffectType<unknown>;
  /**
   * Host-injected UI strings (thunks; see `ShortcodeBlockStrings`). Missing
   * entries fall back to the English defaults. moss passes `() => t('…')`
   * so locale switches stay live; other hosts may pass nothing.
   */
  strings?: Partial<ShortcodeBlockStrings>;
}

export function shortcodeBlockExtension(opts: ShortcodeBlockOptions = {}): Extension {
  return [
    shortcodeBlockField(opts),
    atomicField,
    EditorView.atomicRanges.of((view) => view.state.field(atomicField)),
  ];
}
