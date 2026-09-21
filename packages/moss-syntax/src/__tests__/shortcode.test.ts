import { readFileSync } from 'node:fs';
import { describe, test, expect } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { shortcodeBlockConfig, parseAttrKvSpans, shortcodeAssetRef } from '../shortcode.js';
import { SHORTCODES } from '../contract/shortcodes.generated.js';

function nodes(doc: string) {
  const lang = markdown({ extensions: [shortcodeBlockConfig] });
  const tree = lang.language.parser.parse(doc);
  const out: { name: string; from: number; to: number }[] = [];
  tree.iterate({ enter: (n) => { out.push({ name: n.name, from: n.from, to: n.to }); } });
  return out;
}

describe('fence-marker shortcode grammar', () => {
  test('emits ShortcodeOpenLine + ShortcodeName for an opener', () => {
    const ns = nodes(':::grid {cols=3}\ncell\n:::\n');
    const open = ns.find(n => n.name === 'ShortcodeOpenLine');
    const name = ns.find(n => n.name === 'ShortcodeName');
    expect(open).toMatchObject({ from: 0, to: 16 });
    expect(name).toMatchObject({ from: 3, to: 7 });
  });
  test('emits ShortcodeAttrs when attrs present', () => {
    const ns = nodes(':::grid {cols=3}\nx\n:::\n');
    const attrs = ns.find(n => n.name === 'ShortcodeAttrs');
    expect(attrs).toBeTruthy();
    expect(attrs!.from).toBe(7);
  });
  test('emits ShortcodeCloseLine for the close fence', () => {
    const ns = nodes(':::grid\ncell\n:::\n');
    const close = ns.find(n => n.name === 'ShortcodeCloseLine');
    expect(close).toBeTruthy();
    expect(close!.from).toBe(':::grid\ncell\n'.length);
  });
  test('body markdown is parsed into real nodes (Image/Emphasis), not opaque', () => {
    const ns = nodes(':::grid\n![alt](img.png) and *em*\n:::\n');
    expect(ns.find(n => n.name === 'Image')).toBeTruthy();
    expect(ns.find(n => n.name === 'Emphasis')).toBeTruthy();
  });
  test('no ShortcodeOpenLine for a colon line with no name', () => {
    const ns = nodes('::: not an opener\ntext\n');
    expect(ns.find(n => n.name === 'ShortcodeOpenLine')).toBeFalsy();
  });
  test('a nameless attrs-only opener (`:::{.class}`, Pure-CSS region) still opens', () => {
    const ns = nodes(':::{.tagline}\nx\n:::\n');
    const open = ns.find(n => n.name === 'ShortcodeOpenLine');
    expect(open).toBeTruthy();
    expect(ns.find(n => n.name === 'ShortcodeName')).toBeFalsy();
    const attrs = ns.find(n => n.name === 'ShortcodeAttrs');
    expect(attrs).toBeTruthy();
    expect(attrs!.from).toBe(3); // right after the `:::`, no name to skip
  });
  test('a bare `:::` (no name, no attrs) is still just a close fence', () => {
    const ns = nodes(':::grid\nx\n:::\n');
    expect(ns.filter(n => n.name === 'ShortcodeOpenLine').length).toBe(1);
    expect(ns.filter(n => n.name === 'ShortcodeCloseLine').length).toBe(1);
  });
  test('nested ::::buttons inside :::grid yields two opens + two closes', () => {
    const ns = nodes(':::grid\n::::buttons\n[a](#)\n::::\ncell\n:::\n');
    expect(ns.filter(n => n.name === 'ShortcodeOpenLine').length).toBe(2);
    expect(ns.filter(n => n.name === 'ShortcodeCloseLine').length).toBe(2);
  });
  test('close fence inside a blockquote still matches (composite-aware)', () => {
    const ns = nodes('> :::grid\n> body\n> :::\n');
    expect(ns.find(n => n.name === 'ShortcodeOpenLine')).toBeTruthy();
    expect(ns.find(n => n.name === 'ShortcodeCloseLine')).toBeTruthy();
  });
});

