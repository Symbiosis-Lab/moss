/**
 * Integration test for inode-based identity reuse.
 *
 * Verifies that when an external rename happens between two refreshTree
 * snapshots — file with file_id='42' appears at 'new.md' in the second
 * snapshot where it was at 'old.md' in the first — the registry
 * preserves the original EntryId via byFileId lookup, even though the
 * path-string differs.
 *
 * A previous change preserved identity via
 * path string only. External renames that missed the watcher's rename
 * pairing would silently retire the active EntryId on next refreshTree.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, dir, file } from './test-helpers';

installPointerEventPolyfill();

const { mockListTree, mockGetProjectPath, mockRenameEntry } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockRenameEntry: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: mockRenameEntry,
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
}

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/blog';

function makeTreeWithFileIds(
  files: Array<{ name: string; fileId: string | null }>,
): unknown {
  return dir(ROOT, files.map(({ name, fileId }) => file(`${ROOT}/${name}`, { file_id: fileId })), { name: 'blog', file_id: '1' });
}

describe('inode-based identity reuse on external rename', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: makeTreeWithFileIds([
        { name: 'index.md', fileId: '42' },
        { name: 'about.md', fileId: '43' },
      ]),
    });
    mockRenameEntry.mockResolvedValue({ status: 'ok' });

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

  // Registry queries use project-root-relative paths.
  // The TreeNode payload from makeTreeWithFileIds() keeps
  // absolute paths (matching what Rust list_directory_inner emits); walkTree
  // converts at the ingest boundary.

  test('external rename: file_id 42 at new path inherits the original EntryId', async () => {
    const x = registry.getByPath('index.md')!;
    expect(x).not.toBeNull();
    fileTree.highlight(`${ROOT}/index.md`);
    expect(registry.activeEntryId).toBe(x);

    // Simulate external rename: index.md is now home.md, same inode 42.
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: makeTreeWithFileIds([
        { name: 'home.md', fileId: '42' },        // renamed file, same inode
        { name: 'about.md', fileId: '43' },
      ]),
    });
    await fileTree.refreshAndRender();

    // Identity preserved: X now maps to home.md (project-root-relative).
    expect(registry.getById(x)?.path).toBe('home.md');
    expect(registry.getByPath('home.md')).toBe(x);
    expect(registry.getByPath('index.md')).toBeNull();
    expect(registry.activeEntryId).toBe(x);
  });

  test('null fileId (unknown): identity NOT preserved across path change', async () => {
    fileTree.unmount();
    container.remove();
    container = document.createElement('div');
    document.body.appendChild(container);

    mockListTree.mockResolvedValue({
      status: 'ok',
      data: makeTreeWithFileIds([
        { name: 'index.md', fileId: null },
        { name: 'about.md', fileId: null },
      ]),
    });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    await fileTree.mount(container, ROOT, false);

    const x = registry.getByPath('index.md')!;
    fileTree.highlight(`${ROOT}/index.md`);

    mockListTree.mockResolvedValue({
      status: 'ok',
      data: makeTreeWithFileIds([
        { name: 'home.md', fileId: null },
        { name: 'about.md', fileId: null },
      ]),
    });
    await fileTree.refreshAndRender();

    // null bypasses byFileId — identity NOT preserved.
    expect(registry.getByPath('home.md')).not.toBe(x);
    expect(registry.getById(x)).toBeNull();  // X retired
  });
});
