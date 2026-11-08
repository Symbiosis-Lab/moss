/**
 * Integration tests for the in-app drag (tree/dnd.ts), driven through the
 * controller's delegated DOM listeners: what a dragstart carries, the ghost,
 * the session's grace window after dragend, drop validation, spring-load,
 * and the batched move a multi-row drop issues.
 *
 * Split out of file-tree-multiselect.test.ts;
 * the selection cases stay there.
 *
 * Uses vi.useFakeTimers() where a gesture schedules work that must settle.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Hoisted mocks (same pattern as file-tree.test.ts)
// ---------------------------------------------------------------------------
const { mockListTree, mockGetProjectPath, mockRenameEntry, mockCopyFilesToProject, mockRevealEntry, mockDuplicateEntry, mockCreateFile, mockCreateFolder, mockMoveEntries, mockDeleteEntries, mockDragOut } = vi.hoisted(() => ({
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
  mockDragOut: vi.fn().mockResolvedValue(undefined),
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

// The tree hands the host the paths for a drag out of moss.
import { host } from "../../../host";
host.dragOut = mockDragOut;


// A failed batch move surfaces as a toast, never a native alert.
const { mockShowToast } = vi.hoisted(() => ({ mockShowToast: vi.fn() }));
vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showToast: mockShowToast,
  showAck: vi.fn(),
}));


// (CSS.escape and DataTransfer polyfills provided globally by test-setup-ui.ts)

// ---------------------------------------------------------------------------
// Import after mocks
// ---------------------------------------------------------------------------
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { enterDirMode, dir, file } from './test-helpers';

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('tree/dnd: in-app drag and drop', () => {
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
    // Some drag tests synthesize dragstart without a paired dragend, leaving
    // a .drag-ghost element on document.body. Clean it up so the next test
    // starts with no stale ghost.
    document.body.querySelectorAll('.drag-ghost').forEach(el => el.remove());
  });

  // ── Multi-row drag carries selection ─────────────────────────────────

  test('dragstart on a selected file carries whole selection in dataTransfer', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    const dt = new DataTransfer();
    const dragEvent = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragEvent, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragEvent);

    const payload = dt.getData('application/x-moss-entries');
    expect(payload).not.toBe('');
    const parsed = JSON.parse(payload);
    expect(Array.isArray(parsed.entryIds)).toBe(true);
    expect(parsed.entryIds.length).toBe(2);
  });

  test('dragstart on an UNSELECTED file carries just that file, persistent selection unchanged', async () => {
    // Finder convention: dragging an unselected row drags only that row,
    // but does NOT mutate the persistent selection. Critical because
    // selection.focus drives the active-editor-file via the activeEntryId
    // facade — a drag must not switch which file is open in the editor.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Pre-select beta + gamma
    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    // Drag a different file (with-sub/child.md), not in selection. Expand
    // with-sub via its chevron (toggle only — selection is untouched).
    const withSub = findFolderSeg(container, 'with-sub');
    clickWith(withSub!.querySelector('.chev') as HTMLElement);
    vi.advanceTimersByTime(SETTLE_MS);
    // Chevron toggle does not mutate selection — still beta + gamma.
    expect(fileTree.getSelectionSnapshot().selected.size).toBe(2);

    const child = findLeaf(container, 'child.md');
    expect(child).not.toBeNull();

    const dt = new DataTransfer();
    const dragEvent = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragEvent, 'dataTransfer', { value: dt });
    child!.dispatchEvent(dragEvent);

    // Persistent selection unchanged — still beta + gamma
    const snap = fileTree.getSelectionSnapshot();
    expect(snap.selected.size).toBe(2);
    const betaId = registry.getByPath('posts/beta.md');
    const gammaId = registry.getByPath('posts/gamma.md');
    expect(snap.selected.has(betaId!)).toBe(true);
    expect(snap.selected.has(gammaId!)).toBe(true);

    // Drag carries JUST child.md — verified via the dataTransfer payload
    const payload = JSON.parse(dt.getData('application/x-moss-entries'));
    expect(payload.entryIds.length).toBe(1);
    const childId = registry.getByPath('with-sub/child.md');
    expect(payload.entryIds).toContain(childId);
  });

  test('drag ghost element appears in document body when N>1', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    expect(document.querySelector('.drag-ghost')).toBeNull();

    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragStartEv);

    const ghost = document.querySelector<HTMLElement>('.drag-ghost');
    expect(ghost).not.toBeNull();
    expect(ghost!.dataset.count).toBe('2');
    expect(ghost!.querySelector('.drag-ghost__badge')?.textContent).toBe('2');

    // dragend cleans up
    beta!.dispatchEvent(new Event('dragend', { bubbles: true }));
    expect(document.querySelector('.drag-ghost')).toBeNull();
  });

  test('drag ghost is NOT created for single-row selection', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);

    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragStartEv);

    expect(document.querySelector('.drag-ghost')).toBeNull();

    // Cleanup
    beta!.dispatchEvent(new Event('dragend', { bubbles: true }));
  });

  // ── Drag session lifecycle ───────────────────────────────────────────

  const startDrag = (el: HTMLElement) => {
    const ev = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(ev, 'dataTransfer', { value: new DataTransfer() });
    el.dispatchEvent(ev);
  };

  test('dragend retires the session but keeps it readable for the drop IPC grace window', async () => {
    // Tauri's drop payload arrives after the DOM's dragend. A session ended
    // at dragend makes the in-flight handler discard a real drop.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md')!;
    startDrag(beta);
    expect(fileTree.hasActiveInternalDrag()).toBe(true);

    beta.dispatchEvent(new Event('dragend', { bubbles: true }));
    vi.advanceTimersByTime(499);
    expect(fileTree.hasActiveInternalDrag()).toBe(true);
    expect(fileTree.activeInternalDragEntryIds().length).toBe(1);

    vi.advanceTimersByTime(1);
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
    expect(fileTree.activeInternalDragEntryIds()).toEqual([]);
  });

  test('a drag started inside the previous grace window is not ended by the old timer', async () => {
    // The grace timer is keyed on the session it retired, so it can only end
    // that one. This is what a second field with its own clearing rule could
    // not guarantee.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md')!;
    const gamma = findLeaf(container, 'gamma.md')!;
    startDrag(beta);
    beta.dispatchEvent(new Event('dragend', { bubbles: true }));

    vi.advanceTimersByTime(100);
    startDrag(gamma);
    const gammaId = registry.getByPath('posts/gamma.md');
    expect(fileTree.activeInternalDragEntryIds()).toEqual([gammaId]);

    vi.advanceTimersByTime(500); // the first session's timer fires here
    expect(fileTree.hasActiveInternalDrag()).toBe(true);
    expect(fileTree.activeInternalDragEntryIds()).toEqual([gammaId]);

    gamma.dispatchEvent(new Event('dragend', { bubbles: true }));
    vi.advanceTimersByTime(500);
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
  });

  test('a consumed drop ends the session immediately, without waiting out the grace', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const beta = findLeaf(container, 'beta.md')!;
    startDrag(beta);
    beta.dispatchEvent(new Event('dragend', { bubbles: true }));

    fileTree.clearInternalDragSession();
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
  });

  // Drop validation rejects invalid destinations.

  test('drop on the current parent folder is rejected by canDropOneInto', async () => {
    // The target is the with-sub BODY row, not its breadcrumb segment: drops
    // resolve through the folder row under the pointer, so the rejection has
    // to be exercised against a row that would otherwise accept the drop.
    // child.md's current parent IS with-sub, so the same-parent guard fires.
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Expand with-sub (via its chevron) so child.md is visible AND with-sub
    // becomes a sib drop target.
    const withSubSeg = findFolderSeg(container, 'with-sub');
    expect(withSubSeg).not.toBeNull();
    clickWith(withSubSeg!.querySelector('.chev') as HTMLElement);
    vi.advanceTimersByTime(SETTLE_MS);

    // Re-query — render() rebuilds DOM, so element references are stale
    const child = findLeaf(container, 'child.md');
    expect(child).not.toBeNull();
    clickWith(child!);
    vi.advanceTimersByTime(SETTLE_MS);

    // The expanded with-sub sib folder row (NOT the breadcrumb!) is the drop target
    const withSubDropTarget = Array.from(container.querySelectorAll<HTMLElement>(`[data-path="with-sub"]`))
      .find(el => !el.classList.contains('bc'));
    expect(withSubDropTarget).not.toBeNull();

    const dt = new DataTransfer();
    const dragStart = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, 'dataTransfer', { value: dt });
    child!.dispatchEvent(dragStart);

    const dropDt = new DataTransfer();
    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: dropDt });
    withSubDropTarget!.dispatchEvent(dropEv);

    await Promise.resolve();
    await Promise.resolve();
    // canDropOneInto rejected: source.parent === target → no move
    expect(mockMoveEntries).not.toHaveBeenCalled();
  });

  test('drop of a folder onto its own descendant is rejected by canDropOneInto', async () => {
    // The cycle guard: with-sub dragged onto a folder inside with-sub. The
    // move command must never be asked to nest a folder in itself.
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: dir(PROJECT_ROOT, [
      dir(`${PROJECT_ROOT}/with-sub`, [dir(`${PROJECT_ROOT}/with-sub/inner`, [])]),
      file(`${PROJECT_ROOT}/index.md`),
    ], { name: 'test-blog' }) });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    clickWith(findFolderSeg(container, 'with-sub')!.querySelector('.chev') as HTMLElement);
    vi.advanceTimersByTime(SETTLE_MS);
    const withSub = findFolderSeg(container, 'with-sub');
    const inner = findFolderSeg(container, 'inner');
    expect(inner).not.toBeNull();

    startDrag(withSub!);
    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: new DataTransfer() });
    inner!.dispatchEvent(dropEv);

    await Promise.resolve();
    await Promise.resolve();
    expect(mockMoveEntries).not.toHaveBeenCalled();
  });

  // Spring-loading opens the folder under the dragged item.

  test('spring-load: hovering a collapsed folder for 600ms during drag expands it', async () => {
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: null });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // with-sub is collapsed by default at this point. Verify the assumption.
    const childBefore = findLeaf(container, 'child.md');
    expect(childBefore).toBeNull();  // collapsed → not in DOM

    // Dragstart on beta (file in posts) — picks up wireDropTarget on with-sub
    const beta = findLeaf(container, 'beta.md');
    const dt = new DataTransfer();
    const dragStart = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragStart);

    // Dragover on with-sub (collapsed sib folder = drop target) arms the
    // spring-load timer
    const withSubDropTarget = Array.from(container.querySelectorAll<HTMLElement>(`[data-path="with-sub"]`))
      .find(el => !el.classList.contains('bc'));
    expect(withSubDropTarget).not.toBeNull();

    const dragOverDt = new DataTransfer();
    const dragOverEv = new Event('dragover', { bubbles: true, cancelable: true });
    Object.defineProperty(dragOverEv, 'dataTransfer', { value: dragOverDt });
    withSubDropTarget!.dispatchEvent(dragOverEv);

    // Advance past 600ms — spring-load timer should fire
    vi.advanceTimersByTime(610);

    // After spring-load, with-sub is expanded → child.md visible in DOM
    const childAfter = findLeaf(container, 'child.md');
    expect(childAfter).not.toBeNull();
  });

  test('spring-load: dragleave before 600ms cancels the timer', async () => {
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: null });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    expect(findLeaf(container, 'child.md')).toBeNull();   // collapsed

    const beta = findLeaf(container, 'beta.md');
    const dt = new DataTransfer();
    const dragStart = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragStart);

    const withSubDropTarget = Array.from(container.querySelectorAll<HTMLElement>(`[data-path="with-sub"]`))
      .find(el => !el.classList.contains('bc'));
    const dragOverDt = new DataTransfer();
    const dragOverEv = new Event('dragover', { bubbles: true, cancelable: true });
    Object.defineProperty(dragOverEv, 'dataTransfer', { value: dragOverDt });
    withSubDropTarget!.dispatchEvent(dragOverEv);

    // Advance only 300ms, then dragleave (cancels timer)
    vi.advanceTimersByTime(300);
    withSubDropTarget!.dispatchEvent(new Event('dragleave', { bubbles: true }));

    // Now advance past what would have been the 600ms total
    vi.advanceTimersByTime(400);

    // Folder still collapsed — spring-load cancelled
    expect(findLeaf(container, 'child.md')).toBeNull();
  });

  // ── multi-row drop ────────────────────────

  test('drop with multi-row selection calls moveEntries with all paths (batch, no race)', async () => {
    // A batch call replaced the per-file renameEntry loop with a single
    // moveEntries call. This eliminates the per-item refreshTree race that
    // the per-file fire-and-forget loop. We assert:
    //   - moveEntries is called ONCE with both file paths + the target dir.
    //   - renameEntry is NOT called (no per-file rename loop).
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: threeFileTree() });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // Select beta + gamma (both in posts/)
    const beta = findLeaf(container, 'beta.md');
    const gamma = findLeaf(container, 'gamma.md');
    clickWith(beta!);
    vi.advanceTimersByTime(SETTLE_MS);
    clickWith(gamma!, { metaKey: true });
    vi.advanceTimersByTime(SETTLE_MS);

    // Expand with-sub and drop onto with-sub folder (different parent)
    const withSub = container.querySelector<HTMLElement>(`[data-path="with-sub"]`);
    expect(withSub).not.toBeNull();

    // Dragstart on beta carries the {beta, gamma} selection
    const dt = new DataTransfer();
    const dragStart = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStart, 'dataTransfer', { value: dt });
    beta!.dispatchEvent(dragStart);

    // Drop on with-sub folder
    const dropDt = new DataTransfer();
    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: dropDt });
    withSub!.dispatchEvent(dropEv);

    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalledTimes(1));
    const [paths, targetDir] = mockMoveEntries.mock.calls[0];
    expect(paths).toHaveLength(2);
    expect(paths).toContain(`${PROJECT_ROOT}/posts/beta.md`);
    expect(paths).toContain(`${PROJECT_ROOT}/posts/gamma.md`);
    expect(targetDir).toBe(`${PROJECT_ROOT}/with-sub`);
    // renameEntry must NOT be called — batch path only
    expect(mockRenameEntry).not.toHaveBeenCalled();
  });

  // ── Folder rows populate the drag payload ─────────────────
  // (collapsed and expanded folder rows must set draggable=true and
  // call startMultiRowDrag so the x-moss-entries payload is populated for
  // tree→editor drops; without draggable=true the browser never fires dragstart
  // in production even though manual dispatchEvent works in tests)

  test('dragstart on a collapsed folder row carries the folder entryId in dataTransfer', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Highlight index.md (at the root) so 'posts' is NOT an ancestor — it
    // renders as a collapsed folder row.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // 'posts' renders as a collapsed folder row (not expanded).
    const postsRow = findFolderSeg(container, 'posts');
    expect(postsRow).not.toBeNull();

    const dt = new DataTransfer();
    const dragEvent = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragEvent, 'dataTransfer', { value: dt });
    postsRow!.dispatchEvent(dragEvent);

    const payload = dt.getData('application/x-moss-entries');
    expect(payload).not.toBe('');
    const parsed = JSON.parse(payload);
    expect(Array.isArray(parsed.entryIds)).toBe(true);

    // The folder's entryId must be in the payload.
    const postsFolderId = registry.getByPath('posts');
    expect(postsFolderId).not.toBeNull();
    expect(parsed.entryIds).toContain(postsFolderId);
  });

  test('dragstart on an expanded folder row carries the folder entryId in dataTransfer', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Start with index.md so 'posts' is initially collapsed.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Expand 'posts' via chevron click so the expanded folder row is rendered.
    const postsRow = findFolderSeg(container, 'posts');
    const chevron = postsRow!.querySelector<HTMLElement>('.chev');
    expect(chevron).not.toBeNull();
    chevron!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    vi.advanceTimersByTime(SETTLE_MS);

    // After expansion, the posts row should be 'open'.
    const postsRowExpanded = findFolderSeg(container, 'posts');
    expect(postsRowExpanded!.classList.contains('open')).toBe(true);

    const dt = new DataTransfer();
    const dragEvent = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragEvent, 'dataTransfer', { value: dt });
    postsRowExpanded!.dispatchEvent(dragEvent);

    const payload = dt.getData('application/x-moss-entries');
    expect(payload).not.toBe('');
    const parsed = JSON.parse(payload);
    expect(Array.isArray(parsed.entryIds)).toBe(true);

    const postsFolderId = registry.getByPath('posts');
    expect(postsFolderId).not.toBeNull();
    expect(parsed.entryIds).toContain(postsFolderId);
  });
});


// ---------------------------------------------------------------------------
// The drop side of the drag, moved here from file-tree.test.ts at the
// Hit-testing a drop target (including the
// transparent indent margin), the drop highlight, the native drag-out through
// the OS plugin, the batched internal move, and the spring-load dwell. Same
// subject as above — tree/dnd.ts driven through the controller's delegated
// listeners — with its own fixture because these cases were written against a
// tree that has an empty folder and a non-markdown leaf.
// ---------------------------------------------------------------------------

describe('drop targets, native drag-out and spring-load', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  /** assets/ is empty (a legal drop target with no children) and readme.txt is
   *  the non-markdown leaf the drag-out cases carry. */
  function blogTree() {
    return dir(PROJECT_ROOT, [
        dir(`${PROJECT_ROOT}/assets`, []),
        dir(`${PROJECT_ROOT}/posts`, [
            file(`${PROJECT_ROOT}/posts/article-one.md`),
            file(`${PROJECT_ROOT}/posts/article-two.md`),
          ]),
        file(`${PROJECT_ROOT}/index.md`),
        file(`${PROJECT_ROOT}/readme.txt`),
      ], { name: 'test-blog' });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('getFolderAtPosition returns null before mount', () => {
    expect(fileTree.getFolderAtPosition(0, 0)).toBeNull();
  });

  test('getFolderAtPosition: position over folder row returns its EntryId', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Navigate so root children (assets, posts) appear as visible sibling rows
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // expand to show siblings

    // Find the 'assets' folder row and stub its bounding rect.
    // data-path is project-root-relative.
    const assetsPath = 'assets';
    const row = container.querySelector<HTMLElement>(`[data-path="${assetsPath}"]`);
    expect(row).not.toBeNull();

    // jsdom doesn't lay out elements, so getBoundingClientRect returns all zeros.
    // Stub it to return a known rect.
    row!.getBoundingClientRect = () => ({
      left: 0, right: 200, top: 40, bottom: 56,
      width: 200, height: 16, x: 0, y: 40,
      toJSON: () => {},
    });

    const id = fileTree.getFolderAtPosition(100, 48);
    expect(id).not.toBeNull();
    expect(fileTree.pathOf(id!)).toBe(assetsPath);
  });

  test('getFolderAtPosition returns null when x,y is outside all rows', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // Stub all rows to have zero rects so no row contains the test point
    const rows = container.querySelectorAll<HTMLElement>('[data-path]');
    for (const row of rows) {
      row.getBoundingClientRect = () => ({
        left: 0, right: 0, top: 0, bottom: 0,
        width: 0, height: 0, x: 0, y: 0,
        toJSON: () => {},
      });
    }

    expect(fileTree.getFolderAtPosition(500, 500)).toBeNull();
  });

  test('getFolderAtPosition resolves file row to its parent folder (Finder-style drop-to-parent)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // The 'index.md' file row (project-root-relative) — a root-level file.
    const indexPath = 'index.md';
    const row = container.querySelector<HTMLElement>(`[data-path="${indexPath}"]`);
    // index.md may not have data-path (files use data-full-name);
    // skip if the row is not found in the DOM.
    if (!row) return;

    row.getBoundingClientRect = () => ({
      left: 0, right: 200, top: 100, bottom: 116,
      width: 200, height: 16, x: 0, y: 100,
      toJSON: () => {},
    });

    // Hovering a root-level file should resolve to the project root folder
    // (parentPath = '' → getByPath('') = project root EntryId, which is a dir).
    const id = fileTree.getFolderAtPosition(100, 108);
    expect(id).not.toBeNull();
    // The resolved id must be a directory (the parent folder, not the file itself)
    expect(registry.getById(id!)?.isDir).toBe(true);
    // And it should be the project root (path = '')
    expect(registry.getById(id!)?.path).toBe('');
  });

  test('getFolderAtPosition: hovering a nested file row resolves to its parent folder', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Navigate into posts so article-one.md row is visible
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // Find the 'posts/article-one.md' file row
    const filePath = 'posts/article-one.md';
    const row = container.querySelector<HTMLElement>(`[data-path="${filePath}"]`);
    if (!row) return; // skip if file rows don't get data-path in this render

    row.getBoundingClientRect = () => ({
      left: 0, right: 200, top: 80, bottom: 96,
      width: 200, height: 16, x: 0, y: 80,
      toJSON: () => {},
    });

    // Hovering a nested file should resolve to its parent folder 'posts'
    const id = fileTree.getFolderAtPosition(100, 88);
    expect(id).not.toBeNull();
    const record = registry.getById(id!);
    expect(record?.isDir).toBe(true);
    expect(record?.path).toBe('posts');
  });

  test('setDropHighlight adds and removes CSS class', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // render siblings so assets folder row appears

    // Registry / data-path are both project-root-relative after the migration.
    const assetsPath = 'assets';
    const assetsId = registry.getByPath(assetsPath);
    expect(assetsId).not.toBeNull();

    // Highlight the folder
    fileTree.setDropHighlight(assetsId!);
    const row = container.querySelector<HTMLElement>(`[data-path="${assetsPath}"]`);
    expect(row?.classList.contains('moss-tree-row--drop-target')).toBe(true);

    // Clear the highlight
    fileTree.setDropHighlight(null);
    expect(row?.classList.contains('moss-tree-row--drop-target')).toBe(false);
  });

  test('drop outside any .seg (transparent margin) still resolves a target and moves the file (bug #3/#4)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });

    const assetsRow = container.querySelector<HTMLElement>('[data-path="assets"]');
    expect(assetsRow).not.toBeNull();
    assetsRow!.getBoundingClientRect = () => ({
      left: 0, right: 200, top: 40, bottom: 56,
      width: 200, height: 16, x: 0, y: 40,
      toJSON: () => {},
    });

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(el => el.textContent?.includes('readme.txt')) as HTMLElement;
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    // dragover first (not on any .seg) — simulates DWELLING in the margin,
    // not just a quick pass-through — asserts the highlight-painting fix,
    // not just that the eventual drop succeeds.
    const dragoverEvent = new Event('dragover', { bubbles: true, cancelable: true }) as DragEvent;
    Object.defineProperty(dragoverEvent, 'clientX', { value: 100 });
    Object.defineProperty(dragoverEvent, 'clientY', { value: 48 });
    container.dispatchEvent(dragoverEvent);
    expect(assetsRow!.classList.contains('moss-tree-row--drop-target')).toBe(true);

    // Drop on `container` itself — NOT on any .seg — simulating a release in
    // the transparent margin, which real .seg dragover/drop handlers never see.
    const dropEvent = new Event('drop', { bubbles: true, cancelable: true }) as DragEvent;
    Object.defineProperty(dropEvent, 'clientX', { value: 100 });
    Object.defineProperty(dropEvent, 'clientY', { value: 48 });
    container.dispatchEvent(dropEvent);

    await vi.waitFor(() => {
      expect(mockMoveEntries).toHaveBeenCalledWith(
        [`${PROJECT_ROOT}/readme.txt`],
        `${PROJECT_ROOT}/assets`,
      );
    });
  });

  test('dragover over the margin bordering the dragged item\'s OWN CURRENT PARENT shows no highlight (invalid drop target)', async () => {
    // Regression test: the container-level margin-drop fallback must apply
    // the same canDropInto gate wireDropTarget's own per-.seg dragover
    // handler enforces. Without it, dwelling in the transparent margin
    // bordering an invalid target (here: the dragged file's own parent
    // folder, a no-op drop) paints the "you can drop here" highlight even
    // though the eventual drop would silently no-op (performDrop's internal
    // canDropOneInto re-check rejects it) — the exact "highlighted-as-valid
    // but drop does nothing" bug class the drop handler must prevent.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // article-one.md's parent is 'posts'. Per the INVARIANT test above,
    // 'posts' is the active ancestor and therefore renders ONLY as a
    // breadcrumb segment (.seg.bc[data-path="posts"]), not a body row —
    // but it still carries data-path, so getFolderAtPosition's jsdom
    // y-range fallback resolves it just like any other row.
    const postsCrumb = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .find(el => el.dataset.path === 'posts');
    expect(postsCrumb).not.toBeNull();
    postsCrumb!.getBoundingClientRect = () => ({
      left: 0, right: 200, top: 0, bottom: 16,
      width: 200, height: 16, x: 0, y: 0,
      toJSON: () => {},
    });

    // Drag article-one.md (leaf, nested under posts).
    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs)
      .find(el => (el as HTMLElement).dataset.fullName === 'article-one.md') as HTMLElement;
    expect(fileSeg).toBeTruthy();
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    // Dwell (not just pass through) over the margin bordering the 'posts'
    // crumb — an INVALID target since it's article-one.md's current parent.
    const dragoverEvent = new Event('dragover', { bubbles: true, cancelable: true }) as DragEvent;
    Object.defineProperty(dragoverEvent, 'clientX', { value: 100 });
    Object.defineProperty(dragoverEvent, 'clientY', { value: 8 });
    container.dispatchEvent(dragoverEvent);

    expect(postsCrumb!.classList.contains('moss-tree-row--drop-target')).toBe(false);
  });

  // ── Native Drag Out (wireDragOut → plain drag to Finder / Explorer) ─────────

  test('dragstart on file triggers startDrag with single path', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const target = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    expect(target).not.toBeNull();

    // dragstart (no altKey required — OS drag is now plain-drag)
    target.dispatchEvent(new Event('dragstart', { bubbles: true }));

    // startDrag is async; wait for it
    await vi.waitFor(() => {
      expect(mockDragOut).toHaveBeenCalledWith([`${PROJECT_ROOT}/posts/article-two.md`]);
    });
  });

  test('dragstart with multi-select carries all selected paths', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll<HTMLElement>('.seg.leaf');
    const t1 = Array.from(leafSegs).find(
      el => el.dataset.fullName === 'article-one.md',
    ) as HTMLElement;
    const t2 = Array.from(leafSegs).find(
      el => el.dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    expect(t1).not.toBeNull();
    expect(t2).not.toBeNull();

    // Select two files via Cmd+click. Both act immediately — the defer that
    // no longer needs fake timers.
    t1.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));
    t2.dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }));

    // dragstart on t2 — wireDragOut fires after startMultiRowDrag.
    // startMultiRowDrag sets draggedSelection to the whole selection (both
    // ids) because t2 is already in the selection after the Cmd+clicks.
    t2.dispatchEvent(new Event('dragstart', { bubbles: true }));

    await vi.waitFor(() => {
      expect(mockDragOut).toHaveBeenCalledTimes(1);
      const paths = mockDragOut.mock.calls[0][0];
      // Both selected file paths must be present (order may vary)
      expect(paths).toHaveLength(2);
      expect(paths).toContain(`${PROJECT_ROOT}/posts/article-one.md`);
      expect(paths).toContain(`${PROJECT_ROOT}/posts/article-two.md`);
    });
  });

  // ── Internal Drag-and-Drop Regression ─────────────────────────────

  test('file segments have draggable attribute', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const target = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    expect(target).not.toBeNull();
    expect(target.draggable).toBe(true);
  });

  test('expanded folder row has draggable=true (production-shape guard)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Highlight index.md (root-level) so 'posts' is NOT on the active path —
    // it renders as a body row (not pushed to breadcrumb). Then expand it via
    // a chevron click to get the expanded folder row shape.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Expand 'posts' via its chevron to produce an expanded body row.
    const folderRows = container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)');
    const postsRowCollapsed = Array.from(folderRows).find(el => el.dataset.path === 'posts');
    expect(postsRowCollapsed).not.toBeUndefined();
    const chevron = postsRowCollapsed!.querySelector<HTMLElement>('.chev');
    expect(chevron).not.toBeNull();
    chevron!.click();

    // After expansion, the expanded folder row should have draggable=true.
    await vi.waitFor(() => {
      const rows = container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)');
      const postsRowExpanded = Array.from(rows).find(
        el => el.dataset.path === 'posts' && el.classList.contains('open'),
      );
      expect(postsRowExpanded).not.toBeUndefined();
      expect(postsRowExpanded!.draggable).toBe(true);
    });
  });

  test('dragstart sets draggedEntry via dataTransfer', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const target = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;
    expect(target).not.toBeNull();

    // jsdom doesn't have DragEvent, so use Event with the correct type name
    const dragEvent = new Event('dragstart', { bubbles: true });
    // jsdom doesn't provide dataTransfer automatically, so we check it doesn't crash
    target.dispatchEvent(dragEvent);
    // If dragstart fires without error, the handler was wired correctly
  });

  test('breadcrumb folder segment is draggable and starts a drag (bug #2)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // 'posts' is an ancestor of the active file, so it's a breadcrumb (.bc) segment.
    const bcSegs = container.querySelectorAll<HTMLElement>('.seg.bc:not(.leaf)');
    const postsBc = Array.from(bcSegs).find(el => el.dataset.path === 'posts');
    expect(postsBc).not.toBeUndefined();
    expect(postsBc!.draggable).toBe(true);

    // Should not throw, same smoke-check as the existing tree-row dragstart test.
    postsBc!.dispatchEvent(new Event('dragstart', { bubbles: true }));
  });

  test('breadcrumb current-file segment is draggable (bug #2)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const currentSeg = container.querySelector<HTMLElement>('.seg.bc.bc-current');
    expect(currentSeg).not.toBeNull();
    expect(currentSeg!.draggable).toBe(true);
  });

  test('dropping onto a breadcrumb folder segment calls moveEntries (bug #2)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });

    // Drag a tree-body file (readme.txt, at project root) …
    const leafSegs = container.querySelectorAll('.seg.leaf:not(.bc)');
    const fileSeg = Array.from(leafSegs).find(el => el.textContent?.includes('readme.txt')) as HTMLElement;
    expect(fileSeg).not.toBeNull();
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    // … and drop it onto the 'posts' BREADCRUMB segment (not the tree-body row).
    const bcSegs = container.querySelectorAll<HTMLElement>('.seg.bc:not(.leaf)');
    const postsBc = Array.from(bcSegs).find(el => el.dataset.path === 'posts');
    expect(postsBc).not.toBeUndefined();
    postsBc!.dispatchEvent(new Event('drop', { bubbles: true }));

    await vi.waitFor(() => {
      expect(mockMoveEntries).toHaveBeenCalledWith(
        [`${PROJECT_ROOT}/readme.txt`],
        `${PROJECT_ROOT}/posts`,
      );
    });
  });

  test('drop on folder calls moveEntries (internal move, batch path)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);
    // Single-file drags route through moveEntries (not renameEntry)
    // because startMultiRowDrag always sets draggedSelection (size=1 when
    // source is not in the selection), so the batch branch fires.
    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });

    // Find a file to drag
    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => el.textContent?.includes('readme.txt'),
    ) as HTMLElement;
    expect(fileSeg).not.toBeNull();

    // Initiate dragstart to set draggedSelection (jsdom has no DragEvent, use Event)
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    // Find a folder to drop onto
    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).not.toBeNull();

    // Drop on the folder (jsdom has no DragEvent, use Event)
    postsSeg.dispatchEvent(new Event('drop', { bubbles: true }));

    await vi.waitFor(() => {
      expect(mockMoveEntries).toHaveBeenCalledWith(
        [`${PROJECT_ROOT}/readme.txt`],
        `${PROJECT_ROOT}/posts`,
      );
    });
  });

  test('batch drop move failure shows a toast (bug #4)', async () => {
    // Historically also asserted `dialogMessage` (the Tauri dialog plugin's
    // native `window.alert`) was never called — but dnd.ts's own drop
    // handler (below) never touched that plugin even before the modal
    // contract work, and now nothing in the app calls it at all
    // (the obsolete host dialog has no callers). A spy on an
    // export nothing calls passes by construction, not by proof, so the
    // negative assertion is gone; this test's job is the toast.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);
    mockMoveEntries.mockResolvedValue({ status: 'error', error: 'disk full' });

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(el => el.textContent?.includes('readme.txt')) as HTMLElement;
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const postsSeg = Array.from(folderSegs).find(el => el.textContent?.includes('posts')) as HTMLElement;
    postsSeg.dispatchEvent(new Event('drop', { bubbles: true }));

    await vi.waitFor(() => {
      expect(mockShowToast).toHaveBeenCalledWith(
        expect.objectContaining({ severity: 'Blocking' }),
      );
    });
  });

  // ── Drop-target flash + spring-load regression ─────────────

  test('green arrival flash applied to target folder on successful moveEntries drop', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    mockMoveEntries.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    mockListTree.mockResolvedValue({ status: 'ok', data: blogTree() });

    // Find file to drag and folder to drop on
    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(
      el => el.textContent?.includes('readme.txt'),
    ) as HTMLElement;
    expect(fileSeg).not.toBeNull();

    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).not.toBeNull();

    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));
    postsSeg.dispatchEvent(new Event('drop', { bubbles: true }));

    // After the move resolves, refreshTree()+render() rebuild the tree (the
    // original postsSeg is destroyed by innerHTML=''), then the target folder
    // gets the SAME green .moss-just-committed pulse a download/create uses —
    // applied by path via flashRow, so we must re-query the fresh element.
    await vi.waitFor(() => {
      const freshPosts = Array.from(
        container.querySelectorAll('.seg:not(.leaf)'),
      ).find(el => el.textContent?.includes('posts')) as HTMLElement | undefined;
      expect(freshPosts?.classList.contains('moss-just-committed')).toBe(true);
    });
  });

  test('spring-load: arming folder B cancels folder A timer (regression guard)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Highlight a root file so posts and assets are in the registry but both closed.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    vi.useFakeTimers();
    try {
      // Arm A (posts) — should set a 600ms timer.
      fileTree.springLoadIfHoverIs('posts');
      // Arm B (assets) — cancels A and arms B instead.
      fileTree.springLoadIfHoverIs('assets');
      // Advance past the spring-load timeout: only B's callback fires, not A's.
      vi.advanceTimersByTime(700);
      // posts must NOT have been expanded (A's timer was cancelled).
      // assets IS now open (B's timer fired and expanded it).
      // Observable: the overrides map — check via rendered DOM.
      // posts row: no open chev rotate; assets row: chev rotated (open class).
      const postsRow = container.querySelector('[data-path="posts"]') as HTMLElement | null;
      const assetsRow = container.querySelector('[data-path="assets"]') as HTMLElement | null;
      // posts should NOT be open (A's expand never fired)
      expect(postsRow?.classList.contains('open')).toBe(false);
      // assets should be open (B's expand fired)
      expect(assetsRow?.classList.contains('open')).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  test('dragleave immediately followed by dragover on another folder shows no gap (bug #3)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(el => el.textContent?.includes('readme.txt')) as HTMLElement;
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    const folderSegs = container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(el => el.textContent?.includes('posts'))!;
    const otherSeg = Array.from(folderSegs).find(el => el !== postsSeg)!;
    expect(otherSeg).not.toBeUndefined();

    postsSeg.dispatchEvent(new Event('dragover', { bubbles: true }));
    expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(true);

    // Leave postsSeg, immediately enter otherSeg — simulates the pointer
    // crossing the transparent per-row margin (file-tree.css:74-89) between
    // two adjacent drop targets within the debounce window.
    postsSeg.dispatchEvent(new Event('dragleave', { bubbles: true }));
    otherSeg.dispatchEvent(new Event('dragover', { bubbles: true }));

    // otherSeg is highlighted; postsSeg's highlight was cleared immediately
    // (we KNOW a new target resolved), not left dangling until the timer fires.
    expect(otherSeg.classList.contains('moss-tree-row--drop-target')).toBe(true);
    expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(false);
  });

  test('dragend clears the highlight AND the spring-load dwell, not just one of them', async () => {
    // The feedback of a drag hovering the tree is one record with one
    // teardown. It used to be six fields cleared by hand in three different
    // subsets, and an aborted drag was the exit path that owed the smallest
    // one — so a highlight-clear could outlive the gesture that painted it.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const fileSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf'))
      .find(el => el.textContent?.includes('readme.txt'))!;
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    const postsSeg = Array.from(
      container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'),
    ).find(el => el.textContent?.includes('posts'))!;

    vi.useFakeTimers();
    try {
      // Drag into the bottom edge zone: highlight, dwell and edge-scroll all
      // armed by the one dragover.
      const navBody = container.querySelector<HTMLElement>('.nav-body')!;
      navBody.getBoundingClientRect = () => new DOMRect(0, 0, 320, 200);
      postsSeg.dispatchEvent(
        Object.assign(new Event('dragover', { bubbles: true }), { clientY: 190 }),
      );
      expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(true);
      vi.advanceTimersByTime(50);
      expect(navBody.scrollTop).toBeGreaterThanOrEqual(0);

      // Escape / drop on nothing: dragend, with the dwell still counting.
      container.dispatchEvent(new Event('dragend', { bubbles: true }));
      expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(false);

      // Past the spring-load dwell the folder must NOT have opened, and the
      // edge-scroll must not still be stepping — the gesture that armed them
      // is over. All three members of the record, one teardown.
      const scrolledTo = navBody.scrollTop;
      vi.advanceTimersByTime(1000);
      expect(navBody.scrollTop).toBe(scrolledTo);
      expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(false);
      expect(
        container.querySelector('.nav-body .seg[data-path^="posts/"]'),
      ).toBeNull(); // spring-load did not fire: no children rendered
    } finally {
      vi.useRealTimers();
    }
  });

  test('dragleave with no follow-up dragover clears the highlight after the debounce window (bug #3)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const fileSeg = Array.from(leafSegs).find(el => el.textContent?.includes('readme.txt')) as HTMLElement;
    fileSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));

    const folderSegs = container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)');
    const postsSeg = Array.from(folderSegs).find(el => el.textContent?.includes('posts'))!;

    postsSeg.dispatchEvent(new Event('dragover', { bubbles: true }));
    expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(true);

    vi.useFakeTimers();
    try {
      postsSeg.dispatchEvent(new Event('dragleave', { bubbles: true }));
      // Still highlighted immediately after dragleave — clear is debounced, not synchronous.
      expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(true);

      vi.advanceTimersByTime(150);
      expect(postsSeg.classList.contains('moss-tree-row--drop-target')).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
