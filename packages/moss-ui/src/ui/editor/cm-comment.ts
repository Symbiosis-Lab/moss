// packages/moss-ui/src/ui/editor/cm-comment.ts
import { Decoration, type DecorationSet, EditorView, gutter, GutterMarker, keymap, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { RangeSetBuilder, StateField, StateEffect, EditorSelection, Prec, type StateEffectType, type Extension, type RangeSet } from '@codemirror/state';
import type { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { parseMarks, type ParsedMark } from '@symbiosis-lab/moss-syntax/cm6';
import { t } from "../i18n/index";
import { suppressTooltips } from "../components/tooltip";

export interface Comment {
  from: number;
  to: number;
  spanFrom?: number;
  spanTo?: number;
  noteFrom: number;
  noteTo: number;
  line: number;
}

/**
 * Derive comments from the CriticMarkup marks. A `highlight` immediately
 * followed (no gap) by a `comment` is a span comment anchored to the highlight;
 * a lone `comment` is a point comment. Unpaired highlights are left to
 * cm-criticmarkup's plain-highlight fallback.
 */
export function parseComments(state: EditorState): Comment[] {
  const marks = parseMarks(state); // already sorted, non-overlapping
  const out: Comment[] = [];
  for (let i = 0; i < marks.length; i++) {
    const m = marks[i];
    if (m.type === 'comment') {
      out.push(pointComment(state, m));
    } else if (m.type === 'highlight') {
      const next = marks[i + 1];
      if (next && next.type === 'comment' && next.from === m.to) {
        out.push(spanComment(state, m, next));
        i++; // consume the paired comment
      }
      // else: unpaired highlight → ignored here (cm-criticmarkup renders it)
    }
  }
  return out;
}

function lineOf(state: EditorState, pos: number): number {
  return state.doc.lineAt(pos).number;
}

// `{>>` = 3 chars, `<<}` = 3 chars.
function pointComment(state: EditorState, c: ParsedMark): Comment {
  return {
    from: c.from,
    to: c.to,
    noteFrom: c.from + 3,
    noteTo: c.to - 3,
    line: lineOf(state, c.from),
  };
}

// highlight `{==` = 3, `==}` = 3; comment `{>>` = 3, `<<}` = 3.
function spanComment(state: EditorState, hl: ParsedMark, c: ParsedMark): Comment {
  return {
    from: hl.from,
    to: c.to,
    spanFrom: hl.from + 3,
    spanTo: hl.to - 3,
    noteFrom: c.from + 3,
    noteTo: c.to - 3,
    line: lineOf(state, hl.from),
  };
}

// ── Shared parsed-comments field ──────────────────────────────────────────────
// Single source: parse once per transaction, all other fields derive from here.

/** Exported for tests that want to read the pre-parsed comment list directly. */
export const commentParsedField = StateField.define<Comment[]>({
  create: (state) => parseComments(state),
  update(value, tr) {
    if (tr.docChanged) return parseComments(tr.state);
    if (syntaxTree(tr.state) != syntaxTree(tr.startState)) return parseComments(tr.state);
    return value;
  },
});

// ── Decorations ────────────────────────────────────────────────────────────

const HIGHLIGHT = Decoration.mark({ class: 'cm-criticmarkup-highlight' });
// The markup (`{==`, `==}{>>`, `<<}`) and the note are real characters on screen: the
// markup faint like every other CriticMarkup delimiter, the note faint and smaller.
// Both carry `cm-comment-trigger`: a press on them opens the comment popover, while
// a press on the highlighted text just places the caret in it.
const DELIM = Decoration.mark({ class: 'cm-criticmarkup-delim cm-comment-trigger' });
const NOTE = Decoration.mark({ class: 'cm-comment-note-text cm-comment-trigger' });

/** Private builder — accepts pre-parsed comments, no re-parse. Nothing is hidden or replaced. */
function buildDecorations(comments: Comment[]): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const c of comments) {
    if (c.spanFrom !== undefined && c.spanTo !== undefined) {
      builder.add(c.from, c.spanFrom, DELIM);                                    // {==
      if (c.spanFrom < c.spanTo) builder.add(c.spanFrom, c.spanTo, HIGHLIGHT);   // fox
      builder.add(c.spanTo, c.noteFrom, DELIM);                                  // ==}{>>
    } else {
      builder.add(c.from, c.noteFrom, DELIM);                                    // {>>
    }
    if (c.noteFrom < c.noteTo) builder.add(c.noteFrom, c.noteTo, NOTE);          // the note
    builder.add(c.noteTo, c.to, DELIM);                                          // <<}
  }
  return builder.finish();
}

