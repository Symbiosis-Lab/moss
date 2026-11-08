import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { Strikethrough, Table } from '@lezer/markdown';
import { shortcodeBlockConfig, wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import {
  extractCalloutBlocks,
  calloutLineClass,
  calloutMarkClass,
  buildCalloutDecorations,
  calloutExtension,
} from '../cm-callout';

/** Editor-parity markdown state — same grammar set as cm-editor.ts. */
function mdState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [markdown({ extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig] })],
  });
}

describe('extractCalloutBlocks (tree-driven)', () => {
  test('finds > [!info] block', () => {
    const doc = '> [!info]\n> This is an info callout\n> with multiple lines';
    const blocks = extractCalloutBlocks(mdState(doc));
    expect(blocks).toHaveLength(1);
    expect(blocks[0].type).toBe('info');
    expect(blocks[0].from).toBe(0);
    expect(blocks[0].to).toBe(doc.length);
  });

  test('ends when > prefix stops', () => {
    const doc = '> [!warning]\n> Be careful\n\nNormal text here';
    const blocks = extractCalloutBlocks(mdState(doc));
    expect(blocks).toHaveLength(1);
    expect(blocks[0].type).toBe('warning');
    // The Blockquote node ends at the last quoted line, before the blank line.
    const expectedEnd = '> [!warning]\n> Be careful'.length;
    expect(blocks[0].to).toBe(expectedEnd);
  });

  test('finds multiple callout blocks', () => {
    const doc = '> [!info]\n> Info text\n\n> [!warning]\n> Warning text';
    const blocks = extractCalloutBlocks(mdState(doc));
    expect(blocks).toHaveLength(2);
    expect(blocks[0].type).toBe('info');
    expect(blocks[1].type).toBe('warning');
  });

  test('does not match regular blockquotes', () => {
    const doc = '> This is a regular quote\n> Not a callout';
    expect(extractCalloutBlocks(mdState(doc))).toHaveLength(0);
  });

  test('handles callout with title text', () => {
    const doc = '> [!tip] Pro tip\n> Use this approach';
    const blocks = extractCalloutBlocks(mdState(doc));
    expect(blocks).toHaveLength(1);
    expect(blocks[0].type).toBe('tip');
  });

  test('handles empty callout body', () => {
    const doc = '> [!note]';
    const blocks = extractCalloutBlocks(mdState(doc));
    expect(blocks).toHaveLength(1);
    expect(blocks[0].type).toBe('note');
    expect(blocks[0].from).toBe(0);
    expect(blocks[0].to).toBe(doc.length);
  });

  test('does not match a callout pattern inside a fenced code block', () => {
    // The regex parser needed hand-rolled fence masking for this; the tree
    // gives it for free — a fenced region produces no Blockquote node.
    const doc = '```\n> [!info]\n> fake\n```';
    expect(extractCalloutBlocks(mdState(doc))).toHaveLength(0);
  });
});

describe('calloutLineClass', () => {
  test('returns base + type-specific class for warning', () => {
    expect(calloutLineClass('warning')).toBe('cm-callout-block cm-callout-warning');
  });

  test('returns base + type-specific class for info', () => {
    expect(calloutLineClass('info')).toBe('cm-callout-block cm-callout-info');
  });

  test('returns base + type-specific class for tip', () => {
    expect(calloutLineClass('tip')).toBe('cm-callout-block cm-callout-tip');
  });

  test('returns base + type-specific class for note', () => {
    expect(calloutLineClass('note')).toBe('cm-callout-block cm-callout-note');
  });

  test('returns base + type-specific class for danger', () => {
    expect(calloutLineClass('danger')).toBe('cm-callout-block cm-callout-danger');
  });

  test('returns base + type-specific class for error', () => {
    expect(calloutLineClass('error')).toBe('cm-callout-block cm-callout-error');
  });

  test('lowercases the type', () => {
    expect(calloutLineClass('WARNING')).toBe('cm-callout-block cm-callout-warning');
  });
});

describe('calloutMarkClass', () => {
  test('returns base + type-specific class for warning', () => {
    expect(calloutMarkClass('warning')).toBe('cm-callout-mark cm-callout-warning');
  });

  test('returns base + type-specific class for info', () => {
    expect(calloutMarkClass('info')).toBe('cm-callout-mark cm-callout-info');
  });

  test('returns base + type-specific class for tip', () => {
    expect(calloutMarkClass('tip')).toBe('cm-callout-mark cm-callout-tip');
  });

  test('lowercases the type', () => {
    expect(calloutMarkClass('NOTE')).toBe('cm-callout-mark cm-callout-note');
  });
});

