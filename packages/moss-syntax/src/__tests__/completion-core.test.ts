/**
 * Unit tests for the PURE completion helpers — line parsing, attr-value span
 * math, attr-value rendering, wikilink tail math. No DOM, no CM6.
 *
 * The `renderShortcodeAttrValue` block is the TS half of a cross-language
 * contract: it reads the SAME golden vectors that
 * `crates/moss-core/tests/attr_value_render.rs` feeds to the real `parse_attrs`
 * grammar. If both sides pass, what the editor writes into `{image=…}` is
 * provably what the build reads back.
 *
 * `parseCompletionContext` — the dispatcher over these helpers —
 * lives app-side (it consults the app's presentation catalog) and is tested
 * on the app's own side.
 */

import { readFileSync } from 'node:fs';
import { describe, test, expect } from 'vitest';
import {
  parseInlineTarget,
  parseInlineTargetPhase,
  parseWikilinkPhase,
  parseGalleryBodyLine,
  attrValueSpan,
  renderShortcodeAttrValue,
  wikilinkTailLength,
  findUnescapedPipe,
} from '../completion-core.js';

describe('parseInlineTarget', () => {
  test('unanchored — the SECOND image on the line wins when the cursor is in it', () => {
    const line = '![a](one.png) and ![b](two.p';
    expect(parseInlineTarget(line, line.length))
      .toEqual({ query: 'two.p', from: line.indexOf('two.p'), to: line.length, image: true });
  });

  test('cursor between two images belongs to neither', () => {
    const line = '![a](one.png) and ![b](two.png)';
    expect(parseInlineTarget(line, line.indexOf(' and ') + 3)).toBeNull();
  });

  test('a closing paren bounds the target', () => {
    const line = '![a](one.png) trailing';
    expect(parseInlineTarget(line, line.length)).toBeNull();
    expect(parseInlineTarget(line, '![a](one'.length))
      .toMatchObject({ query: 'one', to: '![a](one.png'.length });
  });

  test('an unescaped | inside the target bounds it (display attrs)', () => {
    const line = '![a](one.png|width=50)';
    expect(parseInlineTarget(line, '![a](one.p'.length))
      .toMatchObject({ to: '![a](one.png'.length });
    expect(parseInlineTarget(line, '![a](one.png|wid'.length)).toBeNull();
  });

  test('still typing — no closing paren yet', () => {
    const line = '![](關於/頭';
    expect(parseInlineTarget(line, line.length))
      .toEqual({ query: '關於/頭', from: 4, to: line.length, image: true });
  });

  test('cursor at the open paren does not fire', () => {
    const line = '![a](one.png)';
    expect(parseInlineTarget(line, '![a]('.length - 1)).toBeNull();
  });
});

describe('parseInlineTarget — a [text](… link is the same scan without the !', () => {
  test('a link target is found and flagged as not an image', () => {
    const line = 'see [here](no';
    expect(parseInlineTarget(line, line.length))
      .toEqual({ query: 'no', from: line.indexOf('no'), to: line.length, image: false });
  });

  test('link text may hold anything but ]', () => {
    const line = '[a (b) c](/ab';
    expect(parseInlineTarget(line, line.length)).toMatchObject({ query: '/ab', image: false });
  });
});

describe('parseInlineTargetPhase', () => {
  test('an image target is the image phase', () => {
    const line = '![](pho';
    expect(parseInlineTargetPhase(line, line.length)).toEqual({ phase: 'image', query: 'pho', from: 4, to: 7 });
  });

  test('a link target is the link phase, with the leading / kept in the query', () => {
    const line = '[t](/ab';
    expect(parseInlineTargetPhase(line, line.length)).toEqual({ phase: 'link', query: '/ab', from: 4, to: 7 });
  });

  test('a # inside a link target is the inline heading phase, page part before it', () => {
    const line = '[t](notes#get)';
    expect(parseInlineTargetPhase(line, '[t](notes#get'.length))
      .toEqual({ phase: 'heading', syntax: 'inline', page: 'notes', query: 'get', from: 10, to: 13 });
    expect(parseInlineTargetPhase('[t](#get', 8)).toMatchObject({ phase: 'heading', page: null, query: 'get' });
  });

  test('a # after a /-rooted page part offers nothing — the site has no source file to read', () => {
    const line = '[t](/about/#sec';
    expect(parseInlineTargetPhase(line, line.length)).toBeNull();
  });
});

