/**
 * Tests for the pure Lezer-based link-target extractor (`cm-link-extract.ts`).
 *
 * Each test builds an `EditorState` with the same markdown language
 * configuration used by cm-editor.ts (markdown + wikilinkConfig extensions)
 * so that `Wikilink` / `WikilinkTarget` nodes appear in the syntax tree.
 */

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '../../wikilink-grammar.js';
import { extractLinkTargets, type ExtractedTarget } from '../cm-link-extract.js';

// ── EditorState factory ──────────────────────────────────────────────────────

/**
 * Create an EditorState configured with the markdown language + wikilinkConfig.
 * This mirrors the production setup in cm-editor.ts line 67:
 *   `markdown({ extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig] })`
 * (Strikethrough / Table / shortcodeBlockConfig are omitted here — they don't
 * affect Link / Wikilink node emission.)
 */
function makeState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ extensions: [wikilinkConfig] })],
  });
}

/** Convenience: extract just the target strings in order. */
function targets(doc: string): string[] {
  return extractLinkTargets(makeState(doc)).map(t => t.target);
}

/** Confirm that sliceString(from, to) matches target for every result. */
function assertPositions(results: ExtractedTarget[], doc: string): void {
  for (const r of results) {
    expect(doc.slice(r.from, r.to)).toBe(r.target);
  }
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('extractLinkTargets', () => {
  test('1. standard markdown link → one target at URL span', () => {
    const doc = '[See research](/research/)';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('/research/');
    // The from/to must reproduce the target via sliceString.
    assertPositions(results, doc);
  });

  test('2. basic wikilink [[Research]] → one target "Research"', () => {
    const doc = '[[Research]]';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('Research');
    assertPositions(results, doc);
  });

  test('3. piped wikilink [[Page|Display]] → target "Page", span covers the visible label', () => {
    const doc = '[[Page|Display]]';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    // Resolution uses the page name…
    expect(results[0].target).toBe('Page');
    // …but from/to cover the VISIBLE label, so Cmd+click and the clickable/dim
    // marks land on what live-preview renders (the alias), not the hidden target.
    expect(doc.slice(results[0].from, results[0].to)).toBe('Display');
  });

  test('3b. piped wikilink with empty label [[Page|]] → span falls back to the target', () => {
    const doc = '[[Page|]]';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('Page');
    expect(doc.slice(results[0].from, results[0].to)).toBe('Page');
  });

  test('4. image embed ![alt](/img/x.png) → NO target extracted', () => {
    const doc = '![alt](/img/x.png)';
    const results = extractLinkTargets(makeState(doc));

    // Image nodes are NOT Link nodes — the extractor must not emit a target.
    expect(results).toHaveLength(0);
  });

  test('5. embed wikilink ![[embed.png]] → NO target extracted', () => {
    const doc = '![[embed.png]]';
    const results = extractLinkTargets(makeState(doc));

    // The Wikilink is preceded by `!` → treated as an asset embed, not a link.
    expect(results).toHaveLength(0);
  });

  test('6. links inside a fenced code block → NO targets extracted', () => {
    // Lezer's block parser claims the FencedCode before inline parsers run,
    // so Link and Wikilink nodes are never emitted inside the fence.
    const doc = '```\n[x](/y)\n[[z]]\n```\n';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(0);
  });

  test('7. multiple links on one line → multiple targets in document order', () => {
    const doc = '[first](/a/) and [second](/b/) end';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(2);
    expect(results[0].target).toBe('/a/');
    expect(results[1].target).toBe('/b/');
    // Earlier result starts before the later one.
    expect(results[0].from).toBeLessThan(results[1].from);
    assertPositions(results, doc);
  });

  test('7b. mixed markdown + wikilinks on one line → all in document order', () => {
    const doc = 'See [page](/p/) and [[Wiki]] and [[Other|Alt]].';
    const ts = targets(doc);

    expect(ts).toEqual(['/p/', 'Wiki', 'Other']);
  });

  // ── Extra coverage ───────────────────────────────────────────────────────

  test('image embed does not suppress a real link on the same line', () => {
    const doc = '![img](/i.png) and [link](/l/)';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('/l/');
  });

  test('embed wikilink does not suppress a real wikilink on the same line', () => {
    const doc = '![[embed.png]] and [[RealPage]]';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('RealPage');
  });

  test('a link wrapping an embed reports the OUTER url', () => {
    // Before this fix the outer Link never survived the parse, so this yielded
    // zero targets: the destination was neither lintable nor Cmd-clickable.
    const results = extractLinkTargets(makeState('[![[a.png]]](/awards/)'));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('/awards/');
  });

  test('empty document → empty results', () => {
    expect(extractLinkTargets(makeState(''))).toHaveLength(0);
  });

  test('wikilink with surrounding spaces → target is trimmed, span covers raw text', () => {
    // `[[  Spaced  ]]` — the WikilinkTarget includes the spaces; the lookup
    // string must be trimmed but from/to must reproduce the raw span.
    const doc = '[[  Spaced  ]]';
    const results = extractLinkTargets(makeState(doc));

    expect(results).toHaveLength(1);
    expect(results[0].target).toBe('Spaced');
    // from/to are the raw span (including spaces), not the trimmed string.
    expect(doc.slice(results[0].from, results[0].to)).toContain('Spaced');
  });
});
