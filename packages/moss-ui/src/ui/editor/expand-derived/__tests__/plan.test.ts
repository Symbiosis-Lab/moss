import { describe, expect, it } from 'vitest';
import {
  changesFor, derivedFragmentOf, diffLines, hunksForLine, maskLines, readyLines,
} from '../plan';

const apply = (text: string, changes: { from: number; to: number; insert: string }[]): string => {
  let out = text;
  for (const c of [...changes].reverse()) out = out.slice(0, c.from) + c.insert + out.slice(c.to);
  return out;
};

const lines = (s: string): string[] => s.split('\n');

describe('derivedFragmentOf', () => {
  it('names the text fragment of a file', () => {
    expect(derivedFragmentOf('summary-2019.pdf#text')).toBe('text');
  });
  it('refuses viewer fragments, bare files and markdown sections', () => {
    expect(derivedFragmentOf('summary-2019.pdf#page=3')).toBeNull();
    expect(derivedFragmentOf('summary-2019.pdf')).toBeNull();
    expect(derivedFragmentOf('note#Heading')).toBeNull();
    expect(derivedFragmentOf('note.md#Heading')).toBeNull();
  });
});

describe('readyLines', () => {
  it('keeps only the embed lines the probe changed', () => {
    const before = '---\ninferred: [title]\n---\n\n![[a.pdf#text]]\n\n![[b.pdf#text]]\n\n![[c.pdf]]';
    const after = '---\ntitle: A\n---\n\nHello\n\n![[b.pdf#text]]\n\n![[c.pdf]]';
    const hunks = diffLines(lines(before), lines(after));
    expect(readyLines([4, 6], hunks)).toEqual([4]);
  });
  it('is empty when nothing is ready', () => {
    const t = lines('x\n\n![[a.pdf#text]]');
    expect(readyLines([2], diffLines(t, t))).toEqual([]);
  });
});

describe('single-line edit', () => {
  const current = 'intro\n\n![[a.pdf#text]]\n\n![[b.pdf#text]]\n';
  const expanded = 'intro\n\nAAA\n\nBBB\n';

  it('applies only the targeted line and leaves the rest of the text alone', () => {
    const { masked, restore } = maskLines(lines(current), [4]);
    expect(masked).toContain('<!-- moss-keep-0 -->');
    const result = restore('intro\n\nAAA\n\n<!-- moss-keep-0 -->\n');
    const hunks = hunksForLine(diffLines(lines(current), lines(result)), 2, true);
    const changes = changesFor(lines(current), lines(result), hunks);
    expect(changes).toEqual([{ from: 7, to: 23, insert: 'AAA\n' }]);
    expect(apply(current, changes)).toBe('intro\n\nAAA\n\n![[b.pdf#text]]\n');
  });

  it('whole-page result: later line keeps its own hunk only', () => {
    const hunks = diffLines(lines(current), lines(expanded));
    const second = hunksForLine(hunks, 4, false);
    expect(second).toHaveLength(1);
    expect(apply(current, changesFor(lines(current), lines(expanded), second))).toBe('intro\n\n![[a.pdf#text]]\n\nBBB\n');
  });

  it('front matter changes ride with the first ready embed only', () => {
    const before = '---\ninferred: [title]\n---\n![[a.pdf#text]]\n![[b.pdf#text]]';
    const after = '---\ntitle: T\n---\nAAA\nBBBB';
    const hunks = diffLines(lines(before), lines(after));
    expect(hunksForLine(hunks, 3, true).length).toBeGreaterThan(hunksForLine(hunks, 3, false).length);
  });

  it('handles a hunk at the last line', () => {
    const before = lines('a\n![[x.pdf#text]]');
    const after = lines('a\nTEXT');
    const changes = changesFor(before, after, diffLines(before, after));
    expect(apply('a\n![[x.pdf#text]]', changes)).toBe('a\nTEXT');
  });
});
