// Footnote rendering for live preview — `[^1]` markers and `[^1]: note` lines.
//
// A self-contained pass beside the other syntax renderers: it reads the syntax
// tree, returns ranges, and touches nothing else. The grammar that produces the
// nodes it reads is `footnoteConfig` in ../footnote-grammar.ts.
//
// ── The one rule that is easy to get wrong ────────────────────────────
// A marker whose label has NO definition in the document renders LITERALLY —
// as the characters the author typed. That is not a stylistic choice, it is
// what the build does (pulldown-cmark, asserted by "Use [^0-9] to strip
// non-digits" in crates/moss-core/tests/footnotes_and_strikethrough.rs), and
// it is the difference between an editor that helps and one that eats your
// regex character classes. So this pass collects the defined labels FIRST and
// decorates only markers that match one.
//
// Reveal follows the editor-wide contract (cm-live-preview.ts header):
//   - a marker is an INLINE node → per-node reveal, raw when the cursor
//     touches it, its siblings on the same line unaffected;
//   - a definition line is BLOCK CHROME → per-line reveal, raw when the
//     cursor is on it.
// The definition's muted line tint is always applied, never toggled — the
// same no-layout-shift rule heading sizes and callout tints follow.
//
// The editor does NOT number the markers and does NOT hoist definitions into
// an endnote section, both of which the build does at render time. The editor
// renders source (ADR-022): the label is what the author typed, and the note
// stays on the line they put it on. A document whose markers read 2, 1, 3 is
// showing the author something true about their file.

import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import type { EditorState, Extension } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { getActiveLines, isNodeActive, nodeTouchesSelection } from './cm-active-lines.js';
import { revealInputsChangedIn } from './cm-source-mode.js';

/** A decoration to place, in the shape cm-live-preview collects. */
export interface FootnoteDeco {
  from: number;
  to: number;
  deco: Decoration;
}

const REPLACE = Decoration.replace({});
/** The label, raised — `[^` and `]` are hidden, so this IS the marker. */
const MARK_REF = Decoration.mark({ class: 'cm-lp-footnote-ref' });
/** The same treatment on a definition line, so marker and note read as a pair. */
const MARK_DEF_LABEL = Decoration.mark({ class: 'cm-lp-footnote-label' });
/** "There is somewhere to go from here" — the editor's ONE followable-cursor
 *  class (`.cm-editor.cm-meta-held .cm-link-clickable`, editor.css), the same
 *  one links and embed cards wear.
 *
 *  It is emitted from OUTSIDE the reveal gate below, and that is the whole
 *  point: a footnote is just as followable with its `[^1]` showing as with it
 *  rendered, so an affordance derived from the rendering promised nothing in
 *  raw source while Cmd+click followed anyway. Followability is a fact about
 *  the document, not about what the cursor happens to be near. */
const CLICKABLE = Decoration.mark({ class: 'cm-link-clickable' });
/** Always-on line tint marking a line as note rather than body prose. Applied
 *  to every line the definition spans — a note is a container, not a line. */
const LINE_DEF = Decoration.line({ class: 'cm-lp-footnote-def' });

/** Direct children of `node` named `name`, in order. */
function childrenNamed(node: any, name: string): Array<{ from: number; to: number }> {
  const out: Array<{ from: number; to: number }> = [];
  const cur = node.cursor();
  if (cur.firstChild()) {
    do {
      if (cur.name === name) out.push({ from: cur.from, to: cur.to });
    } while (cur.nextSibling());
  }
  return out;
}

/** Where a footnote label is written: the whole construct, and its label span. */
export interface FootnoteSite {
  /** The whole `[^1]` / `[^1]: …` node. */
  from: number;
  to: number;
  /** The label text alone — where a jump puts the caret, and what is raised. */
  labelFrom: number;
  labelTo: number;
  /** End of the trailing mark (`]` or `]:`), i.e. the end of the CHROME. On a
   *  definition this is well short of `to`; the note body follows. */
  markEnd: number;
}

