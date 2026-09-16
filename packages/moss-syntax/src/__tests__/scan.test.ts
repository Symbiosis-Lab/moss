/**
 * Tests for the text-driven scanner (scan.ts) — the parse Obsidian uses,
 * since it cannot register a @lezer/markdown grammar extension.
 *
 * Two layers:
 * 1. Direct semantics: offsets, nesting, arity pairing, unclosed blocks,
 *    attrs, CRLF — pinned against the stub contract in
 *    packages/obsidian-moss/src/syntax/shortcode-parser.ts.
 * 2. Cross-check: on comparable documents (no fenced-code / blockquote /
 *    indented-code wrappers, which only a real markdown parse can see) the
 *    scanner and the Lezer grammar must produce the SAME closed-block
 *    structure. The Lezer side below pairs Open/Close nodes with the same
 *    arity-stack rule the editor's `collectShortcodeBlocks` uses.
 */
import { describe, test, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { parser as markdownParser } from '@lezer/markdown';
import { scanShortcodeBlocks, KNOWN_SHORTCODES, type ShortcodeBlock } from '../scan.js';
import {
  shortcodeBlockConfig, SHORTCODE_OPEN_RE, parseAttrKvSpans, shortcodeAssetRef,
} from '../shortcode.js';
import { SHORTCODES } from '../contract/shortcodes.generated.js';

// ── 1. direct semantics ─────────────────────────────────────────────────────

describe('scanShortcodeBlocks — offsets and shape', () => {
  test('simple block: absolute offsets for from/to/closeFrom', () => {
    const text = ':::grid {cols=2}\na\n:::\n';
    const [b] = scanShortcodeBlocks(text);
    expect(b).toMatchObject({
      from: 0,
      to: text.indexOf('\n:::') + 4, // end of the close-fence line
      name: 'grid',
      attrs: '{cols=2}',
      arity: 3,
      closeFrom: text.indexOf('\n:::') + 1,
      children: [],
    });
    expect(text.slice(b.closeFrom!, b.to)).toBe(':::');
  });

  test('open-fence line start includes indent (from = line start)', () => {
    const text = 'para\n  :::grid\nx\n:::\n';
    const [b] = scanShortcodeBlocks(text);
    expect(b.from).toBe(text.indexOf('  :::grid'));
    expect(b.name).toBe('grid');
  });

  test('nameless attrs-only opener (`:::{.class}`) opens; bare `:::` does not', () => {
    const [b] = scanShortcodeBlocks(':::{.tagline}\nx\n:::\n');
    expect(b).toMatchObject({ name: '', attrs: '{.tagline}' });
    // A bare `:::` with nothing open is an unpaired close fence — plain text.
    expect(scanShortcodeBlocks(':::\nx\n')).toEqual([]);
  });

  test('a colon line with interior spaces is neither open nor close', () => {
    expect(scanShortcodeBlocks('::: :::\n')).toEqual([]);
  });

  test('multiple top-level blocks come back in document order', () => {
    const text = ':::grid\nx\n:::\n\ntext\n\n:::hero {image=h.png}\nhi\n:::\n';
    const blocks = scanShortcodeBlocks(text);
    expect(blocks.map((b) => b.name)).toEqual(['grid', 'hero']);
    expect(blocks[1].from).toBe(text.indexOf(':::hero'));
  });
});

describe('scanShortcodeBlocks — arity stack and nesting', () => {
  test('`::::` inside `:::` nests: close pairs by arity, not position', () => {
    const text = ':::grid\n::::buttons\n[a](/a)\n::::\ncell\n:::\n';
    const [grid] = scanShortcodeBlocks(text);
    expect(grid.name).toBe('grid');
    expect(grid.arity).toBe(3);
    expect(grid.children).toHaveLength(1);
    const buttons = grid.children[0];
    expect(buttons).toMatchObject({ name: 'buttons', arity: 4 });
    expect(buttons.from).toBe(text.indexOf('::::buttons'));
    expect(buttons.closeFrom).toBe(text.indexOf('\n::::\n') + 1);
    expect(grid.closeFrom).toBe(text.lastIndexOf(':::'));
  });

  test('a close fence with no open of its arity is ignored', () => {
    // ::: cannot close ::::a — the arity-4 open stays open to EOF.
    const [b] = scanShortcodeBlocks('::::a\nx\n:::\n');
    expect(b).toMatchObject({ name: 'a', arity: 4, closeFrom: null });
  });
});

describe('scanShortcodeBlocks — unclosed blocks (closeFrom: null)', () => {
  test('unclosed top-level block: to = end of last line', () => {
    const text = ':::hero {image=a.jpg}\ntext';
    const [b] = scanShortcodeBlocks(text);
    expect(b).toMatchObject({ name: 'hero', closeFrom: null, to: text.length });
  });

  test('unclosed inner open becomes an unclosed child of the block that closes', () => {
    const text = ':::grid\n::::buttons\nx\n:::\n';
    const [grid] = scanShortcodeBlocks(text);
    expect(grid.closeFrom).not.toBeNull();
    expect(grid.children).toHaveLength(1);
    expect(grid.children[0]).toMatchObject({
      name: 'buttons',
      closeFrom: null,
      to: text.indexOf('\nx') + 2, // end of its last body line, before the close fence
    });
  });

  test('unclosed opens at EOF fold into a chain (each inside the previous body)', () => {
    const text = ':::grid\n::::buttons\nx';
    const blocks = scanShortcodeBlocks(text);
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ name: 'grid', closeFrom: null });
    expect(blocks[0].children[0]).toMatchObject({ name: 'buttons', closeFrom: null, to: text.length });
  });
});

