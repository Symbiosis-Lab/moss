/**
 * The file tree's context menus (tree/menu.ts, driven through the controller).
 *
 * N-aware menus:
 *
 * Verifies:
 * - the exact rows and groups of the file, folder (root and not), empty-area
 *   and batch menus
 * - Finder convention: right-clicking a non-selected item collapses selection to it
 * - "Delete N items" calls deleteEntries with confirm
 * - "Move N items to…" calls moveEntries after prompt
 * - each single-menu action reaches tree/ops.ts with the right path
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const {
  mockListTree, mockGetProjectPath, mockRenameEntry, mockCopyFilesToProject,
  mockRevealEntry, mockDuplicateEntry,
  mockCreateFile, mockCreateFolder, mockMoveEntries, mockDeleteEntries,
  mockScanReferencesForDelete, mockCleanReferencesAndDelete, mockImportSite,
} = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockRenameEntry: vi.fn(),
  mockCopyFilesToProject: vi.fn(),
  mockRevealEntry: vi.fn(),
  mockDuplicateEntry: vi.fn(),
  mockCreateFile: vi.fn(),
  mockScanReferencesForDelete: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  mockCleanReferencesAndDelete: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  mockCreateFolder: vi.fn(),
  mockMoveEntries: vi.fn(),
  mockDeleteEntries: vi.fn(),
  mockImportSite: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    createTemplate: vi.fn(),
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: mockRenameEntry,
    copyFilesToProject: mockCopyFilesToProject,
    revealEntry: mockRevealEntry,
    duplicateEntry: mockDuplicateEntry,
    createFiles: mockCreateFile,
    scanReferencesForDelete: mockScanReferencesForDelete,
    cleanReferencesAndDelete: mockCleanReferencesAndDelete,
    createFolder: mockCreateFolder,
    moveEntries: mockMoveEntries,
    deleteEntries: mockDeleteEntries,
  },
}));

/** The two Versions items ask the editor webview to enter Versions mode
 *  through a document event (editor-main.ts listens); this records its
 *  detail. Registered per test so a listener never outlives its DOM. */
function openVersionsRequests(): Array<{ scope: string; path: string; isRoot: boolean }> {
  const seen: Array<{ scope: string; path: string; isRoot: boolean }> = [];
  document.addEventListener('editor:open-versions', ((e: CustomEvent) => { seen.push(e.detail); }) as EventListener);
  return seen;
}

// The host's folder picker answers a cancel unless a test picks a folder.
import { host } from "../../../host";
const mockPickFolder = vi.fn(async (_defaultPath?: string): Promise<string | null> => null);
host.pickFolder = mockPickFolder;
host.reveal = mockRevealEntry;
host.importSite = mockImportSite;

/** tree/ops.ts's failure alerts are a real moss-alert DOM modal now (moss-
 *  alert.ts), not the Tauri dialog plugin — read its title instead of a
 *  spied plugin call. */
function mossAlertTitle(): string | null {
  return document.querySelector('.moss-modal-title')?.textContent ?? null;
}