/**
 * Every footnote in the document, indexed by label: where it is defined, and
 * where it is first referenced.
 *
 * One walk answers all three questions the feature asks — may this marker
 * render (is it defined), does this definition earn a back-jump (is it
 * referenced), and where does either jump land. Splitting them across three
 * walks is how a marker and its jump end up disagreeing about which
 * definition is "the" one when a label is defined twice.
 *
 * Duplicate labels: FIRST wins, on both sides. The build renders the first
 * definition's body and back-links to the first reference (ADR-035), so the
 * editor's jump lands where the reader's would.
 */
export function footnoteIndex(state: EditorState): {
  definitions: Map<string, FootnoteSite>;
  firstRefs: Map<string, FootnoteSite>;
} {
  const definitions = new Map<string, FootnoteSite>();
  const firstRefs = new Map<string, FootnoteSite>();

  syntaxTree(state).iterate({
    enter(node) {
      const into =
        node.name === 'FootnoteDefinition' ? definitions :
        node.name === 'FootnoteRef' ? firstRefs : null;
      if (!into) return;
      const label = childrenNamed(node.node, 'FootnoteLabel')[0];
      const marks = childrenNamed(node.node, 'FootnoteMark');
      if (!label || marks.length === 0) return;
      const text = state.doc.sliceString(label.from, label.to);
      if (!into.has(text)) {
        into.set(text, {
          from: node.from,
          to: node.to,
          labelFrom: label.from,
          labelTo: label.to,
          markEnd: marks[marks.length - 1].to,
        });
      }
      // Descend on a definition (its body can hold markers of its own — the
      // build walks them too, see FootnoteIndex::build); never on a marker.
      return node.name === 'FootnoteDefinition' ? undefined : false;
    },
  });

  return { definitions, firstRefs };
}

/**
 * Where following the footnote at `pos` should land, or null.
 *
 * A footnote is a reference like any other, so it answers the same two
 * gestures every other reference does (Cmd/Ctrl+click and Mod-Enter, wired in
 * cm-link-nav.ts) — but it navigates WITHIN the document rather than opening a
 * file, so it cannot go through `navigateToReference`.
 *
 * Both directions, matching what the built page gives a reader:
 *   - on a marker  → its definition;
 *   - on a definition's MARKER → its first reference (the build's `↩`
 *     back-link).
 *
 * The definition side is deliberately scoped to the `[^1]:` chrome rather than
 * the whole note. The note body is prose the author edits, and Mod-Enter with
 * the caret in the middle of a sentence should insert a line, not teleport.
 *
 * Ends excluded, the rule `followTargetAt` uses and for the same reason: at
 * `node.from` the caret is beside the construct, not on it, and Mod-Enter
 * should still be Mod-Enter there.
 */
export function footnoteJumpTarget(state: EditorState, pos: number): FootnoteSite | null {
  // Resolve what the caret is ON from the tree, not from the index: the index
  // keeps only the FIRST reference per label, and the second `[^1]` in a
  // paragraph must follow just as well as the first.
  let node: { name: string; from: number; to: number; node: any; parent: any } | null =
    syntaxTree(state).resolveInner(pos, 1);
  while (node && node.name !== 'FootnoteRef' && node.name !== 'FootnoteDefinition') {
    node = node.parent;
  }
  if (!node) return null;

  const label = childrenNamed(node.node, 'FootnoteLabel')[0];
  if (!label) return null;
  const text = state.doc.sliceString(label.from, label.to);
  const { definitions, firstRefs } = footnoteIndex(state);

  if (node.name === 'FootnoteRef') {
    return pos > node.from && pos < node.to ? definitions.get(text) ?? null : null;
  }
  const marks = childrenNamed(node.node, 'FootnoteMark');
  const markEnd = marks.length ? marks[marks.length - 1].to : node.from;
  return pos > node.from && pos < markEnd ? firstRefs.get(text) ?? null : null;
}

/**
 * Decorations for every footnote marker and definition in `state`.
 *
 * Returns an empty array for the overwhelmingly common case of a document
 * with no footnotes, so the cost of the feature on an ordinary page is one
 * tree walk that matches nothing.
 */
