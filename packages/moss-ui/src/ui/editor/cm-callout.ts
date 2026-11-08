// CM6 decorations for > [!callout] blocks.
// (Renamed from cm-shortcode.ts — shortcode BLOCK rendering moved to the Lezer
// grammar + block field in @symbiosis-lab/moss-syntax: shortcode.ts / cm6/cm-shortcode-block.ts.)
//
// Callout detection is TREE-DRIVEN: a callout IS a markdown Blockquote whose
// first line opens with `[!type]`, so the extractor walks `syntaxTree(state)`
// Blockquote nodes instead of regex-scanning the document text. Code-region
// suppression falls out for free — fenced/indented code produces no
// Blockquote node — and the per-keystroke cost is a tree walk over an
// incrementally-maintained parse, not a full-document split.

import { Extension, RangeSetBuilder, StateField } from '@codemirror/state';
import type { EditorState } from '@codemirror/state';
import { Decoration, DecorationSet, EditorView } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';

// ── Types ──────────────────────────────────────────────────────────────

export interface CalloutBlock {
  /** Character offset of the block start */
  from: number;
  /** Character offset of the block end */
  to: number;
  /** Callout type (e.g. "info", "warning") */
  type: string;
}

// ── Extractor (pure over EditorState) ──────────────────────────────────

/** `[!type]` after the `>` marker on the blockquote's first line. */
const CALLOUT_OPEN = /^>\s*\[!(\w+)\]/;

/**
 * Collect callout blocks from the syntax tree: every OUTERMOST Blockquote
 * whose first line matches `> [!type]`. Nested blockquotes are not descended
 * into — a callout is a top-level authoring construct, and the outer block
 * owns the tint/label.
 */
export function extractCalloutBlocks(state: EditorState): CalloutBlock[] {
  const blocks: CalloutBlock[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== 'Blockquote') return;
      const line = state.doc.lineAt(node.from);
      const m = CALLOUT_OPEN.exec(state.doc.sliceString(line.from, line.to));
      if (m) blocks.push({ from: node.from, to: node.to, type: m[1] });
      return false; // never descend into nested quotes
    },
  });
  return blocks;
}

/**
 * Generate the CSS class string for a callout line decoration.
 * Always includes the base `cm-callout-block` class plus a type-specific class.
 */
export function calloutLineClass(type: string): string {
  return `cm-callout-block cm-callout-${type.toLowerCase()}`;
}

/**
 * Generate the CSS class string for the `[!type]` label's characters. Always
 * includes the base `cm-callout-mark` class plus a type-specific class (which
 * also sets the callout's colour variable).
 */
export function calloutMarkClass(type: string): string {
  return `cm-callout-mark cm-callout-${type.toLowerCase()}`;
}

// ── Decoration builders ────────────────────────────────────────────────

/**
 * Build callout decorations (moving the caret never changes what you see;
 * nothing is hidden):
 *
 *   - The tint line class is ALWAYS applied to EVERY line of the block (no
 *     layout shift — same rule as headings).
 *     The class carries the bar/tint ::before AND the +12px text indent
 *     that keeps text clear of the bar.
 *   - The `[!type]` label on the first line stays as its own characters,
 *     faint and tinted in the callout's colour (a mark, never a replace). `> `
 *     is hung by cm-markdown-style.ts on every blockquote line, callout or not.
 *   - Continuation lines carry only the tint line class.
 */
export function buildCalloutDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const decorations: { from: number; to: number; dec: Decoration }[] = [];

  for (const block of extractCalloutBlocks(state)) {
    decorations.push({
      from: block.from,
      to: block.from,
      dec: Decoration.line({ class: calloutLineClass(block.type) }),
    });

    const firstLine = state.doc.lineAt(block.from);
    const m = CALLOUT_OPEN.exec(state.doc.sliceString(firstLine.from, firstLine.to));
    if (m) {
      decorations.push({
        from: firstLine.from + m[0].indexOf('['),
        to: firstLine.from + m[0].length,
        dec: Decoration.mark({ class: calloutMarkClass(block.type) }),
      });
    }

    // Continuation lines: same tint line class (bar + indent span the whole
    // block). Their leading `> ` is cm-markdown-style's hang, not hidden here.
    let pos = firstLine.to + 1;
    while (pos <= block.to) {
      const ln = state.doc.lineAt(pos);
      decorations.push({
        from: ln.from,
        to: ln.from,
        dec: Decoration.line({ class: calloutLineClass(block.type) }),
      });
      if (ln.to + 1 > block.to) break;
      pos = ln.to + 1;
    }
  }

  decorations.sort((a, b) => a.from - b.from || a.to - b.to);
  for (const d of decorations) {
    builder.add(d.from, d.to, d.dec);
  }

  return builder.finish();
}

// ── StateField ─────────────────────────────────────────────────────────

const calloutDecorations = StateField.define<DecorationSet>({
  create(state) {
    return buildCalloutDecorations(state);
  },
  update(value, tr) {
    // Rebuild on doc changes, or when the incremental parse advanced (the
    // parse worker extends the tree via effect-only transactions on large
    // docs). Never on selection — the badge is always on now, so nothing
    // here reads the caret (the global "no decoration builder reads the
    // selection" constraint).
    const treeAdvanced = syntaxTree(tr.state) != syntaxTree(tr.startState);
    if (!tr.docChanged && !treeAdvanced) return value;
    return buildCalloutDecorations(tr.state);
  },
  provide(field) {
    return EditorView.decorations.from(field);
  },
});

// ── Public extension ───────────────────────────────────────────────────

export function calloutExtension(): Extension[] {
  // Callout (> [!note]) decorations only — the slash-command insertion menu
  // is wired directly in cm-editor.ts now, not smuggled through here.
  return [calloutDecorations];
}