/** Build the always-on comment decorations (no cursor-reveal toggle).
 *  Test seam: accepts an EditorState directly (calls parseComments internally). */
export function commentDecorations(state: EditorState): DecorationSet {
  return buildDecorations(parseComments(state));
}

const commentField = StateField.define<DecorationSet>({
  create: (state) => buildDecorations(state.field(commentParsedField)),
  update(value, tr) {
    if (tr.docChanged) return buildDecorations(tr.state.field(commentParsedField));
    if (syntaxTree(tr.state) != syntaxTree(tr.startState)) return buildDecorations(tr.state.field(commentParsedField));
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

// ── Gutter ────────────────────────────────────────────────────────────────────

export const openCommentPopover: StateEffectType<{ from: number }> =
  StateEffect.define<{ from: number }>();

/** Test seam: the positions where gutter markers go. */
export function commentGutterMarkers(state: EditorState): { from: number }[] {
  return parseComments(state).map((c) => ({ from: c.from }));
}

class CommentGutterMarker extends GutterMarker {
  eq() { return true; }
  toDOM() {
    const el = document.createElement('span');
    el.className = 'cm-comment-gutter';
    el.textContent = '💬';
    return el;
  }
}
const GUTTER_MARKER = new CommentGutterMarker();

/** Private builder — accepts pre-parsed comments, no re-parse. */
function buildGutterFromComments(comments: Comment[]): RangeSet<GutterMarker> {
  const b = new RangeSetBuilder<GutterMarker>();
  for (const c of comments) b.add(c.from, c.from, GUTTER_MARKER);
  return b.finish();
}

const gutterMarkerField = StateField.define<RangeSet<GutterMarker>>({
  create: (state) => buildGutterFromComments(state.field(commentParsedField)),
  update(value, tr) {
    if (tr.docChanged) return buildGutterFromComments(tr.state.field(commentParsedField));
    if (syntaxTree(tr.state) != syntaxTree(tr.startState)) return buildGutterFromComments(tr.state.field(commentParsedField));
    return value;
  },
});

export function commentGutter(): Extension {
  return [
    gutterMarkerField,
    gutter({
      class: 'cm-comment-gutter-col',
      markers: (view) => view.state.field(gutterMarkerField),
      // No initialSpacer — the gutter column only reserves width once a comment
      // exists, so comment-free files keep their full text width.
      domEventHandlers: {
        mousedown(view, line) {
          // One glyph per line → clicking opens the FIRST comment on that line.
          // Additional comments on the same line are reached via their inline
          // pin / highlight. Accepted limitation, not a bug.
          const hit = view.state.field(commentParsedField).find(
            (c) => view.state.doc.lineAt(c.from).from === line.from,
          );
          if (!hit) return false;
          view.dispatch({ effects: openCommentPopover.of({ from: hit.from }) });
          return true;
        },
      },
    }),
  ];
}

/** Insert a span or point comment and request the popover on the new note. */
export function insertCommentAndEdit(view: EditorView): boolean {
  if (view.state.readOnly) return false;
  let { from, to } = view.state.selection.main;   // use `let`, not const
  if (from !== to && view.state.doc.sliceString(from, to).includes('\n\n')) {
    to = from; // collapse → point comment at the selection start
  }
  let insert: string;
  let noteStart: number; // doc offset of the first char inside {>> … <<}
  if (from === to) {
    insert = '{>><<}';
    noteStart = from + 3;
  } else {
    const sel = view.state.doc.sliceString(from, to);
    insert = `{==${sel}==}{>><<}`;
    noteStart = from + `{==${sel}==}{>>`.length;
  }
  view.dispatch({
    changes: { from, to, insert },
    selection: { anchor: noteStart },
    effects: openCommentPopover.of({ from }),
  });
  view.focus();
  return true;
}

export function commentKeymap(): Extension {
  return keymap.of([{ key: 'Mod-Shift-m', run: insertCommentAndEdit }]);
}

// ── Obsidian-style %% comment toggle (Mod-/) ───────────────────────────────────

/**
 * Toggle Obsidian-style `%%…%%` comments on ⌘/ (Ctrl+/).
 *
 * - With a selection: wrap it in `%%…%%`, or unwrap if it already is one.
 * - With no selection: operate on the current line's trimmed content (wrap it,
 *   or unwrap if the line is already a `%%…%%` comment). An empty line inserts
 *   `%%%%` with the cursor between the delimiters.
 *
 * Aligns moss with Obsidian, replacing CM6's default `toggleComment` (bound to
 * Mod-/ by `defaultKeymap`), which inserts HTML `<!-- -->` in markdown. Bind at
 * high precedence (see `obsidianCommentKeymap`) so it wins over that default.
 */
export function toggleObsidianComment(view: EditorView): boolean {
  const { state } = view;
  const tr = state.changeByRange((range) => {
    let from = range.from;
    let to = range.to;
    if (from === to) {
      // No selection → the current line's trimmed content.
      const line = state.doc.lineAt(from);
      const lead = line.text.length - line.text.trimStart().length;
      const trail = line.text.length - line.text.trimEnd().length;
      from = line.from + lead;
      to = line.to - trail;
    }
    const selected = state.doc.sliceString(from, to);
    const inner = /^%%([\s\S]*)%%$/.exec(selected);
    if (inner) {
      // Unwrap: drop the delimiters, keep the inner text selected.
      const insert = inner[1];
      return { changes: { from, to, insert }, range: EditorSelection.range(from, from + insert.length) };
    }
    // Wrap. With no prior selection, place the cursor between the delimiters.
    const insert = `%%${selected}%%`;
    const wasEmpty = range.from === range.to;
    return {
      changes: { from, to, insert },
      range: wasEmpty
        ? EditorSelection.cursor(from + 2)
        : EditorSelection.range(from, from + insert.length),
    };
  });
  view.dispatch(state.update(tr, { userEvent: 'input.comment', scrollIntoView: true }));
  return true;
}

/** High-precedence Mod-/ keymap so the Obsidian `%%` toggle wins over CM6's
 *  default `toggleComment` (HTML `<!-- -->`) from `minimalSetup`'s defaultKeymap. */
export function obsidianCommentKeymap(): Extension {
  return Prec.high(keymap.of([{ key: 'Mod-/', run: toggleObsidianComment }]));
}

// ── Resolve helper ────────────────────────────────────────────────────────────

/** Resolve = apply CriticMarkup "accept" to one unit: span → its text, point → "". */
export function resolveCommentChange(c: Comment, state: EditorState): { from: number; to: number; insert: string } {
  const insert = c.spanFrom !== undefined && c.spanTo !== undefined
    ? state.doc.sliceString(c.spanFrom, c.spanTo)
    : '';
  return { from: c.from, to: c.to, insert };
}

// ── Note sanitization ─────────────────────────────────────────────────────────

/**
 * Prevent the closing delimiter `<<}` from appearing inside a committed note.
 * If the user types `<<}` it would close the comment early and leak stray text
 * into the document. Replace any occurrence with `<< }` (a space inserted before
 * the brace) — visually indistinguishable and safe.
 */
export function sanitizeNote(text: string): string {
  return text.replaceAll('<<}', '<< }');
}

// ── Popover ViewPlugin ────────────────────────────────────────────────────────

const commentPopoverPlugin = ViewPlugin.fromClass(
  class {
    private el: HTMLDivElement | null = null;
    private activeFrom: number | null = null;
    private domAbort = new AbortController();

    constructor(private view: EditorView) {
      this.view.dom.addEventListener('mousedown', (ev) => {
        const t = ev.target as HTMLElement;
        // Only the comment's markup and note open the popover on a press;
        // a press inside a highlighted span must NOT preventDefault so the
        // caret can be placed normally inside the commented text. The gutter 💬
        // glyph opens it too (commentGutter.domEventHandlers).
        if (!t.closest('.cm-comment-trigger')) return;
        const pos = this.view.posAtDOM(t);
        const hit = this.view.state.field(commentParsedField).find((c) => pos >= c.from && pos <= c.to);
        if (!hit) return;
        ev.preventDefault();
        this.view.dispatch({ effects: openCommentPopover.of({ from: hit.from }) });
      }, { signal: this.domAbort.signal });
    }

    update(u: ViewUpdate) {
      if (this.activeFrom !== null && u.docChanged) {
        this.activeFrom = u.changes.mapPos(this.activeFrom, 1);
      }
      let toOpen: number | null = null;
      for (const tr of u.transactions) {
        for (const e of tr.effects) if (e.is(openCommentPopover)) toOpen = e.value.from;
      }
      if (toOpen !== null) { this.openFor(toOpen); return; }
      if (this.el && this.activeFrom !== null && u.docChanged) {
        const still = this.view.state.field(commentParsedField).some((c) => c.from === this.activeFrom);
        if (!still) this.close();
        else this.scheduleReposition();
      }
    }

    private current(): Comment | undefined {
      if (this.activeFrom === null) return undefined;
      return this.view.state.field(commentParsedField).find((c) => c.from === this.activeFrom);
    }

    private releaseTooltips: (() => void) | null = null;

    private openFor(from: number) {
      this.close();
      this.activeFrom = from;
      const c = this.current();
      if (!c) { this.activeFrom = null; return; }
      const el = document.createElement('div');
      el.className = 'cm-comment-popover';
      const ta = document.createElement('textarea');
      ta.className = 'cm-comment-note';
      ta.value = this.view.state.doc.sliceString(c.noteFrom, c.noteTo);
      ta.setAttribute('aria-label', t('editor.comment_note_aria'));
      const resolveBtn = document.createElement('button');
      resolveBtn.className = 'moss-link--subtle';
      resolveBtn.textContent = t('editor.comment_resolve');
      resolveBtn.addEventListener('mousedown', (e) => { e.preventDefault(); this.resolve(); });
      ta.addEventListener('blur', () => this.commit(ta.value));
      ta.addEventListener('keydown', (ev) => {
        if (ev.key === 'Escape') { ev.preventDefault(); this.commit(ta.value); this.view.focus(); }
        if (ev.key === 'Enter' && (ev.metaKey || ev.ctrlKey)) { ev.preventDefault(); this.commit(ta.value); this.view.focus(); }
      });
      el.append(ta, resolveBtn);
      el.style.visibility = 'hidden'; // hide until positioned to prevent (0,0) flash
      document.body.appendChild(el);
      this.el = el;
      // `--z-overlay`, above a `--z-popup` tooltip — see cm-slash-menu.ts for
      // why opening on a keystroke does not excuse the wiring.
      this.releaseTooltips = suppressTooltips(el);
      this.scheduleReposition();
      ta.focus();
    }

    // coordsAtPos is a layout read — forbidden during update(); defer to the measure phase.
    private scheduleReposition() {
      this.view.requestMeasure({
        read: () => (this.activeFrom !== null ? this.view.coordsAtPos(this.activeFrom) : null),
        write: (coords) => {
          if (!coords || !this.el) return;
          const w = this.el.offsetWidth, h = this.el.offsetHeight;
          const left = Math.max(8, Math.min(coords.left, window.innerWidth - w - 8));
          const top = Math.max(8, Math.min(coords.bottom + 4, window.innerHeight - h - 8));
          this.el.style.left = `${left}px`;
          this.el.style.top = `${top}px`;
          this.el.style.visibility = 'visible'; // reveal after positioning
        },
      });
    }

    private commit(text: string) {
      const c = this.current();
      if (c && !this.view.state.readOnly) {
        const safe = sanitizeNote(text);
        const cur = this.view.state.doc.sliceString(c.noteFrom, c.noteTo);
        if (cur !== safe) this.view.dispatch({ changes: { from: c.noteFrom, to: c.noteTo, insert: safe } });
      }
      this.close();
    }

    private resolve() {
      const c = this.current();
      if (c && !this.view.state.readOnly) this.view.dispatch({ changes: resolveCommentChange(c, this.view.state) });
      this.close();
      this.view.focus();
    }

    private close() {
      this.el?.remove();
      this.el = null;
      this.activeFrom = null;
      this.releaseTooltips?.();
      this.releaseTooltips = null;
    }

    destroy() { this.close(); this.domAbort.abort(); }
  },
);

export function commentPopover(): Extension { return [commentPopoverPlugin]; }

export function commentExtension(): Extension {
  return [
    commentParsedField,        // FIRST: other fields read this
    commentField,
    commentGutter(),
    commentKeymap(),
    commentPopover(),
  ];
}

// ── Percent comments (%% ... %%) ──────────────────────────────────────────────
//
// Obsidian-style hidden notes: %%content%% (inline, same line) and block form
// where each %% delimiter is alone on its own line. Shown muted in the editor,
// stripped from build output (Rust `strip_percent_comments` in html_post.rs).
//
// NOT wired into the CriticMarkup resolve/popover workflow: these are simple
// hidden notes with no accept/reject semantics (matches Obsidian behavior).
//
// Guard: a lone `%` in prose (e.g. "50% off") does NOT contain `%%` and is
// never matched. Only a balanced `%%...%%` pair on one line (inline) or a pair
// where each delimiter is alone on its line (block) is treated as a comment.

/** A single percent-comment span in the document. */
export interface PercentComment {
  /** Start offset of the opening `%%` */
  from: number;
  /** End offset (exclusive) of the closing `%%` */
  to: number;
  /** 'inline' = balanced pair on one line; 'block' = delimiters on own lines */
  kind: 'inline' | 'block';
}

/** Node names whose content is literal code — percent comments are not stripped there. */
const PCT_CODE_NODE_NAMES = new Set(['FencedCode', 'CodeBlock', 'InlineCode']);

/**
 * Return the document text with code region characters replaced by spaces
 * (newlines preserved). Uses the syntax tree for accurate code detection,
 * mirroring the strategy in cm-criticmarkup.ts.
 */
function maskCodeForPercent(state: EditorState): string {
  const text = state.doc.toString();
  const ranges: Array<{ from: number; to: number }> = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (!PCT_CODE_NODE_NAMES.has(node.name)) return;
      ranges.push({ from: node.from, to: node.to });
      return false;
    },
  });
  if (ranges.length === 0) return text;
  const out = text.split('');
  for (const { from, to } of ranges) {
    for (let i = from; i < to && i < out.length; i++) {
      if (out[i] !== '\n') out[i] = ' ';
    }
  }
  return out.join('');
}

