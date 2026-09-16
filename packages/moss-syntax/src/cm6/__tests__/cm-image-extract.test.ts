/**
 * Tests for the shared Lezer-based image/asset-target extractor.
 *
 * Verifies that `extractImageTargets` handles both standard markdown image
 * syntax `![alt](url)` and wikilink embed syntax `![[file]]`, using the same
 * markdown parser configuration as the production editor (cm-editor.ts line 67).
 */

import { readFileSync } from 'node:fs';
import { describe, it, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '../../wikilink-grammar.js';
import { shortcodeBlockConfig } from '../../shortcode.js';
import { syntaxTree } from '@codemirror/language';
import {
  extractImageTargets, embedNodeAt, embedParts, isEmbedNode,
  parseFolderParams, folderChips, folderParamsFromEmbed, type EmbedParts,
} from '../cm-image-extract.js';

/**
 * Create an EditorState configured identically to cm-editor.ts:
 *   `markdown({ extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig] })`
 * Strikethrough/Table/shortcodeBlockConfig are omitted — they don't affect
 * Image/Wikilink node emission.  wikilinkConfig is required so that `![[...]]`
 * parses correctly (without it the `[[` would mis-parse as a Link bracket).
 */
function state(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ extensions: [wikilinkConfig] })],
  });
}

describe('extractImageTargets', () => {
  it('extracts ![](url) target with URL-child range', () => {
    const doc = '![a](/img/x.png)';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(1);
    expect(t[0].target).toBe('/img/x.png');
    // URL child spans exactly the path text — verify with sliceString
    expect(doc.slice(t[0].from, t[0].to)).toBe('/img/x.png');
    // Concrete offsets: '![a](' is 5 chars → from=5, to=15
    expect(t[0].from).toBe(5);
    expect(t[0].to).toBe(15);
  });

  it('extracts ![[embed]] target with whole-Image range', () => {
    const doc = '![[embed.png]]';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(1);
    expect(t[0].target).toBe('embed.png');
    // For embed wikilinks the whole Image node span is reported
    expect(t[0].from).toBe(0);
    expect(t[0].to).toBe(14);
  });

  it('captures pic.jpg (not pic.jpg "title") for titled images', () => {
    const doc = '![](pic.jpg "title")';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(1);
    // URL child must stop before the title string
    expect(t[0].target).toBe('pic.jpg');
  });

  it('wikilink embed with pipe alias ![[file.png|alt]] → target is file.png only', () => {
    const doc = '![[file.png|alt text]]';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(1);
    expect(t[0].target).toBe('file.png');
  });

  it('plain wikilink [[Page]] (not embed) → NOT extracted', () => {
    const doc = '[[Page]]';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(0);
  });

  it('plain markdown link [text](/url) → NOT extracted', () => {
    const doc = '[text](/url)';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(0);
  });

  it('multiple images in one document → both extracted in order', () => {
    const doc = '![a](/one.png) and ![b](/two.png)';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(2);
    expect(t[0].target).toBe('/one.png');
    expect(t[1].target).toBe('/two.png');
    expect(t[0].from).toBeLessThan(t[1].from);
  });

  it('images inside fenced code blocks → NOT extracted', () => {
    const doc = '```\n![a](/x.png)\n![[y.png]]\n```\n';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(0);
  });

  it('embed inside a link → one target, spanning the EMBED not the Link', () => {
    // The walk descends into the wrapping Link (the Image branch no longer
    // prunes it away), and the reported span stays the embed's.
    const doc = '[![[a.png]]](/u)';
    const t = extractImageTargets(state(doc));
    expect(t).toHaveLength(1);
    expect(t[0].target).toBe('a.png');
    expect([t[0].from, t[0].to]).toEqual([1, 11]);
  });

  it.each([
    ['![[]]', 'slash-menu inserts this on every image verb'],
    ['![]()', 'an empty markdown image'],
  ])('%s yields no target (%s)', (doc) => {
    expect(extractImageTargets(state(doc))).toHaveLength(0);
  });
});

/**
 * A hero names its image in an ATTRIBUTE, not an embed — the one asset in a
 * document that no Lezer `Image`/`WikilinkEmbed` node covers. It is extracted
 * here rather than by a fourth scanner because everything downstream reads
 * this one list: resolution, the broken-asset underline, and the hover card.
 */
describe('extractImageTargets — shortcode attributes', () => {
  /** `state()` above omits shortcodeBlockConfig; open fences need it. */
  function scState(doc: string): EditorState {
    return EditorState.create({
      doc,
      extensions: [markdown({ extensions: [shortcodeBlockConfig, wikilinkConfig] })],
    });
  }

  it('reports a hero image, flagged as an attribute, spanning the path', () => {
    const doc = ':::hero {image=assets/cover.jpg}\n# Title\n:::\n';
    const t = extractImageTargets(scState(doc));
    expect(t).toHaveLength(1);
    expect(t[0].target).toBe('assets/cover.jpg');
    expect(doc.slice(t[0].from, t[0].to)).toBe('assets/cover.jpg');
    // The flag is what stops the hover popover suppressing itself: nothing
    // paints a hero attribute inline, so the hover is the only way to see it.
    expect(t[0].attr).toBe(true);
  });

  it('an embed keeps `attr` unset, so the inline-render gate still applies', () => {
    const t = extractImageTargets(scState('![[a.png]]\n'));
    expect(t[0].attr).toBeUndefined();
  });

  it('a hero attribute and a body embed are both reported, in document order', () => {
    // Document order is not cosmetic: cm-reference-resolver feeds these
    // straight to a RangeSetBuilder, which throws on an out-of-order add.
    const doc = ':::hero {image=cover.jpg}\n![[in-body.png]]\n:::\n\n![[after.png]]\n';
    const t = extractImageTargets(scState(doc));
    expect(t.map((x) => x.target)).toEqual(['cover.jpg', 'in-body.png', 'after.png']);
    expect(t.map((x) => x.from)).toEqual([...t.map((x) => x.from)].sort((a, b) => a - b));
  });

  it('shortcodes that name no asset contribute nothing', () => {
    const doc = ':::grid {cols=2}\na\n+++\nb\n:::\n\n:::subscribe {button="Go"}\n:::\n';
    expect(extractImageTargets(scState(doc))).toHaveLength(0);
  });
});

