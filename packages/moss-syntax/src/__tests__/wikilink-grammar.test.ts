/**
 * Tests for the @lezer/markdown Wikilink inline grammar extension.
 *
 * Mirrors the structure of cm-shortcode-lezer.test.ts.
 */

import { describe, test, expect } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '../wikilink-grammar.js';

/** Build the parser once, reuse for all tests. */
const lang = markdown({ extensions: [wikilinkConfig] });

/** Walk the full syntax tree for `doc` and return every node. */
function nodes(doc: string): { name: string; from: number; to: number }[] {
  const tree = lang.language.parser.parse(doc);
  const out: { name: string; from: number; to: number }[] = [];
  tree.iterate({ enter: (n) => { out.push({ name: n.name, from: n.from, to: n.to }); } });
  return out;
}

/** Return the text of `doc` sliced to node `{from, to}`. */
function text(doc: string, node: { from: number; to: number }): string {
  return doc.slice(node.from, node.to);
}

describe('Wikilink inline grammar', () => {
  test('1. basic wikilink produces Wikilink node spanning [[…]]', () => {
    const doc = 'See [[Research]]';
    const ns = nodes(doc);

    // There must be a Wikilink node.
    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'expected a Wikilink node').toBeTruthy();

    // It must span exactly `[[Research]]`.
    const wikilinkText = text(doc, wikilink!);
    expect(wikilinkText).toBe('[[Research]]');

    // The WikilinkTarget must contain the bare page name.
    const target = ns.find(n => n.name === 'WikilinkTarget');
    expect(target, 'expected a WikilinkTarget child').toBeTruthy();
    expect(text(doc, target!)).toBe('Research');

    // There must be NO Link node that overlaps the wikilink range.
    const overlap = ns.find(n =>
      n.name === 'Link' &&
      n.from < wikilink!.to &&
      n.to > wikilink!.from
    );
    expect(overlap, 'unexpected Link node overlapping [[Research]]').toBeUndefined();
  });

  test('2. piped wikilink [[Page|Display]] — target is Page only', () => {
    const doc = '[[Page|Display text]]';
    const ns = nodes(doc);

    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'expected a Wikilink node').toBeTruthy();
    expect(text(doc, wikilink!)).toBe('[[Page|Display text]]');

    const target = ns.find(n => n.name === 'WikilinkTarget');
    expect(target, 'expected a WikilinkTarget child').toBeTruthy();
    // Target ends at the `|` — should be just `Page`.
    expect(text(doc, target!)).toBe('Page');
  });

  test('3. normal markdown link [text](/url/) still produces a Link node', () => {
    const doc = '[text](/research/)';
    const ns = nodes(doc);

    // Standard Link must still parse.
    const link = ns.find(n => n.name === 'Link');
    expect(link, 'expected a Link node for a normal markdown link').toBeTruthy();

    // Must have a URL child.
    const url = ns.find(n => n.name === 'URL');
    expect(url, 'expected a URL node inside the standard Link').toBeTruthy();

    // Must NOT produce a Wikilink.
    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'unexpected Wikilink node for a normal link').toBeUndefined();
  });

  test('4. unclosed [[NotClosed produces no Wikilink', () => {
    const doc = '[[NotClosed';
    const ns = nodes(doc);

    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'unexpected Wikilink for unclosed [[').toBeUndefined();
  });

  test('4b. [[NotClosed — closing ]] on next line is not matched (single-line constraint)', () => {
    const doc = '[[NotClosed\n]]';
    const ns = nodes(doc);

    // The closing ]] is on the next line; the inline parser must bail.
    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'unexpected Wikilink spanning a newline').toBeUndefined();
  });

  test('5. [[x]] inside a fenced code block produces no Wikilink', () => {
    const doc = '```\n[[x]]\n```\n';
    const ns = nodes(doc);

    // Inline parsers don't run inside FencedCode — the block parser consumes it first.
    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink, 'unexpected Wikilink inside a fenced code block').toBeUndefined();
  });

  test('WikilinkMark nodes delimit [[ and ]]', () => {
    const doc = '[[Hello]]';
    const ns = nodes(doc);

    const marks = ns.filter(n => n.name === 'WikilinkMark');
    expect(marks.length).toBe(2);

    const [open, close] = marks;
    expect(text(doc, open)).toBe('[[');
    expect(text(doc, close)).toBe(']]');
  });

  test('wikilink at start of document parses correctly', () => {
    const doc = '[[Intro]] is the first page.';
    const ns = nodes(doc);

    const wikilink = ns.find(n => n.name === 'Wikilink');
    expect(wikilink).toBeTruthy();
    expect(text(doc, wikilink!)).toBe('[[Intro]]');
  });

  test('multiple wikilinks in one paragraph each produce a Wikilink node', () => {
    const doc = 'See [[Alpha]] and [[Beta]] for details.';
    const ns = nodes(doc);

    const wikilinks = ns.filter(n => n.name === 'Wikilink');
    expect(wikilinks.length).toBe(2);
    expect(text(doc, wikilinks[0])).toBe('[[Alpha]]');
    expect(text(doc, wikilinks[1])).toBe('[[Beta]]');
  });
});

