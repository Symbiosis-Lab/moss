/**
 * The gate that keeps the editor's markdown parser in step with the build's.
 *
 * moss has two markdown parsers: pulldown-cmark for the site
 * (`parser_options()` in crates/moss-core/src/ast/parser.rs) and
 * @lezer/markdown for the editor (`MARKDOWN_EXTENSIONS` in cm/source-extensions.ts).
 * Nothing tied them together, so they drifted silently:
 *
 *   - ENABLE_FOOTNOTES was on while the editor
 *     had no footnote grammar, so `[^1]` rendered as a fake hyperlink.
 *   - ENABLE_TASKLISTS was on and
 *     the editor kept a comment saying the build didn't render task lists,
 *     so `- [x] done` rendered as a blue `x` with no checkbox.
 *
 * Both were silent because "the editor doesn't know this syntax" does NOT
 * degrade to raw source — CommonMark claims unfamiliar brackets. So this test
 * asserts the harder thing: not that a flag is *mentioned* somewhere, but that
 * the editor's real extension list actually PARSES the construct the flag
 * turns on. Adding a flag in Rust fails here until someone writes the probe.
 */

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, test, expect } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { MARKDOWN_EXTENSIONS } from '../cm-editor';

// Plain paths from `import.meta.dirname`, not URL objects: this file runs in
// the jsdom lane, where the global `URL` is jsdom's rather than Node's and
// readFileSync rejects it with "The URL must be of scheme file". The gate
// threw on every run instead of checking anything.
const HERE = import.meta.dirname;
const PARSER_RS = join(HERE, '../../../../../../crates/moss-core/src/ast/parser.rs');
const SOURCE_EXTENSIONS_TS = join(HERE, '../cm/source-extensions.ts');

/** Every `Options::ENABLE_*` the build's parser turns on, conditional or not. */
function buildFlags(): Set<string> {
  const src = readFileSync(PARSER_RS, 'utf8');
  const out = new Set<string>();
  for (const m of src.matchAll(/options\.insert\(Options::(ENABLE_\w+)\)/g)) out.add(m[1]);
  return out;
}

/**
 * One probe per build flag: markdown the flag turns on, and the node name the
 * editor must produce for it.
 *
 * `node: null` means the editor deliberately has no grammar for the flag, with
 * the reason spelled out — the only escape hatch, and it still forces a
 * decision rather than an oversight.
 */
const PROBES: Record<string, { doc: string; node: string | null; why?: string }> = {
  ENABLE_STRIKETHROUGH: { doc: '~~gone~~ stays', node: 'Strikethrough' },
  ENABLE_TABLES: { doc: '| h |\n| --- |\n| c |\n', node: 'Table' },
  ENABLE_FOOTNOTES: { doc: 'a[^1] b\n\n[^1]: the note\n', node: 'FootnoteRef' },
  ENABLE_TASKLISTS: { doc: '- [x] done\n', node: 'TaskMarker' },
  ENABLE_WIKILINKS: { doc: 'see [[Page]] here', node: 'Wikilink' },
  ENABLE_MATH: { doc: 'let $E=mc^2$ be', node: 'InlineMath' },
  // Source-only: the editor shows the author's term and definition lines as
  // plain paragraph text. The probe still fails if the grammar starts claiming it.
  ENABLE_DEFINITION_LIST: {
    doc: 'Term\n: the definition\n',
    node: null,
    why: 'source-only: @lezer/markdown has no definition-list extension',
  },
  ENABLE_CJK_FRIENDLY_EMPHASIS: {
    doc: '這是**粗體**字，「*斜體*」也是',
    node: 'StrongEmphasis',
    // @lezer/markdown pairs these spans identically to
    // pulldown-cmark with the flag on — CJK text and fullwidth punctuation on
    // either side of the delimiters. The probe still runs, so a regression in
    // either parser's flanking rules shows up as a failure here.
    why: 'no extension needed — plain CommonMark flanking already agrees',
  },
};

const lang = markdown({ extensions: MARKDOWN_EXTENSIONS });

function nodeNames(doc: string): Set<string> {
  const out = new Set<string>();
  lang.language.parser.parse(doc).iterate({ enter: (n) => void out.add(n.name) });
  return out;
}

describe('editor / build markdown parity', () => {
  test('every build flag has a probe, and no probe outlives its flag', () => {
    expect(Object.keys(PROBES).sort()).toEqual([...buildFlags()].sort());
  });

  for (const [flag, probe] of Object.entries(PROBES)) {
    test(`${flag}: the editor parses what the build renders`, () => {
      const names = nodeNames(probe.doc);
      if (probe.node) expect(names.has(probe.node)).toBe(true);
      // A source-only probe must not come out as any construct another flag
      // would have produced; the text stays a paragraph.
      else {
        expect(names.has('Paragraph')).toBe(true);
        for (const other of ['Table', 'FootnoteRef', 'TaskMarker']) expect(names.has(other)).toBe(false);
      }
      // Whatever the flag turns on, it must not come out a fake link — the
      // failure mode all three reported bugs shared.
      expect(names.has('Link')).toBe(false);
    });
  }

  test('the parity matrix in source-extensions.ts names every flag', () => {
    // The matrix is where a reader learns WHY a flag is handled the way it is;
    // a flag missing from it is a flag nobody decided about.
    const src = readFileSync(SOURCE_EXTENSIONS_TS, 'utf8');
    for (const flag of buildFlags()) expect(src).toContain(flag);
  });
});
