/**
 * Enforced shortcode-structure parity gate — SHARED-PACKAGE side.
 *
 * Runs the shared corpus (fixtures/shortcode_structure_corpus.json, source of
 * truth crates/moss-core/tests/fixtures/) through the pure text scanner
 * (`scanShortcodeBlocks`) and asserts the extracted structure — names +
 * nesting + grid cell counts — matches the corpus.
 *
 * Its Rust twin, crates/moss-core/tests/shortcode_structure_parity.rs, asserts
 * the BUILD parser (`moss_core::ast::parse`) matches the SAME corpus. Two
 * gates, one source of truth: the shared syntax layer cannot silently drift
 * from the build (the `+++` divider / nested `::::buttons` rendering bugs)
 * without turning one of these red. The Lezer grammar is tied to the scanner
 * by the cross-check in scan.test.ts, closing the triangle.
 *
 * (Before L1 of the extraction this file drove the editor's Lezer tree walk,
 * `collectShortcodeBlocks`; that walk now pairs fences with the same arity
 * stack and is exercised app-side by cm-shortcode-block.test.ts.)
 */
import { describe, test, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { scanShortcodeBlocks, type ShortcodeBlock } from '../scan.js';

interface SummaryNode {
  name: string;
  gridCells: number | null;
  children: SummaryNode[];
}
interface Case { name: string; input: string; blocks: SummaryNode[] }

const corpus = JSON.parse(
  readFileSync(new URL('../../fixtures/shortcode_structure_corpus.json', import.meta.url), 'utf8'),
) as { cases: Case[] };

/** Cells in a grid = `+++` dividers in its DIRECT body + 1 (lines inside a
 *  nested child block don't count — they divide that child, not the grid). */
function countGridCells(text: string, b: ShortcodeBlock): number {
  if (b.closeFrom === null) return 1;
  let dividers = 0;
  let pos = text.indexOf('\n', b.from) + 1; // first body line
  while (pos > 0 && pos < b.closeFrom) {
    let end = text.indexOf('\n', pos);
    if (end === -1) end = text.length;
    const insideChild = b.children.some((c) => c.from <= pos && pos <= c.to);
    if (!insideChild && text.slice(pos, end).trim() === '+++') dividers++;
    pos = end + 1;
  }
  return dividers + 1;
}

function summarize(text: string, blocks: ShortcodeBlock[]): SummaryNode[] {
  return blocks.map((b) => ({
    name: b.name,
    gridCells: b.name === 'grid' ? countGridCells(text, b) : null,
    children: summarize(text, b.children),
  }));
}

describe('text scanner matches the shared shortcode-structure corpus', () => {
  test('corpus is non-empty', () => {
    expect(corpus.cases.length).toBeGreaterThan(0);
  });

  for (const c of corpus.cases) {
    test(c.name, () => {
      expect(summarize(c.input, scanShortcodeBlocks(c.input))).toEqual(c.blocks);
    });
  }
});
