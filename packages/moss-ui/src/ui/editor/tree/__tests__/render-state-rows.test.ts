/**
 * Tests for hidden-count state-row DOM rendering, including the
 * 'hidden' kind.
 *
 * Open empty folders keep their normal folder row without a synthetic child.
 * Hidden and unavailable listings retain informational rows. DOM and projected
 * row counts must agree so removing the empty row also removes its height.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, idOf, dir, file, setView } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: vi.fn(),
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


import { FileTree } from '../controller';
import { EntryRegistry, type EntryId } from '../../entry-registry';
import { navH } from '../spine-projection';

const ROOT = '/Users/me/Sites/blog';

/** Root with one genuinely empty folder ("empty"), one normal folder
 *  ("posts"), and one folder whose listing filtered something out
 *  (".moss", `hidden: 2`, no `config.toml`/`theme` on the allowlist yet). */
function makeTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
      dir(`${ROOT}/empty`, []),
      dir(`${ROOT}/posts`, [
          file(`${ROOT}/posts/article-one.md`),
        ]),
      dir(`${ROOT}/.moss`, [], { hidden: 2 }),
    ], { name: 'blog' });
}

describe('state rows', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree() });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('opening a genuinely empty folder keeps only its normal folder row', () => {
    priv(fileTree).handleToggleTop(0); // open root so "empty" is a sibling row
    const emptyFolderSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'))
      .find(el => el.dataset.path === 'empty');
    expect(emptyFolderSeg).toBeDefined();
    (emptyFolderSeg!.querySelector('.chev') as HTMLElement).click();

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).toBeNull();
    expect(emptyFolderSeg!.isConnected).toBe(true);
  });

  test('opening a folder moss filtered content from renders one .nav-state-row--hidden placeholder, not empty', () => {
    priv(fileTree).handleToggleTop(0); // open root so ".moss" is a sibling row
    const mossFolderSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'))
      .find(el => el.dataset.path === '.moss');
    expect(mossFolderSeg).toBeDefined();
    (mossFolderSeg!.querySelector('.chev') as HTMLElement).click();

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).not.toBeNull();
    expect(stateRow!.classList.contains('nav-state-row--hidden')).toBe(true);
    expect(stateRow!.classList.contains('nav-state-row--empty')).toBe(false);
    expect(stateRow!.dataset.stateRow).toBe('hidden');
    expect(stateRow!.textContent).not.toBe('');
  });

  test('a re-list with no hidden content removes the informational child row', async () => {
    // Unlike the previous test, children stays [] across both listings — the
    // only thing that changes is the folder's own hidden count. dirIndex is
    // rebuilt wholesale on every refresh, so an empty children list can never
    // go stale; only EntryRecord.hidden can — this is the site the reuse
    // path (entry-registry.ts's refresh()) must actually update.
    priv(fileTree).handleToggleTop(0);
    const mossFolderSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'))
      .find(el => el.dataset.path === '.moss');
    (mossFolderSeg!.querySelector('.chev') as HTMLElement).click();
    expect(container.querySelector<HTMLElement>('.nav-state-row')?.dataset.stateRow).toBe('hidden');

    // Second listing: .moss is still empty, but nothing is filtered anymore
    // (e.g. the leftover agents/ and state.toml were deleted).
    const relisted = dir(ROOT, [
        file(`${ROOT}/index.md`),
        dir(`${ROOT}/empty`, []),
        dir(`${ROOT}/posts`, [file(`${ROOT}/posts/article-one.md`)]),
        dir(`${ROOT}/.moss`, [], { hidden: 0 }),
      ], { name: 'blog' });
    mockListTree.mockResolvedValue({ status: 'ok', data: relisted });
    await fileTree.refreshAndRender();

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).toBeNull();
  });

  test('renderedRowCount() and the DOM row count agree when a level is empty', () => {
    priv(fileTree).handleToggleTop(0);
    const emptyFolderSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'))
      .find(el => el.dataset.path === 'empty');
    (emptyFolderSeg!.querySelector('.chev') as HTMLElement).click();

    const domRows = container.querySelectorAll('.nav-body [data-path]:not(.bc), .nav-body .nav-state-row').length;
    expect(domRows).toBe(priv(fileTree).render.renderedRowCount());
  });

  test('opening a folder dirIndex has no entry for renders .nav-state-row--unavailable', () => {
    priv(fileTree).handleToggleTop(0);
    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'))
      .find(el => el.dataset.path === 'posts');
    expect(postsSeg).toBeDefined();

    // Simulate a degenerate index: posts has never been (or failed to be)
    // scanned into dirIndex, even though it's a known directory and open.
    priv<{ dirIndex: Map<EntryId, unknown[]> }>(fileTree).dirIndex.delete(idOf(fileTree, 'posts'));

    (postsSeg!.querySelector('.chev') as HTMLElement).click();

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).not.toBeNull();
    expect(stateRow!.classList.contains('nav-state-row--unavailable')).toBe(true);
    expect(stateRow!.dataset.stateRow).toBe('unavailable');
  });

  test('a folder whose only child is the active file renders no state row (not degenerate)', () => {
    // "posts" here has just article-one.md as its only child, and it's the
    // active file — spineBody deliberately does NOT emit a state row for
    // this case (the content renders in the breadcrumb, not nowhere).
    fileTree.highlight(`${ROOT}/posts/article-one.md`);
    priv(fileTree).handleToggleTop(0);

    expect(container.querySelector('.nav-state-row')).toBeNull();
  });

  test('folder-valued leaf: an empty root adds no child placeholder', () => {
    // Simulate a generated home (no root index.md, activePath = ['']) whose
    // root folder is genuinely empty.
    priv<{ dirIndex: Map<EntryId, unknown[]> }>(fileTree).dirIndex.set(idOf(fileTree, ''), []);
    setView(fileTree, { anchor: [''] });
    setView(fileTree, { overrides: new Map([[registry.getByPath('')!, 'open']]) });
    priv(fileTree).render.render();

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).toBeNull();
    expect(priv(fileTree).render.renderedRowCount()).toBe(0);
  });
});