describe('scanShortcodeBlocks — attrs', () => {
  test('attrs text round-trips through the shared attr parser with spans', () => {
    const text = ':::hero {image="my photo.jpg" wide}\nx\n:::\n';
    const [b] = scanShortcodeBlocks(text);
    expect(b.attrs).toBe('{image="my photo.jpg" wide}');
    const kvs = parseAttrKvSpans(b.attrs)!;
    expect(kvs[0].value).toBe('my photo.jpg');
    expect(b.attrs.slice(kvs[0].from, kvs[0].to)).toBe('my photo.jpg');
    // …and the asset reader reads the same attrs string.
    expect(shortcodeAssetRef(b.name, b.attrs)!.target).toBe('my photo.jpg');
  });

  test('positional attrs (`:::grid 2`) come back verbatim, trimmed', () => {
    const [b] = scanShortcodeBlocks(':::grid 2\nx\n:::\n');
    expect(b.attrs).toBe('2');
  });
});

describe('scanShortcodeBlocks — CRLF tolerance', () => {
  test('\\r is a line terminator: offsets never include it', () => {
    const text = ':::grid\r\na\r\n:::\r\n';
    const [b] = scanShortcodeBlocks(text);
    expect(b.from).toBe(0);
    expect(b.closeFrom).toBe(text.indexOf('\n:::') + 1);
    expect(text.slice(b.closeFrom!, b.to)).toBe(':::'); // no trailing \r
  });

  test('CRLF and LF documents scan to the same structure', () => {
    const lf = ':::grid\n::::buttons\nx\n::::\n:::\n';
    const crlf = lf.replace(/\n/g, '\r\n');
    const names = (bs: ShortcodeBlock[]): unknown[] =>
      bs.map((b) => [b.name, b.arity, names(b.children)]);
    expect(names(scanShortcodeBlocks(crlf))).toEqual(names(scanShortcodeBlocks(lf)));
  });
});

describe('KNOWN_SHORTCODES', () => {
  test('carries every authorable catalog entry, and only those', () => {
    expect(KNOWN_SHORTCODES.map((s) => s.name).sort()).toEqual(
      SHORTCODES.filter((s) => s.authorable).map((s) => s.name).sort(),
    );
    expect(KNOWN_SHORTCODES.some((s) => s.name === 'apply')).toBe(false);
  });

  test('docs are one-line and name the fence', () => {
    for (const s of KNOWN_SHORTCODES) {
      expect(s.doc.startsWith(`:::${s.name}`)).toBe(true);
      expect(s.doc).not.toContain('\n');
      expect(s.doc).not.toContain('${'); // tab stops flattened
    }
  });
});