/**
 * Parse `%%...%%` comments from the document. Skips code regions.
 * Block form: each `%%` delimiter is alone on its own line (after trim).
 * Inline form: a balanced `%%...%%` pair on a single line.
 */
export function parsePercentComments(state: EditorState): PercentComment[] {
  const masked = maskCodeForPercent(state);
  const text = state.doc.toString();
  const result: PercentComment[] = [];

  // Build line-start byte offsets.
  const lineCount = state.doc.lines;
  const lineStarts: number[] = [];
  for (let i = 1; i <= lineCount; i++) {
    lineStarts.push(state.doc.line(i).from);
  }

  // Track which character ranges are already consumed by block comments so the
  // inline pass skips them.
  const blockCovered: Array<{ from: number; to: number }> = [];

  // --- Pass 1: block form ---
  // A "block delimiter" line is one whose masked content, after trim, equals '%%'.
  let lineIdx = 0;
  while (lineIdx < lineCount) {
    const lineFrom = lineStarts[lineIdx];
    const lineTo = lineIdx + 1 < lineCount ? lineStarts[lineIdx + 1] - 1 : text.length;
    const maskedLine = masked.slice(lineFrom, lineTo);
    if (maskedLine.trim() === '%%') {
      // Found opening delimiter. Search for the next block delimiter.
      const openLineIdx = lineIdx;
      let j = lineIdx + 1;
      let found = false;
      while (j < lineCount) {
        const jFrom = lineStarts[j];
        const jTo = j + 1 < lineCount ? lineStarts[j + 1] - 1 : text.length;
        const jLine = masked.slice(jFrom, jTo);
        if (jLine.trim() === '%%') {
          // Closing delimiter at line j.
          const commentFrom = lineStarts[openLineIdx];
          // Include the newline after the closing delimiter line if present.
          const commentTo = j + 1 < lineCount ? lineStarts[j + 1] : text.length;
          result.push({ from: commentFrom, to: commentTo, kind: 'block' });
          blockCovered.push({ from: commentFrom, to: commentTo });
          lineIdx = j + 1;
          found = true;
          break;
        }
        j++;
      }
      if (!found) {
        lineIdx++;
      }
    } else {
      lineIdx++;
    }
  }

  // --- Pass 2: inline form ---
  // On each line not already covered by a block comment, find balanced `%%...%%`
  // pairs within the masked text.
  for (let i = 0; i < lineCount; i++) {
    const lineFrom = lineStarts[i];
    const lineTo = i + 1 < lineCount ? lineStarts[i + 1] - 1 : text.length;

    // Skip if this line is inside a block comment.
    const covered = blockCovered.some((r) => r.from <= lineFrom && r.to >= lineTo);
    if (covered) continue;

    const maskedLine = masked.slice(lineFrom, lineTo);
    let searchFrom = 0;
    for (;;) {
      const first = maskedLine.indexOf('%%', searchFrom);
      if (first === -1) break;
      const afterFirst = first + 2;
      const second = maskedLine.indexOf('%%', afterFirst);
      if (second === -1) break;
      const secondEnd = second + 2;
      result.push({
        from: lineFrom + first,
        to: lineFrom + secondEnd,
        kind: 'inline',
      });
      searchFrom = secondEnd;
    }
  }

  // Sort by document order.
  result.sort((a, b) => a.from - b.from);
  return result;
}

/** Muted decoration for the entire %%...%% span (including delimiters). */
const PCT_COMMENT_MARK = Decoration.mark({ class: 'cm-pct-comment' });

function buildPercentDecorations(state: EditorState): DecorationSet {
  const comments = parsePercentComments(state);
  const builder = new RangeSetBuilder<Decoration>();
  for (const c of comments) {
    builder.add(c.from, c.to, PCT_COMMENT_MARK);
  }
  return builder.finish();
}

const percentCommentField = StateField.define<DecorationSet>({
  create: (state) => buildPercentDecorations(state),
  update(value, tr) {
    if (!tr.docChanged && syntaxTree(tr.state) === syntaxTree(tr.startState)) {
      return value.map(tr.changes);
    }
    return buildPercentDecorations(tr.state);
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** CM6 extension for %% comment decoration. */
export function percentCommentExtension(): Extension {
  return [percentCommentField];
}