// ── WikilinkEmbed (ADR-041) ──────────────────────────────────────────
// `![[…]]` is its own node, not an `Image` in disguise. The consequence that
// motivated the change is test 2: a real `Link` may now wrap an embed.

describe('WikilinkEmbed inline grammar', () => {
  test('![[a.png]] is a WikilinkEmbed with ![[ / target / ]] children — and no Image', () => {
    const doc = '![[a.png]]';
    const ns = nodes(doc);

    const embed = ns.find(n => n.name === 'WikilinkEmbed');
    expect(embed).toBeTruthy();
    expect([embed!.from, embed!.to]).toEqual([0, 10]);
    // The node STARTS at the `!` — the same offset the old Image mis-parse
    // reported, which is why no consumer span moved.
    expect(text(doc, embed!)).toBe('![[a.png]]');

    const marks = ns.filter(n => n.name === 'WikilinkMark');
    expect(marks.map(m => text(doc, m))).toEqual(['![[', ']]']);
    expect(text(doc, ns.find(n => n.name === 'WikilinkTarget')!)).toBe('a.png');

    expect(ns.find(n => n.name === 'Image')).toBeUndefined();
    expect(ns.find(n => n.name === 'Wikilink')).toBeUndefined();
  });

  test('![[a.png|55%]] — WikilinkTarget stops at the pipe', () => {
    const doc = '![[a.png|55%]]';
    const ns = nodes(doc);
    expect(text(doc, ns.find(n => n.name === 'WikilinkEmbed')!)).toBe(doc);
    expect(text(doc, ns.find(n => n.name === 'WikilinkTarget')!)).toBe('a.png');
  });

  test('a Link may wrap an embed: [![[a.png]]](/awards/)', () => {
    // The headline of ADR-041. Before, the `[a.png]` inside the Image tripped
    // CommonMark's no-nested-links rule and destroyed the outer Link entirely.
    const doc = '[![[a.png]]](/awards/)';
    const ns = nodes(doc);

    const link = ns.find(n => n.name === 'Link');
    expect(link).toBeTruthy();
    expect(text(doc, link!)).toBe(doc);

    const url = ns.find(n => n.name === 'URL');
    expect(text(doc, url!)).toBe('/awards/');

    const embed = ns.find(n => n.name === 'WikilinkEmbed');
    expect(text(doc, embed!)).toBe('![[a.png]]');
    // The embed is INSIDE the link span.
    expect(embed!.from).toBeGreaterThan(link!.from);
    expect(embed!.to).toBeLessThan(link!.to);
  });

  test('ordering regression: registration must stay before:"Link"', () => {
    // `before: 'Image'` would let the built-in Link parser claim the first `[`
    // of `[[Page]]`, silently killing every plain wikilink in the document.
    const ns = nodes('[[Page]]');
    expect(ns.find(n => n.name === 'Wikilink')).toBeTruthy();
    expect(ns.find(n => n.name === 'Link')).toBeUndefined();
  });

  test.each([
    ['`![[a.png]]`', 'inline code claims the span first'],
    ['![[a.png]', 'unclosed — no closing ]]'],
    ['\\![[Page]]', 'escaped bang — an Escape plus a plain Wikilink, i.e. a link'],
  ])('%s produces no WikilinkEmbed (%s)', (doc) => {
    expect(nodes(doc).find(n => n.name === 'WikilinkEmbed')).toBeUndefined();
  });
});
