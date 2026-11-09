/**
 * Regression test for the commitRename ordering invariant introduced by
 * Rename reconciliation through the registry, the canonical owner.
 *
 * flattenTree calls registry.reconcileSnapshot which
 * RETIRES entries no longer in the snapshot. If commitRename ran
 * refreshTree BEFORE dispatchRenameCompleted, the snapshot would have
 * newPath but not oldPath; reconcileSnapshot would retire the active
 * EntryId X for oldPath; dispatchRenameCompleted's editor:file-renamed
 * handler would then call registry.rename(oldPath, newPath) — but X is
 * already retired, so rename no-ops and reconcile re-mints a fresh
 * EntryId Y for newPath. Identity lost.
 *
 * The fix: dispatch FIRST so registry.rename moves X.path to newPath
 * before refreshTree's reconcile runs. Reconcile then sees newPath
 * bound to X and preserves identity.
 *
 * This file locks the ordering against future refactors. The existing
 * file-tree-rename.test.ts ALSO covers this invariant (4 inline-rename
 * invariants plus drag-drop) — see the byte-identical-with-develop guard
 * in the plan. This file is the literal-spec parity counterpart.
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
    // Bundle H: commitRename now calls renameEntryWithRefs (drop-in).
    renameEntryWithRefs: mockRenameEntry,
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

const ROOT = '/Users/me/Sites/Mira Vale';

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'Mira Vale' });
}

describe('commitRename ordering survives reconcileSnapshot', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });
    mockRenameEntry.mockResolvedValue({
      status: 'ok',
      data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' },
    });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    // Simulate editor-main.ts's editor:file-renamed handler that calls
    // registry.rename. Without this, the production order swap can't be
    // exercised — file-tree dispatches the event, editor-main is the
    // consumer.
    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('inline rename preserves the active EntryId across reconcileSnapshot retirement window', async () => {
    // Get the EntryId for index.md (the soon-to-be-renamed active file).
    const indexId = registry.getByPath('index.md')!;
    fileTree.highlight(`${ROOT}/index.md`);
    expect(registry.activeEntryId).toBe(indexId);

    // listTree after rename: index.md is gone, Mira Vale.md is present.
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['Mira Vale.md', 'about.md']) });

    // The seam runs commitRename which does the order swap internally.
    await fileTree.renameThroughTree(`${ROOT}/index.md`, `${ROOT}/Mira Vale.md`);

    // The original EntryId X (was for index.md) now bound to Mira Vale.md.
    expect(registry.getById(indexId)?.path).toBe('Mira Vale.md');

    // byPath has the new key, not the old.
    expect(registry.getByPath('Mira Vale.md')).toBe(indexId);
    expect(registry.getByPath('index.md')).toBeNull();

    // Registry's canonical activeEntryId still X — identity preserved
    // across the rename + reconcile window.
    expect(registry.activeEntryId).toBe(indexId);
    expect(fileTree.getActiveEntryId()).toBe(indexId);

    // If commitRename had the OLD order (refreshTree before
    // dispatchRenameCompleted), reconcileSnapshot would have retired X
    // and a fresh Y would have been minted for Mira Vale.md. This assertion
    // locks against that regression:
    expect(registry.activeEntryId).not.toBeNull();
    expect(registry.getById(registry.activeEntryId!)?.path).toBe('Mira Vale.md');
  });
});
