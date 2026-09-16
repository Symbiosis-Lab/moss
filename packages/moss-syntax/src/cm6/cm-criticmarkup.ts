// CM6 ViewPlugin for CriticMarkup syntax highlighting.
// Recognizes the 5 CriticMarkup token types and decorates them with CSS classes.
// Skips matches inside fenced/indented/inline code so example markup in docs
// is preserved.
//
// Patterns follow the MultiMarkdown-6 spec: https://fletcher.github.io/MultiMarkdown-6/syntax/critic.html
//
// ── Why this is NOT a Lezer inline grammar (deliberate, evaluated for #486) ──
//
// The build pipeline's `accept_criticmarkup` (src-tauri/src/build/markdown/
// html_post.rs) strips/applies CriticMarkup with `(?s)` regexes that CROSS
// PARAGRAPH BOUNDARIES — `{++a\n\nb++}` is one accepted edit. A Lezer inline
// parser is paragraph-bounded by construction (inline contexts never span
// blank lines), so a grammar-based editor layer would fail to mark spans the
// build WILL accept — divergence in the worse direction. Instead this module
// runs the SAME five patterns as the Rust side (parity by construction) and
// delegates the one thing the syntax tree IS authoritative for — which
// regions are code — to `syntaxTree(state)` instead of a hand-rolled
// CommonMark re-implementation.

import { ViewPlugin, ViewUpdate, Decoration, DecorationSet, EditorView } from '@codemirror/view';
import { RangeSetBuilder, Extension, ChangeSet } from '@codemirror/state';
import type { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';

// ── Types ──────────────────────────────────────────────────────────────

export type CMType = 'addition' | 'deletion' | 'substitution' | 'highlight' | 'comment';

export interface ParsedMark {
  /** Character offset of the opening `{` */
  from: number;
  /** Character offset just after the closing `}` */
  to: number;
  /** Token type */
  type: CMType;
  /** For substitution only: offset of the `~>` split */
  mid?: number;
}

// ── Patterns ───────────────────────────────────────────────────────────
//
// MIRROR of the build's accept_criticmarkup regexes (html_post.rs) — keep in
// sync. Order matters: substitution must be tried before deletion so
// `{~~...~~}` is not misread. All patterns use non-greedy `[\s\S]*?` so they
// can span lines (and paragraphs — see header) but don't overreach.

const PATTERNS: Array<{ re: RegExp; type: CMType }> = [
  { re: /\{~~([\s\S]*?)~>([\s\S]*?)~~\}/g, type: 'substitution' },
  { re: /\{\+\+([\s\S]*?)\+\+\}/g, type: 'addition' },
  { re: /\{--([\s\S]*?)--\}/g, type: 'deletion' },
  { re: /\{==([\s\S]*?)==\}/g, type: 'highlight' },
  { re: /\{>>([\s\S]*?)<<\}/g, type: 'comment' },
];

// ── Parser ─────────────────────────────────────────────────────────────

/**
 * Parse CriticMarkup tokens from the document, skipping tokens inside code
 * regions (fenced blocks, indented blocks, inline spans) as reported by the
 * SYNTAX TREE — the same code regions the editor highlights as code.
 *
 * @returns Array of parsed marks in document order.
 */
export function parseMarks(state: EditorState): ParsedMark[] {
  // Step 1: mask out code regions by replacing their characters with spaces.
  // Offsets are preserved so match indexes map back to the original text.
  const masked = maskCodeFromTree(state);

  // Step 2: run each pattern on the masked text.
  const marks: ParsedMark[] = [];
  for (const { re, type } of PATTERNS) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(masked)) !== null) {
      const mark: ParsedMark = {
        from: m.index,
        to: m.index + m[0].length,
        type,
      };
      if (type === 'substitution') {
        // `m[1]` is the old side — the `~>` sits right after it.
        mark.mid = m.index + 3 + m[1].length;
      }
      marks.push(mark);
    }
  }

  // Step 3: sort by start offset so adjacent marks come out in document order.
  marks.sort((a, b) => a.from - b.from);

  // Step 4: drop any mark whose range overlaps an earlier one. When both the
  // outer highlight `{==...==}` and an inner pattern like `{--...--}` embedded
  // in the highlight's content match, we want to keep the outer and discard
  // the inner — otherwise CM6's RangeSetBuilder rejects the overlap.
  const result: ParsedMark[] = [];
  let lastEnd = 0;
  for (const m of marks) {
    if (m.from < lastEnd) continue;
    result.push(m);
    lastEnd = m.to;
  }
  return result;
}

