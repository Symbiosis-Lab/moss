/**
 * Tests for the warm-reopen collapseForReopen() path.
 *
 * Root cause: the warm panel never calls mount() on reopen, so
 * the previous session's open-folder `overrides` persist into the new session.
 * collapseForReopen() resets overrides so the tree starts collapsed for each
 * reopen.
 *
 * These tests assert:
 *   - After populate + collapseForReopen(), the tree renders collapsed (0 rows)
 *   - a subsequent reopen (handleToggleTop(0)) opens only the root — no stale
 *     multi-folder forest resurfaces (sash model: there is no remembered
 *     forest to resurrect in the first place)
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, dir, file, setView } from './test-helpers';

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
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/blog';

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'blog' });
}

describe('collapseForReopen: warm reopen starts collapsed', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('collapseForReopen() renders 0 body rows (tree appears collapsed)', () => {
    // Simulate a prior session where the user opened the root folder so the tree
    // body renders some rows and `overrides` is genuinely populated.
    fileTree.highlight(`${ROOT}/index.md`);
    priv(fileTree).handleToggleTop(0); // opens root → overrides non-empty, body rows render
    expect(priv(fileTree).view.overrides.size).toBeGreaterThan(0);
    expect(container.querySelectorAll('.nav-body [data-path]:not(.bc)').length).toBeGreaterThan(0);
    // A drag-parked height left over from the previous session.
    setView(fileTree, { park: 240 });

    // NOW simulate the warm reopen: openFromWarmReopen() calls collapseForReopen()
    fileTree.collapseForReopen();
    fileTree.highlight(`${ROOT}/index.md`);

    // Wired to collapseAll (semantics in tree/__tests__/model.test.ts): the forest
    // and the park are gone, so the body renders 0 rows (breadcrumb only) at content-fit.
    expect(priv(fileTree).view.overrides.size).toBe(0);
    expect(priv(fileTree).view.park).toBeNull();
    const bodyRows = container.querySelectorAll('.nav-body [data-path]:not(.bc)');
    expect(bodyRows.length).toBe(0);
  });

  test('collapseForReopen() leaves no forest for a subsequent reopen to resurrect', () => {
    // Sash model: there is no lastOpenOverrides stash to prevent resurrecting —
    // collapseForReopen() wiping `overrides` is the whole story. A subsequent
    // reopen (handleToggleTop(0)) can only open the root; the prior session's
    // multi-folder forest is simply gone, not remembered-then-suppressed.
    fileTree.highlight(`${ROOT}/index.md`);
    priv(fileTree).handleToggleTop(0);
    expect(priv(fileTree).view.overrides.size).toBeGreaterThan(0);

    fileTree.collapseForReopen();
    expect(priv(fileTree).view.overrides.size).toBe(0);
    fileTree.highlight(`${ROOT}/index.md`);

    priv(fileTree).handleToggleTop(0);
    // Only the root is open now (1 entry), not the multi-folder prior set.
    expect(priv(fileTree).view.overrides.size).toBe(1);
  });

  test('collapseForReopen() is safe to call on a tree with no populated overrides', () => {
    // Edge case: called before any expansion has occurred
    expect(() => {
      fileTree.collapseForReopen();
    }).not.toThrow();
  });
});
