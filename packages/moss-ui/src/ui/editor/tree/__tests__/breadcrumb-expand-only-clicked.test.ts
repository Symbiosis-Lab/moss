/**
 * A breadcrumb folder opens by itself: its ancestors stay as crumbs, and
 * their other children do not appear in the tree body.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, dir, file, overrideAt } from './test-helpers';

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

describe('breadcrumb expand opens only the clicked folder', () => {
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: dir(ROOT, [
        file('home.md'),
        dir(`${ROOT}/a`, [
          file('a/sibling.md'),
          dir(`${ROOT}/a/b`, [file(`${ROOT}/a/b/doc.md`), file('a/b/other.md')]),
        ]),
      ], { name: 'blog' }),
    });
    fileTree = new FileTree(new EntryRegistry());
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/a/b/doc.md`);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('clicking the deepest folder crumb leaves its parent and the root closed', () => {
    const anchor: string[] = priv(fileTree).view.anchor;
    const index = anchor.indexOf('a/b');
    expect(index).toBeGreaterThan(0);

    priv(fileTree).handleToggleTop(index);

    expect(overrideAt(fileTree, 'a/b')).toBe('open');
    expect(overrideAt(fileTree, 'a')).not.toBe('open');
    expect(overrideAt(fileTree, '')).not.toBe('open');
    const rows = [...container.querySelectorAll('.nav-body [data-path]:not(.bc)')].map(
      (el) => el.getAttribute('data-path'),
    );
    expect(rows).toContain('a/b/other.md');
    expect(rows).not.toContain('a/sibling.md');
    expect(rows).not.toContain('home.md');
  });
});