describe('extractCalloutBlocks type field used for CSS classes', () => {
  const cases: Array<[string, string]> = [
    ['> [!warning] Careful\n> Be warned', 'cm-callout-block cm-callout-warning'],
    ['> [!info] Information\n> Some details', 'cm-callout-block cm-callout-info'],
    ['> [!tip] Pro tip', 'cm-callout-block cm-callout-tip'],
    ['> [!note] Remember', 'cm-callout-block cm-callout-note'],
    ['> [!danger] Destructive action', 'cm-callout-block cm-callout-danger'],
    ['> [!error] Failed', 'cm-callout-block cm-callout-error'],
  ];
  for (const [doc, expected] of cases) {
    test(`${expected} via calloutLineClass`, () => {
      const blocks = extractCalloutBlocks(mdState(doc));
      expect(blocks).toHaveLength(1);
      expect(calloutLineClass(blocks[0].type)).toBe(expected);
    });
  }

  test('callout mark class includes the type, which sets the colour', () => {
    const blocks = extractCalloutBlocks(mdState('> [!warning] Watch out'));
    expect(calloutMarkClass(blocks[0].type)).toBe('cm-callout-mark cm-callout-warning');
  });
});

describe('buildCalloutDecorations', () => {
  /** Helper: collect all decorations from a DecorationSet into a flat array */
  function collectDecorations(decoSet: ReturnType<typeof buildCalloutDecorations>) {
    const result: { from: number; to: number; spec: any }[] = [];
    const iter = decoSet.iter();
    while (iter.value) {
      result.push({ from: iter.from, to: iter.to, spec: iter.value.spec });
      iter.next();
    }
    return result;
  }

  test('produces a line decoration and a mark over the [!type] label, and no widget', () => {
    const doc = '> [!info]\n> Some info\n\nprose';
    const decos = collectDecorations(buildCalloutDecorations(
      EditorState.create({
        doc,
        selection: { anchor: doc.indexOf('prose') },
        extensions: [markdown({ extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig] })],
      }),
    ));

    const lineDecos = decos.filter(d =>
      d.spec?.class === 'cm-callout-block cm-callout-info' && d.from === d.to
    );
    expect(lineDecos.length).toBeGreaterThan(0);
    expect(decos.some(d => d.spec?.widget)).toBe(false);
    expect(decos).toContainEqual({ from: 2, to: 9, spec: expect.objectContaining({ class: 'cm-callout-mark cm-callout-info' }) });
  });

  test('produces no decorations for a plain document', () => {
    const decos = collectDecorations(buildCalloutDecorations(mdState('Just some normal text\nwith no callouts')));
    expect(decos).toHaveLength(0);
  });

  test('ignores :::shortcode blocks (rendered by the Lezer block field now)', () => {
    const decos = collectDecorations(buildCalloutDecorations(mdState(':::grid 3\ncol1\ncol2\n:::')));
    expect(decos).toHaveLength(0);
  });
});

test('the [!type] label stays as its own characters: a mark over "[!note]", nothing replaced', () => {
  const state = EditorState.create({ doc: '> [!note] title', extensions: [markdown()] });
  const set = buildCalloutDecorations(state);
  const marks: { from: number; to: number; cls?: string; widget: boolean }[] = [];
  const cur = set.iter();
  while (cur.value) {
    const spec = cur.value.spec as { widget?: unknown; class?: string };
    marks.push({ from: cur.from, to: cur.to, cls: spec.class, widget: spec.widget != null });
    cur.next();
  }
  expect(marks.filter((m) => m.widget || m.cls == null)).toEqual([]); // no widget, no bare hide
  expect(marks).toContainEqual({ from: 2, to: 9, cls: 'cm-callout-mark cm-callout-note', widget: false }); // "[!note]", not its trailing space
});

test('calloutExtension no longer bundles the slash menu', () => {
  expect(calloutExtension()).toHaveLength(1); // was 2 ([calloutDecorations, slashMenuExtension()])
});

describe('StateField integration', () => {
  test('creates EditorState with callout content without errors', async () => {
    const { calloutExtension } = await import('../cm-callout');

    const state = EditorState.create({
      doc: '> [!info]\n> Some info',
      extensions: [
        markdown({ extensions: [Strikethrough, Table, shortcodeBlockConfig, wikilinkConfig] }),
        calloutExtension(),
      ],
    });

    expect(state.doc.toString()).toBe('> [!info]\n> Some info');
  });
});
