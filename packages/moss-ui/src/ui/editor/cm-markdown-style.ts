// Unconditional markdown line/mark styling: presentation is a pure function
// of the document, never of the selection. Every construct below renders the
// same way regardless of caret position — this module is what makes that
// true for headings, blockquotes, lists, fenced code, math, wikilinks,
// strikethrough/inline-code payload style and done-task muting; the rest
// (faint inline marks, table pipes, the bold header row, emphasis weight)
// already comes for free from `mossHighlight` + `@lezer/markdown`'s own
// `styleTags`.
//
// The margin is an indicator column:
// only a heading's "## " and a quote's "> " hang. Each is a `cm-hang` mark inside
// a `cm-hung` line, positioned into the `--moss-editor-hang-gutter` reserved
// column (cm-markdown-style.css). Everything else stays in the text column: a
// list marker, a task bracket, a code or math fence, a footnote definition's
// "[^1]:", a `:::` shortcode fence — faint, never hung. No
// decoration builder here reads `state.selection` — that is the whole point
// of the rewrite, and the caret-invariance test below is what proves it.

import { EditorState, RangeSet, RangeSetBuilder, StateEffect, type Extension } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import { t, onLocaleChange } from "../i18n/index";
import { hangsInRanges, type Hang } from './cm-hangs';
import { headFitExtension } from './cm-head-fit';
import { listDecorations, listHangExtension, type DecoRange, type SyntaxNode } from './cm-lists';
import { footnoteDecorations, type FootnoteDeco } from '@symbiosis-lab/moss-syntax/cm6';

const HEADING_CLASSES: Record<string, string> = {
  ATXHeading1: 'cm-lp-h1',
  ATXHeading2: 'cm-lp-h2',
  ATXHeading3: 'cm-lp-h3',
  ATXHeading4: 'cm-lp-h4',
  ATXHeading5: 'cm-lp-h5',
  ATXHeading6: 'cm-lp-h6',
};
const LINE_HEADING: Record<string, Decoration> = Object.fromEntries(
  Object.entries(HEADING_CLASSES).map(([name, cls]) => [name, Decoration.line({ class: cls })]),
);

/** The text a hovered indicator names: "Heading 2", "Quote". */
const indicatorLabel = (h: Pick<Hang, 'kind' | 'level'>): string =>
  h.kind === 'heading' ? t('indicator.heading', { level: String(h.level) }) : t('indicator.quote');

/** An indicator in the margin: brightens on hover, names its line type in a tooltip, and is the click target of the line-type menu (cm-format-keys.ts). */
const markHang = (h: Hang) =>
  Decoration.mark({
    class: 'cm-hang',
    attributes: { 'data-indicator': h.kind, 'data-tooltip': indicatorLabel(h), 'data-tooltip-position': 'top' },
  });
const LINE_HUNG = Decoration.line({ class: 'cm-hung' });
const MARK_HANG_RUN = Decoration.mark({ class: 'cm-hang-run' });

export type { SyntaxNode };

/** The structural cursor shape every mark-walking helper below needs — named
 * once and reused (mirrors `cm-live-preview.ts`'s `InlineNode` convention)
 * instead of retyping the same literal in each function signature. */
interface StructuralCursor {
  firstChild(): boolean;
  nextSibling(): boolean;
  name: string;
  from: number;
  to: number;
}

interface NodeWithCursor {
  node: { cursor(): StructuralCursor };
}

/** `NodeWithCursor` plus the node's own span — only `handleDisplayMath` needs
 * `from`/`to` on the node itself (to locate the line each fence sits on). */
interface RangedNodeWithCursor extends NodeWithCursor {
  from: number;
  to: number;
}

