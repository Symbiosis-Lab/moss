import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { GFM } from '@lezer/markdown';
import { buildMarkdownStyleDecorations } from '../cm-markdown-style';
import { GUIDE_SLOTS, listHang, type DecoRange, type SyntaxNode } from '../cm-lists';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** What the hang puts on each line of `doc`: the line's `--lp-hang`/`--lp-pfx` source text, the shim position, the measured mark's range. */
function hangOf(doc: string) {
  const state = EditorState.create({ doc, extensions: [markdown({ extensions: [GFM] })] });
  const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: doc.length }]);
  const lines = new Map<number, { hang?: string; pfx?: string; shim?: number; mark?: [number, number]; key?: string }>();
  for (const cur = set.iter(); cur.value; cur.next()) {
    const spec = cur.value.spec as { class?: string; attributes?: Record<string, string>; widget?: unknown };
    const ln = state.doc.lineAt(cur.from).number, at = lines.get(ln) ?? {};
    if (spec.class === 'cm-li-hang') {
      const style = spec.attributes!.style;
      at.hang = /--lp-hang:var\(--lpw-([\w]+)/.exec(style)![1];
      at.pfx = /--lp-pfx:var\(--lpw-([\w]+)/.exec(style)?.[1];
    } else if (spec.class === 'cm-li-pfx') {
      at.mark = [cur.from - state.doc.line(ln).from, cur.to - state.doc.line(ln).from];
      at.key = spec.attributes!['data-lpk'];
    } else if (spec.widget) at.shim = cur.from - state.doc.line(ln).from;
    lines.set(ln, at);
  }
  return lines;
}
const key = (s: string) => [...s].map((c) => c.codePointAt(0)!.toString(36)).join('_');

describe('list hanging indent decorations', () => {
  test('an item holds its text column from its first line: the prefix is indent + marker + spaces (+ a task box)', () => {
    for (const [line, prefix] of [['- a', '- '], ['1. a', '1. '], ['10. a', '10. '], ['- [ ] a', '- [ ] '], ['- [x] a', '- [x] '], ['  - a', '  - '], ['-   a', '-   ']] as const) {
      const l = hangOf(line).get(1)!;
      expect(l.hang, line).toBe(key(prefix));
      expect(l.key, line).toBe(prefix);
      expect(l.mark, line).toEqual([0, prefix.length]);
      expect(l.shim, line).toBe(0);
    }
  });

  test('a quote prefix is not part of the item prefix: the shim and the measured mark start after it', () => {
    const l = hangOf('> - a').get(1)!;
    expect(l.key).toBe('- ');
    expect(l.mark).toEqual([2, 4]);
    expect(l.shim).toBe(2);
  });

  test('a continuation and a lazy line hang under the item; the continuation names its own leading whitespace', () => {
    const lines = hangOf('- a\n  b\nc');
    expect(lines.get(2)).toMatchObject({ hang: key('- '), pfx: key('  '), shim: 0, mark: [0, 2] });
    expect(lines.get(3)).toMatchObject({ hang: key('- ') });
    expect(lines.get(3)!.shim).toBeUndefined(); // nothing to pull back
  });

  test('a nested item owns its own lines and the parent keeps only its own', () => {
    const lines = hangOf('- a\n  - b\n    more b\n  after the nested list?\n- c');
    expect(lines.get(1)!.hang).toBe(key('- '));
    expect(lines.get(2)!.hang).toBe(key('  - '));
    expect(lines.get(3)!.hang).toBe(key('  - '));
  });

  test('blank lines, plain text and a second marker on the same line get nothing', () => {
    const lines = hangOf('plain\n\n- a\n\n  b\n\n- - c');
    expect(lines.has(1)).toBe(false);
    expect(lines.has(2)).toBe(false);
    expect(lines.get(4)).toBeUndefined();
    expect(lines.get(5)!.hang).toBe(key('- '));
    expect(lines.get(7)!.hang).toBe(key('- ')); // the outer item owns `- - c`; the inner marker is not at the line start
  });
});

describe('list decoration edge cases', () => {
  test('an item whose line holds no item prefix decorates nothing instead of throwing', () => {
    const state = EditorState.create({ doc: '1: a', extensions: [markdown({ extensions: [GFM] })] });
    const item = { name: 'ListItem', from: 0, to: 4, firstChild: { from: 0, to: 1, name: 'ListMark', firstChild: null, nextSibling: null }, nextSibling: null } as unknown as SyntaxNode;
    const decos: DecoRange[] = [];
    expect(() => listHang(item, state, 0, 4, decos)).not.toThrow();
    expect(decos).toEqual([]);
  });

  test('a heading line is measured in its own font: its prefix keys carry the heading level', () => {
    const lines = hangOf('- # Title\n- plain');
    expect(lines.get(1)!.key).toBe('- #h1');
    expect(lines.get(1)!.hang).toBe(key('- #h1'));
    expect(lines.get(2)!.key).toBe('- ');
  });

  test('a tab in a prefix is fixed to the columns it spans, so it ends on the same stop wherever the shim puts it', () => {
    const state = EditorState.create({ doc: '- p\n\t- a\n-\tb\n- c\n  \td', extensions: [markdown({ extensions: [GFM] })] });
    const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: state.doc.length }]);
    const tabs: [number, string][] = [];
    for (const cur = set.iter(); cur.value; cur.next()) {
      const spec = cur.value.spec as { class?: string; attributes?: Record<string, string> };
      if (spec.class === 'cm-li-tab') tabs.push([cur.from, spec.attributes!.style]);
    }
    // `\t-`: 4 columns; `-\t`: column 1 to 4 is 3; `  \t` (a continuation of "- c"): column 2 to 4 is 2.
    expect(tabs).toEqual([[state.doc.line(2).from, 'tab-size:4;margin-right:calc(4 * var(--moss-list-indent))'], [state.doc.line(3).from + 1, 'tab-size:3'], [state.doc.line(5).from + 2, 'tab-size:2']]);
  });

  test('only the indent before a marker is widened: not the gap after it, not a continuation line', () => {
    const state = EditorState.create({ doc: '- a\n  - b\n    cont\n-  c', extensions: [markdown({ extensions: [GFM] })] });
    const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: state.doc.length }]);
    const ind: [number, number][] = [];
    for (const cur = set.iter(); cur.value; cur.next()) if ((cur.value.spec as { class?: string }).class === 'cm-li-ind') ind.push([cur.from, cur.to]);
    expect(ind).toEqual([[state.doc.line(2).from, state.doc.line(2).from + 2]]);
  });

  test('guides of two parents at one depth and column are one cached decoration', () => {
    const state = EditorState.create({ doc: '- a\n  - b\n- c\n  - d', extensions: [markdown({ extensions: [GFM] })] });
    const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: state.doc.length }]);
    const guides = [];
    for (const cur = set.iter(); cur.value; cur.next()) if ((cur.value.spec as { class?: string }).class === 'cm-lp-guided') guides.push(cur.value);
    expect(guides.length).toBe(2);
    expect(new Set(guides).size).toBe(1);
  });

  /** The lines carrying a guide in the slot of `depth`, 1-based. */
  function guidedLines(doc: string, depth: number): number[] {
    const state = EditorState.create({ doc, extensions: [markdown({ extensions: [GFM] })] });
    const set = buildMarkdownStyleDecorations(state, [{ from: 0, to: doc.length }]);
    const out: number[] = [];
    for (const cur = set.iter(); cur.value; cur.next()) {
      const spec = cur.value.spec as { class?: string; attributes?: Record<string, string> };
      if (spec.class === 'cm-lp-guided' && spec.attributes!.style.includes(`--lp-g${depth}:`)) out.push(state.doc.lineAt(cur.from).number);
    }
    return out;
  }

  test('a guide runs from below its parent to the next item at its level, through one block only', () => {
    const doc = ['- one', '  - a', '  - b', '- two', '  - c', '', '  - d', '- three'].join('\n');
    // one: its block reaches two. two: a blank line ends its block, so d, past it, gets no guide.
    expect(guidedLines(doc, 0)).toEqual([2, 3, 5]);
  });

  test('a blank line ends the guide, even before a paragraph that belongs to the item', () => {
    const doc = ['- one', '  - a', '', '  more of one', '', 'after'].join('\n');
    expect(guidedLines(doc, 0)).toEqual([2]);
  });

  test('the stylesheet draws exactly GUIDE_SLOTS guide slots', () => {
    const css = readFileSync(resolve(__dirname, '../cm-markdown-style.css'), 'utf8');
    const slots = new Set([...css.matchAll(/var\(--lp-g(\d+),/g)].map((m) => Number(m[1])));
    expect([...slots].sort()).toEqual(Array.from({ length: GUIDE_SLOTS }, (_, i) => i));
  });
});