describe('parseWikilinkPhase', () => {
  test('returns null when not inside a wikilink, or after a closed one', () => {
    expect(parseWikilinkPhase('plain text', 5)).toBeNull();
    expect(parseWikilinkPhase('[[x]] then', 10)).toBeNull();
  });

  test('page completion right after [[, and mid-line', () => {
    expect(parseWikilinkPhase('[[', 2)).toEqual({ phase: 'wikilink', embed: false, query: '', from: 2, to: 2 });
    expect(parseWikilinkPhase('see [[No', 8)).toEqual({ phase: 'wikilink', embed: false, query: 'No', from: 6, to: 8 });
  });

  test('flags embed for ![[', () => {
    expect(parseWikilinkPhase('![[ph', 5)).toMatchObject({ phase: 'wikilink', embed: true, query: 'ph', from: 3 });
  });

  test('the span covers the rest of the target: up to ], | or #', () => {
    expect(parseWikilinkPhase('[[No]]', 4)).toMatchObject({ from: 2, to: 4 });
    expect(parseWikilinkPhase('[[Nozz|alias]]', 4)).toMatchObject({ from: 2, to: 6 });
    expect(parseWikilinkPhase('[[Nozz#h]]', 4)).toMatchObject({ from: 2, to: 6 });
  });

  test('heading completion after #, page part before it, null for the same page', () => {
    expect(parseWikilinkPhase('[[Notes#Get', 11))
      .toEqual({ phase: 'heading', syntax: 'wikilink', page: 'Notes', query: 'Get', from: 8, to: 11 });
    expect(parseWikilinkPhase('[[#Get', 6)).toMatchObject({ phase: 'heading', page: null, query: 'Get', from: 3 });
    expect(parseWikilinkPhase('[[Notes#Getting Started', 23)).toMatchObject({ query: 'Getting Started' });
  });
});

describe('parseGalleryBodyLine — bare-path arm (image arm delegates)', () => {
  test('leading whitespace is not part of the query', () => {
    expect(parseGalleryBodyLine('  photo.j', 9))
      .toEqual({ query: 'photo.j', from: 2, to: 9 });
  });
  test('an image line with the cursor outside its target yields null', () => {
    expect(parseGalleryBodyLine('![alt](a.png) x', 15)).toBeNull();
  });
});