export function buildMarkdownStyleDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[],
  hangs: readonly Hang[] = hangsInRanges(state, ranges),
): DecorationSet {
  const decos: DecoRange[] = [];
  // Footnotes: moss-syntax's own plugin replaces each reference `[^1]` with a
  // chip and hangs each definition's `[^1]:`. The decorations it computes are
  // used here instead, with the replace swapped for a mark and the hang for a
  // faint mark in the column, so the characters stay where the text is and the
  // app does not load the plugin (see cm-editor.ts).
  for (const d of footnoteDecorations(state)) {
    const cls = (d.deco.spec as { class?: string }).class ?? '';
    if ((d.deco.spec as { widget?: unknown }).widget) decos.push({ from: d.from, to: d.to, deco: MARK_FOOTNOTE_REF });
    else if (isHangDeco(d.deco)) decos.push({ from: d.from, to: d.to, deco: MARK_DELIM });
    else if (cls !== 'cm-hung') decos.push(d);
  }
  for (const { from, to } of ranges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter(node) {
        const headingDeco = LINE_HEADING[node.name];
        if (headingDeco) {
          const line = state.doc.lineAt(node.from);
          decos.push({ from: line.from, to: line.from, deco: headingDeco });
          return; // descend — inline marks inside the heading still need a visit
        }
        if (node.name === 'Blockquote') {
          if (!hasBlockquoteAncestor(node.node)) handleBlockquote(node, state, decos);
          return;
        }
        if (node.name === 'DisplayMath') {
          handleDisplayMath(node, state, decos);
          return false; // no nested constructs render inside display math
        }
        if (node.name === 'CodeInfo') {
          decos.push({ from: node.from, to: node.to, deco: MARK_FENCE_LANG });
          return false;
        }
        if (node.name === 'Strikethrough') {
          styleBetweenMarks(node, 'StrikethroughMark', MARK_STRIKE, decos);
          return;
        }
        if (node.name === 'InlineCode') {
          styleBetweenMarks(node, 'CodeMark', MARK_CODE, decos);
          return false;
        }
        if (node.name === 'Escape') {
          decos.push({ from: node.from, to: node.from + 1, deco: MARK_DELIM }); // the backslash is faint; the escaped character is plain
          return false;
        }
        if (node.name === 'Wikilink') {
          handleWikilink(node, state, decos);
          return false;
        }
        if (node.name === 'InlineMath') {
          handleInlineMath(node, decos);
          return false;
        }
        if (node.name === 'Link') {
          styleLinkText(node, state, decos);
          return;
        }
        if (node.name === 'Image' || node.name === 'WikilinkEmbed') {
          styleEmbedSource(node, decos);
          return false;
        }
        if (node.name === 'HorizontalRule') {
          decos.push({ from: node.from, to: node.from, deco: LINE_RULE });
          decos.push({ from: node.from, to: node.to, deco: MARK_DELIM });
          return false;
        }

        if (node.name === 'ListItem') {
          listDecorations(node.node, state, from, to, decos);
          return; // descend — the marker and the item's text still get their own marks
        }
        if (node.name === 'ListMark') {
          // A task's dash is the one list marker in the accent colour.
          decos.push({ from: node.from, to: node.to, deco: node.node.nextSibling?.name === 'Task' ? MARK_TASK_DASH : MARK_LIST });
          return false;
        }
        if (node.name === 'Task') {
          styleTask(node, state, decos);
          return; // descend — the task's text still holds inline marks
        }
      },
    });
  }
  for (let i = 0; i < hangs.length; i++) {
    const h = hangs[i], lineFrom = state.doc.lineAt(h.from).from;
    if (i === 0 || state.doc.lineAt(hangs[i - 1].from).from !== lineFrom) {
      decos.push({ from: lineFrom, to: lineFrom, deco: LINE_HUNG });
      // `> # h`: both indicators hang from the same static position and would overlap, so a run of several is positioned as one box and the indicators sit side by side inside it.
      let last = i;
      while (hangs[last + 1] && state.doc.lineAt(hangs[last + 1].from).from === lineFrom) last++;
      if (last > i) decos.push({ from: h.from, to: hangs[last].to, deco: MARK_HANG_RUN });
    }
    decos.push({ from: h.from, to: h.to, deco: markHang(h) });
  }
  return Decoration.set(decos.map((d) => d.deco.range(d.from, d.to)), true);
}

const isHangDeco = (deco: Decoration): boolean => /(^|\s)cm-hang(\s|$)/.test((deco.spec as { class?: string }).class ?? '');

const ATOMIC = Decoration.replace({});

