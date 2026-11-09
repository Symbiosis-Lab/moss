/**
 * Integration test for the tree→editor drop flow (substituted
 * integration test, acceptable per spec when the full Tauri e2e harness is
 * not runnable in CI).
 *
 * Exercises the real tree→editor flow end-to-end:
 *   1. Mount a FileTree with a real EntryRegistry.
 *   2. Dispatch a `dragstart` on a tree file row — TreeDnd.startMultiRowDrag
 *      sets the `application/x-moss-entries` payload with the entry ID.
 *   3. Dispatch a `drop` on the editor's dropExtension — it reads the payload,
 *      calls resolveDropEntries, and inserts an embed at the drop position.
 *   4. Assert the embed appears in the document.
 *
 * Covers a page, an image and a folder, each written as `![](path)` from the page.
 *
 * RENDERING MODEL: after `highlight(filePath)` + `enterDirMode()`, the tree
 * renders the highlighted file and its ancestors as BREADCRUMB segs (.bc).
 * SIBLINGS of the highlighted file render as tree LEAF segs (.leaf). We
 * target siblings (not the highlighted file itself) for drag tests.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { dropExtension } from '../../cm-drop';
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { enterDirMode, dir, file } from './test-helpers';

// ---------------------------------------------------------------------------
// Mock Tauri bindings (required by FileTree)
// ---------------------------------------------------------------------------
const { mockListTree } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
    renameEntry: vi.fn(),
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    moveEntries: vi.fn(),
    deleteEntries: vi.fn(),
    setImageWidth: vi.fn(),
  },
}));


// (CSS.escape and DataTransfer polyfills provided globally by test-setup-ui.ts)

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const PROJECT_ROOT = '/projects/test-blog';

/**
 * Tree structure:
 *   test-blog/
 *     posts/              ← folder (collapsed, renders as leaf tree row)
 *       alpha.md          ← HIGHLIGHTED (renders as breadcrumb)
 *       beta.md           ← sibling → renders as .seg.leaf tree row
 *       image.png         ← sibling → renders as .seg.leaf tree row
 *     assets/             ← sibling folder → renders as .seg (non-leaf) tree row
 *     index.md            ← sibling → renders as .seg.leaf tree row
 *
 * After highlight(alpha.md) + enterDirMode():
 *   - Breadcrumbs: test-blog > posts > alpha.md  (.seg.bc)
 *   - Tree rows: beta.md, image.png (siblings of alpha), assets (sibling dir)
 */
function buildTree() {
  return dir(PROJECT_ROOT, [
      dir(`${PROJECT_ROOT}/posts`, [
          file(`${PROJECT_ROOT}/posts/alpha.md`),
          file(`${PROJECT_ROOT}/posts/beta.md`),
          file(`${PROJECT_ROOT}/posts/image.png`),
        ]),
      dir(`${PROJECT_ROOT}/assets`, []),
      file(`${PROJECT_ROOT}/index.md`),
    ], { name: 'test-blog' });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Find a tree row (.seg.leaf) by its data-full-name attribute. */
function findLeaf(container: HTMLElement, name: string): HTMLElement | null {
  return (
    Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf:not(.bc)')).find(
      (el) => el.dataset.fullName === name,
    ) ?? null
  );
}

/**
 * Find a collapsed folder seg by its data-path attribute (folder rows do not
 * set data-full-name; they set data-path to the project-root-relative folder
 * path, e.g. "assets" not "/projects/test-blog/assets").
 */
function findFolderSeg(container: HTMLElement, relPath: string): HTMLElement | null {
  return (
    container.querySelector<HTMLElement>(`.seg[data-path="${CSS.escape(relPath)}"]`) ?? null
  );
}

// ---------------------------------------------------------------------------
// Shared setup
// ---------------------------------------------------------------------------

let registry: EntryRegistry;
let fileTree: FileTree;
let treeContainer: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  registry = new EntryRegistry();
  fileTree = new FileTree(registry);
  treeContainer = document.createElement('div');
  document.body.appendChild(treeContainer);
  mockListTree.mockResolvedValue({ status: 'ok', data: buildTree() });
});

afterEach(() => {
  vi.useRealTimers();
  fileTree.unmount();
  treeContainer.remove();
  document.body.querySelectorAll('.drag-ghost').forEach((el) => el.remove());
  document.body.querySelectorAll('.cm-editor').forEach((el) => el.remove());
});

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

