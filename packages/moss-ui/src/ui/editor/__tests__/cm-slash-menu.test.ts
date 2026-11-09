import { describe, test, expect } from 'vitest';
import { parseSlashContext } from '../cm-slash-menu';

describe('parseSlashContext', () => {
  test('returns null for non-slash line', () => {
    expect(parseSlashContext('hello', 5)).toBeNull();
  });

  // --- line-start cases ---
  test('returns empty query for lone slash at line start', () => {
    expect(parseSlashContext('/', 1)).toMatchObject({ query: '', from: 0 });
  });
  test('returns query for /gr at line start', () => {
    expect(parseSlashContext('/gr', 3)).toMatchObject({ query: 'gr', from: 0 });
  });
  test('returns null once space follows slash at line start', () => {
    expect(parseSlashContext('/grid ', 6)).toBeNull();
  });

  // --- mid-line cases ---
  test('slash immediately after a space → context', () => {
    // "some /text" — slash at index 5, cursor at 10
    const result = parseSlashContext('some /text', 10);
    expect(result).toMatchObject({ query: 'text', from: 5 });
  });
  test('slash after tab → context', () => {
    const result = parseSlashContext('\t/h1', 4);
    expect(result).toMatchObject({ query: 'h1', from: 1 });
  });
  test('empty query when slash is right after space and cursor is on slash+1', () => {
    // "foo /" — cursor at position 5 (right after "/")
    const result = parseSlashContext('foo /', 5);
    expect(result).toMatchObject({ query: '', from: 4 });
  });

  // --- rejection cases ---
  test('mid-word slash → null', () => {
    // "ab/cd" — the "/" is not at BOL and not after whitespace
    expect(parseSlashContext('ab/cd', 5)).toBeNull();
  });
  test('"http://" → null (mid-word slashes)', () => {
    expect(parseSlashContext('http://example', 14)).toBeNull();
  });
  test('space in query → null', () => {
    // "/grid foo" — query would contain a space
    expect(parseSlashContext('/grid foo', 9)).toBeNull();
  });
  test('"/ text" (space right after slash) → null', () => {
    expect(parseSlashContext('/ text', 6)).toBeNull();
  });
  test('"some /query here" with space in query → null', () => {
    expect(parseSlashContext('some /query here', 16)).toBeNull();
  });
});

// ── applyInsert — shortcode routed through the ONE snippet path ───────────

import { EditorView as SlashEditorView } from '@codemirror/view';
import { EditorState as SlashEditorState } from '@codemirror/state';
import { applyInsert } from '../cm-slash-menu';
import { SLASH_CATALOG, BLOCK_COMMAND_CATALOG } from '../cm-shortcode-catalog';

describe('applyInsert (shortcode — snippet with tab stops)', () => {
  test('replaces /query with the expanded template, first field SELECTED', () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new SlashEditorView({
      state: SlashEditorState.create({ doc: '/gr', selection: { anchor: 3 } }),
      parent: container,
    });
    const grid = SLASH_CATALOG.find((s) => s.name === 'grid')!;
    expect(grid.kind).toBe('shortcode');

    applyInsert(view, grid, { active: true, query: 'gr', triggerFrom: 0, selectedIndex: 0 });

    const doc = view.state.doc.toString();
    expect(doc.startsWith(':::grid 2')).toBe(true);
    expect(doc).toContain('cell one');
    // The first placeholder is selected — NOT the old dead spot at :::|grid.
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('2');
    view.destroy();
    container.remove();
  });
});

// ── applyInsert — block commands ──────────────────────────────────────────

