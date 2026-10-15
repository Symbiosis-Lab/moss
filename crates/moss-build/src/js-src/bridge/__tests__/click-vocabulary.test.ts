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
import { classifyClickGesture, installViewerGuard, shouldLeaveClickToPage } from '../click-vocabulary';

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
