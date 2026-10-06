import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { buildLinkDecorations, runLinkLintSource, type ReferenceCacheRead } from '../cm-link-resolver.js';

// Two targets, two feedback kinds: 'nowhere' dims (not-found, never an
// error — drafting toward a page you'll create is normal), 'gone' lints
// (moved — a certain 404). Real feedback survives on whichever link the
// caret is NOT touching; that is the whole claim under test.
const CACHE: ReferenceCacheRead = {
  get: (target) => {
    if (target === 'nowhere') {
      return { target, is_embed: false, kind: { kind: 'not-found' }, resolved: null,
        url: null, anchor: null, resolved_path: null, message: null, candidates: [], folder: null };
    }
    if (target === 'gone') {
      return { target, is_embed: false, kind: { kind: 'moved' }, resolved: null,
        url: 'new-place', anchor: null, resolved_path: null, message: 'moved to new-place',
        candidates: [], folder: null };
    }
    return undefined;
  },
};

function countDims(state: EditorState): number {
  let dims = 0;
  buildLinkDecorations(state, CACHE).between(0, state.doc.length, (_f, _t, deco) => {
    if ((deco.spec as { class?: string }).class === 'cm-link-unresolved-dim') dims++;
  });
  return dims;
}

function viewFor(doc: string, cursor: number): EditorView {
  return new EditorView({
    state: EditorState.create({ doc, selection: { anchor: cursor }, extensions: [markdown()] }),
  });
}

describe('diagnostics survive elsewhere, suppressed only where the caret is typing', () => {
  // Two spaces between the links: nodeTouchesSelection is boundary-inclusive
  // (`<=`), so a single-space gap leaves no position untouched by either
  // node's edge (`[a](nowhere)` spans [0,12], and position 12 already touches
  // it). The extra space opens position 13 as genuinely outside both nodes.
  const doc = '[a](nowhere)  [b](gone)';

  test('caret outside both links: the not-found link dims and the moved link lints', () => {
    const view = viewFor(doc, 13);
    try {
      expect(countDims(view.state)).toBe(1);
      expect(runLinkLintSource(view, CACHE)).toHaveLength(1);
    } finally { view.destroy(); }
  });

  test('caret inside the not-found link: its dim stops, the sibling lint is unaffected', () => {
    const view = viewFor(doc, doc.indexOf('nowhere') + 1);
    try {
      expect(countDims(view.state)).toBe(0);
      expect(runLinkLintSource(view, CACHE)).toHaveLength(1);
    } finally { view.destroy(); }
  });

  test('caret inside the moved link: its diagnostic stops, the sibling dim is unaffected', () => {
    const view = viewFor(doc, doc.indexOf('gone') + 1);
    try {
      expect(countDims(view.state)).toBe(1);
      expect(runLinkLintSource(view, CACHE)).toHaveLength(0);
    } finally { view.destroy(); }
  });
});