describe('applyInsert (block commands)', () => {
  function makeView(doc: string, anchor: number) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    return {
      view: new SlashEditorView({
        state: SlashEditorState.create({ doc, selection: { anchor } }),
        parent: container,
      }),
      container,
    };
  }

  test('h1: inserts "# " at line start and removes the /query', () => {
    // doc = "/h1", cursor at 3, trigger "/" is at 0
    const { view, container } = makeView('/h1', 3);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'h1')!;
    applyInsert(view, spec, { active: true, query: 'h1', triggerFrom: 0, selectedIndex: 0 });
    // triggerTo = 0 + 1 + 2 = 3; line.from = 0
    // applyLinePrefix inserts "# " at 0, then removes chars [0+2, 3+2) = [2, 5) from "# /h1"
    // Result should be "# "
    expect(view.state.doc.toString()).toBe('# ');
    // Cursor should be after the prefix (at offset 2)
    expect(view.state.selection.main.anchor).toBe(2);
    view.destroy();
    container.remove();
  });

  test('task: inserts "- [ ] " at line start with cursor after it', () => {
    const { view, container } = makeView('/task', 5);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'task')!;
    applyInsert(view, spec, { active: true, query: 'task', triggerFrom: 0, selectedIndex: 0 });
    expect(view.state.doc.toString()).toBe('- [ ] ');
    expect(view.state.selection.main.anchor).toBe('- [ ] '.length);
    view.destroy();
    container.remove();
  });

  test('callout: inserts "> [!note] " at line start with cursor after it', () => {
    const { view, container } = makeView('/callout', 8);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'callout')!;
    applyInsert(view, spec, { active: true, query: 'callout', triggerFrom: 0, selectedIndex: 0 });
    expect(view.state.doc.toString()).toBe('> [!note] ');
    // Cursor after the template, ready for the callout text.
    expect(view.state.selection.main.anchor).toBe('> [!note] '.length);
    view.destroy();
    container.remove();
  });

  test('hr: replaces current line with "---", cursor on next line', () => {
    // doc = "/hr\nnext line", cursor at 3
    const { view, container } = makeView('/hr\nnext line', 3);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'hr')!;
    applyInsert(view, spec, { active: true, query: 'hr', triggerFrom: 0, selectedIndex: 0 });
    const doc = view.state.doc.toString();
    // Line 1 should be "---", line 2 should be "next line"
    expect(doc.startsWith('---')).toBe(true);
    expect(doc).toContain('next line');
    // Cursor at position 4 = "---\n" (start of next line)
    expect(view.state.selection.main.anchor).toBe(4);
    view.destroy();
    container.remove();
  });

  test('img: inserts the standard form "![]()" with the cursor in the destination (offset +4)', () => {
    // doc = "/img", cursor at 4, triggerFrom = 0, triggerTo = 4
    const { view, container } = makeView('/img', 4);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'img')!;
    applyInsert(view, spec, { active: true, query: 'img', triggerFrom: 0, selectedIndex: 0 });
    expect(view.state.doc.toString()).toBe('![]()');
    // Cursor between the parentheses: "![](" = offset 4
    expect(view.state.selection.main.anchor).toBe(4);
    view.destroy();
    container.remove();
  });

  test('math: inserts "$$\\n\\n$$" with cursor on the blank middle line', () => {
    // doc = "/math", cursor at 5, triggerFrom = 0, triggerTo = 5
    const { view, container } = makeView('/math', 5);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'math')!;
    applyInsert(view, spec, { active: true, query: 'math', triggerFrom: 0, selectedIndex: 0 });
    expect(view.state.doc.toString()).toBe('$$\n\n$$');
    // Cursor on the blank middle line: "$$\n" = offset 3
    expect(view.state.selection.main.anchor).toBe(3);
    view.destroy();
    container.remove();
  });

  test('imath: inserts "$$" with cursor between the dollars', () => {
    // doc = "/imath", cursor at 6, triggerFrom = 0, triggerTo = 6
    const { view, container } = makeView('/imath', 6);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'imath')!;
    applyInsert(view, spec, { active: true, query: 'imath', triggerFrom: 0, selectedIndex: 0 });
    expect(view.state.doc.toString()).toBe('$$');
    // Cursor between the dollars: offset 1 → typing yields $x$
    expect(view.state.selection.main.anchor).toBe(1);
    view.destroy();
    container.remove();
  });

  test('h1 mid-line: "Hello /h1" inserts prefix and removes slash-query', () => {
    // "Hello /h1" — trigger "/" is at index 6, triggerTo = 6+1+2 = 9
    const doc = 'Hello /h1';
    const { view, container } = makeView(doc, 9);
    const spec = BLOCK_COMMAND_CATALOG.find(s => s.name === 'h1')!;
    applyInsert(view, spec, { active: true, query: 'h1', triggerFrom: 6, selectedIndex: 0 });
    // applyLinePrefix: line.from = 0; inserts "# " at 0, then deletes [6+2, 9+2) = [8, 11)
    // "# Hello /h1" → remove chars at [8,11) = "/h1" → "# Hello "
    expect(view.state.doc.toString()).toBe('# Hello ');
    view.destroy();
    container.remove();
  });
});

