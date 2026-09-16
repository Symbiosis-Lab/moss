/**
 * Unit tests for the footnote grammar (`[^label]` / `[^label]: note`).
 *
 * Every behavior here was MEASURED, never reasoned from a spec:
 *   - the mis-parses in the "before" block are what @lezer/markdown actually
 *     produced for the reported document, captured so a future change to the
 *     registration order cannot quietly restore them;
 *   - the build-side expectations (a marker with no definition stays literal,
 *     a marker in a code span stays literal) are the ones pulldown-cmark is
 *     held to by crates/moss-core/tests/footnotes_and_strikethrough.rs.
 */

import { describe, test, expect } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { Strikethrough, Table, TaskList } from '@lezer/markdown';
import { shortcodeBlockConfig } from '../shortcode.js';
import { wikilinkConfig } from '../wikilink-grammar.js';
import { mathConfig } from '../math-grammar.js';
import { footnoteConfig } from '../footnote-grammar.js';

/** The EXACT extension set cm-editor.ts registers, so interactions with the
 *  sibling grammars are part of what these tests verify. */
const lang = markdown({
  extensions: [Strikethrough, Table, TaskList, shortcodeBlockConfig, wikilinkConfig, mathConfig, footnoteConfig],
});

/** Every node of `name`, as `[from, to, text]`, in document order. */
function nodes(doc: string, name: string): Array<[number, number, string]> {
  const tree = lang.language.parser.parse(doc);
  const out: Array<[number, number, string]> = [];
  tree.iterate({
    enter(n) {
      if (n.name === name) out.push([n.from, n.to, doc.slice(n.from, n.to)]);
    },
  });
  return out;
}

/** Node names present anywhere in the tree — for asserting an ABSENCE. */
function nodeNames(doc: string): Set<string> {
  const tree = lang.language.parser.parse(doc);
  const out = new Set<string>();
  tree.iterate({ enter: (n) => void out.add(n.name) });
  return out;
}

describe('footnote markers', () => {
  test('a marker is a FootnoteRef with label and marks', () => {
    const doc = '阿公[^2]告訴我。';
    expect(nodes(doc, 'FootnoteRef')).toEqual([[2, 6, '[^2]']]);
    expect(nodes(doc, 'FootnoteLabel')).toEqual([[4, 5, '2']]);
    expect(nodes(doc, 'FootnoteMark')).toEqual([[2, 4, '[^'], [5, 6, ']']]);
  });

  test('non-numeric labels work', () => {
    expect(nodes('see[^note-a] here', 'FootnoteLabel')).toEqual([[5, 11, 'note-a']]);
  });

  test('a marker in a code span stays literal', () => {
    // Rust counterpart: "code span mangled" in footnotes_and_strikethrough.rs.
    const names = nodeNames('Write `[^1]` where it belongs.');
    expect(names.has('InlineCode')).toBe(true);
    expect(names.has('FootnoteRef')).toBe(false);
  });

  test('an escaped bracket does not open a marker', () => {
    expect(nodeNames('a \\[^1] b').has('FootnoteRef')).toBe(false);
  });

  test('an empty label is not a marker', () => {
    expect(nodeNames('a [^] b').has('FootnoteRef')).toBe(false);
  });
});

describe('footnote definitions', () => {
  test('the note body is parsed as markdown, not swallowed as a URL', () => {
    // THE REGRESSION. A body with no ASCII space used to be a valid link
    // DESTINATION, so `[^1]: …` parsed as a link reference definition and the
    // whole note became one opaque `URL` node — nothing inside it rendered.
    const doc = '[^1]: 伊：代词他/她，i1。潮汕地區各地的潮汕話有少許差異，本文腳註皆為揭陽音。';
    const names = nodeNames(doc);
    expect(names.has('FootnoteDefinition')).toBe(true);
    expect(names.has('LinkReference')).toBe(false);
    expect(names.has('URL')).toBe(false);
    expect(nodes(doc, 'FootnoteLabel')).toEqual([[2, 3, '1']]);
    expect(nodes(doc, 'FootnoteMark')).toEqual([[0, 2, '[^'], [3, 5, ']:']]);
  });

  test('a body WITH a space parses identically', () => {
    // The two footnotes in the reported document rendered differently only
    // because one body contained an ASCII space and the other did not.
    const withSpace = nodeNames('[^2]: 阿公：祖父，a1 gong1');
    const without = nodeNames('[^1]: 阿公：祖父，a1gong1');
    expect(withSpace.has('FootnoteDefinition')).toBe(true);
    expect(without.has('FootnoteDefinition')).toBe(true);
    expect(withSpace.has('Link')).toBe(false);
  });

  test('inline markup inside a note renders', () => {
    const names = nodeNames('[^1]: see *this* and [that](/t)');
    expect(names.has('Emphasis')).toBe(true);
    expect(names.has('Link')).toBe(true);
  });

  test('an empty note body is still a definition', () => {
    expect(nodeNames('[^1]:').has('FootnoteDefinition')).toBe(true);
  });

  test('four spaces of indent is still indented code, not a definition', () => {
    const names = nodeNames('para\n\n    [^1]: not a note\n');
    expect(names.has('CodeBlock')).toBe(true);
    expect(names.has('FootnoteDefinition')).toBe(false);
  });

  test('a real link reference definition is untouched', () => {
    const names = nodeNames('[ref]: https://example.com');
    expect(names.has('LinkReference')).toBe(true);
    expect(names.has('FootnoteDefinition')).toBe(false);
  });

  test('a definition inside a blockquote is still a definition', () => {
    // Rust counterpart: "> [^1]: nested def" in footnotes_and_strikethrough.rs.
    const names = nodeNames('> [^1]: nested def\n');
    expect(names.has('Blockquote')).toBe(true);
    expect(names.has('FootnoteDefinition')).toBe(true);
  });
});

