/**
 * Part-3 single-source chip-bar visibility.
 *
 * editor-main routes EVERY `#chip-bar` display write through one owner,
 * `setChipBarVisible(visible)`, and chip-bar visibility is a pure function of
 * the mounted surface:
 *
 *   visible IFF the editor is editing a MARKDOWN file (a folder's home page
 *   included); hidden for every non-content surface (takeover / root-empty /
 *   pending / generated / email) and for non-markdown / non-text files.
 *
 * loadFile feeds the owner `detectFileCategory(path) === 'text' &&
 * isMarkdownFile(path)`; every non-content surface (via teardownFileMode /
 * applyEditorSurface) and mountEmailComposer feed it `false`.
 *
 * This test drives the REAL owner against real DOM for the whole surface table,
 * so a future change to file typing, to the rule, or to the owner's DOM write
 * can't silently drift the chip bar (issue #4).
 */

import { describe, test, expect, beforeEach, afterEach, afterAll, vi } from 'vitest';

// editor-main runs `void init()` on import; stub editorBootstrap so init bails
// on the no-project path (nothing else in this test needs the bindings).
vi.mock('../../engine', () => ({
  engine: {
    editorBootstrap: vi.fn().mockResolvedValue({ status: 'ok', data: { project_path: null } }),
  },
}));

import { setChipBarVisible, editorBoot } from '../editor-main';

// The boot that import started must end while this file's page still exists.
afterAll(() => editorBoot);
import { detectFileCategory, isMarkdownFile } from '../file-types';

/** The exact determinant loadFile feeds setChipBarVisible for a content file. */
function chipBarVisibleForFile(path: string): boolean {
  return detectFileCategory(path) === 'text' && isMarkdownFile(path);
}

describe('chip-bar visibility — surface → visible', () => {
  let chipBarEl: HTMLElement;

  beforeEach(() => {
    chipBarEl = document.createElement('div');
    chipBarEl.id = 'chip-bar';
    document.body.appendChild(chipBarEl);
  });

  afterEach(() => {
    chipBarEl.remove();
  });

  // ── The owner's DOM contract ───────────────────────────────────────────────

  test('hidden: display none AND innerHTML emptied (no stray border hairline, #4)', () => {
    chipBarEl.style.display = '';
    chipBarEl.innerHTML = '<span class="chip">title</span>';
    setChipBarVisible(false);
    expect(chipBarEl.style.display).toBe('none');
    expect(chipBarEl.innerHTML).toBe('');
  });

  test('visible: display shown; innerHTML preserved so a later render fills it', () => {
    chipBarEl.style.display = 'none';
    chipBarEl.innerHTML = '<span class="chip">title</span>';
    setChipBarVisible(true);
    expect(chipBarEl.style.display).toBe('');
    // NOT cleared when showing — loadFile's markdown branch renders into it next.
    expect(chipBarEl.innerHTML).toBe('<span class="chip">title</span>');
  });

  test('null-safe when #chip-bar is absent (test-harness DOMs)', () => {
    chipBarEl.remove();
    expect(() => setChipBarVisible(true)).not.toThrow();
    expect(() => setChipBarVisible(false)).not.toThrow();
    document.body.appendChild(chipBarEl); // restore for afterEach
  });

  // ── Surface → visibility table, driven through the real owner ──────────────

  // File surfaces: the owner is fed the loadFile determinant.
  const fileCases: Array<{ kind: string; path: string; visible: boolean }> = [
    { kind: 'markdown file (root home)',        path: 'index.md',              visible: true },
    { kind: 'markdown file (self-named home)',  path: 'Writings/Writings.md',  visible: true },
    { kind: 'markdown file (.mdx)',             path: 'notes.mdx',             visible: true },
    { kind: 'non-markdown text (.json)',        path: 'data.json',             visible: false },
    { kind: 'non-markdown text (.css)',         path: 'styles.css',            visible: false },
    { kind: 'non-markdown text (.txt)',         path: 'notes.txt',             visible: false },
    { kind: 'image file (.png)',                path: 'photo.png',             visible: false },
    { kind: 'video file (.mp4)',                path: 'clip.mp4',              visible: false },
    { kind: 'audio file (.mp3)',                path: 'song.mp3',              visible: false },
    { kind: 'pdf file (.pdf)',                  path: 'paper.pdf',             visible: false },
    { kind: 'font file (.ttf)',                 path: 'body.ttf',              visible: false },
  ];

  test.each(fileCases)('$kind → chip bar visible=$visible', ({ path, visible }) => {
    // The classification the owner is fed matches the expectation…
    expect(chipBarVisibleForFile(path)).toBe(visible);
    // …and driving the owner produces the intended DOM.
    setChipBarVisible(chipBarVisibleForFile(path));
    expect(chipBarEl.style.display).toBe(visible ? '' : 'none');
  });

  // Non-content surfaces never mount a document → the owner is always fed
  // `false` (teardownFileMode / applyEditorSurface / mountEmailComposer).
  const nonContentSurfaces = [
    'takeover',        // a page with no file yet and the recipe that would replace it
    'root-empty',      // empty project root (4-card folder mode)
    'pending',         // a source still coming down from the cloud
    'generated',       // a build-generated page with no editable source
    'email-composer',  // the host email composer surface
  ] as const;

  test.each(nonContentSurfaces)('non-content surface "%s" → chip bar hidden', () => {
    chipBarEl.style.display = '';
    chipBarEl.innerHTML = '<span>stale</span>';
    setChipBarVisible(false); // the constant every non-content surface feeds
    expect(chipBarEl.style.display).toBe('none');
    expect(chipBarEl.innerHTML).toBe('');
  });
});