const fixtures = JSON.parse(
  readFileSync(new URL('../../fixtures/shortcode-tokens.json', import.meta.url), 'utf8'),
);

describe('Lezer grammar ↔ shared corpus parity', () => {
  for (const fx of (fixtures as Array<{ description: string; kind: string; input: string; expected: Array<{ type: string; from: number; to: number }> }>)) {
    if (fx.kind !== 'opening') continue;
    const nameTok = fx.expected.find(t => t.type === 'name');
    if (!nameTok) continue; // unnamed div / no-name openers: skip
    // CONVENTION: a marker indented >=4 spaces/tab is an indented code block per
    // CommonMark (Example 134), matching Pandoc fenced divs + Hugo/Jekyll/Zola —
    // NOT a shortcode. The grammar correctly does NOT fire on such lines. Skip
    // indented inputs here; the tab-indented corpus entry documents the OLD
    // regex tokenizer (Rust parser_parity.rs still covers it).
    const indent = /^(\s*)/.exec(fx.input)![1].replace(/\t/g, '    ');
    if (indent.length >= 4) continue;
    test(`corpus: ${fx.description}`, () => {
      const ns = nodes(`${fx.input}\nbody\n:::\n`);
      const open = ns.find(n => n.name === 'ShortcodeName');
      expect(open, `expected a ShortcodeName for "${fx.input}"`).toBeTruthy();
      expect(open!.from).toBe(nameTok.from);
      expect(open!.to).toBe(nameTok.to);
    });
  }
});

// ── Attribute blocks ────────────────────────────────────────────────────────
//
// The reader behind a hero's thumbnail, its hover card and its missing-file
// marker. Two things are worth testing and nothing else is: that the port
// agrees with moss-core's grammar on the cases an author actually types
// (quoted paths, non-ASCII filenames, the `|crop` suffix), and that it agrees
// with Rust on FAILURE — where a malformed block yields no attributes at all
// rather than the ones parsed before the mistake. A looser reader would put a
// "missing asset" marker on a hero whose path the build never reads.



describe('parseAttrKvSpans', () => {
  test('bareword value, with spans pointing at the value and the key', () => {
    const kvs = parseAttrKvSpans('{image=photo.jpg}')!;
    expect(kvs).toEqual([
      { key: 'image', value: 'photo.jpg', from: 7, to: 16, keyFrom: 1, keyTo: 6 },
    ]);
  });

  test('quoted value keeps its spaces; the span excludes the quotes', () => {
    const src = '{image="my photo.jpg"}';
    const kvs = parseAttrKvSpans(src)!;
    expect(kvs[0].value).toBe('my photo.jpg');
    expect(src.slice(kvs[0].from, kvs[0].to)).toBe('my photo.jpg');
  });

  test('a `}` inside quotes does not close the block', () => {
    const kvs = parseAttrKvSpans('{caption="a } b" image=p.jpg}')!;
    expect(kvs.map((k) => k.key)).toEqual(['caption', 'image']);
  });

  test('escapes are decoded', () => {
    expect(parseAttrKvSpans('{caption="say \\"hi\\""}')![0].value).toBe('say "hi"');
  });

  test('non-ASCII filenames read unquoted (is_bareword is Unicode)', () => {
    expect(parseAttrKvSpans('{image=頭像.png}')![0].value).toBe('頭像.png');
    expect(parseAttrKvSpans('{image=café.jpg}')![0].value).toBe('café.jpg');
  });

  test('classes, id and bare width tokens are consumed, not errors', () => {
    const kvs = parseAttrKvSpans('{.wide-hero #top full image=p.jpg}')!;
    expect(kvs.map((k) => k.key)).toEqual(['image']);
  });

  test('the `scroll` bare flag is consumed, not an error', () => {
    // MIRROR of moss-core attrs.rs: `scroll` is recognized the same way the
    // width tokens are, so a `:::grid {scroll label="…"}` line still gets
    // `label=` highlighted instead of the whole block going dark.
    const kvs = parseAttrKvSpans('{scroll label="Related articles"}')!;
    expect(kvs.map((k) => k.key)).toEqual(['label']);
  });

  test('a malformed item discards the WHOLE block, as Rust does', () => {
    // `!` is not a key start → InvalidKey, which aborts. `image` parsed fine
    // and is still dropped: matching `.unwrap_or_default()` at every Rust
    // call site.
    expect(parseAttrKvSpans('{image=p.jpg !bad}')).toBeNull();
    // A bare keyword that is not one of the five width tokens is the same
    // error.
    expect(parseAttrKvSpans('{image=p.jpg masonry}')).toBeNull();
    // `'` is not a bareword char, so a single-quoted value is EmptyValue.
    expect(parseAttrKvSpans("{image='p.jpg'}")).toBeNull();
    expect(parseAttrKvSpans('{image=p.jpg')).toBeNull();      // unclosed brace
    expect(parseAttrKvSpans('{image="p.jpg}')).toBeNull();    // unterminated quote
    expect(parseAttrKvSpans('image=p.jpg')).toBeNull();       // no open brace
  });
});