/** Lezer node names whose content is literal code — no CriticMarkup inside. */
const CODE_NODE_NAMES = new Set(['FencedCode', 'CodeBlock', 'InlineCode']);

/**
 * Return the document text with characters inside code regions replaced by
 * spaces (newlines preserved so offsets and line numbers still line up).
 * Code regions come from the syntax tree — fenced blocks, INDENTED blocks
 * (which the old hand-rolled masking missed), and inline spans — so this
 * agrees byte-for-byte with what the editor treats as code.
 */
function maskCodeFromTree(state: EditorState): string {
  const text = state.doc.toString();
  const ranges: Array<{ from: number; to: number }> = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (!CODE_NODE_NAMES.has(node.name)) return;
      ranges.push({ from: node.from, to: node.to });
      return false; // children (CodeText, CodeMark) are covered by the parent
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

// ── Decorations ────────────────────────────────────────────────────────

const addMark = Decoration.mark({ class: 'cm-criticmarkup-addition' });
const delMark = Decoration.mark({ class: 'cm-criticmarkup-deletion' });
const subMark = Decoration.mark({ class: 'cm-criticmarkup-substitution' });
const subOldMark = Decoration.mark({ class: 'cm-criticmarkup-substitution-old' });
const subNewMark = Decoration.mark({ class: 'cm-criticmarkup-substitution-new' });
const hlMark = Decoration.mark({ class: 'cm-criticmarkup-highlight' });
// The 3-char `{++` / `++}` markers, muted — delimiter class kept distinct from
// the payload classes (design-vocabulary "revealed markup is always
// token-highlighted"; same split as cm-sc-delim in cm-shortcode-block.ts).
const delimMark = Decoration.mark({ class: 'cm-criticmarkup-delim' });

/** Emit one mark as muted 3-char delimiters + styled payload between them.
 *  Empty payloads (`{++++}`) add no payload range — CM6 rejects empty marks. */
function addDelimited(builder: RangeSetBuilder<Decoration>, m: ParsedMark, payload: Decoration): void {
  builder.add(m.from, m.from + 3, delimMark);
  if (m.from + 3 < m.to - 3) builder.add(m.from + 3, m.to - 3, payload);
  builder.add(m.to - 3, m.to, delimMark);
}

function buildDecorations(state: EditorState): DecorationSet {
  const marks = parseMarks(state);
  const builder = new RangeSetBuilder<Decoration>();
  for (let i = 0; i < marks.length; i++) {
    const m = marks[i];
    switch (m.type) {
      case 'addition': addDelimited(builder, m, addMark); break;
      case 'deletion': addDelimited(builder, m, delMark); break;
      case 'highlight': {
        // Only draw the unpaired highlight fallback when NOT immediately followed
        // by a comment mark — cm-comment.ts owns paired highlight+comment spans.
        const next = marks[i + 1];
        const paired = next && next.type === 'comment' && next.from === m.to;
        if (!paired) addDelimited(builder, m, hlMark);
        break;
      }
      // (no 'comment' case — cm-comment.ts renders comments)
      case 'substitution':
        if (m.mid === undefined) {
          addDelimited(builder, m, subMark);
          break;
        }
        // Delimiters muted; the payload keeps the wash (subMark) with the
        // old/new halves layered on top (adds stay sorted by `from` for the
        // RangeSetBuilder). The `~>` split keeps subMark alone, as before.
        builder.add(m.from, m.from + 3, delimMark);
        if (m.from + 3 < m.to - 3) builder.add(m.from + 3, m.to - 3, subMark);
        if (m.from + 3 < m.mid) builder.add(m.from + 3, m.mid, subOldMark);
        if (m.mid + 2 < m.to - 3) builder.add(m.mid + 2, m.to - 3, subNewMark);
        builder.add(m.to - 3, m.to, delimMark);
        break;
    }
  }
  return builder.finish();
}

// ── CM6 Extension ──────────────────────────────────────────────────────

// PATTERN 7 — INCREMENTAL DECORATION via RangeSet.map + MatchDecorator
// see docs/archive/2026-05-22-editor-state-architecture.md
// Canonical reference: @codemirror/view src/matchdecorator.ts (MatchDecorator.updateDeco)

// Characters that can create, destroy, or restructure a CriticMarkup mark or
// the code regions that suppress them. If neither the inserted nor the
// deleted text contains any of these, the set of marks in the document is
// unchanged and we can re-use the existing DecorationSet by simply mapping
// it through the transaction's changes. Otherwise we re-parse the whole doc.
// (Known gap: an indented code block created by typing leading SPACES won't
// trigger until the next trigger-char edit — accepted, the old masking did
// not handle indented code at all.)
const REBUILD_TRIGGER = /[{}+\-=<>~`]/;

/** Slice all inserted text from a ChangeSet into a single string. */
function changedInsertedText(changes: ChangeSet): string {
  let inserted = '';
  changes.iterChanges((_fromA, _toA, _fromB, _toB, ins) => {
    if (ins.length > 0) inserted += ins.toString();
  });
  return inserted;
}

/** Slice all removed text out of the pre-change doc into a single string. */
function changedRemovedText(changes: ChangeSet, oldDoc: { sliceString(from: number, to: number): string }): string {
  let removed = '';
  changes.iterChanges((fromA, toA) => {
    if (toA > fromA) removed += oldDoc.sliceString(fromA, toA);
  });
  return removed;
}

/** Number of changed bytes (max contiguous changed window in the new doc). */
function changeSpan(changes: ChangeSet): number {
  let lo = Infinity;
  let hi = -Infinity;
  changes.iterChanges((_fromA, _toA, fromB, toB) => {
    if (fromB < lo) lo = fromB;
    if (toB > hi) hi = toB;
  });
  return hi < 0 ? 0 : hi - lo;
}

/**
 * CM6 extension that highlights CriticMarkup tokens.
 *
 * Incremental strategy (PATTERN 7): on each transaction, `RangeSet.map` re-
 * positions the existing decorations through the change set in O(log n). Only
 * when the change inserts/deletes a CriticMarkup or code trigger character —
 * or the change is unusually large — do we fall back to a full re-parse.
 * Typical typing of plain prose costs one `RangeSet.map`, not a whole-document
 * regex scan. Effect-only transactions rebuild only when the incremental
 * parse advanced (code masking beyond the initially-parsed region).
 */
export function criticmarkupExtension(): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      // Test-only counter: counts full-document rebuilds. Read in benchmark
      // assertions to prove the incremental path is taken on common edits.
      _fullBuilds = 0;

      constructor(view: EditorView) {
        this.decorations = buildDecorations(view.state);
        this._fullBuilds = 1;
      }

      update(update: ViewUpdate) {
        if (!update.docChanged) {
          // Parse-worker advance (large docs): code masking may now cover
          // regions it could not see at the last build — re-mask and re-parse.
          if (syntaxTree(update.state) != syntaxTree(update.startState)) {
            this.decorations = buildDecorations(update.state);
            this._fullBuilds++;
          }
          return;
        }
        const { changes } = update;
        // Fast path: re-map existing decorations through the change set when
        // no trigger character was inserted or deleted and the change is small.
        // Mirrors @codemirror/view MatchDecorator.updateDeco's `viewportMoved ||
        // changeTo - changeFrom > 1000` guard, plus a content-trigger check
        // tuned to CriticMarkup's delimiter alphabet.
        const span = changeSpan(changes);
        const inserted = changedInsertedText(changes);
        const removed = changedRemovedText(changes, update.startState.doc);
        const touchesTrigger = REBUILD_TRIGGER.test(inserted) || REBUILD_TRIGGER.test(removed);
        if (span <= 1000 && !touchesTrigger) {
          this.decorations = this.decorations.map(changes);
          return;
        }
        this.decorations = buildDecorations(update.state);
        this._fullBuilds++;
      }
    },
    {
      decorations: (v) => v.decorations,
    },
  );
}