// JSDOM doesn't implement CSS.escape
if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
} else if (typeof globalThis.CSS.escape !== 'function') {
  (globalThis.CSS as { escape: (s: string) => string }).escape = (s: string) =>
    s.replace(/(["\\])/g, '\\$1');
}

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { enterDirMode, dir, file, priv } from './test-helpers';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const PROJECT_ROOT = '/projects/my-site';

function defaultTree() {
  // The backend emits the home DECISION per node (`TreeNode.is_home`, elected
  // in list_tree_inner_cached) and sorts the elected home first; the frontend
  // only consumes the flag. So in this mock payload the flag IS the contract:
  // the root's `index.md` carries is_home → no "Create home file" item for the
  // root. `posts/` is depicted HOME-LESS (no flagged child) to exercise the
  // no-home branch — a real backend would alpha-fallback-elect alpha.md;
  // election realism is covered by the Rust is_home_* tests and
  // file-tree-home-parity.test.ts, while alpha/beta here serve as plain leaf
  // rows for the multi-select cases below.
  return dir(PROJECT_ROOT, [
      file(`${PROJECT_ROOT}/index.md`, { is_home: true }),
      dir(`${PROJECT_ROOT}/posts`, [
          file(`${PROJECT_ROOT}/posts/alpha.md`),
          file(`${PROJECT_ROOT}/posts/beta.md`),
        ]),
    ], { name: 'my-site' });
}

/** Long enough for any work a click schedules to settle before asserting. */
const SETTLE_MS = 260;

/** Find a leaf row by its full-name data attribute. */
function findLeaf(container: HTMLElement, name: string): HTMLElement | null {
  return Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf'))
    .find(el => el.dataset.fullName === name) ?? null;
}

/** Fire a contextmenu event at the given element. */
function rightClick(el: HTMLElement): void {
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
}

/** Find a .ctx-menu item whose text matches the predicate. */
function findMenuItem(text: string): HTMLElement | null {
  return Array.from(document.querySelectorAll<HTMLElement>('.ctx-item'))
    .find(el => el.textContent?.includes(text)) ?? null;
}

/** The delete modal, which opens in the same frame as the Delete click. */
function deleteModal(): HTMLElement | null {
  return document.querySelector<HTMLElement>('.moss-modal--delete-confirm.visible');
}

/** Press one of the delete modal's buttons by its stable `data-action`. */
function answerDelete(action: 'trash' | 'cancel' | 'trash-anyway' | 'clean-and-trash'): void {
  const modal = deleteModal();
  if (!modal) throw new Error('delete modal is not open');
  modal.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`)!.click();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('file-tree context menu — N-aware', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    vi.useRealTimers();
    // Close any open context menu
    document.querySelector('.ctx-menu')?.remove();
    // A fire-and-forget mossAlert() (tree.must_inside) is never clicked away
    // by these tests — tear its DOM out directly. BaseModal's open() already
    // treats a disconnected backdrop as a zombie rather than a live slot
    // (see its "does not let a modal whose DOM was wiped block the next
    // open" guard), so this alone is enough to keep the next test's alert
    // from being silently refused.
    document.querySelectorAll('.moss-modal, .moss-modal-backdrop').forEach((el) => el.remove());
    fileTree.unmount();
    container.remove();
  });

  // ── Multi-file context menu (N>1) ─────────────────────────────────────────

  test('N>1 file right-click shows batch menu (Open all N, Delete N items, Move N items to…)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Select alpha + beta via Cmd+click simulation through the selection API
    const alphaId = registry.getByPath('posts/alpha.md');
    const betaId = registry.getByPath('posts/beta.md');
    expect(alphaId).not.toBeNull();
    expect(betaId).not.toBeNull();

    // Build a 2-item selection via the public getSelectionSnapshot API
    // by dispatching the right selection state directly.
    fileTree.getSelectionSnapshot(); // warm up

    // Use internal test hook: select both by dispatching click with modifier
    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    expect(alpha).not.toBeNull();
    expect(beta).not.toBeNull();

    // Plain click selects alpha
    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(1);

    // Cmd+click adds beta
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // Right-click on alpha while 2 selected → batch menu
    rightClick(alpha!);

    expect(findMenuItem('Open all 2')).not.toBeNull();
    expect(findMenuItem('Delete 2 items')).not.toBeNull();
    expect(findMenuItem('Move 2 items to')).not.toBeNull();
    expect(findMenuItem('Copy 2 paths')).not.toBeNull();

    // Single-item labels must NOT appear for N>1
    expect(findMenuItem('Rename')).toBeNull();
    expect(findMenuItem('Duplicate')).toBeNull();
  });

  // ── Finder convention: right-click non-selected collapses to target ───────

  test('right-clicking an unselected file collapses selection to just it', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');

    // Select both
    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // Right-click on alpha (which IS in selection) — stays multi
    rightClick(alpha!);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);
    document.querySelector('.ctx-menu')?.remove();

    // Right-click on index.md (which is NOT in selection) — collapses to 1
    const indexMd = findLeaf(container, 'index.md');
    expect(indexMd).not.toBeNull();
    rightClick(indexMd!);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(1);
    // Single-item menu should appear
    expect(findMenuItem('Rename')).not.toBeNull();
  });

  // ── "Delete N items" confirm flow ─────────────────────────────────────────

  test('"Delete N items" calls deleteEntries once the modal is answered', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: 2 });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');

    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    rightClick(alpha!);
    const deleteItem = findMenuItem('Delete 2 items');
    expect(deleteItem).not.toBeNull();

    deleteItem!.click();
    expect(deleteModal()?.querySelector('.moss-modal-title')?.textContent).toBe('Move 2 items to Trash?');
    answerDelete('trash');

    await vi.waitFor(() => expect(mockDeleteEntries).toHaveBeenCalledTimes(1));
    const [paths] = mockDeleteEntries.mock.calls[0];
    expect(paths).toHaveLength(2);
    expect(paths).toContain(`${PROJECT_ROOT}/posts/alpha.md`);
    expect(paths).toContain(`${PROJECT_ROOT}/posts/beta.md`);

    vi.restoreAllMocks();
  });

  // ── "Move N items to…" uses tauri-plugin-dialog folder picker (item 7) ──────

  test('"Move N items to…" calls openFolderDialog and moveEntries on confirm', async () => {
    // Override the global dialog mock: user picks a folder inside the project
    const targetDir = `${PROJECT_ROOT}/archive`;
    mockPickFolder.mockResolvedValue(targetDir);
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');

    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    rightClick(alpha!);
    const moveItem = findMenuItem('Move 2 items to');
    expect(moveItem).not.toBeNull();

    moveItem!.click();
    // Wait for the async dialog + moveEntries call
    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalledTimes(1));

    // openFolderDialog must have been called (not window.prompt)
    expect(mockPickFolder).toHaveBeenCalled();
    // moveEntries gets the dialog's returned path
    const [paths, dir] = mockMoveEntries.mock.calls[0];
    expect(dir).toBe(targetDir);
    expect(paths).toHaveLength(2);
  });

  test('"Move N items to…" re-roots the breadcrumb when the anchor file is moved', async () => {
    // The anchor file is dragged/moved → the breadcrumb should follow it to the
    // new location. Before the frozen-anchor fix, promptMoveEntries never touched
    // activePath, so the breadcrumb went stale (pointing at the old location).
    const targetDir = `${PROJECT_ROOT}/archive`;
    mockPickFolder.mockResolvedValue(targetDir);
    mockMoveEntries.mockResolvedValue({
      status: 'ok',
      data: {
        moved: [
          { old_path: 'posts/alpha.md', new_path: 'archive/alpha.md', is_dir: false },
          { old_path: 'posts/beta.md', new_path: 'archive/beta.md', is_dir: false },
        ],
        edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1',
      },
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`); // anchor = posts/alpha.md
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    rightClick(alpha!);
    findMenuItem('Move 2 items to')!.click();
    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalledTimes(1));

    // The anchor (alpha) was among the moved set → breadcrumb re-roots to its
    // new home; the non-anchor sibling (beta) does not disturb it.
    await vi.waitFor(() => {
      const ap = priv(fileTree).view.anchor;
      expect(ap[ap.length - 1]).toBe('archive/alpha.md');
    });
  });

  test('"Move to…" of the ACTIVE file fires the rename lifecycle drag-drop fires', async () => {
    // Move to… and a drag-drop are the same move (tree/ops.ts moveEntriesTo).
    // Until they shared an owner the menu path skipped editor:file-renaming —
    // so editor-main never registered the pending pair, the watcher echo was
    // not silenced, and the editor jumped to home — and never broadcast
    // editor:rename-complete. Same assertions as file-tree.test.ts's
    // drag-drop regression, reached through the menu.
    const targetDir = `${PROJECT_ROOT}/archive`;
    mockPickFolder.mockResolvedValue(targetDir);
    mockMoveEntries.mockResolvedValue({
      status: 'ok',
      data: {
        moved: [{ old_path: 'posts/alpha.md', new_path: 'archive/alpha.md', is_dir: false }],
        edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1',
      },
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`); // alpha is the open file
    enterDirMode(fileTree);

    // editor-main's synchronous editor:file-renamed handler, which remaps the
    // active EntryId before refreshTree so reconcileSnapshot keeps it.
    container.addEventListener('editor:file-renamed', (e) => {
      const { oldPath, newPath } = (e as CustomEvent).detail;
      registry.rename(oldPath, newPath);
    });
    const renaming = vi.fn();
    const renamed = vi.fn();
    const renameComplete = vi.fn();
    container.addEventListener('editor:file-renaming', renaming);
    container.addEventListener('editor:file-renamed', renamed);
    container.addEventListener('editor:rename-complete', renameComplete);

    rightClick(findLeaf(container, 'alpha.md')!);
    findMenuItem('Move to')!.click();
    await vi.waitFor(() => expect(renameComplete).toHaveBeenCalledTimes(1));

    const detail = { oldPath: 'posts/alpha.md', newPath: 'archive/alpha.md' };
    expect((renaming.mock.calls[0][0] as CustomEvent).detail).toEqual(detail);
    expect((renamed.mock.calls[0][0] as CustomEvent).detail).toEqual(detail);
    expect((renameComplete.mock.calls[0][0] as CustomEvent).detail).toEqual(detail);
    // renaming BEFORE the move (so the pending pair is registered first),
    // renamed after it.
    expect(renaming.mock.invocationCallOrder[0]).toBeLessThan(mockMoveEntries.mock.invocationCallOrder[0]);
    expect(mockMoveEntries.mock.invocationCallOrder[0]).toBeLessThan(renamed.mock.invocationCallOrder[0]);
  });

  test('"Move N items to…" is a no-op when user cancels dialog', async () => {
    // Dialog returns null (default mock) = user cancelled
    mockPickFolder.mockResolvedValue(null);

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');

    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    rightClick(alpha!);
    const moveItem = findMenuItem('Move 2 items to');
    expect(moveItem).not.toBeNull();

    moveItem!.click();
    await Promise.resolve();
    await Promise.resolve();

    // No backend call when user cancels
    expect(mockMoveEntries).not.toHaveBeenCalled();
  });

  test('"Move N items to…" shows alert when selected folder is outside project root', async () => {
    const outsideDir = '/Users/someone/Desktop';
    mockPickFolder.mockResolvedValue(outsideDir);

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');

    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    rightClick(alpha!);
    const moveItem = findMenuItem('Move 2 items to');
    expect(moveItem).not.toBeNull();

    moveItem!.click();
    await vi.waitFor(() => expect(mossAlertTitle()).not.toBeNull());

    // Alert should mention the project root
    expect(mossAlertTitle()).toContain(PROJECT_ROOT);
    // moveEntries must NOT be called for out-of-project target
    expect(mockMoveEntries).not.toHaveBeenCalled();

    vi.restoreAllMocks();
  });

  test('"Move N items to…" rejects a SIBLING dir sharing the root as a string prefix', async () => {
    // '/Users/test/blog2' starts with '/Users/test/blog' but is OUTSIDE the
    // project — a bare startsWith accepted it and the backend then failed
    // with a confusing containment error. The guard must be separator-aware.
    const siblingDir = `${PROJECT_ROOT}2`;
    mockPickFolder.mockResolvedValue(siblingDir);

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    rightClick(alpha!);
    findMenuItem('Move 2 items to')!.click();
    await vi.waitFor(() => expect(mossAlertTitle()).not.toBeNull());
    expect(mockMoveEntries).not.toHaveBeenCalled();

    vi.restoreAllMocks();
  });

  test('"Move N items to…" accepts the project root itself as the target', async () => {
    // Equality is containment: moving items to the root must not trip the
    // separator-guarded prefix check.
    mockPickFolder.mockResolvedValue(PROJECT_ROOT);
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    alpha!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    beta!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    rightClick(alpha!);
    findMenuItem('Move 2 items to')!.click();
    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalledTimes(1));
    expect(mossAlertTitle()).toBeNull();

    vi.restoreAllMocks();
  });

  // ── Menu composition: the exact rows and groups of each single menu ───────
  //
  // Groups are joined by the primitive (joinCtxGroups omits empty groups and
  // never doubles a separator — its own tests prove that); what the tree owns
  // is WHICH rows land in which group, pinned here as the full ordered list.

  /** The open .ctx-menu's rows in order: '—' for a separator, else the
   *  `.ctx-label` text (the primitive appends a `.ctx-kbd` chip after it). */
  function ctxMenuRows(): string[] {
    return Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu > div'))
      .map(el => el.classList.contains('ctx-sep') ? '—' : (el.querySelector('.ctx-label') ?? el).textContent ?? '');
  }

  const REVEAL_LABELS = ['Reveal in Finder', 'Show in folder'];
  /** The platform-dependent reveal label, whichever this jsdom reports. */
  const reveal = () => ctxMenuRows().find(r => REVEAL_LABELS.includes(r))!;

  test('root folder menu: Add content, Filesystem, View — no Organize, no Delete', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    // The root breadcrumb seg carries data-path equal to the project root ('').
    const rootSeg = container.querySelector<HTMLElement>('.seg[data-path=""]');
    expect(rootSeg).not.toBeNull();
    rightClick(rootSeg!);

    // The root HAS a home file (index.md carries the backend's is_home flag)
    // → no "Create home file"; it is the OS-owned workspace folder → no
    // Rename/Duplicate/Move to…/Delete. "Versions…" is the View group's
    // first item (the root has no Organize group of its own) — one label
    // for every scope
    const rows = ctxMenuRows();
    expect(rows).toEqual([
      'New Page', 'New Folder', 'Import from URL', '—',
      reveal(), 'Copy path', '—',
      'Versions…', rows[8],
    ]);
    expect(['Show internal files', 'Hide internal files']).toContain(rows[8]);
  });

  test('non-root folder menu: Add content (with Create home file), Organize, Filesystem, Delete', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');
    expect(postsSeg).not.toBeNull();
    rightClick(postsSeg!);

    // posts/ is home-less, so "Create home file" lands INSIDE Add content.
    // "Versions…" is the Organize group's last item — the same label the
    // root's View group and a file's Organize group use
    // Scope is already fully determined by what was clicked,
    // so the label no longer repeats it back ("Folder versions…" here,
    // before).
    expect(ctxMenuRows()).toEqual([
      'New Page', 'New Folder', 'Import from URL', 'Create home file', '—',
      'Rename', 'Duplicate', 'Move to…', 'Versions…', '—',
      reveal(), 'Copy path', '—',
      'Delete',
    ]);
  });

  test('a host with no folder picker offers no Move to…', async () => {
    const { host } = await import("../../../host");
    const pickFolder = host.pickFolder;
    host.pickFolder = undefined;
    try {
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);
      const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
        .find(el => el.dataset.path === 'posts');
      rightClick(postsSeg!);
      expect(ctxMenuRows()).not.toContain('Move to…');
      expect(ctxMenuRows()).toContain('Duplicate');
    } finally {
      host.pickFolder = pickFolder;
    }
  });

  test('file menu: Add content (into the parent), Organize with Save as Template…, Filesystem, Delete', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    rightClick(findLeaf(container, 'beta.md')!);

    // No Import from URL on a file row: it lives on the folder menu only.
    // "Versions…" is the Organize group's last item.
    expect(ctxMenuRows()).toEqual([
      'New Page', 'New Folder', '—',
      'Rename', 'Duplicate', 'Save as Template…', 'Move to…', 'Versions…', '—',
      reveal(), 'Copy path', '—',
      'Delete',
    ]);
  });

  test('a loose document\'s folder is not a site: a page offers no template or publish history', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    rightClick(findLeaf(container, 'beta.md')!);
    expect(ctxMenuRows()).toEqual([
      'New Page', 'New Folder', '—',
      'Rename', 'Duplicate', 'Move to…', '—',
      reveal(), 'Copy path', '—',
      'Delete',
    ]);
  });

  test('a non-markdown asset row omits Save as Template… (nothing to capture)', async () => {
    const tree = defaultTree();
    tree.children![1].children!.push(file(`${PROJECT_ROOT}/posts/photo.jpg`));
    mockListTree.mockResolvedValue({ status: 'ok', data: tree });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    rightClick(findLeaf(container, 'photo.jpg')!);
    expect(ctxMenuRows()).toEqual([
      'New Page', 'New Folder', '—',
      'Rename', 'Duplicate', 'Move to…', 'Versions…', '—',
      reveal(), 'Copy path', '—',
      'Delete',
    ]);
  });

  test('a document menu\'s "Versions…" opens the page scope for that file', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const seen = openVersionsRequests();
    rightClick(findLeaf(container, 'beta.md')!);
    findMenuItem('Versions…')!.click();

    expect(seen).toEqual([{ scope: 'page', path: 'posts/beta.md', isRoot: false }]);
  });

  test('a folder menu\'s "Versions…" opens the folder scope for that folder', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');
    const seen = openVersionsRequests();
    rightClick(postsSeg!);
    findMenuItem('Versions…')!.click();

    expect(seen).toEqual([{ scope: 'folder', path: 'posts', isRoot: false }]);
  });

  test('the root menu\'s "Versions…" opens the whole-site scope for the root', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    const rootSeg = container.querySelector<HTMLElement>('.seg[data-path=""]');
    const seen = openVersionsRequests();
    rightClick(rootSeg!);
    findMenuItem('Versions…')!.click();

    expect(seen).toEqual([{ scope: 'site', path: '', isRoot: true }]);
  });

  test('N>1 folder right-click shows batch folder menu (Move N, Delete N, Copy paths)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Need two separate directories selected — use posts/ folder + index.md
    // (mixed file+folder multi-select is valid)
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const indexMd = findLeaf(container, 'index.md');
    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');

    expect(indexMd).not.toBeNull();
    expect(postsSeg).not.toBeNull();

    // Select index.md
    indexMd!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    // Cmd+click posts/ to add it
    postsSeg!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // Right-click on posts/ → folder ctx
    rightClick(postsSeg!);

    expect(findMenuItem('Move 2 items to')).not.toBeNull();
    expect(findMenuItem('Delete 2 items')).not.toBeNull();
    expect(findMenuItem('Copy 2 paths')).not.toBeNull();

    // Mixed selection (1 file + 1 folder): "Open all" counts only the FILES —
    // single/multi symmetry gives every batch menu the same items.
    expect(findMenuItem('Open all 1')).not.toBeNull();

    // Single-folder items (New Page, New Folder) must be absent for N>1
    expect(findMenuItem('New Page')).toBeNull();
    expect(findMenuItem('New Folder')).toBeNull();
  });

  test('mixed-selection "Open all" opens only the files in the selection', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const indexMd = findLeaf(container, 'index.md');
    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');
    indexMd!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    postsSeg!.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    const opened: string[] = [];
    container.addEventListener('editor:file-selected', (e) =>
      opened.push((e as CustomEvent).detail.filePath));

    rightClick(postsSeg!);
    findMenuItem('Open all 1')!.click();

    expect(opened).toEqual(['index.md']); // the folder never "opens"
  });

  // ── "Import from URL" wires to startNativeImportTask for the right-clicked folder ─

  test('"Import from URL" opens URL overlay and calls startNativeImportTask with correct folder', async () => {
    mockImportSite.mockResolvedValue({
      status: 'ok',
      data: { success: true, total_pages: '3', error: null },
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');
    expect(postsSeg).not.toBeNull();

    // Right-click the posts/ folder
    postsSeg!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 100, clientY: 200 }));

    const importUrlItem = findMenuItem('Import from URL');
    expect(importUrlItem).not.toBeNull();

    // Click "Import from URL" — should close context menu and show the URL overlay
    importUrlItem!.click();

    // The URL input overlay should now be in the DOM
    const overlay = document.querySelector('.ctx-menu') as HTMLElement | null;
    // ctx-menu may have been replaced by the overlay (previous one closed)
    const urlInput = document.querySelector('input[type="url"]') as HTMLInputElement | null;
    expect(urlInput).not.toBeNull();

    // Fill in the URL and submit
    urlInput!.value = 'https://example.com/post';
    urlInput!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // Wait for the async import call
    await vi.waitFor(() => expect(mockImportSite).toHaveBeenCalledTimes(1));
    const [url, dir] = mockImportSite.mock.calls[0];
    expect(url).toBe('https://example.com/post');
    // Must target the ABSOLUTE path of the right-clicked folder
    expect(dir).toBe(`${PROJECT_ROOT}/posts`);

    // Clean up overlay from DOM
    document.querySelector('.ctx-menu')?.remove();
    document.querySelector('input[type="url"]')?.closest('div')?.remove();
  });

  test('unmount() mid-overlay removes the URL overlay AND its document listeners', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'posts');
    postsSeg!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 100, clientY: 200 }));
    findMenuItem('Import from URL')!.click();

    // Let the deferred document-listener attach fire (setTimeout 0).
    await vi.advanceTimersByTimeAsync(0);
    expect(document.querySelector('input[type="url"]')).not.toBeNull();

    const removeSpy = vi.spyOn(document, 'removeEventListener');
    fileTree.unmount();

    // Overlay element gone from document.body (it was untracked —
    // unmount leaked the element and both capture-phase listeners).
    expect(document.querySelector('input[type="url"]')).toBeNull();
    const removed = removeSpy.mock.calls.map(c => [c[0], c[2]]);
    expect(removed).toContainEqual(['mousedown', true]);
    expect(removed.some(([type]) => type === 'keydown')).toBe(true);
    removeSpy.mockRestore();
  });
});

// ---------------------------------------------------------------------------
// The folder context menu creates a home file when none exists.
//
// Shown ONLY for a home-less folder — gated on homeEntry() === null, which consumes
// the backend's per-node election flag (TreeNode.is_home), root included.
// Clicking it dispatches editor:create-home carrying the folder's absolute
// path + its root-aware display name; editor-main routes that to the shared
// create-home action, which creates `<folderName>.md`.
// ---------------------------------------------------------------------------

/** A minimal TreeNode with the fields list_tree returns. `is_home` mirrors the
 *  backend's per-folder election — set it on the elected home child. */
function node(name: string, path: string, is_dir: boolean, children: unknown[] | null = null, is_home = false) {
  return { name, path, is_dir, modified: null, publish_date: null, date_source: 'None', is_home, children };
}

describe('folder context menu — "Create home file" item', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.querySelector('.ctx-menu')?.remove();
    fileTree.unmount();
    container.remove();
  });

  /** Right-click a non-root folder ROW (needs dir-mode so it renders in the body). */
  async function rightClickFolder(root: string, tree: unknown, folderRel: string, highlightRel: string): Promise<void> {
    mockListTree.mockResolvedValue({ status: 'ok', data: tree });
    await fileTree.mount(container, root, false);
    fileTree.highlight(`${root}/${highlightRel}`);
    enterDirMode(fileTree);
    const seg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === folderRel);
    expect(seg, `folder row for '${folderRel}'`).not.toBeUndefined();
    rightClick(seg!);
  }

  /** Right-click the ROOT breadcrumb seg (data-path=''). */
  async function rightClickRoot(root: string, tree: unknown): Promise<void> {
    mockListTree.mockResolvedValue({ status: 'ok', data: tree });
    await fileTree.mount(container, root, false);
    const rootSeg = container.querySelector<HTMLElement>('.seg[data-path=""]');
    expect(rootSeg).not.toBeNull();
    rightClick(rootSeg!);
  }

  /** Capture editor:create-home details fired while clicking `item`. */
  function clickAndCapture(item: HTMLElement): Array<{ folderPath: string; folderName: string }> {
    const events: Array<{ folderPath: string; folderName: string }> = [];
    const handler = (e: Event) => events.push((e as CustomEvent).detail);
    document.addEventListener('editor:create-home', handler);
    item.click();
    document.removeEventListener('editor:create-home', handler);
    return events;
  }

  test('home-less subfolder shows the item and dispatches editor:create-home with its abs path + name', async () => {
    await rightClickFolder(PROJECT_ROOT, defaultTree(), 'posts', 'index.md');

    const item = findMenuItem('Create home file');
    expect(item).not.toBeNull();

    const events = clickAndCapture(item!);
    expect(events).toEqual([{ folderPath: `${PROJECT_ROOT}/posts`, folderName: 'posts' }]);
  });

  test('subfolder that HAS a self-named home file does NOT show the item', async () => {
    // docs/ contains docs.md (self-named folder note — the backend elects it
    // and emits is_home; the frontend consumes the flag).
    const tree = node('my-site', PROJECT_ROOT, true, [
      node('index.md', `${PROJECT_ROOT}/index.md`, false, null, true),
      node('docs', `${PROJECT_ROOT}/docs`, true, [
        node('docs.md', `${PROJECT_ROOT}/docs/docs.md`, false, null, true),
        node('guide.md', `${PROJECT_ROOT}/docs/guide.md`, false),
      ]),
    ]);
    await rightClickFolder(PROJECT_ROOT, tree, 'docs', 'index.md');

    expect(findMenuItem('New Page')).not.toBeNull();      // sanity: folder menu shown
    expect(findMenuItem('Create home file')).toBeNull();  // has a home → no item
  });

  test('root with a SELF-NAMED home note does NOT show the item (root home parity)', async () => {
    // Root basename "William Blake"; its home is the self-named `William Blake.md`
    // (NOT index.md). The backend's election runs with the real root basename
    // and emits is_home on it — the frontend just consumes the flag (the old
    // homeRank mirror once derived folderName='' here and missed it, wrongly
    // offering "Create home file").
    const ROOT = '/sites/William Blake';
    const tree = node('William Blake', ROOT, true, [
      node('William Blake.md', `${ROOT}/William Blake.md`, false, null, true),
      node('poems', `${ROOT}/poems`, true, [node('tyger.md', `${ROOT}/poems/tyger.md`, false, null, true)]),
    ]);
    await rightClickRoot(ROOT, tree);

    expect(findMenuItem('New Page')).not.toBeNull();      // sanity: root folder menu shown
    expect(findMenuItem('Create home file')).toBeNull();  // self-named root home → no item
  });

  test('home-less root shows the item and dispatches with the project-root basename', async () => {
    // Root "empty-site" with NO home file (only a subfolder). The item targets
    // the root abs path + its basename so the shared action creates `empty-site.md`.
    const ROOT = '/sites/empty-site';
    const tree = node('empty-site', ROOT, true, [
      // note.md is drafts/' alpha-fallback home in a real payload — flagged
      // for realism; the ROOT itself has no doc child, so it stays home-less.
      node('drafts', `${ROOT}/drafts`, true, [node('note.md', `${ROOT}/drafts/note.md`, false, null, true)]),
    ]);
    await rightClickRoot(ROOT, tree);

    const item = findMenuItem('Create home file');
    expect(item).not.toBeNull();

    const events = clickAndCapture(item!);
    expect(events).toHaveLength(1);
    expect(events[0].folderName).toBe('empty-site');
    // toAbsolute('', root) resolves to the root itself (a trailing slash is a
    // harmless artifact the Rust path-join normalizes).
    expect(events[0].folderPath.replace(/\/+$/, '')).toBe(ROOT);
  });
});

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

describe('Tree empty area + file-row Add content', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    vi.useRealTimers();
    document.querySelector('.ctx-menu')?.remove();
    fileTree.unmount();
    container.remove();
  });

  // ── Empty-area menu ────────────────────────────────────────────────────────

  test('empty-area right-click suppresses the native menu and shows the Add-content menu', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const navBody = container.querySelector<HTMLElement>('.nav-body')!;
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 300 });
    navBody.dispatchEvent(ev);

    // The surface owns its right-click — the native webview menu never leaks.
    expect(ev.defaultPrevented).toBe(true);
    // New Page, New Folder, then the destination note — templates live on
    // the ＋▾ menu and Import from URL on the folder menu.
    expect(Array.from(document.querySelectorAll('.ctx-menu .ctx-item .ctx-label'), el => el.textContent))
      .toEqual(['New Page', 'New Folder']);
    // No row was clicked, so a trailing note names the create destination
    // (same hint as the ＋▾ menu) — here the browsed folder is the project root.
    const hint = document.querySelector('.ctx-menu .ctx-hint');
    expect(hint?.textContent).toContain('my-site');
  });

  test('empty-area New Page / New Folder dispatch the create-flow request events', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const requested: string[] = [];
    const onPage = () => requested.push('page');
    const onFolder = () => requested.push('folder');
    document.addEventListener('editor:request-new-page', onPage);
    document.addEventListener('editor:request-new-folder', onFolder);

    const navBody = container.querySelector<HTMLElement>('.nav-body')!;
    navBody.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    findMenuItem('New Page')!.click();

    navBody.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    findMenuItem('New Folder')!.click();

    expect(requested).toEqual(['page', 'folder']);
    document.removeEventListener('editor:request-new-page', onPage);
    document.removeEventListener('editor:request-new-folder', onFolder);
  });

  test('right-click on a text input keeps the native editing menu', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    const input = document.createElement('input');
    container.appendChild(input);
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    input.dispatchEvent(ev);

    expect(ev.defaultPrevented).toBe(false);
    expect(document.querySelector('.ctx-menu')).toBeNull();
  });

  // ── File-row Add-content group targets the file's PARENT folder ───────────

  test('file-row New Page / New Folder create into the file\'s parent folder', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);
    const startCreate = vi.spyOn(fileTree, 'startCreate').mockResolvedValue(undefined);

    const alpha = findLeaf(container, 'alpha.md')!;
    rightClick(alpha);
    findMenuItem('New Page')!.click();
    expect(startCreate).toHaveBeenCalledWith('posts', 'file');

    rightClick(alpha);
    findMenuItem('New Folder')!.click();
    expect(startCreate).toHaveBeenCalledWith('posts', 'folder');
    startCreate.mockRestore();
  });

  // ── Single-file Move to… (WCAG 2.5.7 parity with folder rows) ─────────────

  test('single-file "Move to…" opens the folder picker and moves the file', async () => {
    const targetDir = `${PROJECT_ROOT}/archive`;
    mockPickFolder.mockResolvedValue(targetDir);
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    rightClick(findLeaf(container, 'alpha.md')!);
    findMenuItem('Move to…')!.click();

    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalledTimes(1));
    const [paths, dir] = mockMoveEntries.mock.calls[0];
    expect(paths).toEqual([`${PROJECT_ROOT}/posts/alpha.md`]);
    expect(dir).toBe(targetDir);
  });
});


// ---------------------------------------------------------------------------
// The menu items that reach tree/ops.ts.
// delete (empty folder, non-empty folder,
// file, and the confirm that must not wait for the reference scan), reveal,
// duplicate, copy path, the optimistic local apply, and the surface the
// delete fallback navigates to. Same subject as
// above — the menus driven through the controller — with its own fixture,
// because these cases need an empty folder and a home-less parent.
// ---------------------------------------------------------------------------

describe('context menu actions on the vault operations', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  function blogTree() {
    return dir(PROJECT_ROOT, [
        dir(`${PROJECT_ROOT}/assets`, []),
        dir(`${PROJECT_ROOT}/posts`, [
            file(`${PROJECT_ROOT}/posts/article-one.md`),
            file(`${PROJECT_ROOT}/posts/article-two.md`),
          ]),
        file(`${PROJECT_ROOT}/index.md`),
        file(`${PROJECT_ROOT}/readme.txt`),
      ], { name: 'my-site' });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    mockScanReferencesForDelete.mockResolvedValue({ status: 'ok', data: [] });
    mockCleanReferencesAndDelete.mockResolvedValue({ status: 'ok', data: null });
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    // Any ctx menu the test opened and left open, and the clipboard mock,
    // which otherwise leaks across tests.
    document.body.querySelectorAll('.ctx-menu').forEach(n => n.remove());
    delete (navigator as { clipboard?: unknown }).clipboard;
  });

  // ── Context menus: Delete / Reveal ─────────────────────────────────

  /** Fire a contextmenu event that the handlers treat as a right-click. */
  function rightClick(el: HTMLElement): void {
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, button: 2 }));
  }

  /** Item label = the `.ctx-label` span (the primitive appends a `.ctx-kbd`
   *  shortcut chip after it, which is not part of the label). */
  function ctxMenuItems(): string[] {
    const items = document.body.querySelectorAll('.ctx-menu .ctx-item');
    return Array.from(items).map(n => (n.querySelector('.ctx-label') ?? n).textContent ?? '');
  }

  function clickMenuItem(label: string): void {
    const items = document.body.querySelectorAll('.ctx-menu .ctx-item');
    const target = Array.from(items)
      .find(n => (n.querySelector('.ctx-label') ?? n).textContent === label) as HTMLElement | undefined;
    if (!target) throw new Error(`Menu item "${label}" not found; have: ${ctxMenuItems().join(', ')}`);
    target.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  }

  test('deleting an empty folder opens no modal and trashes on a clean scan', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // `assets` folder has no children in the default tree.
    const folderSegs = container.querySelectorAll('.seg:not(.leaf):not(.bc)');
    const assetsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('assets'),
    ) as HTMLElement;
    expect(assetsSeg).toBeDefined();
    rightClick(assetsSeg);
    clickMenuItem('Delete');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/assets`]);
    });
    expect(deleteModal()).toBeNull();
  });

  test('deleting an empty folder that other pages link to opens the disclosure directly', async () => {
    mockScanReferencesForDelete.mockResolvedValue({
      status: 'ok',
      data: [{ referencing_file: `${PROJECT_ROOT}/index.md`, ref_text: '[[assets]]', line: 3 }],
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const assetsSeg = Array.from(container.querySelectorAll('.seg:not(.leaf):not(.bc)')).find(
      el => el.textContent?.includes('assets'),
    ) as HTMLElement;
    rightClick(assetsSeg);
    clickMenuItem('Delete');

    await vi.waitFor(() => expect(deleteModal()).not.toBeNull());
    expect(deleteModal()!.querySelector('.moss-modal-title')?.textContent).toBe('Deleting this breaks 1 link');
    expect(mockDeleteEntries).not.toHaveBeenCalled();
    answerDelete('cancel');
  });

  test('deleting a non-empty folder asks, with the child count, and Cancel trashes nothing', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // `posts` has 2 children in the default tree.
    const folderSegs = container.querySelectorAll('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).toBeDefined();
    rightClick(postsSeg);
    clickMenuItem('Delete');

    const modal = deleteModal()!;
    expect(modal.querySelector('.moss-modal-title')?.textContent).toContain('posts');
    expect(modal.querySelector('.moss-modal-body')?.textContent).toContain('2 item');
    answerDelete('cancel');
    await Promise.resolve();
    expect(mockDeleteEntries).not.toHaveBeenCalled();
  });

  test('deleting a file asks once and trashes on Move to Trash', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    expect(deleteModal()!.querySelector('.moss-modal-title')?.textContent).toBe('Move "article-two.md" to Trash?');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });
    expect(mockDeleteEntries).toHaveBeenCalledTimes(1);
    expect(deleteModal()).toBeNull();
  });

  test('an error-shaped trash result leaves the row on screen and in the registry', async () => {
    // `trash` throws the error result so it reaches deleteEntries' runAction
    // (one failure surface); the mutation that swallows it would purge a row
    // for a file still on disk.
    mockDeleteEntries.mockResolvedValue({ status: 'error', error: 'EACCES' });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const fileSeg = Array.from(container.querySelectorAll('.seg.leaf')).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => expect(mockDeleteEntries).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    await Promise.resolve();
    expect(Array.from(container.querySelectorAll('.seg.leaf')).some(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    )).toBe(true);
    expect(registry.getByPath('posts/article-two.md')).not.toBeNull();
  });

  test('the modal is on screen, asking, before the project-wide reference scan resolves', async () => {
    // The scan reads every .md in the project — seconds on a real vault. Hold
    // it open forever: the modal must already be up, in the click's own
    // frame, with the scan reported as running.
    // must not sit in front of a native prompt or behind the
    // answer, so a quick "yes" must not be followed by seconds of nothing on screen.
    mockScanReferencesForDelete.mockReturnValue(new Promise(() => { /* never settles */ }));

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const fileSeg = Array.from(container.querySelectorAll('.seg.leaf')).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');

    const modal = deleteModal()!;
    expect(modal.querySelector('.moss-modal-title')?.textContent).toBe('Move "article-two.md" to Trash?');
    expect(modal.querySelector('.moss-inline-status')?.classList).toContain('is-running');
    // The scan was started — it just is not what the modal waits on.
    expect(mockScanReferencesForDelete).toHaveBeenCalledTimes(1);
    expect(mockDeleteEntries).not.toHaveBeenCalled();

    // A repeat Delete on the same path while the decision is in flight is
    // refused: no second scan, no second modal.
    rightClick(fileSeg);
    clickMenuItem('Delete');
    expect(mockScanReferencesForDelete).toHaveBeenCalledTimes(1);
    expect(document.querySelectorAll('.moss-modal--delete-confirm').length).toBe(1);

    answerDelete('cancel');
    mockScanReferencesForDelete.mockResolvedValue({ status: 'ok', data: [] });
  });

  test('reveal menu item calls revealEntry with the path', async () => {
    mockRevealEntry.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const folderSegs = container.querySelectorAll('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).toBeDefined();
    rightClick(postsSeg);

    // Click whichever reveal label exists on this platform.
    const labels = ctxMenuItems();
    const revealLabel = labels.find(l =>
      l === 'Reveal in Finder' || l === 'Show in folder',
    );
    expect(revealLabel).toBeDefined();
    clickMenuItem(revealLabel!);

    await vi.waitFor(() => {
      expect(mockRevealEntry).toHaveBeenCalledWith(`${PROJECT_ROOT}/posts`);
    });
  });

  // ── Context menus: actions reach tree/ops.ts ──

  test('Duplicate menu item calls duplicateEntry with the path', async () => {
    mockDuplicateEntry.mockResolvedValue({ status: 'ok', data: `${PROJECT_ROOT}/posts/article-two_1.md` });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Duplicate');

    await vi.waitFor(() => {
      expect(mockDuplicateEntry).toHaveBeenCalledWith(`${PROJECT_ROOT}/posts/article-two.md`);
    });
  });

  test('Copy path writes absolute path to clipboard', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Copy path');

    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(`${PROJECT_ROOT}/posts/article-two.md`);
    });
  });

  test('right-click on current-file breadcrumb segment opens file context menu (regression)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // The current-file breadcrumb seg carries data-path equal to the file path.
    const fileBcSeg = container.querySelector(
      `.seg.bc[data-path="posts/article-one.md"]`,
    ) as HTMLElement | null;
    expect(fileBcSeg).not.toBeNull();
    rightClick(fileBcSeg!);

    const labels = ctxMenuItems();
    // File menu hallmarks — both menus share the Add-content group, so the
    // discriminators are the file-only Save as Template… vs the folder-only
    // Import from URL.
    expect(labels).toContain('Rename');
    expect(labels).toContain('Delete');
    expect(labels).toContain('Save as Template…');
    expect(labels).not.toContain('Import from URL');
  });

  test('Rename on current-file breadcrumb actually starts inline rename (regression)', async () => {
    // Pre-fix bug: Rename was wired but startRename's seg lookup via
    // `.seg[data-path=...]` returned null for the breadcrumb file segment
    // because dataset.path was only set on folder segments. Moving the
    // assignment outside the isFolder branch fixed it. This test guards
    // against re-introducing the silent no-op.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const fileBcSeg = container.querySelector(
      `.seg.bc[data-path="posts/article-one.md"]`,
    ) as HTMLElement | null;
    expect(fileBcSeg).not.toBeNull();
    rightClick(fileBcSeg!);
    clickMenuItem('Rename');

    // startRename swaps in an editable input inside the seg.
    const input = fileBcSeg!.querySelector('input') as HTMLInputElement | null;
    expect(input).not.toBeNull();
    expect(fileTree.isEditing()).toBe(true);
  });

  test('Duplicate on a folder calls duplicateEntry with the folder path', async () => {
    mockDuplicateEntry.mockResolvedValue({ status: 'ok', data: `${PROJECT_ROOT}/posts_1` });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const folderSegs = container.querySelectorAll('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    rightClick(postsSeg);
    clickMenuItem('Duplicate');

    await vi.waitFor(() => {
      expect(mockDuplicateEntry).toHaveBeenCalledWith(`${PROJECT_ROOT}/posts`);
    });
  });

  // ── Optimistic delete: tree updates immediately on Ok ─────────────────────

  test('deleting a file removes it from the DOM and registry immediately (no watcher needed)', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });

    // Row must be gone from the DOM immediately — no watcher event required.
    const stillThere = Array.from(container.querySelectorAll('.seg.leaf')).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    );
    expect(stillThere).toBeUndefined();

    // Registry entry must be retired.
    expect(registry.getByPath('posts/article-two.md')).toBeNull();
  });

  test('deleting the active file clears activeEntryId immediately', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);
    enterDirMode(fileTree);

    const activeIdBefore = fileTree.getActiveEntryId();
    expect(activeIdBefore).not.toBeNull();

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });

    // Active entry must be cleared — no dangling id pointing at a gone path.
    expect(fileTree.getActiveEntryId()).toBeNull();
  });

  test('deleting a folder removes the folder row and retires all descendant registry entries', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    // Highlight a root-level file so the tree body shows `posts` as a sibling
    // folder row (same pattern as "deleting a non-empty folder prompts for confirmation").
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const folderSegs = container.querySelectorAll('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).toBeDefined();
    rightClick(postsSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts`]);
    });

    // The folder row itself must be gone from the DOM.
    const folderStillThere = Array.from(
      container.querySelectorAll('.seg:not(.leaf):not(.bc)'),
    ).find(el => el.textContent?.includes('posts'));
    expect(folderStillThere).toBeUndefined();

    // The folder and both child files must be gone from the registry.
    expect(registry.getByPath('posts')).toBeNull();
    expect(registry.getByPath('posts/article-one.md')).toBeNull();
    expect(registry.getByPath('posts/article-two.md')).toBeNull();
  });

  test('batch deleteEntries removes all paths from DOM and registry immediately', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const getLeafs = () => Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf'));
    // metaKey+click toggles selection without navigating (handleFileClick:2460).
    getLeafs().find(el => el.dataset.fullName === 'article-one.md')!
      .dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    getLeafs().find(el => el.dataset.fullName === 'article-two.md')!
      .dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));

    // Right-click the second selected row and pick the "Delete N items" item.
    const twoSeg = getLeafs().find(el => el.dataset.fullName === 'article-two.md')!;
    rightClick(twoSeg);
    const deleteItem = Array.from(document.body.querySelectorAll('.ctx-menu .ctx-item')).find(
      el => el.textContent?.startsWith('Delete'),
    ) as HTMLElement | undefined;
    expect(deleteItem).toBeDefined();
    deleteItem!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalled();
    });

    // Both tree-body rows must be gone (excludes .bc breadcrumb segments which
    // reflect activePath — a separate field cleared on next navigation).
    const treeBodyLeafs = () => Array.from(
      container.querySelectorAll<HTMLElement>('.seg.leaf:not(.bc)'),
    );
    expect(treeBodyLeafs().find(el => el.dataset.fullName === 'article-one.md')).toBeUndefined();
    expect(treeBodyLeafs().find(el => el.dataset.fullName === 'article-two.md')).toBeUndefined();

    // Both gone from registry.
    expect(registry.getByPath('posts/article-one.md')).toBeNull();
    expect(registry.getByPath('posts/article-two.md')).toBeNull();
  });

  // ── Delete-fallback forced navigation: editor surface resolution ──────────
  //
  // When the currently-open file is deleted, applyDeleteLocally falls back to
  // the parent folder. Post-D1, selectFolder no longer changes the editor pane,
  // so the fallback path must EXPLICITLY resolve the editor surface:
  //   • Parent has a home file → editor:file-selected (opens the home file)
  //   • Parent has no home file → editor:folder-navigate (shows the CTA,
  //     clears the stale deleted-file content)
  // Plain tree folder clicks (selectFolder) must NOT dispatch editor:folder-navigate.

  test('delete-fallback: parent folder has NO home file → editor:folder-navigate fires (CTA, no stale content)', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    // defaultTree: posts/ has article-one.md + article-two.md — no home file.
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);
    enterDirMode(fileTree);

    const folderNavigate = vi.fn();
    const fileSelected = vi.fn();
    container.addEventListener('editor:folder-navigate', (e) =>
      folderNavigate((e as CustomEvent).detail));
    container.addEventListener('editor:file-selected', (e) =>
      fileSelected((e as CustomEvent).detail));

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });

    // No home file in posts/ → must dispatch editor:folder-navigate to clear
    // the stale surface and show the CTA.
    expect(folderNavigate).toHaveBeenCalledWith(
      expect.objectContaining({ folderName: 'posts' }),
    );
    // Must NOT open a file (there is no home file to open).
    expect(fileSelected).not.toHaveBeenCalled();
    // The breadcrumb follows: the folder itself is the leaf.
    expect(container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc[aria-current="page"]')?.dataset.path)
      .toBe('posts');
  });

  test('delete-fallback: parent folder HAS a home file → editor:file-selected fires with home path', async () => {
    mockDeleteEntries.mockResolvedValue({ status: 'ok', data: null });

    // Build a tree where posts/ has index.md (home file) + article-two.md.
    const treeWithPostsHome = dir(PROJECT_ROOT, [
        dir(`${PROJECT_ROOT}/posts`, [
            file(`${PROJECT_ROOT}/posts/index.md`, { is_home: true }),
            file(`${PROJECT_ROOT}/posts/article-two.md`),
          ]),
        file(`${PROJECT_ROOT}/index.md`),
      ], { name: 'my-site' });
    mockListTree.mockResolvedValue({ status: 'ok', data: treeWithPostsHome });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);
    enterDirMode(fileTree);

    const folderNavigate = vi.fn();
    const fileSelected = vi.fn();
    container.addEventListener('editor:folder-navigate', (e) =>
      folderNavigate((e as CustomEvent).detail));
    container.addEventListener('editor:file-selected', (e) =>
      fileSelected((e as CustomEvent).detail));

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Delete');
    answerDelete('trash');

    await vi.waitFor(() => {
      expect(mockDeleteEntries).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });

    // Home file exists (posts/index.md) → must open it, not show the CTA.
    expect(fileSelected).toHaveBeenCalledWith(
      expect.objectContaining({ filePath: 'posts/index.md' }),
    );
    expect(folderNavigate).not.toHaveBeenCalled();
    // The breadcrumb follows, rendered before the dispatch: the home is the
    // leaf, as if it had been clicked.
    expect(container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc[aria-current="page"]')?.dataset.path)
      .toBe('posts/index.md');
  });

  test('plain folder click (selectFolder) does NOT dispatch editor:folder-navigate', async () => {
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });
    await fileTree.mount(container, PROJECT_ROOT, false);

    const folderNavigate = vi.fn();
    container.addEventListener('editor:folder-navigate', (e) =>
      folderNavigate((e as CustomEvent).detail));

    // @ts-expect-error — private
    fileTree.selectFolder('posts');
    // selectFolder must never dispatch editor:folder-navigate (that is reserved
    // for the forced delete-fallback path in applyDeleteLocally).
    expect(folderNavigate).not.toHaveBeenCalled();
  });

  test('duplicateEntry refreshes tree immediately after Ok without waiting for watcher', async () => {
    mockDuplicateEntry.mockResolvedValue({ status: 'ok', data: null });

    // First call (mount): original tree. Second call (post-duplicate): includes the copy.
    mockListTree
      .mockResolvedValueOnce({ status: 'ok', data: blogTree() })
      .mockResolvedValueOnce({
        status: 'ok',
        data: dir(PROJECT_ROOT, [
            dir(`${PROJECT_ROOT}/assets`, []),
            dir(`${PROJECT_ROOT}/posts`, [
                file(`${PROJECT_ROOT}/posts/article-one.md`),
                file(`${PROJECT_ROOT}/posts/article-two.md`),
                file(`${PROJECT_ROOT}/posts/article-two 2.md`),
              ]),
            file(`${PROJECT_ROOT}/index.md`),
            file(`${PROJECT_ROOT}/readme.txt`),
          ], { name: 'my-site' }),
      });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    rightClick(fileSeg);
    clickMenuItem('Duplicate');

    await vi.waitFor(() => {
      expect(mockDuplicateEntry).toHaveBeenCalled();
    });

    // listTree called twice: once at mount, once after duplicate.
    expect(mockListTree).toHaveBeenCalledTimes(2);

    // The duplicate row must appear in the tree.
    await vi.waitFor(() => {
      const after = Array.from(container.querySelectorAll('.seg.leaf'));
      expect(after.find(el => (el as HTMLElement).dataset.fullName === 'article-two 2.md')).toBeDefined();
    });
  });
});