describe('attrValueSpan', () => {
  /** The accept path always passes the cursor as the fallback end. */
  const span = (line: string, valueFrom: number, cursor = line.length) =>
    attrValueSpan(line, valueFrom, cursor);

  test('bareword value ends at whitespace', () => {
    const line = ':::hero {image=a.jpg wide=true}';
    expect(span(line, line.indexOf('a.jpg')))
      .toEqual({ to: line.indexOf(' wide'), pipeSuffix: '' });
  });

  test('bareword value ends at the closing brace', () => {
    const line = ':::hero {image=a.jpg}';
    expect(span(line, line.indexOf('a.jpg'))).toEqual({ to: line.indexOf('}'), pipeSuffix: '' });
  });

  test('quoted value: the span INCLUDES both quotes', () => {
    const line = ':::hero {image="a b.jpg" wide=true}';
    expect(span(line, line.indexOf('"')))
      .toEqual({ to: line.indexOf('" wide') + 1, pipeSuffix: '' });
  });

  test('an escaped quote does not end the value', () => {
    const line = String.raw`:::hero {image="a\"b.jpg"}`;
    expect(span(line, line.indexOf('"')).to).toBe(line.indexOf('}'));
  });

  test('a } INSIDE a terminated quoted value does not end it', () => {
    const line = ':::hero {image="a}b.jpg" wide=true}';
    expect(span(line, line.indexOf('"')).to).toBe(line.indexOf('" wide') + 1);
  });

  test('an UNTERMINATED quote stops at the block brace, never past it', () => {
    // Was: to = end of line, so accepting deleted the `}` and produced an
    // unparseable shortcode. Reachable from the shipped hero snippet — its
    // `photo.jpg` tab-stop is selected, so typing `"` then a CJK prefix
    // gives exactly this line.
    const line = ':::hero {image="關}';
    expect(span(line, line.indexOf('"'), line.indexOf('}')).to).toBe(line.indexOf('}'));
  });

  test('an unterminated quote with trailing text stops at the brace too', () => {
    const line = ':::hero {image="pho} more';
    expect(span(line, line.indexOf('"'), line.indexOf('}')).to).toBe(line.indexOf('}'));
  });

  test('an unterminated quote with NO brace on the line stops at the cursor', () => {
    // The closing quote may live on the next line of a multi-line attr block
    // (gather_multi_line_attrs is real). Replacing to end of line would leave
    // an orphan quote; stopping at the cursor touches only what was typed.
    const line = ':::hero {image="pho';
    expect(span(line, line.indexOf('"'), line.length).to).toBe(line.length);
  });

  test('the |attrs suffix is reported so the accept path can preserve it', () => {
    const line = ':::hero {image="a.jpg|caption=Foo"}';
    expect(span(line, line.indexOf('"')).pipeSuffix).toBe('|caption=Foo');
  });

  test('the |attrs suffix comes back UNESCAPED, ready to re-render', () => {
    // Returning the raw source slice made the caller escape it a second time:
    // `caption=\"Hi\"` became `caption=\\\"Hi\\\"`, which read_quoted
    // then yields with the backslashes still in it.
    const line = String.raw`:::hero {image="a.png|caption=\"Hi\""}`;
    expect(span(line, line.indexOf('"')).pipeSuffix).toBe('|caption="Hi"');
  });
});

describe('renderShortcodeAttrValue — cross-language golden vectors', () => {
  const fixture = new URL('../../fixtures/attr-value.vectors.json', import.meta.url);
  const { vectors } = JSON.parse(readFileSync(fixture, 'utf8')) as {
    vectors: { raw: string; rendered: string; note: string }[];
  };

  test('the fixture is non-empty (a silent empty read would pass every case)', () => {
    expect(vectors.length).toBeGreaterThan(5);
  });

  for (const v of vectors) {
    test(`${v.note}`, () => {
      expect(renderShortcodeAttrValue(v.raw)).toBe(v.rendered);
    });
  }

  test('an empty value is quoted rather than emitted as nothing', () => {
    expect(renderShortcodeAttrValue('')).toBe('""');
  });
});

describe('wikilinkTailLength', () => {
  test('stops at the closing bracket', () => {
    expect(wikilinkTailLength('graphy.png]] rest')).toBe('graphy.png'.length);
  });
  test('stops at a heading anchor', () => {
    expect(wikilinkTailLength('Page#Section]]')).toBe('Page'.length);
  });
  test('stops at a display-attrs pipe', () => {
    expect(wikilinkTailLength('a.png|width=50]]')).toBe('a.png'.length);
  });
  test('an unclosed link extends by NOTHING', () => {
    // Was `after.length`, so accepting inside `see [[Res and then the rest of
    // the sentence` deleted 33 characters of prose. With no terminator there
    // is no target end to find, so there is nothing safe to extend over.
    expect(wikilinkTailLength('a.png')).toBe(0);
    expect(wikilinkTailLength(' and then the rest of the sentence')).toBe(0);
  });
});

describe('findUnescapedPipe', () => {
  test('an escaped pipe is skipped', () => {
    expect(findUnescapedPipe(String.raw`a\|b|c`)).toBe(4);
  });
  test('no pipe', () => {
    expect(findUnescapedPipe('abc')).toBe(-1);
  });
});
