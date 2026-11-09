/**
 * Unit tests for cm-drop.ts.
 *
 * Asserts:
 *   (a) Dropping `application/x-moss-entries` inserts an embed per entry at
 *       the position returned by `posAtCoords`: `![](path)` written from the
 *       page for every kind; `![[name]]` only where the backend refuses a destination.
 *   (b) A drop event whose dataTransfer types include `Files` returns false
 *       and does not insert anything (OS drop handled by Tauri path).
 *
 * DataTransfer shim copied from file-tree-multiselect.test.ts:79-92 (jsdom
 * lacks a real DataTransfer implementation).
 */

import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { dropExtension, insertEntryEmbedsAtCoords, embedFor, type DestinationFor } from '../cm-drop';

/** Stands in for the engine's `asset_ref_for_path`, for a page in `posts/`. */
/** Lets the async insert (it asks for destinations first) land. */
const flush = () => new Promise((r) => setTimeout(r, 0));

const destinationFor: DestinationFor = async (t) => (t.startsWith('posts/') ? t.slice(6) : `../${t}`);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeView(doc: string, resolveEntries: (ids: number[]) => string[] = () => []): EditorView {
  const container = document.createElement('div');
  document.body.appendChild(container);
  return new EditorView({
    state: EditorState.create({
      doc,
      extensions: [dropExtension({ resolveEntries, destinationFor })],
    }),
    parent: container,
  });
}

function makeDropEvent(dt: DataTransfer, x = 4, y = 4): Event {
  const ev = new Event('drop', { bubbles: true, cancelable: true });
  Object.defineProperty(ev, 'dataTransfer', { value: dt });
  Object.defineProperty(ev, 'clientX', { value: x });
  Object.defineProperty(ev, 'clientY', { value: y });
  return ev;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('cm-drop — internal tree→editor drop', () => {
  test('dropping x-moss-entries inserts ![](path) image embeds at posAtCoords', async () => {
    const resolve = vi.fn((ids: number[]) => ids.map(i => `posts/img-${i}.png`));
    const view = makeView('abc', resolve);

    // Mock posAtCoords to return position 1
    vi.spyOn(view, 'posAtCoords').mockReturnValue(1);

    const dt = new DataTransfer();
    dt.setData('application/x-moss-entries', JSON.stringify({ entryIds: [7, 9] }));
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);
    await flush();

    const doc = view.state.doc.toString();
    expect(doc).toContain('![](img-7.png)');
    expect(doc).toContain('![](img-9.png)');
    expect(resolve).toHaveBeenCalledWith([7, 9]);

    view.destroy();
  });

  test('dropping x-moss-entries inserts multiple page embeds on separate lines', async () => {
    const resolve = vi.fn((ids: number[]) => ids.map(i => `posts/file-${i}.md`));
    const view = makeView('hello\n', resolve);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    const dt = new DataTransfer();
    dt.setData('application/x-moss-entries', JSON.stringify({ entryIds: [1, 2] }));
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);
    await flush();

    const doc = view.state.doc.toString();
    expect(doc).toContain('![](file-1.md)');
    expect(doc).toContain('![](file-2.md)');
    // Both embeds on separate lines (each ends with \n)
    expect(doc).toContain('![](file-1.md)\n![](file-2.md)');

    view.destroy();
  });

  test('a Files-typed drop returns false and does not insert', () => {
    const resolve = vi.fn(() => ['should-not-appear.png']);
    const view = makeView('original doc', resolve);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    const dt = new DataTransfer();
    // Add 'Files' type to simulate an OS file drop.
    // Cast through unknown because DOM lib types DataTransfer.types as readonly string[],
    // but our DataTransferPolyfill defines it as mutable string[].
    (dt as unknown as { types: string[] }).types.push('Files');

    const ev = makeDropEvent(dt);
    view.contentDOM.dispatchEvent(ev);

    // Document must be unchanged
    expect(view.state.doc.toString()).toBe('original doc');
    // resolveEntries should never have been called
    expect(resolve).not.toHaveBeenCalled();

    view.destroy();
  });

  test('unknown drop type (plain text drag) is ignored', () => {
    const resolve = vi.fn(() => ['nope.png']);
    const view = makeView('unchanged', resolve);

    const dt = new DataTransfer();
    dt.setData('text/plain', 'some text drag');
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);

    expect(view.state.doc.toString()).toBe('unchanged');
    expect(resolve).not.toHaveBeenCalled();

    view.destroy();
  });

  test('drop with unresolvable entry IDs does not insert', () => {
    // resolveEntries returns empty array (no known entry)
    const resolve = vi.fn(() => []);
    const view = makeView('doc unchanged', resolve);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    const dt = new DataTransfer();
    dt.setData('application/x-moss-entries', JSON.stringify({ entryIds: [999] }));
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);

    expect(view.state.doc.toString()).toBe('doc unchanged');

    view.destroy();
  });

  test('posAtCoords null falls back to document end', async () => {
    const resolve = vi.fn(() => ['posts/fallback.png']);
    const view = makeView('abc', resolve);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(null);

    const dt = new DataTransfer();
    dt.setData('application/x-moss-entries', JSON.stringify({ entryIds: [42] }));
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);
    await flush();

    const doc = view.state.doc.toString();
    // Should insert at end (after 'abc')
    expect(doc).toBe('abc![](fallback.png)\n');

    view.destroy();
  });

  test('dropping a folder entry inserts ![](../tools/webapp/)', async () => {
    // resolveEntries returns a trailing-slash name for a directory entry
    const resolve = vi.fn((_ids: number[]) => ['tools/webapp/']);
    const view = makeView('', resolve);

    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    const dt = new DataTransfer();
    dt.setData('application/x-moss-entries', JSON.stringify({ entryIds: [3] }));
    const ev = makeDropEvent(dt);

    view.contentDOM.dispatchEvent(ev);
    await flush();

    const doc = view.state.doc.toString();
    expect(doc).toContain('![](../tools/webapp/)');

    view.destroy();
  });
});

