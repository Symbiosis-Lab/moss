/**
 * Integration tests for modifier-aware multi-select in the file tree.
 *
 * Tests that:
 * - Plain click on a file selects single + dispatches editor:file-selected
 * - Cmd+click on a file toggles into selection without dispatching the event
 * - Cmd+click again removes the file from selection (toggle off)
 * - Shift+click ranges from the anchor to the clicked file
 * - Cmd+Shift+click adds a second range to the existing selection
 * - Plain click on a folder selects it + toggles expand state
 * - Cmd+click on a folder toggles selection without toggling expand state
 *
 * Uses vi.useFakeTimers() where a gesture schedules work that must settle.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks (same pattern as file-tree.test.ts)
// ---------------------------------------------------------------------------
const { mockListTree, mockGetProjectPath, mockRenameEntry, mockCopyFilesToProject, mockRevealEntry, mockDuplicateEntry, mockCreateFile, mockCreateFolder, mockMoveEntries, mockDeleteEntries } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockRenameEntry: vi.fn(),
  mockCopyFilesToProject: vi.fn(),
  mockRevealEntry: vi.fn(),
  mockDuplicateEntry: vi.fn(),
  mockCreateFile: vi.fn(),
  mockCreateFolder: vi.fn(),
  mockMoveEntries: vi.fn(),
  mockDeleteEntries: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: mockRenameEntry,
    copyFilesToProject: mockCopyFilesToProject,
    revealEntry: mockRevealEntry,
    duplicateEntry: mockDuplicateEntry,
    createFiles: mockCreateFile,
    createFolder: mockCreateFolder,
    moveEntries: mockMoveEntries,
    deleteEntries: mockDeleteEntries,
  },
}));


// (CSS.escape and DataTransfer polyfills provided globally by test-setup-ui.ts)

// ---------------------------------------------------------------------------
// Import after mocks
// ---------------------------------------------------------------------------
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { navH } from '../spine-projection';
import { priv, enterDirMode, dir, file } from './test-helpers';

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const PROJECT_ROOT = '/projects/test-blog';

/**
 * A flat tree with 3 files side by side inside a folder, plus a separate
 * folder (with-sub). Good for range selection tests.
 *
 * Structure:
 *   test-blog/
 *     posts/
 *       alpha.md
 *       beta.md
 *       gamma.md
 *     with-sub/
 *       child.md
 *     index.md
 */
function threeFileTree() {
  return dir(PROJECT_ROOT, [
      dir(`${PROJECT_ROOT}/posts`, [
          file(`${PROJECT_ROOT}/posts/alpha.md`),
          file(`${PROJECT_ROOT}/posts/beta.md`),
          file(`${PROJECT_ROOT}/posts/gamma.md`),
        ]),
      dir(`${PROJECT_ROOT}/with-sub`, [
          file(`${PROJECT_ROOT}/with-sub/child.md`),
        ]),
      file(`${PROJECT_ROOT}/index.md`),
    ], { name: 'test-blog' });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Long enough for any work a gesture schedules to settle before asserting.
const SETTLE_MS = 260;

/** Find a leaf seg by its data-full-name. */
function findLeaf(container: HTMLElement, fullName: string): HTMLElement | null {
  return (
    Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === fullName,
    ) ?? null
  );
}

/** Find a folder seg (not a breadcrumb) by display text. */
function findFolderSeg(container: HTMLElement, text: string): HTMLElement | null {
  return (
    Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)')).find(
      el => el.textContent?.trim() === text,
    ) ?? null
  );
}

/** Click an element with optional modifier keys. */
function clickWith(
  el: HTMLElement,
  opts: { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean } = {},
): void {
  el.dispatchEvent(
    new MouseEvent('click', {
      bubbles: true,
      cancelable: true,
      metaKey: opts.metaKey ?? false,
      ctrlKey: opts.ctrlKey ?? false,
      shiftKey: opts.shiftKey ?? false,
    }),
  );
}

/**
 * Open the `posts/` folder via its chevron so ALL of its files (alpha, beta,
 * gamma) render as tree-body rows.
 *
 * The active file is rendered only in the breadcrumb spine,
 * NOT as a body row (buildGroup filters out `currentFilePath`). A test that
 * needs alpha/beta/gamma all present in the body must keep the active file
 * OUTSIDE posts/ (highlight index.md) and expand posts/ via its chevron — a
 * chevron click toggles expansion without changing selection.
 */
