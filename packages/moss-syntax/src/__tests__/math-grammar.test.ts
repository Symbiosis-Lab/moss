/**
 * Conformance + unit tests for the `$…$` / `$$…$$` math inline grammar.
 *
 * The conformance half is the CROSS-LANGUAGE CONTRACT: it loads the same
 * golden vectors that gate the build's pulldown-cmark parser
 * (crates/moss-core/tests/fixtures/math-delimiters.vectors.json, held on the
 * Rust side by tests/math_delimiters.rs) and asserts this grammar produces
 * the SAME math spans for every vector — so the editor can never highlight
 * a span the build would not parse, or vice versa. If a vector fails here,
 * fix the GRAMMAR (cm-math-lezer.ts), never the vectors.
 *
 * The unit half pins behaviors the vectors do not cover, each MEASURED by
 * running pulldown-cmark 0.13.3 with `parser_options(true)` (2026-07-22),
 * never derived by reasoning about a spec.
 */

import { readFileSync } from 'node:fs';
import { describe, test, expect } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { Strikethrough, Table } from '@lezer/markdown';
import { shortcodeBlockConfig } from '../shortcode.js';
import { wikilinkConfig } from '../wikilink-grammar.js';
import { mathConfig } from '../math-grammar.js';

interface Span { mode: string; tex: string }
interface Vector { input: string; expect_math_spans: Span[]; note: string }

const VECTORS_PATH = new URL('../../fixtures/math-delimiters.vectors.json', import.meta.url);
const { vectors } = JSON.parse(readFileSync(VECTORS_PATH, 'utf8')) as { vectors: Vector[] };

/** The EXACT extension set cm-editor.ts registers for markdown, so any
 *  interaction with the sibling grammars (shortcode fences, wikilinks, GFM)
 *  is part of what the contract verifies. */
const lang = markdown({
  extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig, mathConfig],
});

/** Extract math spans (document order) exactly as the fixture records them:
 *  the TeX is everything between the delimiters, verbatim. */
function mathSpans(doc: string): Span[] {
  const tree = lang.language.parser.parse(doc);
  const out: Span[] = [];
  tree.iterate({
    enter(n) {
      if (n.name === 'InlineMath') {
        out.push({ mode: 'inline', tex: doc.slice(n.from + 1, n.to - 1) });
      } else if (n.name === 'DisplayMath') {
        out.push({ mode: 'display', tex: doc.slice(n.from + 2, n.to - 2) });
      }
    },
  });
  return out;
}

/** Every node in the tree, for structural assertions. */
function nodes(doc: string): { name: string; from: number; to: number }[] {
  const tree = lang.language.parser.parse(doc);
  const out: { name: string; from: number; to: number }[] = [];
  tree.iterate({ enter: (n) => { out.push({ name: n.name, from: n.from, to: n.to }); } });
  return out;
}

describe('golden-vector conformance (cross-language $-delimiter contract)', () => {
  test('the fixture is intact (a contract is extended, never shrunk)', () => {
    expect(vectors.length).toBeGreaterThanOrEqual(14);
  });

  for (const v of vectors) {
    test(`${JSON.stringify(v.input)} — ${v.note.slice(0, 60)}`, () => {
      const expected = v.expect_math_spans.map(({ mode, tex }) => ({ mode, tex }));
      expect(mathSpans(v.input)).toEqual(expected);
    });
  }
});

describe('measured behaviors beyond the vectors (pulldown-cmark 0.13.3)', () => {
  test('inline $ math spans a soft line break: "$a\\nb$"', () => {
    // Measured: pulldown yields InlineMath "a\nb" — the Wikilink parser's
    // bail-on-newline rule would be WRONG here.
    expect(mathSpans('$a\nb$')).toEqual([{ mode: 'inline', tex: 'a\nb' }]);
  });

  test('display math is inline-level: fires mid-line ("foo $$x^2$$ bar")', () => {
    // Measured: pulldown yields DisplayMath "x^2". This is why $$ is an
    // inline parser, not a block parser (see the module header).
    expect(mathSpans('foo $$x^2$$ bar')).toEqual([{ mode: 'display', tex: 'x^2' }]);
  });

  test('a blank line (paragraph break) stops display math: "$$a\\n\\nb$$"', () => {
    // Measured: pulldown yields no math events.
    expect(mathSpans('$$a\n\nb$$')).toEqual([]);
  });

  test('trailing $$ closes inline math on its first $: "$x$$" → "x"', () => {
    expect(mathSpans('$x$$')).toEqual([{ mode: 'inline', tex: 'x' }]);
  });

  test('unclosed $$ falls back to inline on its second $: "$$x$" → "x"', () => {
    expect(mathSpans('$$x$')).toEqual([{ mode: 'inline', tex: 'x' }]);
  });

  test('adjacent spans share no delimiter: "one $a$$b$ two" → "a", "b"', () => {
    expect(mathSpans('one $a$$b$ two')).toEqual([
      { mode: 'inline', tex: 'a' },
      { mode: 'inline', tex: 'b' },
    ]);
  });

  test('math beats emphasis even when the * opened first: "*a $b* c$"', () => {
    // Measured: pulldown yields InlineMath "b* c" and NO emphasis — the $
    // span claims the closing *, leaving the opener unpaired.
    const doc = '*a $b* c$';
    expect(mathSpans(doc)).toEqual([{ mode: 'inline', tex: 'b* c' }]);
    expect(nodes(doc).find((n) => n.name === 'Emphasis')).toBeUndefined();
  });
});

describe('node structure', () => {
  test('InlineMath carries MathMark children delimiting each $', () => {
    const ns = nodes('$E=mc^2$');
    const math = ns.find((n) => n.name === 'InlineMath');
    expect(math).toBeTruthy();
    const marks = ns.filter((n) => n.name === 'MathMark');
    expect(marks).toEqual([
      { name: 'MathMark', from: 0, to: 1 },
      { name: 'MathMark', from: 7, to: 8 },
    ]);
  });

  test('DisplayMath carries MathMark children delimiting each $$', () => {
    const ns = nodes('$$ x^2 $$');
    const math = ns.find((n) => n.name === 'DisplayMath');
    expect(math).toBeTruthy();
    const marks = ns.filter((n) => n.name === 'MathMark');
    expect(marks).toEqual([
      { name: 'MathMark', from: 0, to: 2 },
      { name: 'MathMark', from: 7, to: 9 },
    ]);
  });

  test('multi-line display math is ONE node spanning the newlines', () => {
    const doc = '$$\n\\frac{a}{b}\n$$';
    const math = nodes(doc).find((n) => n.name === 'DisplayMath');
    expect(math).toBeTruthy();
    expect(doc.slice(math!.from, math!.to)).toBe(doc);
  });
});