describe('shortcodeAssetRef', () => {
  test('hero image= → path and its span', () => {
    const args = ' {image=assets/cover.jpg}';
    const ref = shortcodeAssetRef('hero', args)!;
    expect(ref.target).toBe('assets/cover.jpg');
    expect(args.slice(ref.from, ref.to)).toBe('assets/cover.jpg');
  });

  test('a `|display attrs` suffix is split off, path span still exact', () => {
    const args = '{image="cover.jpg|cover top"}';
    const ref = shortcodeAssetRef('hero', args)!;
    expect(ref.target).toBe('cover.jpg');
    expect(args.slice(ref.from, ref.to)).toBe('cover.jpg');
  });

  test('legacy directive-line path, with and without a trailing attr block', () => {
    expect(shortcodeAssetRef('hero', ' ./cover.jpg')!.target).toBe('./cover.jpg');
    const args = ' ./cover.jpg {.dark}';
    const ref = shortcodeAssetRef('hero', args)!;
    expect(ref.target).toBe('./cover.jpg');
    expect(args.slice(ref.from, ref.to)).toBe('./cover.jpg');
  });

  test('image= wins over a positional path, as parse_hero prioritises it', () => {
    expect(shortcodeAssetRef('hero', ' old.jpg {image=new.jpg}')!.target).toBe('new.jpg');
  });

  test('no asset to name → null', () => {
    expect(shortcodeAssetRef('hero', ' {wide}')).toBeNull();
    expect(shortcodeAssetRef('hero', '')).toBeNull();
    // gallery names its images in the BODY, which the Lezer walk already sees;
    // reading them here too would double every hover and lint.
    expect(shortcodeAssetRef('gallery', ' {cols=3}')).toBeNull();
    expect(shortcodeAssetRef('grid', ' {cols=2}')).toBeNull();
  });
});

describe('agreement with the generated catalog', () => {
  test('every asset-valued attr in the artifact is one this module reads', () => {
    // `shortcodeAssetRef` derives its attr map from the generated catalog
    // (Rust SSOT). This guards the derivation end-to-end: declare an asset
    // attr in `contract/shortcodes.rs`, regenerate, and this fails until the
    // reader actually reads it.
    let assetAttrs = 0;
    for (const info of SHORTCODES) {
      for (const attr of info.attrs) {
        if (!attr.assetKinds) continue;
        assetAttrs++;
        const ref = shortcodeAssetRef(info.name, `{${attr.name}=p.jpg}`);
        expect(ref, `${info.name}.${attr.name} is an asset attr but is not read`).not.toBeNull();
        expect(ref!.target).toBe('p.jpg');
      }
    }
    expect(assetAttrs, 'artifact lost its asset attrs — stale regeneration?').toBeGreaterThan(0);
  });
});