describe('tree→editor drop integration', () => {
  /**
   * Core flow: drag a file row from the tree and drop it onto the editor.
   * The dropExtension reads the `application/x-moss-entries` payload, resolves
   * entry IDs via fileTree.resolveDropEntries, and inserts `![](name)`.
   */
  test('dragstart on file row + drop on editor inserts ![](name)', async () => {
    // ── Mount + set up the file tree ─────────────────────────────
    await fileTree.mount(treeContainer, PROJECT_ROOT, false);
    // Highlight alpha.md so its siblings (beta.md, image.png) render as tree rows
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // ── Build editor with resolveEntries wired to the file tree ──
    const editorContainer = document.createElement('div');
    document.body.appendChild(editorContainer);

    const view = new EditorView({
      state: EditorState.create({
        doc: 'some markdown content',
        extensions: [
          dropExtension({
            resolveEntries: (ids) => fileTree.resolveDropEntries(ids),
            destinationFor: async (t) => t,
          }),
        ],
      }),
      parent: editorContainer,
    });
    vi.spyOn(view, 'posAtCoords').mockReturnValue(5);

    // ── Simulate dragstart on the beta.md row ────────────────────
    // beta.md is a sibling of the highlighted alpha.md → renders as .seg.leaf
    const betaRow = findLeaf(treeContainer, 'beta.md');
    expect(betaRow).not.toBeNull();

    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    betaRow!.dispatchEvent(dragStartEv);

    // Verify the drag payload was populated by startMultiRowDrag
    const payload = dt.getData('application/x-moss-entries');
    expect(payload).not.toBe('');
    const parsed = JSON.parse(payload) as { entryIds: number[] };
    expect(Array.isArray(parsed.entryIds)).toBe(true);
    expect(parsed.entryIds.length).toBe(1);

    // ── Drop on the editor contentDOM ────────────────────────────
    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: dt });
    Object.defineProperty(dropEv, 'clientX', { value: 10 });
    Object.defineProperty(dropEv, 'clientY', { value: 10 });
    view.contentDOM.dispatchEvent(dropEv);

    // Dispatch dragend on the source so TreeDnd.retireDrag clears
    // draggedSelection/draggedEntry, preventing stale drag state from leaking
    // into subsequent drops.
    betaRow!.dispatchEvent(new Event('dragend', { bubbles: true }));

    // ── Assert ![](posts/beta.md) inserted ─────────────────────────────
    await vi.waitFor(() => expect(view.state.doc.toString()).toContain('![](posts/beta.md)'));

    view.destroy();
    editorContainer.remove();
  });

  /**
   * Image drop: the standard form, with the destination the engine gives
   * for the page being edited.
   */
  test('dragstart on an image row + drop on editor inserts ![](path from the page)', async () => {
    await fileTree.mount(treeContainer, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const editorContainer = document.createElement('div');
    document.body.appendChild(editorContainer);
    const view = new EditorView({
      state: EditorState.create({
        doc: 'text',
        extensions: [
          dropExtension({
            resolveEntries: (ids) => fileTree.resolveDropEntries(ids),
            // The engine's answer for a page in `notes/` (asset_ref_for_path).
            destinationFor: async (t) => `../${t}`,
          }),
        ],
      }),
      parent: editorContainer,
    });
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    const imageRow = findLeaf(treeContainer, 'image.png');
    expect(imageRow).not.toBeNull();
    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    imageRow!.dispatchEvent(dragStartEv);

    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: dt });
    Object.defineProperty(dropEv, 'clientX', { value: 10 });
    Object.defineProperty(dropEv, 'clientY', { value: 10 });
    view.contentDOM.dispatchEvent(dropEv);
    imageRow!.dispatchEvent(new Event('dragend', { bubbles: true }));

    await vi.waitFor(() => expect(view.state.doc.toString()).toBe('![](../posts/image.png)\ntext'));

    view.destroy();
    editorContainer.remove();
  });

  /**
   * Folder drop: dragging a folder row produces `![](folder/)` (transcludes
   * the folder index page). This exercises folder-embed behavior and
   * confirms resolveDropEntries correctly appends `/` for directory entries.
   */
  test('dragstart on folder row + drop on editor inserts ![](folder/)', async () => {
    // ── Mount at the root so 'assets' folder is visible as a row ─
    await fileTree.mount(treeContainer, PROJECT_ROOT, false);
    // Highlight index.md → its siblings (posts folder, assets folder) render
    // as tree rows at the root breadcrumb level.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // ── Build editor ──────────────────────────────────────────────
    const editorContainer = document.createElement('div');
    document.body.appendChild(editorContainer);

    const view = new EditorView({
      state: EditorState.create({
        doc: 'content',
        extensions: [
          dropExtension({
            resolveEntries: (ids) => fileTree.resolveDropEntries(ids),
            destinationFor: async (t) => t,
          }),
        ],
      }),
      parent: editorContainer,
    });
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0);

    // ── Find and drag the 'assets' folder row ────────────────────
    // assets is a sibling of index.md at root level → renders as collapsed
    // folder seg (.seg with data-path set to the PROJECT-ROOT-RELATIVE path,
    // e.g. "assets" not "/projects/test-blog/assets"). Folder rows do NOT set
    // data-full-name; look them up via the relative data-path attribute.
    const assetsRow = findFolderSeg(treeContainer, 'assets');
    expect(assetsRow).not.toBeNull();
    // Production-shape guard: folder row must be draggable
    expect(assetsRow!.draggable).toBe(true);

    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    assetsRow!.dispatchEvent(dragStartEv);

    // Verify drag payload
    const payload = dt.getData('application/x-moss-entries');
    expect(payload).not.toBe('');

    // ── Drop on editor ────────────────────────────────────────────
    const dropEv = new Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(dropEv, 'dataTransfer', { value: dt });
    Object.defineProperty(dropEv, 'clientX', { value: 10 });
    Object.defineProperty(dropEv, 'clientY', { value: 10 });
    view.contentDOM.dispatchEvent(dropEv);

    // Dispatch dragend on the source so TreeDnd.retireDrag clears
    // draggedSelection/draggedEntry, preventing stale drag state from leaking
    // into subsequent drops.
    assetsRow!.dispatchEvent(new Event('dragend', { bubbles: true }));

    // ── Assert ![](assets/) inserted (folder embed) ──────────────
    await vi.waitFor(() => expect(view.state.doc.toString()).toContain('![](assets/)'));

    view.destroy();
    editorContainer.remove();
  });

  /**
   * Unit contract test for FileTree.resolveDropEntries: verifies that the
   * method produces bare filenames for files and `name/` for directories.
   * Does not require a mounted tree.
   */
  test('resolveDropEntries maps IDs to project-relative paths, a folder\'s ending in /', () => {
    const localRegistry = new EntryRegistry();
    const localTree = new FileTree(localRegistry);

    localRegistry.upsert('my-file.md', false);
    localRegistry.upsert('my-folder', true);
    localRegistry.upsert('posts/image.png', false);

    const fileId = localRegistry.getByPath('my-file.md');
    const folderId = localRegistry.getByPath('my-folder');
    const imageId = localRegistry.getByPath('posts/image.png');
    expect(fileId).not.toBeNull();
    expect(folderId).not.toBeNull();
    expect(imageId).not.toBeNull();

    const results = localTree.resolveDropEntries([fileId!, folderId!, imageId!]);
    expect(results).toEqual(['my-file.md', 'my-folder/', 'posts/image.png']);

    localTree.unmount();
  });

  /**
   * Flow 3 accessor pair (used by editor-main's Tauri onDragDropEvent
   * routing, where the DOM drop below never fires in production):
   * activeInternalDragEntryIds() exposes the dragged IDs during a session
   * without exposing the private fields; clearInternalDragSession()
   * consumes the session immediately after an editor-side drop.
   */
  test('activeInternalDragEntryIds returns dragged IDs during a session, [] after the dragend grace clear', async () => {
    await fileTree.mount(treeContainer, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    // No session yet.
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
    expect(fileTree.activeInternalDragEntryIds()).toEqual([]);

    const betaRow = findLeaf(treeContainer, 'beta.md');
    expect(betaRow).not.toBeNull();
    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    betaRow!.dispatchEvent(dragStartEv);

    // During the session: the accessor agrees with the drag payload set by
    // startMultiRowDrag.
    const payload = JSON.parse(dt.getData('application/x-moss-entries')) as { entryIds: number[] };
    const ids = fileTree.activeInternalDragEntryIds();
    expect(ids.length).toBe(1);
    expect(ids).toEqual(payload.entryIds);

    // dragend: the session stays armed through the 500ms grace window (the
    // Tauri 'drop' IPC payload can arrive AFTER DOM dragend — see
    // retireDrag's dragend-race doc), then clears.
    betaRow!.dispatchEvent(new Event('dragend', { bubbles: true }));
    expect(fileTree.activeInternalDragEntryIds()).toEqual(ids);
    vi.advanceTimersByTime(600);
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
    expect(fileTree.activeInternalDragEntryIds()).toEqual([]);
  });

  test('clearInternalDragSession consumes the session immediately (no 500ms grace)', async () => {
    await fileTree.mount(treeContainer, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/alpha.md`);
    enterDirMode(fileTree);

    const betaRow = findLeaf(treeContainer, 'beta.md');
    expect(betaRow).not.toBeNull();
    const dt = new DataTransfer();
    const dragStartEv = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(dragStartEv, 'dataTransfer', { value: dt });
    betaRow!.dispatchEvent(dragStartEv);
    expect(fileTree.hasActiveInternalDrag()).toBe(true);

    // The editor-side drop consumed the gesture — the session must die NOW,
    // not after retireDrag's deferred clear, or a follow-up drop inside
    // the grace window would misroute as an internal drag.
    fileTree.clearInternalDragSession();
    expect(fileTree.hasActiveInternalDrag()).toBe(false);
    expect(fileTree.activeInternalDragEntryIds()).toEqual([]);
  });

  /**
   * Stale/unknown entry IDs are silently skipped.
   */
  test('resolveDropEntries skips unknown IDs gracefully', () => {
    const localRegistry = new EntryRegistry();
    const localTree = new FileTree(localRegistry);

    // Upsert one entry, then ask for its ID + a nonexistent ID
    localRegistry.upsert('real.md', false);
    const realId = localRegistry.getByPath('real.md')!;
    const fakeId = 99999 as ReturnType<typeof localRegistry.getByPath>;

    const results = localTree.resolveDropEntries([realId, fakeId!]);
    expect(results).toEqual(['real.md']);

    localTree.unmount();
  });
});