export function footnoteDecorations(
  state: EditorState,
  activeLines: Set<number>,
): FootnoteDeco[] {
  const { definitions, firstRefs } = footnoteIndex(state);
  const decos: FootnoteDeco[] = [];

  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === 'FootnoteRef') {
        const label = childrenNamed(node.node, 'FootnoteLabel')[0];
        if (!label) return false;
        // Undefined label → the build prints it literally, so we show it
        // literally. `[^0-9]` in a sentence about regexes is not a footnote.
        if (!definitions.has(state.doc.sliceString(label.from, label.to))) return false;
        // The follow region, in BOTH states: the whole `[^1]`, matching the hit
        // test in footnoteJumpTarget. Emitted before the reveal gate — the hand
        // and the press must describe the same region or one of them is lying.
        decos.push({ from: node.from, to: node.to, deco: CLICKABLE });
        if (nodeTouchesSelection(state, node.from, node.to)) return false; // revealed: raw
        for (const mark of childrenNamed(node.node, 'FootnoteMark')) {
          decos.push({ from: mark.from, to: mark.to, deco: REPLACE });
        }
        decos.push({ from: label.from, to: label.to, deco: MARK_REF });
        return false;
      }

      if (node.name === 'FootnoteDefinition') {
        // The tint is unconditional — it must not appear and disappear as the
        // cursor crosses the line — and it covers EVERY line the note spans,
        // so a note with a second paragraph or a list still reads as one note
        // rather than as a tinted line followed by ordinary body prose.
        const first = state.doc.lineAt(node.from).number;
        const last = state.doc.lineAt(node.to).number;
        for (let n = first; n <= last; n++) {
          const at = state.doc.line(n).from;
          decos.push({ from: at, to: at, deco: LINE_DEF });
        }
        const label = childrenNamed(node.node, 'FootnoteLabel')[0];
        if (!label) return; // descend — the body still renders
        // Reveal is scoped to the CHROME's line, not the container's whole
        // range. `[^1]:` lives on the first line; a caret three lines down in
        // the note body has not "entered" it, and un-rendering the marker
        // from down there would be a jump the author did not ask for.
        const marks = childrenNamed(node.node, 'FootnoteMark');
        const chromeEnd = marks.length ? marks[marks.length - 1].to : node.from;
        // The back-jump is offered only when there IS a reference to jump back
        // to — a note nobody cites gets no hand, because a pointer that
        // promises a jump and then refuses is the defect `isFollowChord`'s
        // comment in cm-link-nav.ts was written about. Scoped to the `[^1]:`
        // chrome, which is what footnoteJumpTarget accepts.
        if (firstRefs.has(state.doc.sliceString(label.from, label.to)) && chromeEnd > node.from) {
          decos.push({ from: node.from, to: chromeEnd, deco: CLICKABLE });
        }
        if (isNodeActive(state, node.from, chromeEnd, activeLines)) return;
        for (const mark of childrenNamed(node.node, 'FootnoteMark')) {
          decos.push({ from: mark.from, to: mark.to, deco: REPLACE });
        }
        decos.push({ from: label.from, to: label.to, deco: MARK_DEF_LABEL });
        return; // descend — emphasis/links inside the note render as usual
      }
    },
  });

  return decos;
}

/** Base styles, so the feature needs no edit to a stylesheet to work. */
export const footnoteTheme = EditorView.baseTheme({
  '.cm-lp-footnote-ref, .cm-lp-footnote-label': {
    verticalAlign: 'super',
    fontSize: '0.72em',
    color: 'var(--moss-color-accent)',
  },
  '.cm-lp-footnote-def': {
    color: 'var(--moss-color-text-secondary)',
  },
});

/**
 * The extension: a ViewPlugin that decorates the visible document, plus the
 * base theme. Registered separately from cm-live-preview's plugin rather than
 * folded into it — the two produce disjoint ranges, and keeping this pass
 * standalone means the footnote feature is one file to read and one line to
 * remove.
 */
export function footnoteExtension(): Extension {
  return [
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = this.build(view);
        }
        update(update: ViewUpdate) {
          if (revealInputsChangedIn(update) || update.viewportChanged) {
            this.decorations = this.build(update.view);
          }
        }
        build(view: EditorView): DecorationSet {
          const decos = footnoteDecorations(view.state, getActiveLines(view.state));
          return Decoration.set(decos.map((d) => d.deco.range(d.from, d.to)), true);
        }
      },
      { decorations: (v) => v.decorations },
    ),
    footnoteTheme,
  ];
}
