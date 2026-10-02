/**
 * Block-decoration StateField for shortcode blocks. Reads the SYNCHRONOUS Lezer
 * syntax tree and pairs Open↔Close fence markers with an arity stack, so
 * positions are always current for tr.state — no stale offsets, no glitches.
 *
 * The document alone decides how a block renders, never the caret: every
 * fence hangs into the margin and is token-highlighted; body lines are
 * tinted; a `+++` cell divider hangs too. Editing a line ABOVE the block
 * does not change anything below it — there is nothing caret-driven left to
 * toggle.
 *
 * Large-doc caveat: syntaxTree(state) may be incomplete past the viewport for
 * very long docs. The parse worker extends the tree via effect-only
 * transactions; the field rebuilds on that advance (see `treeAdvanced`), so
 * late-parsed blocks decorate as soon as the parse reaches them. We do not
 * assert whole-doc completeness at any single point in time.
 */

import { EditorView, Decoration, WidgetType } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { EditorState, StateField, RangeSetBuilder, type Extension, type Transaction } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { SHORTCODE_OPEN_RE, isOpenMatch, parseAttrKvSpans } from '../shortcode.js';

// ── Host strings seam ─────────────────────────────────────────────────────────
// This module used to call moss's `t()` directly; the package must not know
// about any host's i18n. Each string is a THUNK, not a value, because the old
// code called `t()` at widget-DOM time (`toDOM`) — locale-reactive: a locale
// switch re-renders with fresh text. Hosts that want that pass `() => t('…')`;
// hosts that don't pass nothing and get the English defaults.
export interface ShortcodeBlockStrings {
  /** Inline label after a deprecated `---` cell divider. */
  legacyDividerLabel: () => string;
  /** Tooltip on that label. */
  legacyDividerTooltip: () => string;
}

const DEFAULT_STRINGS: ShortcodeBlockStrings = {
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
 * become one-line tokens) while ordinary body text is untouched. Lives here
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

// ── Fence token marks ────────────────────────────────────────────────────────
// Every fence is shown as RAW SOURCE, regardless of caret — and raw is not
// plain: the fence gets token-level syntax highlighting — delimiters muted,
// payload styled — mirroring Obsidian's formatting/payload class split. The
// repeated fence characters themselves hang into the margin (`cm-hang`); the
// name and attributes after them stay inline and editable.
const MARK_SC_DELIM = Decoration.mark({ class: 'cm-sc-delim' });         // { }
const MARK_SC_FENCE = Decoration.mark({ class: 'cm-sc-delim cm-hang' }); // ::: / +++
const MARK_SC_NAME = Decoration.mark({ class: 'cm-sc-name' });
const MARK_SC_ATTR_KEY = Decoration.mark({ class: 'cm-sc-attr-key' });

/**
 * Token marks for an OPEN fence line (`:::name {k=v}`): the repeated fence
 * characters hang (`cm-hang`, moved to the margin by the `cm-hung` line
 * class), braces muted, name in keyword weight, attr keys secondary, values
 * plain. Reuses the fence grammar's own regex and the attr parser — no second
 * parser. Marks are emitted left-to-right so the RangeSetBuilder stays sorted.
 */
function addOpenFenceTokenMarks(
  builder: RangeSetBuilder<Decoration>,
  ln: { from: number; text: string },
): void {
  const m = SHORTCODE_OPEN_RE.exec(ln.text);
  if (!isOpenMatch(m)) return;
  const colonsFrom = ln.from + m![1].length;
  const colonsTo = colonsFrom + m![2].length;
  builder.add(colonsFrom, colonsTo, MARK_SC_FENCE);
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

/** Hangs the trimmed content of a line that is nothing but repeated fence
 *  characters — the closing `:::` or a `+++` cell divider. Both are "a line
 *  prefix with nothing after it", so the whole trimmed text is the hang. */
function addHungLineMark(
  builder: RangeSetBuilder<Decoration>,
  ln: { from: number; text: string },
): void {
  const indent = ln.text.length - ln.text.trimStart().length;
  const len = ln.text.trim().length;
  if (len > 0) builder.add(ln.from + indent, ln.from + indent + len, MARK_SC_FENCE);
}

function buildBlockDecorations(
  state: EditorState,
  strings: ShortcodeBlockStrings,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const top of collectShortcodeBlocks(state)) {
    const subtree = flattenBlocks([top]);
    const openByStart = new Map<number, ShortcodeBlockInfo>();
    const closeByStart = new Map<number, ShortcodeBlockInfo>();
    for (const b of subtree) { openByStart.set(b.from, b); closeByStart.set(b.closeFrom, b); }

    let pos = state.doc.lineAt(top.from).from;
    while (pos <= top.to) {
      const ln = state.doc.lineAt(pos);
      if (openByStart.has(ln.from)) {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-fence cm-hung' }));
        addOpenFenceTokenMarks(builder, ln);
      } else if (closeByStart.has(ln.from)) {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-fence cm-hung' }));
        addHungLineMark(builder, ln);
      } else if (isCellDividerLine(ln.text) && dividesCellsAt(ln.from, subtree)) {
        builder.add(ln.from, ln.from, Decoration.line({ class: 'cm-sc-line cm-sc-line-divider cm-hung' }));
        addHungLineMark(builder, ln);
      } else if (isLegacyDividerLine(ln.text) && legacyDividesCellsAt(ln.from, subtree)) {
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
  const strings: ShortcodeBlockStrings = { ...DEFAULT_STRINGS, ...opts.strings };
  return StateField.define<DecorationSet>({
    create: (state) => buildBlockDecorations(state, strings),
    update(value, tr) {
      if (!tr.docChanged && !treeAdvanced(tr)) return value;
      return buildBlockDecorations(tr.state, strings);
    },
    provide: (f) => EditorView.decorations.from(f),
  });
}

export interface ShortcodeBlockOptions {
  strings?: Partial<ShortcodeBlockStrings>;
}

export function shortcodeBlockExtension(opts: ShortcodeBlockOptions = {}): Extension {
  return [shortcodeBlockField(opts)];
}