describe('isEmbedNode', () => {
  test.each([
    ['Image', true],
    ['WikilinkEmbed', true],
    ['Wikilink', false],
    ['Link', false],
  ])('%s → %s', (name, expected) => {
    expect(isEmbedNode(name)).toBe(expected);
  });
});

describe('embedParts alt', () => {
  function partsOf(doc: string) {
    const st = state(doc);
    const found: (EmbedParts | null)[] = [];
    syntaxTree(st).iterate({
      enter(n) { if (isEmbedNode(n.name)) { found.push(embedParts(n, st.doc)); return false; } },
    });
    return found[0];
  }

  test.each([
    ['![[a.png]]', 'a.png'],       // no pipe → the target is its own alt
    ['![[a.png|cap]]', 'cap'],     // pothole text
    ['![[a.png|]]', ''],           // a PRESENT but empty pothole wins
  ])('%s → alt %j', (doc, alt) => {
    expect(partsOf(doc)?.alt).toBe(alt);
  });
});

describe('embedNodeAt', () => {
  test.each([
    ['![alt](p.png)', 3, [0, 13]],
    ['![[p.png]]', 4, [0, 10]],
    ['![[folder/]]', 4, [0, 12]],          // folder embed — a WikilinkEmbed too
    ['[![[p.png]]](/u)', 5, [1, 11]],      // linked embed → the EMBED span
  ] as const)('%s @%i → %j', (doc, pos, span) => {
    expect(embedNodeAt(state(doc), pos)).toEqual({ from: span[0], to: span[1] });
  });

  test('a position outside any embed → null', () => {
    expect(embedNodeAt(state('plain text'), 3)).toBeNull();
  });
});

describe('extractImageTargets width', () => {
  test('standard image |55% reports width + span', () => {
    const ts = extractImageTargets(state('![alt|55%](pic.jpg)'));
    expect(ts).toHaveLength(1);
    expect(ts[0].width).toBe('55%');
  });
  test('wikilink |55% reports width', () => {
    const ts = extractImageTargets(state('![[pic.jpg|55%]]'));
    expect(ts[0].width).toBe('55%');
  });
  test('no width → width undefined', () => {
    const ts = extractImageTargets(state('![alt](pic.jpg)'));
    expect(ts[0].width).toBeUndefined();
  });
});

/**
 * The CROSS-LANGUAGE half: the same fixture that gates the build's
 * `folder_list::parse_params` (crates/moss-core/tests/folder_embed_params.rs)
 * gates this read-side twin, so the chips the editor draws cannot claim a
 * param the build would drop. If a vector fails here, fix `parseFolderParams`
 * — never the vectors.
 */
describe('parseFolderParams — shared vectors', () => {
  interface Vector {
    name: string;
    input: string;
    expect: {
      style: string | null; sort: string | null; depth: string | null;
      group: string | null; limit: number | null;
    };
    note: string;
  }
  const VECTORS_PATH = new URL(
    '../../../fixtures/folder-embed-params.vectors.json',
    import.meta.url,
  );
  const { vectors } = JSON.parse(readFileSync(VECTORS_PATH, 'utf8')) as { vectors: Vector[] };

  test.each(vectors.map((v) => [v.name, v] as const))('%s', (_name, v) => {
    const p = parseFolderParams(v.input);
    expect({
      style: p.style ?? null,
      sort: p.sort ?? null,
      depth: p.depth ?? null,
      group: p.group ?? null,
      limit: p.limit ?? null,
    }).toEqual(v.expect);
  });
});

describe('folderChips', () => {
  test('canonical order, independent of typing order', () => {
    expect(folderChips(parseFolderParams('limit:3,sort:title,style:grid')))
      .toEqual(['style:grid', 'sort:title', 'limit:3']);
  });

  test('limit:0 gets no chip — the build only truncates when n > 0', () => {
    expect(folderChips(parseFolderParams('limit:0,style:grid'))).toEqual(['style:grid']);
  });

  test('a param the build drops gets no chip — the absence IS the diagnostic', () => {
    expect(folderChips(parseFolderParams('sort:weght,layout:minimal'))).toEqual([]);
  });
});

describe('folderParamsFromEmbed', () => {
  function partsOf(doc: string): EmbedParts {
    const st = state(doc);
    let out: EmbedParts | null = null;
    syntaxTree(st).iterate({
      enter(n) { if (isEmbedNode(n.name)) { out = embedParts(n, st.doc); return false; } },
    });
    if (!out) throw new Error(`no embed in ${doc}`);
    return out;
  }

  test('a wikilink folder embed yields its parsed pothole', () => {
    expect(folderParamsFromEmbed(partsOf('![[awards/|style:grid,sort:weight]]')))
      .toEqual({ style: 'grid', sort: 'weight' });
  });

  test('a wikilink embed with no pothole yields empty params, not the target', () => {
    expect(folderParamsFromEmbed(partsOf('![[awards/]]'))).toEqual({});
  });

  test('a markdown image folder embed yields null — the build lists nothing there', () => {
    expect(folderParamsFromEmbed(partsOf('![](/awards/)'))).toBeNull();
  });
});
