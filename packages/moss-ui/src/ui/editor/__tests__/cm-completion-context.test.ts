/**
 * Tests for `parseCompletionContext` — the dispatcher that decides
 * which completion phase the cursor is in. It lives in
 * `cm-completion.ts` (it consults the app's presentation catalog), so
 * it stays app-side while the pure line parsers it delegates to moved to
 * `@symbiosis-lab/moss-syntax` (tested there, in
 * packages/moss-syntax/src/__tests__/completion-core.test.ts.
 *
 * No DOM, no CM6 view — runs in the `node` vitest lane (registered in
 * `vitest.config.js`'s `NODE_ENV_TESTS`).
 */

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { parseCompletionContext } from '../cm-completion';

/** Doc-lines accessor (the shape parseCompletionContext takes). */
function docOf(doc: string) {
  return EditorState.create({ doc }).doc;
}

describe('parseCompletionContext — name phase', () => {
  test('returns null for two colons', () => {
    expect(parseCompletionContext(docOf('::'), 1, 2)).toBeNull();
  });
  test('name completion at :::gr', () => {
    expect(parseCompletionContext(docOf(':::gr'), 1, 5))
      .toMatchObject({ phase: 'name', query: 'gr', from: 3 });
  });
  test('name completion at empty :::', () => {
    expect(parseCompletionContext(docOf(':::'), 1, 3))
      .toMatchObject({ phase: 'name', query: '', from: 3 });
  });
});

describe('parseCompletionContext — attr-name phase', () => {
  test('attr-name completion inside braces (no assetKinds attr)', () => {
    expect(parseCompletionContext(docOf(':::grid {co'), 1, 11))
      .toMatchObject({ phase: 'attr-name', blockName: 'grid', query: 'co', from: 9 });
  });
  test('fresh brace with no typed chars yet', () => {
    expect(parseCompletionContext(docOf(':::hero {'), 1, 9))
      .toMatchObject({ phase: 'attr-name', blockName: 'hero', query: '', from: 9 });
  });
  test('second attr on the same line, after a completed first attr', () => {
    const line = ':::hero {image=a.jpg wi';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-name', blockName: 'hero', query: 'wi' });
  });
});

describe('parseCompletionContext — attr-value phase', () => {
  test('cursor inside an unquoted attr value with assetKinds dispatches attr-value', () => {
    const line = ':::hero {image=photo.j';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-value', blockName: 'hero', attrName: 'image', query: 'photo.j' });
  });

  test('QUOTED value is the same phase, same query, as the unquoted one', () => {
    // The user requirement: quotation marks are irrelevant to whether asset
    // search fires. `from` skips the opening quote; `valueFrom` keeps it so the
    // accept path can replace the whole token.
    const line = ':::hero {image="photo.j';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({
        phase: 'attr-value',
        attrName: 'image',
        query: 'photo.j',
        from: line.indexOf('"') + 1,
        valueFrom: line.indexOf('"'),
      });
  });

  test('CJK inside quotes is not truncated', () => {
    const line = ':::hero {image="關於/頭';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-value', attrName: 'image', query: '關於/頭' });
  });

  test('multi-attr line: cursor in the value of the SECOND attr (wide=)', () => {
    const line = ':::hero {image=a.jpg wide=tr';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-value', blockName: 'hero', attrName: 'wide', query: 'tr' });
  });

  test('multi-attr line: cursor back inside the FIRST attr value is unaffected by the second', () => {
    const line = ':::hero {image=a.jpg wide=true}';
    const cursor = line.indexOf('a.jpg') + 3;
    expect(parseCompletionContext(docOf(line), 1, cursor))
      .toMatchObject({ phase: 'attr-value', attrName: 'image', query: 'a.j' });
  });

  test('comma with no following space ends an unquoted value', () => {
    // Regression: the old single regex treated [\s,] as one boundary class and
    // silently swallowed the comma-separated attr.
    const line = ':::recent {count=5,si';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-name', blockName: 'recent', query: 'si' });
  });

  test('quoted value with internal spaces is NOT truncated at the space', () => {
    const line = ':::subscribe {button="Sign up now';
    const result = parseCompletionContext(docOf(line), 1, line.length);
    expect(result).toMatchObject({ phase: 'attr-value', attrName: 'button' });
    if (result?.phase === 'attr-value') expect(result.query).toBe('Sign up now');
  });

  test('cursor immediately after a CLOSED quoted value yields no phase', () => {
    // Was the corrupt query `a.png"` — a search for a path that cannot exist.
    const line = ':::hero {image="a.png"';
    expect(parseCompletionContext(docOf(line), 1, line.length)).toBeNull();
  });

  test('a space after the closed quote starts a fresh attr-name token', () => {
    const line = ':::subscribe {button="Subscribe" pl';
    expect(parseCompletionContext(docOf(line), 1, line.length))
      .toMatchObject({ phase: 'attr-name', blockName: 'subscribe', query: 'pl' });
  });
});