// ── removeTriggerAndClose — abandoned "/" trigger is deleted ───────────────

import { removeTriggerAndClose, setSlashMenu, slashMenuField } from '../cm-slash-menu';

describe('removeTriggerAndClose', () => {
  function makeFieldView(doc: string, anchor: number) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    return {
      view: new SlashEditorView({
        state: SlashEditorState.create({
          doc, selection: { anchor }, extensions: [slashMenuField],
        }),
        parent: container,
      }),
      container,
    };
  }

  test('deletes the "/query" trigger text and closes the menu', () => {
    const { view, container } = makeFieldView('hello /gr', 9);
    view.dispatch({ effects: setSlashMenu.of({ active: true, query: 'gr', triggerFrom: 6, selectedIndex: 0 }) });
    removeTriggerAndClose(view);
    expect(view.state.doc.toString()).toBe('hello ');
    expect(view.state.field(slashMenuField).active).toBe(false);
    view.destroy();
    container.remove();
  });

  test('deletes a lone "/" at line start', () => {
    const { view, container } = makeFieldView('/', 1);
    view.dispatch({ effects: setSlashMenu.of({ active: true, query: '', triggerFrom: 0, selectedIndex: 0 }) });
    removeTriggerAndClose(view);
    expect(view.state.doc.toString()).toBe('');
    view.destroy();
    container.remove();
  });

  test('no-op when the menu is not active', () => {
    const { view, container } = makeFieldView('/gr', 3);
    removeTriggerAndClose(view);
    expect(view.state.doc.toString()).toBe('/gr');
    view.destroy();
    container.remove();
  });

  test('guards against drift: does not delete if the span no longer starts with "/"', () => {
    const { view, container } = makeFieldView('hello', 5);
    view.dispatch({ effects: setSlashMenu.of({ active: true, query: '', triggerFrom: 0, selectedIndex: 0 }) });
    removeTriggerAndClose(view);
    expect(view.state.doc.toString()).toBe('hello');
    expect(view.state.field(slashMenuField).active).toBe(false);
    view.destroy();
    container.remove();
  });
});

// ── SLASH_CATALOG shape ───────────────────────────────────────────────────

describe('SLASH_CATALOG', () => {
  test('block commands come before shortcodes', () => {
    const firstShortcodeIdx = SLASH_CATALOG.findIndex(s => s.kind === 'shortcode');
    const lastBlockIdx = [...SLASH_CATALOG].reverse().findIndex(s => s.kind === 'block');
    // All blocks should appear before any shortcode
    expect(firstShortcodeIdx).toBeGreaterThan(lastBlockIdx);
  });

  test('contains all 13 block commands', () => {
    const blockNames = SLASH_CATALOG.filter(s => s.kind === 'block').map(s => s.name);
    expect(blockNames).toContain('h1');
    expect(blockNames).toContain('h2');
    expect(blockNames).toContain('h3');
    expect(blockNames).toContain('ul');
    expect(blockNames).toContain('task');
    expect(blockNames).toContain('ol');
    expect(blockNames).toContain('bq');
    expect(blockNames).toContain('callout');
    expect(blockNames).toContain('code');
    expect(blockNames).toContain('math');
    expect(blockNames).toContain('imath');
    expect(blockNames).toContain('hr');
    expect(blockNames).toContain('img');
    // 'link' intentionally excluded — the selection toolbar already provides Link.
    expect(blockNames.length).toBe(13);
  });

  test('grid shortcode has kind "shortcode"', () => {
    const grid = SLASH_CATALOG.find(s => s.name === 'grid');
    expect(grid?.kind).toBe('shortcode');
  });
});