/** One atomic caret step per hung indicator: a selection endpoint never lands inside it. */
function hangAtoms(hangs: readonly Hang[]): RangeSet<Decoration> {
  const builder = new RangeSetBuilder<Decoration>();
  for (const h of [...hangs].sort((a, b) => a.atomFrom - b.atomFrom)) builder.add(h.atomFrom, h.to, ATOMIC);
  return builder.finish();
}

/** True if any ancestor of `node` is a Blockquote (i.e. node is a nested quote). */
function hasBlockquoteAncestor(node: { parent: unknown }): boolean {
  let p = node.parent as { name: string; parent: unknown } | null;
  while (p) {
    if (p.name === 'Blockquote') return true;
    p = p.parent as typeof p;
  }
  return false;
}

/** A thematic break: its characters stay faint and a hairline is drawn across the rest of the line (cm-markdown-style.css) — a line class, no replace, no widget. */
const LINE_RULE = Decoration.line({ class: 'cm-lp-rule' });
const LINE_BLOCKQUOTE = Decoration.line({ class: 'cm-lp-blockquote' });
const MARK_MATH = Decoration.mark({ class: 'cm-lp-math' });
const MARK_DELIM = Decoration.mark({ class: 'cm-lp-delim' });
const MARK_STRIKE = Decoration.mark({ class: 'cm-lp-strike' });
const MARK_CODE = Decoration.mark({ class: 'cm-lp-code' });
/** A code fence's language name (` ```js `): the one word on the fence line worth reading, in the accent colour. */
const MARK_FENCE_LANG = Decoration.mark({ class: 'cm-lp-fence-lang' });
/** A footnote reference `[^1]`: its characters stay, in the accent colour, small and raised like a superscript mark. `cm-link-clickable` is the followable-with-the-chord cursor class every reference wears. */
const MARK_FOOTNOTE_REF = Decoration.mark({ class: 'cm-footnote-ref cm-link-clickable' });
const MARK_DONE = Decoration.mark({ class: 'cm-lp-done' });
const MARK_TASK_DASH = Decoration.mark({ class: 'cm-lp-task-dash' });
/** The whole `[ ]` / `[x]`: the click target that toggles the task (cm-act-at-caret.ts). */
const MARK_TASK_TOGGLE = Decoration.mark({ class: 'cm-task-toggle' });
/** The `x` of a done task. */
const MARK_TASK_X = Decoration.mark({ class: 'cm-lp-task-x' });
/** A list marker (`- `, `1.`) — real characters in the text flow, in the accent colour. */
const MARK_LIST = Decoration.mark({ class: 'cm-lp-list-mark' });

/**
 * Style the content between the first and last direct child named
 * `markName` (e.g. `StrikethroughMark`, `CodeMark`) — the markers themselves
 * need no decoration here: `@lezer/markdown`'s own `styleTags` already tags
 * them `tags.processingInstruction`, which `mossHighlight` renders faint.
 */
function styleBetweenMarks(
  node: NodeWithCursor,
  markName: string,
  styleMark: Decoration,
  decos: DecoRange[],
): void {
  const marks: { from: number; to: number }[] = [];
  const cur = node.node.cursor();
  if (cur.firstChild()) {
    do {
      if (cur.name === markName) marks.push({ from: cur.from, to: cur.to });
    } while (cur.nextSibling());
  }
  if (marks.length >= 2) {
    const contentFrom = marks[0].to;
    const contentTo = marks[marks.length - 1].from;
    if (contentFrom < contentTo) decos.push({ from: contentFrom, to: contentTo, deco: styleMark });
  }
}

/** A link's `](url)`: smaller than the text. */
const MARK_LINK_TAIL = Decoration.mark({ class: 'cm-lp-link-tail' });
/** Every destination — a link's URL, a wikilink's page, an embed's path — looks the same: link colour, underlined while ⌘ is held. */
const MARK_LINK_DEST = Decoration.mark({ class: 'cm-lp-link-dest' });
/** A link's visible words (`[text]`'s text, a wikilink's alias): body colour, because the destination is what answers the pointer. */
const MARK_LINK_TEXT = Decoration.mark({ class: 'cm-lp-link-text' });

/**
 * Wikilink (`[[Page]]` / `[[Page|Label]]`): brackets faint, the page the
 * destination (link colour), an alias in the body colour. A content-empty wikilink (`[[]]`, `[[|]]`,
 * whitespace-only `[[ ]]`) renders no label, so there is nothing to style —
 * leave it raw.
 */