describe('the reported document, end to end', () => {
  const DOC = [
    '2017年春天，18歲的我獨自在曼谷「尋找」老二姑。伊[^1]就在大皇宮邊的石龍軍路上，臨行前阿公[^2]告訴我。',
    '',
    '[^1]: 伊：代词他/她，i1。潮汕地區各地的潮汕話有少許差異，本文腳註皆為揭陽音。',
    '',
    '[^2]: 阿公：祖父，a1 gong1',
    '',
  ].join('\n');

  test('two markers, two definitions, and no links anywhere', () => {
    expect(nodes(DOC, 'FootnoteRef').map((n) => n[2])).toEqual(['[^1]', '[^2]']);
    expect(nodes(DOC, 'FootnoteDefinition').length).toBe(2);
    const names = nodeNames(DOC);
    expect(names.has('Link')).toBe(false);
    expect(names.has('LinkReference')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// A definition is a CONTAINER. Each row here was measured against
// pulldown-cmark first (crates/moss-core); the editor matching the build is
// the whole point, so these are build-parity assertions wearing tree clothes.
// ---------------------------------------------------------------------------
describe('a definition spans the lines its note spans', () => {
  const defOf = (doc: string) => {
    const t = lang.language.parser.parse(doc);
    let found: { from: number; to: number } | null = null;
    t.iterate({ enter: (n) => { if (!found && n.name === 'FootnoteDefinition') found = { from: n.from, to: n.to }; } });
    // `found` is assigned inside the callback above, which control-flow
    // analysis does not track, so it stays narrowed to `null` here.
    return found as { from: number; to: number } | null;
  };
  const kindsIn = (doc: string) => {
    const t = lang.language.parser.parse(doc);
    const out: string[] = [];
    let inside = false;
    t.iterate({
      enter: (n) => {
        if (n.name === 'FootnoteDefinition') { inside = true; return; }
        if (inside && n.name !== 'FootnoteMark' && n.name !== 'FootnoteLabel') out.push(n.name);
      },
      leave: (n) => { if (n.name === 'FootnoteDefinition') inside = false; },
    });
    return out;
  };

  test('an unindented next line continues the note (lazy continuation)', () => {
    const doc = '[^1]: first\nsecond\n';
    expect(defOf(doc)?.to).toBe(doc.indexOf('second') + 'second'.length);
  });

  test('a four-space line continues the note — NOT an indented code block', () => {
    // The failure this replaces: careful indentation produced a monospace
    // code box in the editor while the site rendered note prose.
    const doc = '[^1]: first\n    second\n';
    expect(defOf(doc)?.to).toBe(doc.indexOf('second') + 'second'.length);
    expect(kindsIn(doc)).not.toContain('CodeBlock');
  });

  test('a blank line then an indented line is a SECOND paragraph in the note', () => {
    const doc = '[^1]: first\n\n    second para\n';
    expect(kindsIn(doc).filter((k) => k === 'Paragraph')).toHaveLength(2);
    expect(defOf(doc)?.to).toBe(doc.indexOf('second para') + 'second para'.length);
  });

  test('a note can hold a list', () => {
    const doc = '[^1]: note\n\n    - item\n    - item2\n';
    expect(kindsIn(doc)).toContain('BulletList');
  });

  test('an unindented line after a blank ends the note', () => {
    const doc = '[^1]: first\n\nplain para\n';
    expect(defOf(doc)?.to).toBe(doc.indexOf('first') + 'first'.length);
  });

  test('the next definition is its own note, not a continuation', () => {
    const doc = '[^1]: one\n\n[^2]: two\n';
    let n = 0;
    lang.language.parser.parse(doc).iterate({ enter: (x) => { if (x.name === 'FootnoteDefinition') n++; } });
    expect(n).toBe(2);
  });

  test('the note body is parsed as markdown, so emphasis inside it renders', () => {
    expect(kindsIn('[^1]: see **bold**\n')).toContain('StrongEmphasis');
  });
});
