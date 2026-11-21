// Click-count disambiguation for the preview-click vocabulary.
//
// There is deliberately NO delay timer: a 250–300ms hold would make every
// single-click highlight land perceptibly late, and the vocabulary makes it
// unnecessary — the single action (reveal, no focus) is a strict prefix of the
// double (jump), so the first click of a double may fire immediately and be
// superseded. What must NOT happen is the reveal firing twice, which is what
// the detail>1 suppression is for.
import { describe, test, expect, afterAll } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { loadViewer, unloadViewer } from '../../site/__tests__/fixtures/load-viewer';
import { classifyClickGesture, installViewerGuard, loadsDocument, shouldLeaveClickToPage } from '../click-vocabulary';

describe('classifyClickGesture', () => {
  test('a lone click is the reveal, with zero latency', () => {
    expect(classifyClickGesture('click', 1)).toBe('click');
  });

  test('the second click of a double is suppressed — dblclick owns the gesture', () => {
    // Browser sequence for a double click: click(1), click(2), dblclick.
    expect(classifyClickGesture('click', 2)).toBe(null);
    // A triple click (paragraph selection) escalates the same way.
    expect(classifyClickGesture('click', 3)).toBe(null);
  });

  test('dblclick is the jump regardless of detail', () => {
    expect(classifyClickGesture('dblclick', 2)).toBe('dblclick');
    // Some engines report detail differently on synthetic events; the type wins.
    expect(classifyClickGesture('dblclick', 0)).toBe('dblclick');
  });

  test('a full double-click sequence yields exactly one reveal then one jump', () => {
    const posted = (['click', 'click', 'dblclick'] as const)
      .map((type, i) => classifyClickGesture(type, type === 'click' ? i + 1 : 2))
      .filter((x) => x !== null);
    expect(posted).toEqual(['click', 'dblclick']);
  });
});

describe('shouldLeaveClickToPage', () => {
  test('a click inside a gallery image link is left to the page', () => {
    document.body.innerHTML = '<a class="moss-gallery-link" href="/img/a.png"><img alt="a"></a>';
    expect(shouldLeaveClickToPage(document.querySelector('img')!)).toBe(true);
  });

  test('a click inside a plain link is not left to the page', () => {
    document.body.innerHTML = '<a href="/about">About</a>';
    expect(shouldLeaveClickToPage(document.querySelector('a')!)).toBe(false);
  });
});

describe('loadsDocument', () => {
  const current = new URL('http://localhost:8080/notes/?__moss_shell=1#top');
  const link = (attrs = '') => {
    document.body.innerHTML = `<a href="#" ${attrs}>x</a>`;
    return document.querySelector('a')!;
  };
  const at = (path: string) => new URL(path, 'http://localhost:8080');

  test('a link to another page loads a document', () => {
    expect(loadsDocument(link(), at('/pictures/?__moss_shell=1'), current)).toBe(true);
  });

  test('a fragment on the current page only scrolls, even the one already in the URL', () => {
    expect(loadsDocument(link(), at('/notes/?__moss_shell=1#later'), current)).toBe(false);
    expect(loadsDocument(link(), at('/notes/?__moss_shell=1#top'), current)).toBe(false);
  });

  test('a fragment on another page loads that page', () => {
    expect(loadsDocument(link(), at('/pictures/?__moss_shell=1#door'), current)).toBe(true);
  });

  test('a download commits no document', () => {
    expect(loadsDocument(link('download'), at('/files/a.zip'), current)).toBe(false);
  });
});

describe('the editor preview and the picture viewer', () => {
  const PAGE = '<main><figure class="moss-image"><img src="/a.jpg" alt="Harbour"><figcaption>Dusk</figcaption></figure></main>';
  afterAll(unloadViewer);

  test('a click on a figure picture selects in the editor and does not open the viewer', async () => {
    await loadViewer(PAGE);
    installViewerGuard(document);
    let reachedEditor = false;
    document.addEventListener('click', () => (reachedEditor = true));
    document.querySelector('img')!.click();
    expect(reachedEditor).toBe(true);
    expect(document.getElementById('lightbox')).toBeNull();
    document.querySelector('img')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(document.getElementById('lightbox')).toBeNull();
  });

  test('without the guard the same click opens the viewer', async () => {
    await loadViewer(PAGE);
    document.querySelector('img')!.click();
    expect(document.getElementById('lightbox')?.hidden).toBe(false);
  });

  test('the bridge installs the guard', () => {
    const bridge = readFileSync(resolve(__dirname, '../iframe-bridge.ts'), 'utf8');
    expect(bridge).toContain('installViewerGuard(document);');
  });
});