function handleWikilink(
  node: NodeWithCursor,
  state: EditorState,
  decos: DecoRange[],
): void {
  const cur = node.node.cursor();
  if (!cur.firstChild()) return;
  let openMark: { from: number; to: number } | null = null;
  let closeMark: { from: number; to: number } | null = null;
  let target: { from: number; to: number } | null = null;
  do {
    if (cur.name === 'WikilinkMark') {
      if (openMark === null) openMark = { from: cur.from, to: cur.to };
      else closeMark = { from: cur.from, to: cur.to };
    } else if (cur.name === 'WikilinkTarget') {
      target = { from: cur.from, to: cur.to };
    }
  } while (cur.nextSibling());
  if (!openMark || !closeMark || !target) return;
  const labelFrom = target.to + 1;
  const hasAlias = state.doc.sliceString(target.to, labelFrom) === '|' && labelFrom < closeMark.from;
  const labelText = hasAlias
    ? state.doc.sliceString(labelFrom, closeMark.from)
    : state.doc.sliceString(target.from, target.to);
  if (labelText.trim() === '') return; // content-empty — nothing to hide, leave raw
  decos.push({ from: openMark.from, to: openMark.to, deco: MARK_DELIM });
  decos.push({ from: closeMark.from, to: closeMark.to, deco: MARK_DELIM });
  if (hasAlias) {
    decos.push({ from: target.from, to: target.to, deco: MARK_LINK_DEST }); // Page: the destination
    decos.push({ from: target.to, to: labelFrom, deco: MARK_DELIM }); // |
    decos.push({ from: labelFrom, to: closeMark.from, deco: MARK_LINK_TEXT }); // Label
  } else if (target.from < target.to) {
    decos.push({ from: target.from, to: target.to, deco: MARK_LINK_DEST }); // Page (own label)
    if (target.to < closeMark.from) {
      decos.push({ from: target.to, to: closeMark.from, deco: MARK_DELIM }); // dangling |
    }
  }
}

/**
 * A task's `[ ]` / `[x]` stays on screen: brackets faint, the `x` in the accent
 * colour, the whole range a click target for the toggle, and a done item's text
 * muted and struck.
 */
function styleTask(node: RangedNodeWithCursor, state: EditorState, decos: DecoRange[]): void {
  const cur = node.node.cursor();
  if (!cur.firstChild() || cur.name !== 'TaskMarker') return;
  const { from, to } = cur;
  decos.push({ from, to, deco: MARK_TASK_TOGGLE });
  decos.push({ from, to: from + 1, deco: MARK_DELIM });
  decos.push({ from: to - 1, to, deco: MARK_DELIM });
  const marker = state.doc.sliceString(from, to);
  if (marker === '[x]' || marker === '[X]') {
    decos.push({ from: from + 1, to: to - 1, deco: MARK_TASK_X });
    if (to < node.to) decos.push({ from: to, to: node.to, deco: MARK_DONE });
  }
}

/**
 * Inline math (`$x$`): both fences (faint) AND the interior (tinted) always
 * show — matching DisplayMath's own treatment. `MARK_MATH`/`MARK_DELIM` are
 * the same constants the display-math handler above already defines.
 */
function handleInlineMath(
  node: NodeWithCursor,
  decos: DecoRange[],
): void {
  const marks: { from: number; to: number }[] = [];
  const cur = node.node.cursor();
  if (cur.firstChild()) {
    do {
      if (cur.name === 'MathMark') marks.push({ from: cur.from, to: cur.to });
    } while (cur.nextSibling());
  }
  for (const m of marks) decos.push({ from: m.from, to: m.to, deco: MARK_DELIM });
  if (marks.length === 2 && marks[1].from > marks[0].to) {
    decos.push({ from: marks[0].to, to: marks[1].from, deco: MARK_MATH });
  }
}

/**
 * An embed's source (`![[path|50%]]`, `![alt](path)`) stays on screen as text:
 * brackets and any `|width` suffix faint, the path in the link colour. The
 * picture or player is a block widget below the line (cm-block-embeds.ts), so
 * nothing here is replaced. An embed inside a line of prose draws nothing at
 * all: these marks are all it is, and its picture is in the hover card.
 */
