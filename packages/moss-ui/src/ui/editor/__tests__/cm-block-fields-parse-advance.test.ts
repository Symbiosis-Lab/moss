/**
 * Block StateFields must rebuild when the incremental parse ADVANCES.
 *
 * On a large document the markdown parse is incremental: state creation parses
 * only a prefix, and the parse worker extends the tree afterwards via
 * transactions that change neither doc nor selection (Language.setState
 * effects). A StateField gated on `!tr.docChanged && !tr.selection` skips those
 * transactions, so tables / images / shortcode blocks beyond the
 * initially-parsed region never decorate until the user happens to type or
 * move the cursor. The guard is the cheap reference comparison
 * `syntaxTree(tr.state) != syntaxTree(tr.startState)`.
 *
 * `forceParsing(view, …)` drives the same parse-advance path synchronously —
 * it dispatches exactly the effect-only transactions the worker would.
 */

import { describe, test, expect, afterEach } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { Table } from '@lezer/markdown';
import { forceParsing, syntaxTreeAvailable } from '@codemirror/language';
import { wikilinkConfig, shortcodeBlockConfig } from '@symbiosis-lab/moss-syntax';
import { shortcodeBlockExtension } from '@symbiosis-lab/moss-syntax/cm6';
import { blockEmbedsExtension } from '../cm-block-embeds';

// Large enough that state creation's time-budgeted initial parse cannot reach
// the tail (sanity-checked in the test so a faster machine can't silently
// invalidate the premise).
const FILLER = 'lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n'.repeat(30000);
// A GFM table is no longer a block widget (tables are styled source, pipes
// faint), so the tail only needs to prove the two
// remaining field-provided WIDGET decorations (image, shortcode block) still
// pick up the parse advance; the table line stays in the doc as a realistic
// large-document fixture but is not itself asserted on here.
const TAIL =
  '| a | b |\n| --- | --- |\n| 1 | 2 |\n\n' +
  '![alt](p.jpg)\n\n' + //                       image → cm-block-embeds' block field
  ':::note\nbody\n:::\n'; //                     shortcode block → shortcodeBlockField
const DOC = FILLER + TAIL;

/** Count decorations located at/after `pos` across all StateField-provided sets. */
function decorationsAfter(state: EditorState, pos: number): number {
  let count = 0;
  for (const provided of state.facet(EditorView.decorations)) {
    if (typeof provided === 'function') continue;
    const it = (provided as DecorationSet).iter(pos);
    while (it.value) {
      count++;
      it.next();
    }
  }
  return count;
}

describe('block fields follow incremental parse progress', () => {
  let view: EditorView | undefined;

  afterEach(() => {
    view?.destroy();
    view = undefined;
    document.body.innerHTML = '';
  });

  test('tail constructs decorate once the parse reaches them', () => {
    view = new EditorView({
      state: EditorState.create({
        doc: DOC,
        extensions: [
          markdown({ extensions: [Table, shortcodeBlockConfig, wikilinkConfig] }),
          blockEmbedsExtension({
            resolveImage: () => ({ url: 'http://x/p.jpg', width: 40, height: 30 }),
          }),
          shortcodeBlockExtension(),
        ],
      }),
      parent: document.body,
    });

    // Premise: the initial parse must NOT have reached the tail.
    expect(syntaxTreeAvailable(view.state, DOC.length)).toBe(false);
    expect(decorationsAfter(view.state, FILLER.length)).toBe(0);

    // Advance the parse to the end — bounded loop, each round dispatches the
    // worker-style effect-only transaction.
    for (let i = 0; i < 50 && !syntaxTreeAvailable(view.state, DOC.length); i++) {
      forceParsing(view, DOC.length, 2000);
    }
    expect(syntaxTreeAvailable(view.state, DOC.length)).toBe(true);

    // The image block widget and the shortcode block decorations must both
    // have appeared without any doc/selection change.
    expect(decorationsAfter(view.state, FILLER.length)).toBeGreaterThanOrEqual(2);
  });
});
