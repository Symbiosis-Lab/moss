import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { buildLinkDecorations } from '../cm-link-clickable.js';
import { wikilinkConfig } from '../../wikilink-grammar.js';

function clickableSpans(doc: string): string[] {
  const state = EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] })] });
  const spans: string[] = [];
  buildLinkDecorations(state).between(0, state.doc.length, (from, to) => {
    spans.push(state.doc.sliceString(from, to));
  });
  return spans;
}

describe('the followable-cursor mark', () => {
  test('covers every link node and every embed node, with no other mark', () => {
    expect(clickableSpans('[a](nowhere) and [[gone]] and ![[photo.png]]')).toEqual([
      '[a](nowhere)',
      '[[gone]]',
      '![[photo.png]]',
    ]);
  });
});