function styleEmbedSource(node: NodeWithCursor, decos: DecoRange[]): void {
  const cur = node.node.cursor();
  if (!cur.firstChild()) return;
  let targetTo = -1;
  let closeFrom = -1;
  do {
    if (cur.name === 'LinkMark' || cur.name === 'WikilinkMark') {
      decos.push({ from: cur.from, to: cur.to, deco: MARK_DELIM });
      closeFrom = cur.from;
    } else if (cur.name === 'URL' || cur.name === 'WikilinkTarget') {
      decos.push({ from: cur.from, to: cur.to, deco: MARK_LINK_DEST });
      targetTo = cur.to;
    }
  } while (cur.nextSibling());
  // The `|50%` pothole of a wikilink embed sits between target and closing `]]`.
  if (targetTo >= 0 && closeFrom > targetTo) decos.push({ from: targetTo, to: closeFrom, deco: MARK_DELIM });
}

// The `[`…`]` span of a link's text and its `(url)` live here and nowhere else: the colours painted on them (below) and the pointer's hit range (cm-link-nav.ts) both read them, so they cannot drift apart.

/** The slice of a Lezer `Link` node these helpers walk. */
export interface LinkTextNode {
  cursor(): { firstChild(): boolean; nextSibling(): boolean; name: string; from: number; to: number };
}

/** A node's direct children, in order. */
function childNodes(node: LinkTextNode): { name: string; from: number; to: number }[] {
  const out: { name: string; from: number; to: number }[] = [];
  const cur = node.cursor();
  if (cur.firstChild()) do { out.push({ name: cur.name, from: cur.from, to: cur.to }); } while (cur.nextSibling());
  return out;
}

/** The `URL` child of a `Link` (a linked picture's own path is nested inside its `Image`, so it is not this one), or null. */
export function linkUrlRange(node: LinkTextNode): { from: number; to: number } | null {
  return childNodes(node).find((c) => c.name === 'URL') ?? null;
}

/**
 * The text between a `Link`'s first `[` and its last direct `]`, or null when
 * either is missing. An embedded picture's own brackets are nested inside an
 * `Image` child, so they are not direct marks and never end the text early.
 */
export function linkTextRange(node: LinkTextNode, doc: { sliceString(from: number, to: number): string }): { from: number; to: number } | null {
  let from = -1, to = -1;
  for (const c of childNodes(node)) {
    if (c.name !== 'LinkMark') continue;
    const mark = doc.sliceString(c.from, c.to);
    if (mark === '[' && from < 0) from = c.to;
    else if (mark === ']') to = c.from;
  }
  return from >= 0 && to >= from ? { from, to } : null;
}

/**
 * A link's characters all stay on screen:
 * the destination is the link: the URL in the link colour (underlined while ⌘
 * is held, see the stylesheet), and the words before it in the body colour,
 * because they do not answer the pointer. `](` and `)` stay faint; from `]` on
 * everything is a little smaller, the URL free to wrap at any character. A
 * link with no destination (`[text]()`) styles its text only.
 */
function styleLinkText(
  node: RangedNodeWithCursor,
  state: EditorState,
  decos: DecoRange[],
): void {
  // `> [!note]` parses as a link with no destination; its colour is the
  // callout's (cm-callout.ts), not a link's.
  const line = state.doc.lineAt(node.from);
  if (/^\s*>[\s>]*$/.test(state.doc.sliceString(line.from, node.from)) && /^\[!\w+\]/.test(state.doc.sliceString(node.from, line.to))) return;
  const text = linkTextRange(node.node, state.doc);
  const textTo = text ? text.to : null;
  const url = linkUrlRange(node.node);
  if (text) {
    // A picture inside the text keeps its own marks (its path is link-coloured): the text colour goes around it, not over it.
    let from = text.from;
    for (const c of childNodes(node.node)) {
      if (c.name !== 'Image' || c.from < text.from || c.to > text.to) continue;
      if (c.from > from) decos.push({ from, to: c.from, deco: MARK_LINK_TEXT });
      from = c.to;
    }
    if (text.to > from) decos.push({ from, to: text.to, deco: MARK_LINK_TEXT });
  }
  if (textTo != null && url && url.to > url.from) {
    decos.push({ from: textTo, to: node.to, deco: MARK_LINK_TAIL });
    decos.push({ from: url.from, to: url.to, deco: MARK_LINK_DEST });
  }
}