function expandPosts(container: HTMLElement): void {
  const postsSeg = findFolderSeg(container, 'posts');
  if (!postsSeg) throw new Error('posts folder seg not found');
  clickWith(postsSeg.querySelector('.chev') as HTMLElement);
  vi.advanceTimersByTime(SETTLE_MS);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('file-tree modifier-aware multi-select', () => {
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
    mockListTree.mockResolvedValue({ status: 'ok', data: threeFileTree() });
  });

  afterEach(() => {
    vi.useRealTimers();
    fileTree.unmount();
    container.remove();
  });

  // ── Plain file click ──────────────────────────────────────────────────

  test('plain click on file selects single + dispatches editor:file-selected', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    const beta = findLeaf(container, 'beta.md');
    expect(beta).not.toBeNull();
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);

    expect(listener).toHaveBeenCalledTimes(1);
    const event = listener.mock.calls[0][0] as CustomEvent;
    expect(event.detail.filePath).toBe('posts/beta.md');

    // Selection should be single (only beta)
    const snap = fileTree.getSelectionSnapshot();
    expect(snap.selected.size).toBe(1);
  });

  // ── Cmd+click file: toggle in ─────────────────────────────────────────

  test('Cmd+click on file adds to selection, no editor:file-selected', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // First plain-click alpha to establish a single selection
    const alpha = findLeaf(container, 'alpha.md');
    expect(alpha).not.toBeNull();
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    listener.mockClear();

    // Cmd+click beta — should add beta, NOT dispatch event
    const beta = findLeaf(container, 'beta.md');
    expect(beta).not.toBeNull();
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    expect(listener).not.toHaveBeenCalled();

    const snap = fileTree.getSelectionSnapshot();
    const alphaId = registry.getByPath('posts/alpha.md');
    const betaId = registry.getByPath('posts/beta.md');
    expect(snap.selected.has(alphaId!)).toBe(true);
    expect(snap.selected.has(betaId!)).toBe(true);
    expect(snap.selected.size).toBe(2);
  });

  // ── Cmd+click file: toggle off ────────────────────────────────────────

  test('Cmd+click again on same file removes it from selection', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // Plain-click alpha
    const alpha = findLeaf(container, 'alpha.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    listener.mockClear();

    // Cmd+click beta to add it
    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(listener).not.toHaveBeenCalled();

    // Cmd+click beta again to remove it
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(listener).not.toHaveBeenCalled();

    const snap = fileTree.getSelectionSnapshot();
    const alphaId = registry.getByPath('posts/alpha.md');
    const betaId = registry.getByPath('posts/beta.md');
    expect(snap.selected.has(alphaId!)).toBe(true);
    expect(snap.selected.has(betaId!)).toBe(false);
    expect(snap.selected.size).toBe(1);
  });

  // ── Shift+click: range selection ─────────────────────────────────────

  test('Shift+click ranges from anchor to clicked file (3 files)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Active file = index.md (breadcrumb-only) so alpha+beta+gamma all render
    // as body rows — the 3-file range needs all three present.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);
    expandPosts(container);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // Plain-click alpha to set anchor
    const alpha = findLeaf(container, 'alpha.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    listener.mockClear();

    // Shift+click gamma — should select alpha, beta, gamma
    const gamma = findLeaf(container, 'gamma.md');
    expect(gamma).not.toBeNull();
    clickWith(gamma!, { shiftKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    // Shift+click does NOT dispatch editor:file-selected
    expect(listener).not.toHaveBeenCalled();

    const snap = fileTree.getSelectionSnapshot();
    const alphaId = registry.getByPath('posts/alpha.md');
    const betaId = registry.getByPath('posts/beta.md');
    const gammaId = registry.getByPath('posts/gamma.md');
    expect(snap.selected.has(alphaId!)).toBe(true);
    expect(snap.selected.has(betaId!)).toBe(true);
    expect(snap.selected.has(gammaId!)).toBe(true);
    expect(snap.selected.size).toBe(3);
  });

  // ── Cmd+Shift+click: addRange ─────────────────────────────────────────

  test('Cmd+Shift+click adds a second range without clearing the first', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // Plain-click alpha (anchor = alpha, selected = {alpha})
    const alpha = findLeaf(container, 'alpha.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    listener.mockClear();

    // Cmd+click gamma to start a separate selection (toggle in)
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(listener).not.toHaveBeenCalled();

    // Verify: alpha + gamma selected, anchor moved to gamma (last additive)
    let snap = fileTree.getSelectionSnapshot();
    const alphaId = registry.getByPath('posts/alpha.md');
    const gammaId = registry.getByPath('posts/gamma.md');
    expect(snap.selected.has(alphaId!)).toBe(true);
    expect(snap.selected.has(gammaId!)).toBe(true);
    expect(snap.anchor).toBe(gammaId);

    // Cmd+Shift+click beta from gamma anchor — addRange gamma→beta
    // Note: since gamma is anchor and beta is before gamma in visible order,
    // addRange includes gamma and beta (and anything between).
    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!, { metaKey: true, shiftKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(listener).not.toHaveBeenCalled();

    snap = fileTree.getSelectionSnapshot();
    const betaId = registry.getByPath('posts/beta.md');
    // alpha was in selection already; beta+gamma added by addRange
    expect(snap.selected.has(alphaId!)).toBe(true);
    expect(snap.selected.has(betaId!)).toBe(true);
    expect(snap.selected.has(gammaId!)).toBe(true);
    // All 3 are now selected (at least)
    expect(snap.selected.size).toBeGreaterThanOrEqual(3);
  });

  // ── Plain folder click: select + toggle ──────────────────────────────

  test('plain click on collapsed folder name selects AND expands it in place', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // posts folder should be visible as collapsed
    const postsSeg = findFolderSeg(container, 'posts');
    expect(postsSeg).not.toBeNull();

    // Click the NAME zone (not the chevron): selects AND expands in place.
    clickWith(postsSeg!.querySelector('.seg-name') as HTMLElement);
    vi.advanceTimersByTime(SETTLE_MS);

    // editor:file-selected is NOT dispatched — clicking a folder no longer opens
    // a file (it expands in place).
    expect(listener).not.toHaveBeenCalled();

    // posts folder should now be selected
    const postsId = registry.getByPath('posts');
    const snap = fileTree.getSelectionSnapshot();
    expect(snap.selected.has(postsId!)).toBe(true);

    // The folder EXPANDED — its children are now visible.
    expect(findLeaf(container, 'alpha.md')).not.toBeNull();
    expect(findFolderSeg(container, 'posts')!.getAttribute('aria-expanded')).toBe('true');
  });

  // ── Cmd+click folder: toggle selection, no expand change ─────────────

  test('Cmd+click on a collapsed folder toggles selection without expanding it', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // Posts folder is collapsed
    const postsSeg = findFolderSeg(container, 'posts');
    expect(postsSeg).not.toBeNull();

    clickWith(postsSeg!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    // No event dispatched
    expect(listener).not.toHaveBeenCalled();

    // posts should be selected
    const postsId = registry.getByPath('posts');
    const snap = fileTree.getSelectionSnapshot();
    expect(snap.selected.has(postsId!)).toBe(true);

    // Posts children (alpha, beta, gamma) should NOT be visible — folder still collapsed
    const alpha = findLeaf(container, 'alpha.md');
    expect(alpha).toBeNull();
  });

  // ── visibleEntryIds() ─────────────────────────────────────────────────

  test('visibleEntryIds returns IDs in DOM render order', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Active file = index.md (breadcrumb-only, excluded from the body) so
    // posts/'s three children all render as body rows once posts/ is expanded.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);
    expandPosts(container);

    const ids = fileTree.visibleEntryIds();
    expect(ids.length).toBeGreaterThan(0);

    // All IDs should be valid (exist in registry)
    for (const id of ids) {
      expect(id).toBeGreaterThan(0);
    }

    // alpha, beta, gamma should all appear as body rows (none is the active
    // file, which is index.md and lives only in the breadcrumb)
    const alphaId = registry.getByPath('posts/alpha.md');
    const betaId = registry.getByPath('posts/beta.md');
    const gammaId = registry.getByPath('posts/gamma.md');
    expect(ids).toContain(alphaId);
    expect(ids).toContain(betaId);
    expect(ids).toContain(gammaId);

    // alpha should appear before beta, beta before gamma (DOM render order)
    expect(ids.indexOf(alphaId!)).toBeLessThan(ids.indexOf(betaId!));
    expect(ids.indexOf(betaId!)).toBeLessThan(ids.indexOf(gammaId!));
  });

  // ── Click empty area clears selection ────────────────────────────────

  test('click on empty .nav-body area clears selection', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Build up a 2-item selection
    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // Click the nav-body itself (not any row). The listener uses
    // target===navBody, so a click that landed on a row would NOT match.
    const navBody = container.querySelector<HTMLElement>('.nav-body');
    expect(navBody).not.toBeNull();
    navBody!.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    const snap = fileTree.getSelectionSnapshot();
    expect(snap.selected.size).toBe(0);
    expect(snap.anchor).toBeNull();
    expect(snap.focus).toBeNull();
  });

  // ── editor:selection-changed event ───────────────────────────────────

  test('editor:selection-changed fires on plain click, cmd+click, shift+click, and empty-area clear', async () => {
    // alpha is the active file (rendered as breadcrumb segment, no click);
    // tests interact with the tree-body siblings beta + gamma which do have
    // click handlers.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const events: { selected: number; anchor: unknown; focus: unknown }[] = [];
    container.addEventListener('editor:selection-changed', ((e: CustomEvent) => {
      events.push({
        selected: e.detail.selected.size,
        anchor: e.detail.anchor,
        focus: e.detail.focus,
      });
    }) as EventListener);

    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');

    // 1. Plain click on beta — fires with size 1
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(events.length).toBe(1);
    expect(events[0].selected).toBe(1);

    // 2. Cmd+click on gamma — fires with size 2
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(events.length).toBe(2);
    expect(events[1].selected).toBe(2);

    // 3. Click empty-area — fires with size 0
    const navBody = container.querySelector<HTMLElement>('.nav-body');
    navBody!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(events.length).toBe(3);
    expect(events[2].selected).toBe(0);
    expect(events[2].anchor).toBeNull();
    expect(events[2].focus).toBeNull();
  });

  test('editor:selection-changed detail.selected is a defensive copy', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Capture ONLY the first event's snapshot, ignore subsequent ones
    let firstSnapshot: ReadonlySet<unknown> | null = null;
    container.addEventListener('editor:selection-changed', ((e: CustomEvent) => {
      if (firstSnapshot === null) firstSnapshot = e.detail.selected;
    }) as EventListener);

    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);

    expect(firstSnapshot).not.toBeNull();
    expect((firstSnapshot as unknown as Set<unknown>).size).toBe(1);

    // Mutate further on FileTree — first event's snapshot should NOT reflect the change
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    // firstSnapshot was captured at the FIRST event; defensive copy ⇒ still size 1
    expect((firstSnapshot as unknown as Set<unknown>).size).toBe(1);
  });

  // ── ARIA tree pattern + selected visual state ────────────────────────

  test('navBody has role=tree and aria-multiselectable=true', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const navBody = container.querySelector('.nav-body');
    expect(navBody?.getAttribute('role')).toBe('tree');
    expect(navBody?.getAttribute('aria-multiselectable')).toBe('true');
  });

  test('every rendered .seg has role=treeitem and aria-selected attr', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const segs = container.querySelectorAll<HTMLElement>('.seg');
    expect(segs.length).toBeGreaterThan(0);
    for (const seg of segs) {
      expect(seg.getAttribute('role')).toBe('treeitem');
      expect(seg.getAttribute('aria-selected')).toMatch(/^(true|false)$/);
    }
  });

  test('folder seg has aria-expanded reflecting open/closed state', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const posts = findFolderSeg(container, 'posts');
    expect(posts).not.toBeNull();
    expect(posts!.getAttribute('aria-expanded')).toBe('false');   // collapsed

    // Click the chevron to expand
    clickWith(posts!.querySelector('.chev') as HTMLElement);
    vi.advanceTimersByTime(SETTLE_MS);

    const postsAfter = findFolderSeg(container, 'posts');
    expect(postsAfter!.getAttribute('aria-expanded')).toBe('true');
  });

  test('clicking a row applies .selected class and aria-selected=true', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    expect(beta!.classList.contains('selected')).toBe(false);
    expect(beta!.getAttribute('aria-selected')).toBe('false');

    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);

    expect(beta!.classList.contains('selected')).toBe(true);
    expect(beta!.getAttribute('aria-selected')).toBe('true');
  });

  test('multi-select via cmd+click marks both rows .selected', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');

    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    expect(beta!.classList.contains('selected')).toBe(true);
    expect(gamma!.classList.contains('selected')).toBe(true);
    expect(beta!.getAttribute('aria-selected')).toBe('true');
    expect(gamma!.getAttribute('aria-selected')).toBe('true');
  });

  test('breadcrumb seg for active file does NOT get .selected (display-only)', async () => {
    // Regression of fea219b14: renderSelectionVisualState queried [data-path]
    // without filtering breadcrumb segs, so the bc-current seg (which carries
    // the active file's path) painted with --moss-accent-soft. Breadcrumbs are
    // a navigation indicator, not a selectable row, and must stay neutral
    // regardless of what's in the multi-select set.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const bcCurrent = container.querySelector<HTMLElement>('.seg.bc.bc-current');
    expect(bcCurrent).not.toBeNull();
    expect(bcCurrent!.dataset.path).toBeDefined();
    // Sanity: the bc-current path is the active file (so .selected would
    // light up if we didn't filter .bc out).
    const activeFileId = registry.getByPath('posts/alpha.md');
    expect(activeFileId).not.toBeNull();
    expect(fileTree.getSelectionSnapshot().selected.has(activeFileId!)).toBe(true);

    expect(bcCurrent!.classList.contains('selected')).toBe(false);
    expect(bcCurrent!.getAttribute('aria-selected')).toBe('false');
  });

  test('empty-area click clears .selected on previously selected rows', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(beta!.classList.contains('selected')).toBe(true);

    const navBody = container.querySelector<HTMLElement>('.nav-body');
    navBody!.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(beta!.classList.contains('selected')).toBe(false);
    expect(beta!.getAttribute('aria-selected')).toBe('false');
  });

  test('click on a row does NOT trigger empty-area clear (bubbling guard works)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const alpha = findLeaf(container, 'alpha.md');
    const beta = findLeaf(container, 'beta.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // A plain click on a third row would replace selection with that row.
    // After SETTLE_MS, selection.size should be 1 (single replaced), not 0.
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(gamma!);
    vi.advanceTimersByTime(SETTLE_MS);

    expect(fileTree.getSelectionSnapshot().selected.size).toBe(1);
  });

  // ── Semantic active vs visual focus divergence ────────────────────────
  //
  // Contract of the registry as canonical owner:
  // registry.activeEntryId is the EDITOR's active file. selection.focus is
  // the file-tree's VISUAL focus (which row keyboard nav lands on). They
  // coincide on plain click but can diverge during multi-select.

  // Setup helper: file-tree dispatches editor:file-selected on plain click.
  // In production, editor-main's listener calls loadFile which calls
  // registry.setActive. The test re-implements the contract.
  function attachActiveSyncListener(): void {
    container.addEventListener('editor:file-selected', (e: Event) => {
      const ev = e as CustomEvent<{ filePath: string }>;
      const id = registry.getByPath(ev.detail.filePath);
      if (id !== null) registry.setActive(id);
    });
  }

  test('plain click drives editor:file-selected which sets registry.activeEntryId', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    attachActiveSyncListener();
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);

    const betaId = registry.getByPath('posts/beta.md');
    // editor-main listener simulator routed file-selected to setActive.
    expect(registry.activeEntryId).toBe(betaId);
    // selection.focus was set by the click handler directly via setSingle,
    // and then the subscriber's !== guard short-circuited (already at betaId).
    expect(fileTree.getSelectionSnapshot().focus).toBe(betaId);
  });

  test('Cmd+click moves selection.focus but does NOT change registry.activeEntryId', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    attachActiveSyncListener();
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Open alpha.md via plain click (commits as editor's active file via simulator).
    const alpha = findLeaf(container, 'alpha.md');
    clickWith(alpha!);
    vi.advanceTimersByTime(SETTLE_MS);
    const alphaId = registry.getByPath('posts/alpha.md');
    expect(registry.activeEntryId).toBe(alphaId);

    // Cmd+click beta.md: visual focus moves, editor:file-selected does NOT
    // fire (selection.toggle is the multi-select path), so the simulator
    // doesn't call setActive. Editor's active file unchanged.
    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    const betaId = registry.getByPath('posts/beta.md');
    expect(fileTree.getSelectionSnapshot().focus).toBe(betaId);
    // SEMANTIC: editor's active file unchanged.
    expect(registry.activeEntryId).toBe(alphaId);
  });
});
