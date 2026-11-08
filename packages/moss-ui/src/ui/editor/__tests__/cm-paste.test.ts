/**
 * Paste verbs (cm-paste.ts):
 *   - Bare URL pasted over a non-empty selection → `[selection](url)`.
 *   - Clipboard image files (screenshot paste) → saved via the injected
 *     `saveImage` callback, `![](path)` inserted at the cursor, the path
 *     written from the page.
 *   - Anything else falls through to CM6's default paste.
 *
 * Tested through the exported pure handler `handlePaste(view, clipboard)` —
 * jsdom cannot construct real ClipboardEvent/DataTransfer payloads.
 */

import { describe, test, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import { handlePaste, type ClipboardLike } from '../cm-paste';

function makeView(doc: string, anchor = 0, head = anchor): EditorView {
  const container = document.createElement('div');
  document.body.appendChild(container);
  return new EditorView({
    state: EditorState.create({ doc, selection: { anchor, head } }),
    parent: container,
  });
}

/** Same as `makeView`, but with the real markdown + wikilink grammar wired
 * in — `insideOpenWikilink` reads `syntaxTree(state)`, which is an empty
 * stub without a language extension. The production `cm-editor.ts` always
 * configures this pair together, so this is what a real paste sees. */
function makeMarkdownView(doc: string, anchor = 0, head = anchor): EditorView {
  const container = document.createElement('div');
  document.body.appendChild(container);
  return new EditorView({
    state: EditorState.create({
      doc,
      selection: { anchor, head },
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    }),
    parent: container,
  });
}

function textClipboard(text: string): ClipboardLike {
  return { files: [], getData: () => text };
}

function imageFile(name: string, bytes: number[] = [137, 80, 78, 71]): File {
  const f = new File([new Uint8Array(bytes)], name, { type: 'image/png' });
  return f;
}

describe('paste URL over selection → linkify', () => {
  test('bare https URL over a selection produces [selection](url)', () => {
    const view = makeView('check the docs here', 10, 14); // "docs"
    const handled = handlePaste(view, textClipboard('https://example.com/docs'), {});
    expect(handled).toBe(true);
    expect(view.state.doc.toString()).toBe('check the [docs](https://example.com/docs) here');
    view.destroy();
  });

  test('URL paste with NO selection falls through (default paste)', () => {
    const view = makeView('hello', 2);
    const handled = handlePaste(view, textClipboard('https://example.com'), {});
    expect(handled).toBe(false);
    expect(view.state.doc.toString()).toBe('hello'); // untouched — CM6 default handles it
    view.destroy();
  });

  test('non-URL text over a selection falls through', () => {
    const view = makeView('hello world', 0, 5);
    const handled = handlePaste(view, textClipboard('plain words'), {});
    expect(handled).toBe(false);
    view.destroy();
  });
});

describe('paste clipboard image → save + insert', () => {
  // saveImage returns the stored file's project-relative path; the embed is
  // `![](dest)`, `dest` coming from the engine (asset_ref_for_path) as the
  // path from the page. Stubbed here for a page in `posts/`.
  const destinationFor = vi.fn(async (target: string) =>
    target.startsWith('posts/') ? target.slice('posts/'.length).replace(/ /g, '%20') : `../${target}`);
  const opts = (stored: string | null | string[]) => ({
    saveImage: Array.isArray(stored)
      ? stored.reduce((m, v) => m.mockResolvedValueOnce(v), vi.fn())
      : vi.fn().mockResolvedValue(stored),
    destinationFor,
  });

  test('image file is saved and ![](path) inserted at the cursor', async () => {
    const view = makeView('before after', 7);
    const o = opts('posts/pasted-image.png');
    const handled = handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, o);
    expect(handled).toBe(true);

    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('before ![](pasted-image.png)\nafter');
    });
    expect(o.saveImage).toHaveBeenCalledTimes(1);
    // (name, base64) — the bytes round-trip through base64.
    const [name, base64] = o.saveImage.mock.calls[0];
    expect(name).toMatch(/\.png$/);
    expect(typeof base64).toBe('string');
    expect(base64.length).toBeGreaterThan(0);
    expect(destinationFor).toHaveBeenCalledWith('posts/pasted-image.png');
    view.destroy();
  });

  test('the destination is the one the engine gives for the stored file', async () => {
    const view = makeView('', 0);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('assets/my shot.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![](../assets/my shot.png)\n');
    });
    view.destroy();
  });

  test('save failure inserts nothing', async () => {
    const view = makeView('text', 0);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts(null));
    await new Promise((r) => setTimeout(r, 10));
    expect(view.state.doc.toString()).toBe('text');
    view.destroy();
  });

  test('without a saveImage callback, image paste falls through', () => {
    const view = makeView('text', 0);
    const handled = handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, {});
    expect(handled).toBe(false);
    view.destroy();
  });

  test('pasting into the empty image /img leaves fills its parentheses', async () => {
    // cm-shortcode-catalog.ts's `img` entry inserts `![]()` with the caret
    // between the parentheses (offset triggerFrom + 4).
    const view = makeMarkdownView('![]()', 4);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/pasted.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![](pasted.png)');
    });
    view.destroy();
  });

  test('an empty destination is filled and the alt text written before it survives', async () => {
    const view = makeMarkdownView('![Sunset]()', 10);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/pasted.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![Sunset](pasted.png)');
    });
    view.destroy();
  });

  test.each([
    ['in the alt text', 4],
    ['in the destination', 15],
  ])('caret %s of an existing image: nothing is lost, the paste goes after it', async (_label, caret) => {
    const doc = '![Sunset](pier.jpg) end';
    const view = makeMarkdownView(doc, caret);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/shot.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![Sunset](pier.jpg)\n![](shot.png) end');
    });
    view.destroy();
  });

  test('caret inside an existing wiki embed: it is kept, the paste goes after it', async () => {
    const view = makeMarkdownView('![[pier.jpg|Sunset]]', 5);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/shot.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![[pier.jpg|Sunset]]\n![](shot.png)');
    });
    view.destroy();
  });

  test('pasting into brackets CM6 auto-closed after typing ![[ fills them', async () => {
    // cm-wikilink-complete.ts's `wikilinkBracketClose` produces `![[]]` with
    // the caret inside — reproduced here as the resulting document state.
    const view = makeMarkdownView('text ![[]] more', 8);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/screenshot.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('text ![](screenshot.png) more');
    });
    view.destroy();
  });

  test('pasting several images into an empty image fills it with the first, the rest after', async () => {
    const view = makeMarkdownView('![]()', 4);
    handlePaste(
      view,
      { files: [imageFile('a.png'), imageFile('b.png')], getData: () => '' },
      opts(['posts/first.png', 'posts/second.png']),
    );
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('![](first.png)\n![](second.png)');
    });
    view.destroy();
  });

  test('pasting outside any embed keeps the normal insert', async () => {
    const view = makeMarkdownView('before ![[x]] after', 7);
    handlePaste(view, { files: [imageFile('x.png')], getData: () => '' }, opts('posts/pasted.png'));
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe('before ![](pasted.png)\n![[x]] after');
    });
    view.destroy();
  });

  test('avif paste uses .avif extension', async () => {
    const view = makeView('text', 4);
    const saveImage = vi.fn().mockResolvedValue('photo.avif');
    const avifFile = new File([new Uint8Array([0, 0, 0, 24])], 'photo.avif', { type: 'image/avif' });
    handlePaste(view, { files: [avifFile], getData: () => '' }, { saveImage });
    await vi.waitFor(() => {
      // saveImage should have been called with a name ending in .avif
      expect(saveImage.mock.calls[0]?.[0]).toMatch(/\.avif$/);
    });
    view.destroy();
  });
});