/**
 * A `DisplayMath` fence (`$$`) stays in the text column, faint; the interior
 * between the two fences tints.
 */
function handleDisplayMath(
  node: RangedNodeWithCursor,
  state: EditorState,
  decos: DecoRange[],
): void {
  const marks: { from: number; to: number }[] = [];
  const cur = node.node.cursor();
  if (cur.firstChild()) {
    do {
      if (cur.name === 'MathMark') marks.push({ from: cur.from, to: cur.to });
    } while (cur.nextSibling());
  }
  // A fence alone on its line: the tinted interior excludes that line's own
  // newline on either side, so the tint covers only the formula lines. A fence
  // that is NOT alone (mid-paragraph `$$...$$`) bounds the interior itself.
  let contentFrom: number | null = null;
  let contentTo: number | null = null;
  marks.forEach((m, i) => {
    const line = state.doc.lineAt(m.from);
    const alone = line.text.trim() === state.doc.sliceString(m.from, m.to);
    decos.push({ from: m.from, to: m.to, deco: MARK_DELIM });
    if (i === 0) contentFrom = alone ? line.to + 1 : m.to;
    else contentTo = alone ? line.from - 1 : m.from;
  });
  if (marks.length === 2 && contentFrom != null && contentTo != null && contentTo > contentFrom) {
    decos.push({ from: contentFrom, to: contentTo, deco: MARK_MATH });
  }
}

/**
 * Bar line class on every line the Blockquote node spans (its AST range, not
 * `>` marker presence) so CommonMark lazy-continuation lines — no `>`, but
 * folded into the blockquote's paragraph by the parser — bar too. The `>`
 * hang itself is owned by `hangsIn`, not here, so it can
 * sit in the same run as any nested one.
 */
function handleBlockquote(
  node: { from: number; to: number },
  state: EditorState,
  decos: DecoRange[],
): void {
  const firstLine = state.doc.lineAt(node.from).number;
  const lastLine = state.doc.lineAt(node.to).number;
  for (let ln = firstLine; ln <= lastLine; ln++) {
    const line = state.doc.line(ln);
    decos.push({ from: line.from, to: line.from, deco: LINE_BLOCKQUOTE });
  }
}

/** The app's language changed: the indicator tooltips are translated when decorations are built, so they are built again. */
const localeChanged = StateEffect.define<null>();

/** Exported so a test can ask a view for the plugin instance by identity. */
export const markdownStylePlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet = Decoration.none;
    /** One atomic caret step per hung indicator in the rendered viewport. */
    atoms: RangeSet<Decoration> = RangeSet.empty;
    private readonly offLocale: () => void;
    constructor(view: EditorView) {
      this.build(view);
      this.offLocale = onLocaleChange(() => view.dispatch({ effects: localeChanged.of(null) }));
    }
    update(update: ViewUpdate) {
      const treeAdvanced = syntaxTree(update.state) != syntaxTree(update.startState);
      const relocalized = update.transactions.some((tr) => tr.effects.some((e) => e.is(localeChanged)));
      if (!update.docChanged && !update.viewportChanged && !treeAdvanced && !relocalized) return;
      this.build(update.view);
    }
    private build(view: EditorView) {
      const hangs = hangsInRanges(view.state, view.visibleRanges);
      this.decorations = buildMarkdownStyleDecorations(view.state, view.visibleRanges, hangs);
      this.atoms = hangAtoms(hangs);
    }
    destroy() {
      this.offLocale();
    }
  },
  { decorations: (v) => v.decorations },
);

/** A hung indicator is one atomic range, so a character-wise move steps over it in one press; the filter above does the rest. */
const hungPrefixAtomicRanges = (view: EditorView): RangeSet<Decoration> => view.plugin(markdownStylePlugin)?.atoms ?? RangeSet.empty;

export function markdownStyleExtension(): Extension {
  return [
    markdownStylePlugin,
    listHangExtension(),
    EditorView.atomicRanges.of(hungPrefixAtomicRanges),
    headFitExtension(),
  ];
}