describe('parseCompletionContext — gallery body-line phase', () => {
  test('bare path line inside an open gallery block', () => {
    const doc = ':::gallery {cols=3}\nphoto.j';
    const line2 = doc.split('\n')[1];
    expect(parseCompletionContext(docOf(doc), 2, line2.length))
      .toMatchObject({ phase: 'body', blockName: 'gallery', query: 'photo.j', from: 0, to: 7 });
  });

  test('![alt](path) form — cursor inside the parens triggers completion', () => {
    const doc = ':::gallery {cols=3}\n![a cat](photo.j';
    const line2 = doc.split('\n')[1];
    expect(parseCompletionContext(docOf(doc), 2, line2.length))
      .toMatchObject({ phase: 'body', blockName: 'gallery', query: 'photo.j' });
  });

  test('![alt](path) form — cursor inside the alt-text span yields NO completion', () => {
    const doc = ':::gallery {cols=3}\n![a ca](photo.jpg)';
    expect(parseCompletionContext(docOf(doc), 2, '![a ca'.length)).toBeNull();
  });

  test('|attrs suffix: cursor before the pipe still completes the path', () => {
    const doc = ':::gallery {cols=3}\nphoto.jp|caption=hi';
    expect(parseCompletionContext(docOf(doc), 2, 'photo.jp'.length))
      .toMatchObject({ phase: 'body', query: 'photo.jp', to: 'photo.jp'.length });
  });

  test('|attrs suffix: cursor past the pipe yields NO completion', () => {
    const doc = ':::gallery {cols=3}\nphoto.jpg|caption=hi';
    expect(parseCompletionContext(docOf(doc), 2, doc.split('\n')[1].length)).toBeNull();
  });

  test('after the close fence, no body context is detected', () => {
    const doc = ':::gallery {cols=3}\nphoto.jpg\n:::\nsome text';
    expect(parseCompletionContext(docOf(doc), 4, doc.split('\n')[3].length)).toBeNull();
  });
});

describe('parseCompletionContext — image phase (the fallthrough)', () => {
  test('a linked image inside a :::grid cell completes its target', () => {
    // The corpus shape: `[![[card.png]]](/url)` rewritten as a plain markdown
    // image inside a link. `grid` declares no bodyAssetKinds, so control falls
    // through to the inline image scanner instead of returning null.
    const doc = ':::grid {cols=2}\n[![](得獎卡-戰';
    const line2 = doc.split('\n')[1];
    expect(parseCompletionContext(docOf(doc), 2, line2.length))
      .toMatchObject({ phase: 'image', query: '得獎卡-戰' });
  });

  test('ordinary prose — no enclosing block at all', () => {
    const doc = 'See ![alt](photos/cat';
    expect(parseCompletionContext(docOf(doc), 1, doc.length))
      .toMatchObject({ phase: 'image', query: 'photos/cat', from: 'See ![alt]('.length });
  });

  test('a grid cell with no image target still yields nothing', () => {
    const doc = ':::grid {cols=2}\ncell one\n+++\ncell two\n:::';
    expect(parseCompletionContext(docOf(doc), 2, 'cell one'.length)).toBeNull();
  });

  test('plain prose with no image at all yields nothing', () => {
    expect(parseCompletionContext(docOf('hello world'), 1, 5)).toBeNull();
  });
});

describe('parseCompletionContext — link phases and precedence', () => {
  function at(marked: string) {
    const pos = marked.indexOf('|');
    const doc = marked.replace('|', '');
    const state = EditorState.create({ doc });
    const line = state.doc.lineAt(pos);
    return parseCompletionContext(state.doc, line.number, pos - line.from);
  }

  test('![[ wins over the inline scanner, which would read its ! as an image', () => {
    expect(at('![](a.png) ![[cov|')).toMatchObject({ phase: 'wikilink', embed: true });
  });

  test('an inline [text](… target is the link phase, in prose and inside a grid cell', () => {
    expect(at('see [here](/ab|')).toMatchObject({ phase: 'link', query: '/ab' });
    expect(at(':::grid {cols=2}\n[card](/awa|)')).toMatchObject({ phase: 'link', query: '/awa' });
  });

});