// ── 2. cross-check against the Lezer grammar ────────────────────────────────
//
// Same pairing rule as the editor's collectShortcodeBlocks, run over the raw
// Lezer tree (no CM6). Projection is closed blocks only: the scanner reports
// unclosed blocks (a host linting mid-keystroke needs them), the decoration
// walk deliberately drops them.

interface Proj {
  name: string; attrs: string; arity: number;
  openLine: number; closeLine: number; children: Proj[];
}

const lezer = markdownParser.configure(shortcodeBlockConfig);

function lineStartAt(text: string, pos: number): number {
  return text.lastIndexOf('\n', pos - 1) + 1;
}

function lezerBlocks(text: string): Proj[] {
  interface Open { openLine: number; arity: number; name: string; attrs: string; children: Proj[] }
  const stack: Open[] = [];
  const out: Proj[] = [];
  lezer.parse(text).iterate({
    enter(n) {
      if (n.name === 'ShortcodeOpenLine') {
        const openLine = lineStartAt(text, n.from);
        let end = text.indexOf('\n', n.from);
        if (end === -1) end = text.length;
        const m = SHORTCODE_OPEN_RE.exec(text.slice(openLine, end))!;
        stack.push({ openLine, arity: m[2].length, name: m[3] ?? '', attrs: m[4].trim(), children: [] });
      } else if (n.name === 'ShortcodeCloseLine') {
        const arity = text.slice(n.from, n.to).trim().length;
        for (let i = stack.length - 1; i >= 0; i--) {
          if (stack[i].arity === arity) {
            const open = stack.splice(i)[0];
            const p: Proj = {
              name: open.name, attrs: open.attrs, arity: open.arity,
              openLine: open.openLine, closeLine: lineStartAt(text, n.from), children: open.children,
            };
            if (stack.length > 0) stack[stack.length - 1].children.push(p);
            else out.push(p);
            break;
          }
        }
      }
    },
  });
  return out;
}

/** Scanner output projected to closed blocks (dropping unclosed subtrees,
 *  as the Lezer decoration walk does). */
function scanProj(text: string, blocks = scanShortcodeBlocks(text)): Proj[] {
  return blocks
    .filter((b) => b.closeFrom !== null)
    .map((b) => ({
      name: b.name, attrs: b.attrs, arity: b.arity,
      openLine: b.from, closeLine: b.closeFrom!, children: scanProj(text, b.children),
    }));
}

const corpus = JSON.parse(
  readFileSync(new URL('../../fixtures/shortcode_structure_corpus.json', import.meta.url), 'utf8'),
) as { cases: { name: string; input: string }[] };

const EXTRA_COMPARABLE = [
  '',
  'plain text, no fences at all\n',
  ':::\n', // unpaired close fence
  ':::{.tagline}\nsome text\n:::\n', // nameless class region
  'paragraph line\n:::grid\ninterrupts the paragraph\n:::\n',
  ':::grid\n::::buttons\nx\n:::\n', // unclosed inner (both sides drop/report it)
  '::::a\nx\n:::\n', // arity mismatch — nothing closes
  ':::hero {image="a b.jpg"}\n# Title\n:::\nafter\n',
  ':::grid 2\nleft\n+++\nright\n:::\n:::grid\nsecond\n:::\n',
];

describe('scanner ↔ Lezer grammar agreement (comparable documents)', () => {
  for (const c of corpus.cases) {
    test(`corpus: ${c.name}`, () => {
      expect(scanProj(c.input)).toEqual(lezerBlocks(c.input));
    });
  }
  EXTRA_COMPARABLE.forEach((input, i) => {
    test(`extra #${i}: ${JSON.stringify(input.slice(0, 40))}`, () => {
      expect(scanProj(input)).toEqual(lezerBlocks(input));
    });
  });
});