// ---------------------------------------------------------------------------
// insertEntryEmbedsAtCoords — the shared insert used by BOTH the DOM drop
// handler above (safety net) and editor-main.ts's Tauri onDragDropEvent
// routing (the production path for internal tree→editor drops).
// ---------------------------------------------------------------------------

describe('insertEntryEmbedsAtCoords — shared insert helper', () => {
  test('block-snaps the drop position and inserts joined embed lines with trailing newline', async () => {
    const view = makeView('abc');
    // Raw pos 1 is mid-word on the only line; snapToBlockBoundary snaps to
    // the line start (jsdom has no layout, so coordsAtPos yields no
    // bottom-half override) → insert lands at 0, on its own lines.
    vi.spyOn(view, 'posAtCoords').mockReturnValue(1);

    const inserted = await insertEntryEmbedsAtCoords(view, ['posts/a.png', 'posts/b.jpg'], destinationFor, 4, 4);

    expect(inserted).toBe(true);
    expect(view.state.doc.toString()).toBe('![](a.png)\n![](b.jpg)\nabc');

    view.destroy();
  });

  test('posAtCoords null falls back to document end', async () => {
    const view = makeView('abc');
    vi.spyOn(view, 'posAtCoords').mockReturnValue(null);

    const inserted = await insertEntryEmbedsAtCoords(view, ['posts/fallback.png'], destinationFor, -100, -100);

    expect(inserted).toBe(true);
    expect(view.state.doc.toString()).toBe('abc![](fallback.png)\n');

    view.destroy();
  });

  test('empty and falsy-only name lists insert nothing and return false', async () => {
    const view = makeView('unchanged');
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    expect(await insertEntryEmbedsAtCoords(view, [], destinationFor, 4, 4)).toBe(false);
    expect(await insertEntryEmbedsAtCoords(view, ['', ''], destinationFor, 4, 4)).toBe(false);
    expect(view.state.doc.toString()).toBe('unchanged');

    view.destroy();
  });

  test('folder destinations keep the trailing slash', async () => {
    const view = makeView('');
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    await insertEntryEmbedsAtCoords(view, ['webapp/'], destinationFor, 4, 4);

    expect(view.state.doc.toString()).toBe('![](../webapp/)\n');

    view.destroy();
  });
});

// ---------------------------------------------------------------------------
// embedFor — which form each inserted file takes. The destination text is the
// engine's (reference_parity.rs checks it and that the build links it).
// ---------------------------------------------------------------------------

describe('embedFor', () => {
  test.each([
    ['an image', 'posts/photo.jpg', '![](photo.jpg)'],
    ['an image in another folder', 'assets/photo.jpg', '![](../assets/photo.jpg)'],
    ['a video', 'media/clip.mp4', '![](../media/clip.mp4)'],
    ['audio', 'posts/talk.mp3', '![](talk.mp3)'],
    ['a PDF', 'posts/paper.pdf', '![](paper.pdf)'],
    ['an HTML page', 'apps/demo.html', '![](../apps/demo.html)'],
    ['a 3D model', 'posts/chair.glb', '![](chair.glb)'],
    ['a markdown page', 'notes/other note.md', '![](../notes/other note.md)'],
    ['a table', 'data/table.csv', '![](../data/table.csv)'],
    ['a notebook', 'nb/analysis.ipynb', '![](../nb/analysis.ipynb)'],
    ['a folder', 'tools/webapp/', '![](../tools/webapp/)'],
  ])('%s', async (_label, target, want) => {
    expect(await embedFor(target, destinationFor)).toBe(want);
  });

  test('a destination the backend refuses falls back to the wiki form', async () => {
    expect(await embedFor('posts/photo.jpg', async () => null)).toBe('![[photo.jpg]]');
    expect(await embedFor('notes/a.md', async () => null)).toBe('![[a.md]]');
    expect(await embedFor('tools/webapp/', async () => null)).toBe('![[webapp/]]');
  });
});
