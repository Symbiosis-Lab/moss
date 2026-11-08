/**
 * What tree/controller.ts itself owns, driven through a mounted FileTree:
 * mount/unmount and listener composition, breadcrumb rendering and the layout
 * invariants, the anchor policy (highlight / forceAnchor / revealNewFile), the
 * sash machine and its floor, the gesture policy (folder select, chevron
 * toggle, keyboard activate, the double-click over reconciled rows), the
 * queries `os-drag-in` and `editor-main` call, and `setSealed`.
 *
 * Module-specific cases live beside their owners: the drop-target hit test, the native drag-out
 * and the spring-load timers are in `dnd.test.ts`, and the context menus, the
 * delete family and the other vault operations are in `menu.test.ts`.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, findRowByPath, stubPointerCapture, enterDirMode, startRenameByKey, priv, overrideAt, dir, file } from './test-helpers';

installPointerEventPolyfill();

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const { mockListTree, mockGetProjectPath, mockRenameEntry, mockCopyFilesToProject, mockRevealEntry, mockDuplicateEntry, mockCreateFile, mockCreateFolder, mockMoveEntries, mockDeleteEntries, mockScanReferencesForDelete, mockCleanReferencesAndDelete, mockListen } = vi.hoisted(() => ({
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
  // Bundle H: scan returns no broken refs by default → delete takes the
  // no-hits path (single confirm or skip), matching the pre-Bundle-H UX.
  mockScanReferencesForDelete: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  mockCleanReferencesAndDelete: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  mockListen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: mockRenameEntry,
    // Bundle H: commitRename now calls renameEntryWithRefs (drop-in). Alias it
    // to the same spy so existing rename assertions keep working.
    renameEntryWithRefs: mockRenameEntry,
    copyFilesToProject: mockCopyFilesToProject,
    revealEntry: mockRevealEntry,
    duplicateEntry: mockDuplicateEntry,
    createFiles: mockCreateFile,
    createFolder: mockCreateFolder,
    moveEntries: mockMoveEntries,
    deleteEntries: mockDeleteEntries,
    scanReferencesForDelete: mockScanReferencesForDelete,
    cleanReferencesAndDelete: mockCleanReferencesAndDelete,
  },
}));

// The tree hears the engine's events through the host; these tests capture its handler.
import { host } from "../../../host";
host.engine.on = mockListen;

const { mockShowToast } = vi.hoisted(() => ({ mockShowToast: vi.fn() }));
vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showToast: mockShowToast,
  showAck: vi.fn(),
}));

// JSDOM doesn't implement CSS.escape; the production code uses it to
// safely query .seg[data-path=...]. Provide a minimal stub so menu items
// that resolve segs via querySelector work under test.
if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
} else if (typeof globalThis.CSS.escape !== 'function') {
  (globalThis.CSS as { escape: (s: string) => string }).escape = (s: string) =>
    s.replace(/(["\\])/g, '\\$1');
}

// ---------------------------------------------------------------------------
// Import after mocks
// ---------------------------------------------------------------------------
import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';
import { mkBackCrumb } from '../title-bar';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Long enough for any work a gesture schedules to settle before asserting.
 *
 * The fake clock has to be installed BEFORE the gesture — installing it
 * afterwards leaves the timer on the real clock, and every one of these tests
 * asserts that something did NOT happen, so it would pass vacuously.
 */
const SETTLE_MS = 260;

const PROJECT_ROOT = '/projects/my-blog';

/** Build a default tree matching the old defaultDirEntries pattern. */
function defaultTree() {
  return dir(PROJECT_ROOT, [
      dir(`${PROJECT_ROOT}/assets`, []),
      dir(`${PROJECT_ROOT}/posts`, [
          file(`${PROJECT_ROOT}/posts/article-one.md`),
          file(`${PROJECT_ROOT}/posts/article-two.md`),
        ]),
      file(`${PROJECT_ROOT}/index.md`),
      file(`${PROJECT_ROOT}/readme.txt`),
    ], { name: 'my-blog' });
}

/** Tree with a folder that has a home file (index.md) and one without (notes/). */
function treeWithFolders() {
  return dir(PROJECT_ROOT, [
      dir(`${PROJECT_ROOT}/docs`, [
          file(`${PROJECT_ROOT}/docs/index.md`),
          file(`${PROJECT_ROOT}/docs/guide.md`),
        ]),
      dir(`${PROJECT_ROOT}/notes`, [
          file(`${PROJECT_ROOT}/notes/todo.md`),
        ]),
      file(`${PROJECT_ROOT}/index.md`),
    ], { name: 'my-blog' });
}

/**
 * Get all .seg elements' identity from the container.
 *
 * For file rows the visible text is the stem only (`.md` is stripped, other
 * extensions render as a dim `.ext-hint`). Tests mostly care about identity
 * (which file is rendered), so we return `data-full-name` for files and fall
 * back to the visible text for folders and breadcrumbs.
 */
function getSegTexts(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll<HTMLElement>('.seg'))
    .map(el => el.dataset.fullName ?? el.textContent?.trim() ?? '');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('FileTree', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  // ── Connector rail (declarative --ft-rail-offset on group wrappers) ──
  describe('connector rail', () => {
    test('sets a px --ft-rail-offset on every expanded-folder group and draws no .vline divs', async () => {
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
      enterDirMode(fileTree); // opens root + posts so sibling groups render
      await vi.waitFor(() => {
        expect(container.textContent).toContain('article-two');
      });

      const body = container.querySelector('.nav-body')!;
      const groups = body.querySelectorAll<HTMLElement>('[role="group"]');
      expect(groups.length).toBeGreaterThan(0);
      for (const g of groups) {
        // A length like "64px" / "0px" / "128.5px" — the rail's child column.
        expect(g.style.getPropertyValue('--ft-rail-offset')).toMatch(/^-?\d+(\.\d+)?px$/);
      }
      // The imperative rAF line machinery is gone: no .vline elements.
      expect(body.querySelectorAll('.vline').length).toBe(0);
    });

    // BUG 12: a group's rail must span from its top down to its LAST DIRECT
    // CHILD row and STOP — it must NOT run on into that child's own expanded
    // subtree. The wrapper CONTAINS each child folder's nested [role="group"],
    // so when the last direct child is an EXPANDED folder the wrapper extends
    // past it; the old wrapper-bottom rail then dangled below the folder's
    // chevron. syncRailOffsets() sets --ft-rail-rows = the last direct child's
    // visual row index; the CSS turns that into the capped height.
    test('caps the rail at the LAST DIRECT CHILD — an expanded last-child folder does not run the rail into its subtree', async () => {
      // A group with two direct children: a FILE (intro) then an EXPANDED
      // FOLDER (Writings, last) that has its OWN children. Mirrors the reported
      // "William Blake.md · then ∨ Writings (expanded)" case.
      const tree = dir(PROJECT_ROOT, [
          file(`${PROJECT_ROOT}/index.md`),
          file(`${PROJECT_ROOT}/intro.md`),
          dir(`${PROJECT_ROOT}/Writings`, [
              file(`${PROJECT_ROOT}/Writings/Auguries.md`),
              file(`${PROJECT_ROOT}/Writings/Songs.md`),
            ]),
        ], { name: 'my-blog' });
      mockListTree.mockResolvedValue({ status: 'ok', data: tree });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree); // opens root; Writings still collapsed

      // Expand Writings so its subtree renders INSIDE the root group.
      const collapsedWritings = Array.from(container.querySelectorAll('.seg:not(.leaf):not(.bc)'))
        .find(el => el.textContent?.trim().startsWith('Writings')) as HTMLElement;
      (collapsedWritings.querySelector('.chev') as HTMLElement).click();
      await vi.waitFor(() => expect(container.textContent).toContain('Auguries'));

      // render() rebuilds the DOM, so re-query the Writings row from the FRESH
      // tree (the pre-click node is now detached).
      const writingsSeg = container.querySelector<HTMLElement>('.nav-body .seg[data-path="Writings"]')!;
      expect(writingsSeg).not.toBeNull();

      // ── The ROOT group: last direct child is the EXPANDED Writings folder ──
      const rootGroup = writingsSeg.closest('[role="group"]') as HTMLElement;
      expect(rootGroup).not.toBeNull();

      // The LAST DIRECT child row of the group — walk .children directly (a
      // ':scope >' comma selector leaks descendants under jsdom, mirroring the
      // production code). Skips nested [role="group"] subtree wrappers.
      const lastDirectChildRow = (group: HTMLElement): HTMLElement | null => {
        let row: HTMLElement | null = null;
        for (const child of Array.from(group.children)) {
          const c = child as HTMLElement;
          if (c.matches('.seg[data-depth]')) row = c;
          else if (c.classList.contains('seg-pair')) row = c.querySelector<HTMLElement>('.seg[data-depth]');
        }
        return row;
      };

      const lastDirect = lastDirectChildRow(rootGroup)!;
      expect(lastDirect.textContent).toContain('Writings');
      expect(lastDirect.getAttribute('aria-expanded')).toBe('true');

      // ALL rows in the group (incl. Writings' nested subtree), document order.
      const allRows = Array.from(rootGroup.querySelectorAll<HTMLElement>('.seg[data-depth]'));
      const lastDirectIdx = allRows.indexOf(lastDirect);
      // The rail is capped at the LAST DIRECT child's row index (Writings = 1) …
      expect(rootGroup.style.getPropertyValue('--ft-rail-rows')).toBe(String(lastDirectIdx));
      // … and there ARE rows below it (Writings' own children) the rail excludes.
      expect(lastDirectIdx).toBeLessThan(allRows.length - 1);

      // ── Contrast: Writings' OWN group ends in a FILE (Songs) — the cap is the
      //    last row, so the rail is full-length (unchanged from before). ──
      const writingsGroup = writingsSeg.nextElementSibling as HTMLElement;
      expect(writingsGroup.getAttribute('role')).toBe('group');
      const wRows = Array.from(writingsGroup.querySelectorAll<HTMLElement>('.seg[data-depth]'));
      expect(writingsGroup.style.getPropertyValue('--ft-rail-rows')).toBe(String(wRows.length - 1));
    });

    // The rail's TOP differs by group kind so a NESTED group's first segment
    // (first child → parent) matches its inter-child spacing, while the
    // OUTERMOST group stays tucked under the breadcrumb (short first segment).
    // syncRailOffsets() sets --ft-rail-extend-top (0 outermost / 1 nested); the
    // CSS raises the top by extendTop*(rowH/2 + gap).
    test('nested group rail reaches its parent row (equal first segment); outermost group stays tucked under the breadcrumb', async () => {
      const tree = dir(PROJECT_ROOT, [
          file(`${PROJECT_ROOT}/index.md`),
          file(`${PROJECT_ROOT}/intro.md`),
          dir(`${PROJECT_ROOT}/Writings`, [
              file(`${PROJECT_ROOT}/Writings/Auguries.md`),
              file(`${PROJECT_ROOT}/Writings/Songs.md`),
            ]),
        ], { name: 'my-blog' });
      mockListTree.mockResolvedValue({ status: 'ok', data: tree });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);

      const collapsedWritings = Array.from(container.querySelectorAll('.seg:not(.leaf):not(.bc)'))
        .find(el => el.textContent?.trim().startsWith('Writings')) as HTMLElement;
      (collapsedWritings.querySelector('.chev') as HTMLElement).click();
      await vi.waitFor(() => expect(container.textContent).toContain('Auguries'));

      const writingsSeg = container.querySelector<HTMLElement>('.nav-body .seg[data-path="Writings"]')!;
      const rootGroup = writingsSeg.closest('[role="group"]') as HTMLElement;   // parent = .nav-body → outermost
      const nestedGroup = writingsSeg.nextElementSibling as HTMLElement;        // parent = rootGroup → nested

      // Structural sanity: rootGroup sits directly under .nav-body; nestedGroup
      // is inside rootGroup.
      expect(rootGroup.parentElement?.classList.contains('nav-body')).toBe(true);
      expect(nestedGroup.parentElement).toBe(rootGroup);

      // The var syncRailOffsets sets, consumed by the CSS.
      expect(rootGroup.style.getPropertyValue('--ft-rail-extend-top')).toBe('0');
      expect(nestedGroup.style.getPropertyValue('--ft-rail-extend-top')).toBe('1');

      // Derive the geometry the CSS computes (mirrors file-tree.css):
      //   railTop  = -extendTop * (rowH/2 + gap)         (relative to wrapper top)
      //   firstSeg = firstChildCenter(rowH/2) - railTop  (rail top → first child)
      //   pitch    = rowH + gap                          (inter-child spacing)
      const ROW_H = 28, GAP = 1;
      const extendTop = (g: HTMLElement) => parseInt(g.style.getPropertyValue('--ft-rail-extend-top'), 10);
      const firstSeg = (g: HTMLElement) => ROW_H / 2 - (-extendTop(g) * (ROW_H / 2 + GAP));
      const pitch = ROW_H + GAP;

      // Outermost: tucked under the breadcrumb → SHORT first segment (half a row).
      expect(firstSeg(rootGroup)).toBe(ROW_H / 2);
      expect(firstSeg(rootGroup)).toBeLessThan(pitch);
      // Nested: first child → parent equals the inter-child pitch.
      expect(firstSeg(nestedGroup)).toBe(pitch);
    });
  });

  // ── revealNewFile (create-reveal for the "Create folder page" CTA) ─────
  // BUG 10 + CHANGE A: createHomeNoteAndReveal calls revealNewFile, which makes
  // the just-created file the breadcrumb ANCHOR + selection (so the editor
  // opens/anchors it) WITHOUT expanding the tree body — creating a folder page
  // must not force the tree open. `overrides` (expansion state) is untouched
  // (the frozen anchor).
  describe('revealNewFile (create-reveal)', () => {
    function treeWithCollapsibleFolder() {
      return dir(PROJECT_ROOT, [
          dir(`${PROJECT_ROOT}/works`, [
              file(`${PROJECT_ROOT}/works/note.md`),
              file(`${PROJECT_ROOT}/works/draft.md`),
            ]),
          dir(`${PROJECT_ROOT}/blog`, [
              file(`${PROJECT_ROOT}/blog/post.md`),
            ]),
          file(`${PROJECT_ROOT}/index.md`),
        ], { name: 'my-blog' });
    }

    test('re-anchors the breadcrumb + sets selection WITHOUT expanding the target (collapsed folder stays collapsed)', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithCollapsibleFolder() });
      await fileTree.mount(container, PROJECT_ROOT, false);

      // Open the tree window on index.md — enterDirMode opens ROOT only, so the
      // sibling 'works' folder stays COLLAPSED.
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);

      const body = container.querySelector('.nav-body')!;
      expect(body.querySelectorAll('.seg[data-depth]').length).toBeGreaterThan(0);
      expect(registry.getById(fileTree.getActiveEntryId()!)?.path).toBe('index.md');

      // Reveal the "just created" home file inside the collapsed folder.
      fileTree.revealNewFile('works/draft.md');

      // Breadcrumb re-anchored + selection set: the active entry is the new file.
      expect(registry.getById(fileTree.getActiveEntryId()!)?.path).toBe('works/draft.md');
      // The new file is the trailing breadcrumb crumb (aria-current="page").
      const current = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
      expect(current?.dataset.path).toBe('works/draft.md');

      // Expansion NOT changed: 'works' stays COLLAPSED — its sibling child
      // note.md does NOT surface as a tree-body row, and the 'works' breadcrumb
      // crumb reports aria-expanded="false" (overrides never gained 'works').
      const noteRendered = Array.from(container.querySelectorAll<HTMLElement>('.nav-body .seg'))
        .some(el => el.dataset.fullName === 'note.md');
      expect(noteRendered).toBe(false);
      const worksCrumb = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc[data-path="works"]');
      expect(worksCrumb?.getAttribute('aria-expanded')).toBe('false');
    });

    test('leaves overrides untouched — an already-open sibling folder stays open across the reveal', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithCollapsibleFolder() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);

      // Manually OPEN 'blog' (a sibling of the target 'works').
      const blogSeg = Array.from(container.querySelectorAll('.seg:not(.leaf):not(.bc)'))
        .find(el => el.textContent?.trim().startsWith('blog')) as HTMLElement;
      (blogSeg.querySelector('.chev') as HTMLElement).click();
      await vi.waitFor(() => expect(container.textContent).toContain('post'));

      // Reveal a file inside the STILL-collapsed 'works'.
      fileTree.revealNewFile('works/draft.md');

      // overrides untouched: 'blog' stays OPEN (its child still renders) while
      // 'works' stays CLOSED (its child never renders).
      const postRendered = Array.from(container.querySelectorAll<HTMLElement>('.nav-body .seg'))
        .some(el => el.dataset.fullName === 'post.md');
      expect(postRendered).toBe(true);
      const noteRendered = Array.from(container.querySelectorAll<HTMLElement>('.nav-body .seg'))
        .some(el => el.dataset.fullName === 'note.md');
      expect(noteRendered).toBe(false);
    });
  });

  // ── Folder select (chevron-toggle / name-select split) ────────────
  describe('folder select (name zone)', () => {
    test('selectFolder expands in place — it does NOT open the home file', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', (e) =>
        fileSelected((e as CustomEvent).detail.filePath));
      const toggle = vi.fn();
      // @ts-expect-error — exercising a private method directly
      fileTree.selectFolder('docs', toggle);
      // Clicking a folder no longer navigates into / opens its home page.
      expect(fileSelected).not.toHaveBeenCalled();
      // It expands in place via the supplied toggle.
      expect(toggle).toHaveBeenCalled();
    });

    test('selectFolder without a home file does NOT dispatch folder-page-missing', async () => {
      // Plain tree folder clicks must ONLY expand/collapse + update selection.
      // The "create folder page" CTA comes exclusively from preview/URL navigation
      // (applyResolvedNode). Dispatching on a tree click caused the CTA to appear
      // on every plain folder expand, even when the user just wanted to browse.
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      const missing = vi.fn();
      container.addEventListener('editor:folder-page-missing', (e) =>
        missing((e as CustomEvent).detail));
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', fileSelected);
      // @ts-expect-error — private
      fileTree.selectFolder('notes');
      expect(fileSelected).not.toHaveBeenCalled();
      // folder-page-missing must NOT fire from a plain tree click.
      expect(missing).not.toHaveBeenCalled();
    });

    test('clicking a collapsed folder name EXPANDS it in place', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);                              // render sibling rows
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', (e) =>
        fileSelected((e as CustomEvent).detail.filePath));
      const row = findRowByPath(container, 'docs');          // collapsed folder row
      expect(row).toBeDefined();
      (row!.querySelector('.seg-name') as HTMLElement).click();
      // Name-click expands the folder (reveals children); it does NOT open a file.
      await vi.waitFor(() => {
        expect(findRowByPath(container, 'docs')!.classList.contains('open')).toBe(true);
      });
      expect(fileSelected).not.toHaveBeenCalled();
    });

    test('clicking the chevron toggles without selecting', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', fileSelected);
      const row = findRowByPath(container, 'docs')!;
      vi.useFakeTimers();
      (row.querySelector('.chev') as HTMLElement).click();
      vi.advanceTimersByTime(SETTLE_MS);
      vi.useRealTimers();
      expect(fileSelected).not.toHaveBeenCalled();           // chevron never opens a file
    });

    test('clicking an EXPANDED tree-body folder name COLLAPSES it (toggle in place)', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);
      // Expand docs in the tree body via its chevron, then click its name.
      const collapsed = findRowByPath(container, 'docs')!;
      (collapsed.querySelector('.chev') as HTMLElement).click();
      await vi.waitFor(() => {
        expect(findRowByPath(container, 'docs')!.classList.contains('open')).toBe(true);
      });
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', (e) =>
        fileSelected((e as CustomEvent).detail.filePath));
      const expanded = findRowByPath(container, 'docs')!;
      (expanded.querySelector('.seg-name') as HTMLElement).click();
      // Name-click on an already-open folder collapses it (toggle). No file opens.
      await vi.waitFor(() => {
        expect(findRowByPath(container, 'docs')!.classList.contains('open')).toBe(false);
      });
      expect(fileSelected).not.toHaveBeenCalled();
    });

    test('clicking a breadcrumb folder name toggles its sibling group (no file open)', async () => {
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/docs/guide.md`);   // breadcrumb: my-blog > docs > guide.md
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', (e) =>
        fileSelected((e as CustomEvent).detail.filePath));
      const seg = container.querySelector('.seg.bc[data-path="docs"] .seg-name') as HTMLElement;
      expect(seg).not.toBeNull();
      vi.useFakeTimers();
      seg.click();
      vi.advanceTimersByTime(SETTLE_MS);
      vi.useRealTimers();
      // Breadcrumb folder name-click expands/collapses in place — never opens a file.
      expect(fileSelected).not.toHaveBeenCalled();
    });
  });

  // ── Keyboard activate ('open') — Space expands a folder / opens a file ──
  // The ACTIVATE key (the 'open' KeyAction — bound to Space in the current
  // keymap; Enter is bound to rename) selects + expands a folder IN PLACE
  // (reveals its children), mirroring a name-zone click.
  // A file opens in the editor. ArrowRight/ArrowLeft also peek (expand/collapse).
  // Activate never changes the editor surface — the "create folder page" CTA
  // comes exclusively from preview/URL navigation.
  // The other activate cases — a home-less folder expanding without touching
  // the surface, a file opening, ArrowRight and Space both expanding — are
  // tree/__tests__/keyboard-integration.test.ts's.
  describe("keyboard activate ('open') on a folder with a home", () => {
    /** Dispatch a keydown on the document — the tree listens there. */
    function pressKey(key: string): void {
      document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    }
    /**
     * Move keyboard focus onto `targetId` by walking ArrowDown (synchronous, no
     * click-defer). Order-independent: it steps until focus lands on the target,
     * so the caller can assert focus deterministically before activating.
     */
    function focusEntry(targetId: number | null): void {
      for (let i = 0; i < 12; i++) {
        if (fileTree.getSelectionSnapshot().focus === targetId) return;
        pressKey('ArrowDown');
      }
    }

    test('activate on a selected folder WITH a home EXPANDS it, does NOT open the home', async () => {
      // treeWithFolders: docs/ has index.md (home) + guide.md.
      mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
      await fileTree.mount(container, PROJECT_ROOT, false);
      fileTree.highlight(`${PROJECT_ROOT}/index.md`);
      enterDirMode(fileTree);
      const fileSelected = vi.fn();
      container.addEventListener('editor:file-selected', (e) =>
        fileSelected((e as CustomEvent).detail.filePath));

      const docsId = registry.getByPath('docs');
      focusEntry(docsId);
      expect(fileTree.getSelectionSnapshot().focus).toBe(docsId);
      pressKey(' ');

      // Activate expands docs in place; opening the home page is a separate,
      // deliberate action (its inline home-file row), NOT a folder-activate.
      await vi.waitFor(() =>
        expect(findRowByPath(container, 'docs')!.classList.contains('open')).toBe(true));
      expect(fileSelected).not.toHaveBeenCalled();
    });

  });

  // ── Mount/Unmount ─────────────────────────────────────────────────

  // The pre-restructure 'mount creates nav-body wrapper with nav-grid inside'
  // and 'mount creates nav-grid element' tests asserted the old DOM shape
  // (.nav-tree-grid nested in .nav-body). Both are superseded by the new
  // 'LAYOUT INVARIANT: bc-strip and nav-body are children of the file-tree
  // container' test in the Layout invariant section below.

  test('mount shows only root in breadcrumb, not a guessed child', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    const segs = getSegTexts(container);
    expect(segs).toContain('my-blog');
    // Should NOT show any child — mount doesn't know which file is being viewed
    expect(segs).not.toContain('assets');
    expect(segs).not.toContain('posts');
    expect(segs).not.toContain('index.md');
  });

  test('mount calls listTree for project root', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    expect(mockListTree).toHaveBeenCalledWith(false);
  });

  test('refreshTree populates EntryRegistry with all entries', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Every path in defaultTree() should have been upserted into the registry
    // (project-root-relative; the root itself
    // is the empty string PROJECT_ROOT_REL).
    expect(registry.getByPath('')).not.toBeNull();
    expect(registry.getByPath('assets')).not.toBeNull();
    expect(registry.getByPath('posts')).not.toBeNull();
    expect(registry.getByPath('posts/article-one.md')).not.toBeNull();
    expect(registry.getByPath('posts/article-two.md')).not.toBeNull();
    expect(registry.getByPath('index.md')).not.toBeNull();
    expect(registry.getByPath('readme.txt')).not.toBeNull();
  });

  test('toggleInternal(true) passes showInternal=true to listTree', async () => {
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: PROJECT_ROOT });

    await fileTree.mount(container, PROJECT_ROOT, false);
    mockListTree.mockClear();

    await fileTree.toggleInternal(true);

    expect(mockListTree).toHaveBeenCalledWith(true);
  });

  // Helper: build a tree with `.moss` as the first root child (mirrors what
  // the backend returns when `show_internal=true`).
  function treeWithMossNode() {
    const t = defaultTree();
    t.children!.unshift(dir(`${PROJECT_ROOT}/.moss`, [
        file(`${PROJECT_ROOT}/.moss/config.toml`),
      ]));
    return t;
  }

  test('toggleInternal preserves current file when toggling internal visibility', async () => {
    // Backend returns .moss only when show_internal=true.
    mockListTree.mockImplementation(async (showInternal: boolean) =>
      showInternal
        ? { status: 'ok', data: treeWithMossNode() }
        : { status: 'ok', data: defaultTree() }
    );

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // Toggling internal ON should NOT disturb the current file — only add
    // `.moss` as a sibling of `posts/`.
    await fileTree.toggleInternal(true);

    const segs = getSegTexts(container);
    expect(segs).toContain('article-one.md');
    expect(segs).toContain('posts');
    expect(fileTree.isEditing()).toBe(false);
  });

  test('toggleInternal(true) surgically adds .moss as a sibling; toggleInternal(false) removes it', async () => {
    mockListTree.mockImplementation(async (showInternal: boolean) =>
      showInternal
        ? { status: 'ok', data: treeWithMossNode() }
        : { status: 'ok', data: defaultTree() }
    );

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // expand so root-level siblings render
    expect(container.textContent).not.toContain('.moss');

    // Toggle internal ON — .moss should appear; existing sibling group should stay.
    await fileTree.toggleInternal(true);
    expect(container.textContent).toContain('.moss');
    const segsWithMoss = getSegTexts(container);
    expect(segsWithMoss).toContain('article-one.md');

    // Toggle internal OFF — .moss disappears; other tree state is preserved.
    await fileTree.toggleInternal(false);
    expect(container.textContent).not.toContain('.moss');
    const segsAfter = getSegTexts(container);
    expect(segsAfter).toContain('article-one.md');
  });

  test('toggleInternal preserves expanded sibling state for non-.moss folders across toggle', async () => {
    mockListTree.mockImplementation(async (showInternal: boolean) =>
      showInternal
        ? { status: 'ok', data: treeWithMossNode() }
        : { status: 'ok', data: defaultTree() }
    );

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // enterDirMode opens root + posts/ (article-one's ancestor folder), so the
    // posts/ subtree — including article-two — is rendered as open. (Decoupled
    // open-forest model: ancestors of the active file are revealed; there is no
    // separate "expand the sibling group" step anymore.)
    await vi.waitFor(() => {
      expect(container.textContent).toContain('article-two');
    });

    const beforeToggle = container.textContent;

    await fileTree.toggleInternal(true);
    await fileTree.toggleInternal(false);

    // After a round-trip toggle, the non-.moss tree state must be identical —
    // posts/ stays open, article-two stays visible. `.moss` does not appear in
    // either `before` or after (mount and final state are both showInternal=false).
    expect(container.textContent).toBe(beforeToggle);
  });

  test('toggleInternal(false) while viewing a file inside .moss navigates back to root', async () => {
    mockListTree.mockImplementation(async (showInternal: boolean) =>
      showInternal
        ? { status: 'ok', data: treeWithMossNode() }
        : { status: 'ok', data: defaultTree() }
    );

    await fileTree.mount(container, PROJECT_ROOT, true);
    fileTree.highlight(`${PROJECT_ROOT}/.moss/config.toml`);
    expect(container.textContent).toContain('.moss');

    // Toggling internal OFF while inside `.moss/` — .moss and its descendants
    // must disappear from the tree, and the breadcrumb must re-home to root.
    await fileTree.toggleInternal(false);

    expect(container.textContent).not.toContain('.moss');
    const bcSegs = container.querySelectorAll('.seg.bc');
    expect(bcSegs.length).toBe(1);
    expect(bcSegs[0].textContent?.trim()).toBe('my-blog');
  });

  test('mount renders breadcrumb with project root name', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const segs = getSegTexts(container);
    expect(segs).toContain('my-blog');
  });

  test('mount starts file change listener', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // FileChanged is now on the typed bus; onMossEvent registers on "moss-event".
    expect(mockListen).toHaveBeenCalledWith(
      'moss-event',
      expect.any(Function),
    );
  });

  test('unmount cleans up container', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.unmount();
    expect(container.innerHTML).toBe('');
  });

  test('unmount can be called multiple times safely', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.unmount();
    fileTree.unmount();
  });

  test('unmount without mount does not throw', () => {
    fileTree.unmount();
  });

  // ── Breadcrumb strip + scrolling tree body ─────────────────────────

  test('breadcrumb segments live in .nav-bc-strip as direct .seg.bc children', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const strip = container.querySelector('.nav-bc-strip');
    expect(strip).not.toBeNull();
    const bcSegs = strip!.querySelectorAll('.seg.bc');
    expect(bcSegs.length).toBeGreaterThan(0);
    // Identity is stored in data-full-name (visible text strips .md).
    const fullNames = Array.from(bcSegs).map(
      s => (s as HTMLElement).dataset.fullName ?? s.textContent?.trim() ?? ''
    );
    expect(fullNames).toContain('my-blog');
    expect(fullNames).toContain('posts');
    expect(fullNames).toContain('article-one.md');
    // Breadcrumb segs are flex children of the strip — no grid placement.
    // They must NOT carry the tree-row indent attrs (--ft-offset / data-depth
    // are tree-body concepts; breadcrumb width comes from the allocator's inline
    // flex-basis instead).
    for (const seg of bcSegs) {
      const el = seg as HTMLElement;
      // Direct child of the strip
      expect(el.parentElement).toBe(strip);
      // No grid column assignment (fixed-indent model, not grid model)
      expect(el.style.gridColumn).toBeFalsy();
      // No tree-row indent attrs on breadcrumb segs
      expect(el.style.getPropertyValue('--ft-offset')).toBeFalsy();
      expect(el.getAttribute('data-depth')).toBeNull();
    }
  });

  test('breadcrumb tooltip appears ONLY on a truncated segment (BUG 16 refinement)', async () => {
    // BUG 16 refinement: data-tooltip surfaces a segment's FULL name on hover,
    // but ONLY when the segment is actually truncated (its label overflows into
    // an ellipsis). A fully-visible segment gets NO tooltip — echoing its own
    // visible name is noise. moss convention (.claude/CLAUDE.md → UI Conventions)
    // is `data-tooltip=`, never native `title=`.
    //
    // Truncation is a post-allocation LAYOUT fact (the spine allocator sizes the
    // segments after the render pass). jsdom reports 0 for scrollWidth/clientWidth,
    // so we stub those getters on the `.seg-name` text spans (the element that
    // ellipsizes — see file-tree.css) then re-run the truncation pass through the
    // resize re-allocation path (reallocateSpine), which is exactly what a panel/
    // divider resize drives in the real app.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const bcSegs = [...container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc')];
    expect(bcSegs.length).toBeGreaterThanOrEqual(2);

    // Build time sets NO tooltip anymore, and in jsdom (0 widths) nothing
    // truncates, so every segment starts with no tooltip and no native title=.
    for (const seg of bcSegs) {
      expect(seg.dataset.tooltip).toBeUndefined();
      expect(seg.hasAttribute('title')).toBe(false);
    }

    // Stub each `.seg-name`'s box. Getters shadow jsdom's 0-returning prototype
    // getters. Truncated iff scrollWidth - clientWidth > 1 (1px tolerance).
    const stubWidths = (seg: HTMLElement, scrollW: number, clientW: number) => {
      const textEl = seg.querySelector<HTMLElement>('.seg-name')!;
      Object.defineProperty(textEl, 'scrollWidth', { configurable: true, get: () => scrollW });
      Object.defineProperty(textEl, 'clientWidth', { configurable: true, get: () => clientW });
    };
    const byName = (n: string) => bcSegs.find(s => s.dataset.fullName === n)!;
    const root = byName('my-blog');    // fits → NOT truncated
    const posts = byName('posts');     // overflows → truncated (ancestor)
    const leaf = byName('article-one.md'); // overflows → truncated (leaf, keeps .md)
    stubWidths(root, 40, 60);
    stubWidths(posts, 200, 50);
    stubWidths(leaf, 260, 48);

    // Re-run the truncation pass via the resize re-alloc path. applySpineAllocation
    // no-ops in jsdom (0-width panel), but syncBreadcrumbTooltips still runs.
    priv<{ render: { reallocateSpine(): void } }>(fileTree).render.reallocateSpine();

    // Truncated ancestor: tooltip === its full name; native title= never used.
    expect(posts.dataset.tooltip).toBe('posts');
    expect(posts.hasAttribute('title')).toBe(false);

    // Truncated leaf: tooltip carries the FULL name WITH extension, even though
    // the visible text strips ".md".
    expect(leaf.dataset.tooltip).toBe('article-one.md');
    expect(leaf.textContent).not.toContain('.md');
    expect(leaf.hasAttribute('title')).toBe(false);

    // Fully-visible segment: NO tooltip attribute at all.
    expect(root.dataset.tooltip).toBeUndefined();
  });

  test('breadcrumb tooltip is REMOVED when a resize makes a segment fit (BUG 16)', async () => {
    // The inverse of the truncation check: a segment that WAS truncated (tooltip
    // present) must lose its data-tooltip once a resize widens it enough to fit.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const bcSegs = [...container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc')];
    const posts = bcSegs.find(s => s.dataset.fullName === 'posts')!;
    const textEl = posts.querySelector<HTMLElement>('.seg-name')!;

    // First: truncated → tooltip appears.
    Object.defineProperty(textEl, 'scrollWidth', { configurable: true, get: () => 200 });
    Object.defineProperty(textEl, 'clientWidth', { configurable: true, get: () => 50 });
    priv<{ render: { reallocateSpine(): void } }>(fileTree).render.reallocateSpine();
    expect(posts.dataset.tooltip).toBe('posts');

    // Then a resize gives it room → not truncated → tooltip removed.
    Object.defineProperty(textEl, 'clientWidth', { configurable: true, get: () => 200 });
    priv<{ render: { reallocateSpine(): void } }>(fileTree).render.reallocateSpine();
    expect(posts.dataset.tooltip).toBeUndefined();
  });

  test('truncated NON-.md leaf (trailing .ext-hint) still gets a tooltip (BUG 16)', async () => {
    // A non-.md file leaf renders a trailing `.ext-hint` span AFTER `.seg-name`,
    // so `.ext-hint` becomes span:last-child. The truncation check measures the
    // `.seg-name` text span (which carries the label + ellipsis via the dedicated
    // `.seg > .seg-name` rule in file-tree.css — NOT only the last-child rule the
    // ext-hint would otherwise steal), so a truncated non-md leaf must still get
    // its full-name tooltip.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/readme.txt`);

    const bcSegs = [...container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc')];
    const leaf = bcSegs.find(s => s.dataset.fullName === 'readme.txt')!;
    expect(leaf).toBeDefined();

    // The leaf carries a visible `.ext-hint` (".txt") after the `.seg-name`
    // ("readme"): the label clips, the extension stays.
    const textEl = leaf.querySelector<HTMLElement>('.seg-name')!;
    const extHint = leaf.querySelector<HTMLElement>('.ext-hint');
    expect(textEl.textContent).toBe('readme');
    expect(extHint?.textContent).toBe('.txt');

    // Stub the LABEL span (not the ext-hint) as overflowing → truncated.
    Object.defineProperty(textEl, 'scrollWidth', { configurable: true, get: () => 220 });
    Object.defineProperty(textEl, 'clientWidth', { configurable: true, get: () => 40 });
    priv<{ render: { reallocateSpine(): void } }>(fileTree).render.reallocateSpine();

    // Tooltip carries the FULL name WITH extension (existing leaf convention).
    expect(leaf.dataset.tooltip).toBe('readme.txt');
    expect(leaf.hasAttribute('title')).toBe(false);
  });

  test('breadcrumb hides .md extension in visible text', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const bcSegs = container.querySelectorAll('.nav-bc-strip .seg.bc');
    const visibleText = Array.from(bcSegs)
      .map(s => s.textContent?.trim() ?? '')
      .join('|');
    // Breadcrumb shows "article-one" not "article-one.md".
    expect(visibleText).not.toContain('.md');
  });

  test('LAYOUT INVARIANT: last breadcrumb segment carries bc-current class (semantic current marker)', async () => {
    // The last segment is tagged `bc-current` as a semantic "you are here"
    // marker (used for aria-current and the just-committed flash). Under the
    // adaptive-spine model its width — like every segment's — is the allocator's
    // inline flex-basis; there is no longer a `.bc-current { flex: 1 1 auto }`
    // CSS override (JS owns all N widths so the JS offset stays the single
    // source of truth and the spine cannot shear).
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const bcSegs = container.querySelectorAll<HTMLElement>('.seg.bc');
    expect(bcSegs.length).toBeGreaterThanOrEqual(2);

    const last = bcSegs[bcSegs.length - 1];
    expect(last.classList.contains('bc-current')).toBe(true);

    // Ancestors must NOT carry bc-current — they should remain truncatable.
    for (let i = 0; i < bcSegs.length - 1; i++) {
      expect(bcSegs[i].classList.contains('bc-current')).toBe(false);
    }
  });

  test('tree body entries are placed in .nav-body with data-depth + --ft-offset for indentation', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const navBody = container.querySelector('.nav-body');
    expect(navBody).not.toBeNull();

    const bodySegs = navBody!.querySelectorAll('.seg:not(.bc)');
    expect(bodySegs.length).toBeGreaterThan(0);

    // Sibling entries should be present (file rows store full name on dataset).
    const allNames = Array.from(bodySegs).map(
      s => (s as HTMLElement).dataset.fullName ?? s.textContent?.trim()
    );
    expect(allNames).toContain('article-two.md');

    // Adaptive-spine model: each row carries data-depth (its depth index) and
    // --ft-offset (the px inline-start offset O[depth] from the allocator).
    // In jsdom (0-width panel) the allocator no-ops and offsetAt falls back to
    // depth*--ft-indent, but data-depth + --ft-offset are still set by place().
    for (const seg of bodySegs) {
      const el = seg as HTMLElement;
      const depth = el.getAttribute('data-depth');
      // data-depth guards that place() ran; non-negative integer string.
      expect(depth).toBeTruthy();
      expect(parseInt(depth!, 10)).toBeGreaterThanOrEqual(0);
      // --ft-offset is a px value (e.g. '0px','64px',...).
      expect(el.style.getPropertyValue('--ft-offset')).toMatch(/px$/);
      // No grid column placement in the fixed-indent model.
      expect(el.style.gridColumn).toBeFalsy();
    }
  });

  // ── Layout invariant — DO NOT REGRESS ────────────────────────────
  //
  // Visual tree hierarchy depends on tree-row[d] carrying data-depth = d.
  // The jsdom-level spine check below verifies the structural invariant.
  // Exact pixel alignment of breadcrumb-seg[d] over tree-row[d] is verified
  // by the Playwright WebKit layout test (file-tree-layout.spec.ts).

  test('LAYOUT INVARIANT: tree-row[d] carries data-depth d (spine alignment; pixel check in playwright)', async () => {
    // Adaptive-spine model: visual alignment between breadcrumb segment[d] and
    // tree-body rows at depth d is expressed via data-depth + the shared
    // --ft-offset array O[d], NOT via grid columns. The breadcrumb strip uses
    // allocator-assigned flex widths; tree rows use
    // padding-inline-start = --ft-tl + --ft-base + --ft-offset(O[d]).
    // Pixel-level alignment at the allocated widths is verified by the
    // Playwright WebKit test (file-tree-layout.spec.ts); this test guards the
    // structural invariant that data-depth is set correctly per row.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const bcSegs = container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc');
    // activePath = [my-blog, posts, article-one.md] → 3 segments
    expect(bcSegs.length).toBeGreaterThanOrEqual(2);

    // For the active path the tree-body must contain rows at depth 1 (the
    // posts/ folder's siblings) and depth 2 (files inside posts/). Verify
    // that each row carries data-depth equal to its depth index.
    const bodySegs = Array.from(
      container.querySelectorAll<HTMLElement>('.nav-body .seg:not(.bc)')
    );
    expect(bodySegs.length).toBeGreaterThan(0);

    // Depth 1 rows exist (children of root, e.g. posts folder itself)
    const depthOneRows = bodySegs.filter(
      s => s.getAttribute('data-depth') === '1'
    );
    expect(depthOneRows.length).toBeGreaterThan(0);

    // The breadcrumb has a segment at index 1 (depth 1 ancestor = posts)
    expect(bcSegs[1]).toBeTruthy();

    // No body seg should have a gridColumn set (fixed-indent, not grid)
    for (const seg of bodySegs) {
      expect(seg.style.gridColumn).toBeFalsy();
    }
  });

  // The debounce and its WKWebView stall watchdog are proved in
  // tree/__tests__/spine.test.ts, where the record that owns them lives. What
  // has to be proved HERE is the wiring: unmount reaches the spine's teardown,
  // so nothing it scheduled fires against a torn-down tree.
  test('unmount stops a reallocation the tree had already scheduled', async () => {
    vi.useFakeTimers();
    try {
      await fileTree.mount(container, PROJECT_ROOT, false);
      const render = priv(fileTree).render as unknown as { reallocateSpine: () => void; spine: { schedule: () => void } };
      const spy = vi.spyOn(render, 'reallocateSpine').mockImplementation(() => {});
      render.spine.schedule();
      fileTree.unmount();
      vi.advanceTimersByTime(10_000);
      expect(spy).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  // jsdom does not load @import'd stylesheets, so getComputedStyle returns
  // 'static' regardless of the .seg.bc rule's actual position value (false
  // green). Read the CSS source directly, as Breadcrumb positioning tests in
  // theme-tokens.test.ts already do. The new file-tree.css MUST NOT declare
  // position: sticky on .seg.bc — the strip doesn't scroll, so pinning the
  // seg is structurally unnecessary, and any sticky declaration would
  // re-introduce the 10 px first-scroll snap.
  test('LAYOUT INVARIANT: .seg.bc has no position: sticky (flex-column restructure)', () => {
    const fs = require('fs');
    const path = require('path');
    const css = fs.readFileSync(
      path.resolve(__dirname, '../../file-tree.css'),
      'utf8'
    );
    const segBcRules = css.match(/\.seg\.bc[\s\S]*?\{[\s\S]*?\}/g) ?? [];
    for (const rule of segBcRules) {
      expect(rule).not.toMatch(/position:\s*sticky/);
    }
  });

  test('LAYOUT INVARIANT: bc-strip and nav-body are children of the file-tree container', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const bcStrip = container.querySelector<HTMLElement>('.nav-bc-strip');
    const navBody = container.querySelector<HTMLElement>('.nav-body');

    expect(bcStrip).not.toBeNull();
    expect(navBody).not.toBeNull();
    expect(bcStrip!.parentElement).toBe(container);
    expect(navBody!.parentElement).toBe(container);
  });

  // ── Highlight / Navigation ────────────────────────────────────────

  test('highlight builds breadcrumb from root to file', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const segs = getSegTexts(container);
    expect(segs).toContain('my-blog');
    expect(segs).toContain('posts');
    expect(segs).toContain('article-one.md');
  });

  test('highlight does not call listTree again (tree already loaded)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const callCount = mockListTree.mock.calls.length;
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    expect(mockListTree.mock.calls.length).toBe(callCount);
  });

  test('highlight shows file as leaf segment', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    // .md extension is hidden in display; identity comes from data-full-name.
    const leafIds = Array.from(leafSegs).map(
      el => (el as HTMLElement).dataset.fullName ?? el.textContent?.trim()
    );
    expect(leafIds).toContain('index.md');
  });

  test('highlight shows folders with chevrons', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // posts folder should have a chevron
    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const hasChevron = Array.from(folderSegs).some(
      el => el.querySelector('.chev') !== null && el.textContent?.includes('posts'),
    );
    expect(hasChevron).toBe(true);
  });

  // ── Active file always shows in breadcrumb ────────────────────────

  test('the active file appears in the breadcrumb (canonical state)', async () => {
    // The breadcrumb always renders the full active path. The editor heading
    // element mirrors the trailing file segment but does not hide it from
    // the breadcrumb (avoids the navigation/divider regressions caused by
    // the removed `headingVisible` trim path).
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const bcSegs = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .map(el => el.dataset.fullName ?? el.textContent?.trim() ?? '');
    expect(bcSegs).toContain('my-blog');
    expect(bcSegs).toContain('posts');
    expect(bcSegs).toContain('article-one.md');
  });

  // ── Integrated-spine invariant: every node appears EXACTLY once ───────

  test('INVARIANT: an active-path node never appears in the BODY (it lives only in the breadcrumb)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // opens root + the active ancestor `posts`

    // Body rows (excludes the .bc breadcrumb segments).
    const bodyPaths = Array.from(
      container.querySelectorAll<HTMLElement>('.nav-body .seg[data-path]:not(.bc)'),
    ).map(el => el.dataset.path);

    // The active LEAF must NOT be a body row — it is the breadcrumb leaf.
    expect(bodyPaths).not.toContain('posts/article-one.md');
    // The active ANCESTOR folder must NOT be a body row — it is a breadcrumb crumb.
    expect(bodyPaths).not.toContain('posts');

    // But the ancestor's NON-active children DO surface in the body, at the depth
    // they'd occupy under `posts` (depth 2), since `posts` is open.
    const articleTwo = container.querySelector<HTMLElement>(
      '.nav-body .seg[data-path="posts/article-two.md"]:not(.bc)',
    );
    expect(articleTwo).toBeTruthy();
    expect(articleTwo!.getAttribute('data-depth')).toBe('2');

    // Both nodes still appear in the breadcrumb (the spine), exactly once each.
    const bcPaths = Array.from(
      container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc[data-path]'),
    ).map(el => el.dataset.path);
    expect(bcPaths).toContain('posts');
    expect(bcPaths).toContain('posts/article-one.md');
  });

  test('BODY ORDER: deepest-first — the active file’s siblings render BEFORE the root level', async () => {
    // active = posts/article-one.md; navigating opens root + posts (deepest-first
    // body). The body DOM order must be:
    //   posts/article-two.md (depth 2 — the active file's sibling, under the bc),
    //   then root's non-active children (depth 1): assets, index.md, readme.txt.
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Navigation alone keeps the window closed (sticky toggle); this
    // test exercises the OPEN forest's render order, so open it explicitly with
    // enterDirMode (opens root + the active ancestors — the same forest).
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const rows = Array.from(
      container.querySelectorAll<HTMLElement>('.nav-body .seg[data-path]:not(.bc)'),
    ).map(el => ({ path: el.dataset.path, depth: el.getAttribute('data-depth') }));

    // Exact order + depths (deepest ancestor's children first, then root level).
    expect(rows).toEqual([
      { path: 'posts/article-two.md', depth: '2' },
      { path: 'assets', depth: '1' },
      { path: 'index.md', depth: '1' },
      { path: 'readme.txt', depth: '1' },
    ]);

    // Neither active-path node appears in the body (they live in the breadcrumb).
    const bodyPaths = rows.map(r => r.path);
    expect(bodyPaths).not.toContain('posts'); // active ancestor → breadcrumb crumb
    expect(bodyPaths).not.toContain('posts/article-one.md'); // active leaf → bc leaf
  });

  // ── Drag Handle ───────────────────────────────────────────────────

  test('setupDragHandle sets wrapper height from snap state', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);

    // Should have a height set (snap state 0 navH)
    expect(wrapper.style.height).toMatch(/^\d+px$/);
  });

  test('drag preserves manual overrides (bug #2 fix)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);

    enterDirMode(fileTree);  // max snap level → root siblings visible

    // Manually expand posts (the click defers via the dblclick window).
    const postsSeg = Array.from(
      container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'),
    ).find(el => el.textContent?.includes('posts'));
    expect(postsSeg).toBeDefined();
    (postsSeg!.querySelector(".chev") as HTMLElement).click();
    await vi.waitFor(() => {
      expect(getSegTexts(container)).toContain('article-one.md');
    });

    // Capture overrides state immediately before the drag.
    expect(overrideAt(fileTree, 'posts')).toBe('open');

    // Synthesize a divider drag with a tiny movement.
    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0 }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 1 }));
    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1 }));

    // Manual expansion must survive the drag. We inspect controller state
    // directly because the DOM only reflects state at the next render call,
    // which a tiny-movement pointermove may skip — the bug is in the state
    // store, not in render. (In production it surfaces via the very next
    // file-watcher or click that triggers a render.)
    expect(overrideAt(fileTree, 'posts')).toBe('open');
  });

  test('drag max bound includes manual override rows', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);

    const chipBar = document.createElement('div');
    chipBar.id = 'chip-bar';
    const divider = document.createElement('div');
    divider.id = 'divider';
    document.body.appendChild(chipBar);
    document.body.appendChild(divider);

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
    enterDirMode(fileTree);

    // Capture the baseline content height with the active-path forest open but
    // BEFORE any manual sibling expansion. enterDirMode() ran applyWrapperHeight,
    // so wrapper.style.height is the content-fit height for that forest.
    const baselineNavH = parseInt(wrapper.style.height, 10);

    // Manually expand posts — adds 2 row-heights to the true content height,
    // growing the panel above the pre-expansion baseline.
    const postsSeg = Array.from(
      container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'),
    ).find(el => el.textContent?.includes('posts'));
    (postsSeg!.querySelector(".chev") as HTMLElement).click();
    await vi.waitFor(() => {
      expect(getSegTexts(container)).toContain('article-one.md');
    });

    // Drag down hard. With the OLD bound (snap-only navH), pointermove's
    // targetH would clamp at the pre-expansion baseline; with the fix, it grows
    // to include the manually-expanded rows. We check style.height *during* the
    // drag (between pointermove and pointerup) — pointerup calls applyWrapperHeight
    // which fits to the rendered row count and would mask the drag-time clamp.
    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0 }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 1000 }));

    const heightDuringDrag = parseInt(wrapper.style.height, 10);
    expect(heightDuringDrag).toBeGreaterThan(baselineNavH);

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1 }));
    chipBar.remove();
    divider.remove();
  });

  test('overrides for deleted folders are pruned after refresh', async () => {
    let listenCb: ((payload: unknown) => void) | null = null;
    mockListen.mockImplementation(async (_event: string, cb: (payload: unknown) => void) => {
      listenCb = cb;
      return vi.fn();
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Click posts to add an 'open' override.
    const postsSeg = Array.from(
      container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'),
    ).find(el => el.textContent?.includes('posts'));
    (postsSeg!.querySelector(".chev") as HTMLElement).click();
    await vi.waitFor(() => expect(overrideAt(fileTree, 'posts')).toBe('open'));
    // The override is keyed by id; hold the id so the assertion below can ask
    // about it after the path has stopped resolving.
    const postsId = registry.getByPath('posts')!;

    // Simulate the next watcher refresh: posts has been deleted on disk.
    const treeWithoutPosts = defaultTree();
    treeWithoutPosts.children = treeWithoutPosts.children!.filter(c => c.name !== 'posts');
    mockListTree.mockResolvedValue({ status: 'ok', data: treeWithoutPosts });

    expect(listenCb).not.toBeNull();
    listenCb!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });
    await new Promise(r => setTimeout(r, 0));

    // The override for the now-deleted folder must be gone.
    expect(priv(fileTree).view.overrides.has(postsId)).toBe(false);
  });

  test('navigating while the window is OPEN keeps it open and FREEZES the anchor (no reshape)', async () => {
    // Frozen-anchor model: opening a file while the window is open must NOT
    // reshape the forest or re-point the breadcrumb — the layout stays stable.
    mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
    await fileTree.mount(container, PROJECT_ROOT, false);

    // Open the window on docs/ (explicit gesture: enterDirMode opens the active
    // ancestors).
    fileTree.highlight(`${PROJECT_ROOT}/docs/guide.md`);
    enterDirMode(fileTree);
    const rowsBefore = container.querySelectorAll('.nav-body .seg').length;
    expect(rowsBefore).toBeGreaterThan(0);
    const anchorBefore = [...priv(fileTree).view.anchor];

    // Navigate to a different subtree → window stays OPEN, but the anchor is
    // FROZEN: `notes` is NOT opened and the breadcrumb path does not move.
    fileTree.highlight(`${PROJECT_ROOT}/notes/todo.md`);
    expect(container.querySelectorAll('.nav-body .seg').length).toBe(rowsBefore);
    expect(overrideAt(fileTree, 'notes')).toBeUndefined();
    expect(priv(fileTree).view.anchor).toEqual(anchorBefore);
  });

  // ── Frozen breadcrumb anchor (stable layout while expanded) ──
  // While the window is OPEN, opening a file must NOT re-anchor the breadcrumb
  // or reshape the forest — the layout stays stable. The open file is shown as a
  // highlighted BODY row. The breadcrumb only becomes the open file when the
  // sash parks at minimum height (parkAtMinHeight / isAtMinHeight).

  test('opening a file while the window is OPEN keeps the breadcrumb frozen and the layout stable', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Open `assets` too, so we can prove unrelated overrides are preserved.
    const assetsSeg = Array.from(
      container.querySelectorAll<HTMLElement>('.seg:not(.leaf)'),
    ).find(el => el.textContent?.includes('assets'));
    (assetsSeg!.querySelector('.chev') as HTMLElement).click();
    await vi.waitFor(() => {
      expect(overrideAt(fileTree, 'assets')).toBe('open');
    });

    // Snapshot the stable-layout signals BEFORE opening a file.
    const anchorBefore = [...priv(fileTree).view.anchor];
    const overridesBefore = [
      ...priv(fileTree).view.overrides.entries(),
    ];
    const rowsBefore = container.querySelectorAll('.nav-body .seg').length;

    // Open a file under the still-closed `posts` folder (editor round-trip).
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // FROZEN ANCHOR: the breadcrumb path is unchanged.
    expect(priv(fileTree).view.anchor).toEqual(anchorBefore);
    // No forest reshape: `posts` NOT opened, `assets` still open, map identical.
    expect(overrideAt(fileTree, 'posts')).toBeUndefined();
    expect(overrideAt(fileTree, 'assets')).toBe('open');
    expect([...priv(fileTree).view.overrides.entries()]).toEqual(overridesBefore);
    // STABLE LAYOUT: the rendered row count did not change.
    expect(container.querySelectorAll('.nav-body .seg').length).toBe(rowsBefore);
  });

  // ── forceAnchor: external navigation overrides the frozen anchor ──
  // The preview webview navigating to a different page (or any other trigger
  // OUTSIDE the tree) syncs back into the editor via loadFile → highlight(path,
  // {forceAnchor: true}). That is a bigger shift in attention than browsing a
  // sibling inside the currently-open folder, so — unlike the frozen-anchor
  // default above — the breadcrumb MUST re-anchor immediately, even while the
  // tree is expanded. The rule is stated in
  // The highlight() docstring.

  test('forceAnchor re-anchors the breadcrumb even while the window is OPEN (external navigation)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Simulate an external navigation (preview-nav syncing back into the
    // editor) to a DIFFERENT file — forceAnchor re-points the breadcrumb even
    // though the window is open.
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`, { forceAnchor: true });

    const activePath = priv(fileTree).view.anchor;
    expect(activePath[activePath.length - 1]).toBe('posts/article-one.md');
  });

  test('regression: an in-tree click (no forceAnchor) still freezes the anchor while OPEN', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Same navigation as above, but WITHOUT forceAnchor (the in-tree-click
    // shape) — the anchor must stay frozen, unchanged from before this feature.
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const activePath = priv(fileTree).view.anchor;
    expect(activePath[activePath.length - 1]).toBe('index.md');
  });

  test('opening a visible sibling while OPEN highlights its body row without promoting it to the breadcrumb', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // posts open → article-two.md renders as a body row

    // Open the sibling, as the editor's loadFile would after a row click.
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);

    // The breadcrumb leaf is still the FROZEN anchor (article-one), not the
    // newly-opened file.
    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    expect(bcLeaf?.dataset.fullName).toBe('article-one.md');

    // The opened sibling stays a BODY row (never promoted to a .bc segment) and
    // is marked selected (the in-place highlight).
    const twoRow = container.querySelector<HTMLElement>(
      '.nav-body .seg[data-path="posts/article-two.md"]:not(.bc)',
    );
    expect(twoRow).toBeTruthy();
    expect(twoRow!.classList.contains('selected')).toBe(true);
    // ...and it was NOT promoted into the breadcrumb strip (the negative).
    expect(
      container.querySelector('.nav-bc-strip .seg.bc[data-path="posts/article-two.md"]'),
    ).toBeNull();
  });

  // The frozen anchor can diverge from the active file
  // while the window is open (anchor = article-one, active = article-two,
  // per the two tests above), and clicking back to the anchor must still act,
  // though the breadcrumb leaf's isLast branch is where a `click` listener is
  // easiest to leave out. Compare the diverged case (this test) against the identical
  // no-op case ("clicking the anchor breadcrumb segment when NOT diverged
  // is a no-op" below).
  test('clicking the diverged anchor breadcrumb segment re-opens it', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`); // closed → anchor = article-one
    enterDirMode(fileTree); // open

    // Diverge: open a different file while expanded (frozen-anchor model) —
    // anchor stays article-one, active file becomes article-two.
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`);

    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    expect(bcLeaf?.dataset.fullName).toBe('article-one.md');

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    bcLeaf!.click();

    await vi.waitFor(() => expect(listener).toHaveBeenCalledTimes(1));
    const event = listener.mock.calls[0][0] as CustomEvent;
    expect(event.detail.filePath).toBe('posts/article-one.md');
  });

  test('clicking the anchor breadcrumb segment when NOT diverged is a no-op', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // No divergence: anchor === active file (article-one).
    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    expect(bcLeaf?.dataset.fullName).toBe('article-one.md');

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    vi.useFakeTimers();
    bcLeaf!.click();
    vi.advanceTimersByTime(SETTLE_MS);
    vi.useRealTimers();

    expect(listener).not.toHaveBeenCalled();
  });

  test('double-clicking the diverged anchor breadcrumb segment still triggers rename', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-two.md`); // diverge anchor from active

    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    expect(bcLeaf?.dataset.fullName).toBe('article-one.md');

    startRenameByKey(bcLeaf!);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    expect(input).not.toBeNull();
    // Input is scoped to the stem of the ANCHOR (article-one), not the active
    // file (article-two) — rename targets whatever segment was double-clicked.
    expect(input.value).toBe('article-one');
  });

  // ── File Selection ────────────────────────────────────────────────

  test('clicking file seg dispatches editor:file-selected', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // Expand to show siblings
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // Find article-two.md leaf and click it
    const leafSegs = container.querySelectorAll('.seg.leaf');
    const articleTwo = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;

    expect(articleTwo).not.toBeNull();
    articleTwo.click();
    // Click handler defers the file-selected dispatch by the dblclick window
    // (so a follow-up dblclick can preempt it and trigger rename). Wait for
    // the natural timer to fire.
    await vi.waitFor(() => expect(listener).toHaveBeenCalledTimes(1));
    const event = listener.mock.calls[0][0] as CustomEvent;
    expect(event.detail.filePath).toBe('posts/article-two.md');
  });

  // ── Toggle (click folder in breadcrumb) ───────────────────────────

  test('clicking the active-ancestor breadcrumb chevron toggles its sibling visibility', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Navigation keeps the window closed (sticky toggle); open it
    // explicitly with enterDirMode to reach the steady state this test toggles.
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // With `posts` open, the active file's sibling `article-two.md` shows
    // directly under the breadcrumb.
    expect(getSegTexts(container)).toContain('article-two.md');

    // Click the posts breadcrumb chevron to COLLAPSE that level → siblings hide.
    const findPostsBcChev = () =>
      (Array.from(container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg.bc'))
        .find(el => el.textContent?.includes('posts'))
        ?.querySelector('.chev')) as HTMLElement;
    findPostsBcChev().click();
    await vi.waitFor(() => {
      expect(getSegTexts(container)).not.toContain('article-two.md');
    });

    // Click it again to RE-EXPAND → siblings reappear.
    findPostsBcChev().click();
    await vi.waitFor(() => {
      expect(getSegTexts(container)).toContain('article-two.md');
    });
  });

  // ── Expand into directory ─────────────────────────────────────────

  test('clicking sibling folder toggles it synchronously (no lazy load)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // posts should be visible as a collapsed folder
    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const postsSeg = Array.from(folderSegs).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;

    expect(postsSeg).not.toBeNull();

    // Click posts to toggle — no additional listTree call needed
    const callCount = mockListTree.mock.calls.length;
    (postsSeg.querySelector(".chev") as HTMLElement).click();
    expect(mockListTree.mock.calls.length).toBe(callCount);
  });

  // ── Folder click on edge cases ────────────────────────────────────

  test('clicking a home-less empty directory does not crash', async () => {
    // Tree with an empty directory
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: dir(PROJECT_ROOT, [
          dir(`${PROJECT_ROOT}/empty-dir`, []),
          file(`${PROJECT_ROOT}/index.md`),
        ], { name: 'my-blog' }),
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    // Find empty-dir and click to expand into it (toggle makes it appear as mini-breadcrumb)
    // First, click the root folder breadcrumb to show siblings
    const folderSegs = container.querySelectorAll('.seg:not(.leaf)');
    const emptyDir = Array.from(folderSegs).find(
      el => el.textContent?.includes('empty-dir'),
    ) as HTMLElement;

    // Clicking empty-dir should not throw
    expect(() => emptyDir?.click()).not.toThrow();
  });

  test('clicking the root folder name does not append a fake child segment', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    // After mount, activePath is [projectRoot]. Clicking the root NAME selects
    // the folder (opens its home file / offers create-page); it must not
    // append a guessed child to the breadcrumb.
    const rootSeg = Array.from(container.querySelectorAll('.seg:not(.leaf)')).find(
      el => el.textContent?.includes('my-blog'),
    ) as HTMLElement;

    expect(rootSeg).not.toBeNull();
    vi.useFakeTimers();
    (rootSeg.querySelector('.seg-name') as HTMLElement).click(); // name zone = select

    // Breadcrumb should still show only 'my-blog' as the single segment and
    // NOT have appended a child like 'assets' or 'posts'. Folder click is
    // deferred by the dblclick window — run past it so the action lands.
    vi.advanceTimersByTime(SETTLE_MS);
    vi.useRealTimers();
    const segs = getSegTexts(container);
    // The breadcrumb row should contain my-blog
    expect(segs).toContain('my-blog');
    // Siblings should now be visible (expanded to max snap level)
    // but the breadcrumb path itself should end at my-blog
    // Check that no child appears as a breadcrumb segment (bc class = breadcrumb)
    const bcSegs = Array.from(container.querySelectorAll('.nav-bc-strip .seg.bc'));
    expect(bcSegs).toHaveLength(1);
    expect(bcSegs[0].textContent?.trim()).toBe('my-blog');
  });

  // ── Error Handling ────────────────────────────────────────────────

  test('mount handles listTree failure gracefully', async () => {
    mockListTree.mockResolvedValue({ status: 'error', error: 'Permission denied' });
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Should not throw, .nav-body should exist (created by mount before any
    // failure-prone work). The container itself (.nav-grid) is passed in by
    // the caller and exists regardless of mount success.
    expect(container.querySelector('.nav-body')).not.toBeNull();
  });

  // ── File Watcher ───────────────────────────────────────────────────

  test('FileChanged event triggers full re-scan', async () => {
    let listenCallback: Function | null = null;
    mockListen.mockImplementation(async (_event: string, cb: Function) => {
      listenCallback = cb;
      return vi.fn();
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    expect(listenCallback).not.toBeNull();

    const initialCallCount = mockListTree.mock.calls.length;

    // Fire a FileChanged-shaped envelope; the kind filter inside onMossEvent
    // dispatches it to the file-tree handler.
    listenCallback!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });

    await vi.waitFor(() => {
      expect(mockListTree.mock.calls.length).toBeGreaterThan(initialCallCount);
    });
  });

  // ── Rename ─────────────────────────────────────────────────────────

  test('rename cancels on Escape key', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const leafSegs = container.querySelectorAll('.seg.leaf');
    const target = Array.from(leafSegs).find(
      el => (el as HTMLElement).dataset.fullName === 'article-two.md',
    ) as HTMLElement;

    expect(target).not.toBeNull();
    startRenameByKey(target);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    input.value = 'changed.md';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(mockRenameEntry).not.toHaveBeenCalled();
    expect(container.querySelector('.moss-tree-rename-input')).toBeNull();
  });

  test('double-click on breadcrumb file seg starts inline rename', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    // data-path is project-root-relative.
    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .find(el => el.dataset.path === 'posts/article-one.md');

    expect(target).toBeTruthy();
    // The leaf IS the open file: the first press is a no-op by design and
    // the second (detail 2) renames — neither re-opens it.
    target!.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    target!.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 2 }));

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    expect(input).not.toBeNull();
    // Input is scoped to the stem; .md is shown as a non-editable suffix.
    expect(input.value).toBe('article-one');
    expect(listener).not.toHaveBeenCalled();
  });

  test('Rename key on breadcrumb folder seg starts inline rename', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .find(el => el.dataset.path === 'posts');

    expect(target).toBeTruthy();
    startRenameByKey(target!);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    expect(input).not.toBeNull();
    expect(input.value).toBe('posts');
  });

  test('Rename key on project root breadcrumb seg is a no-op', async () => {
    // The project root has a record but no name to edit, so startRename
    // declines it. The right-click context menu omits "Rename" on root
    // entirely (see separate test below); this key path is the fallback guard.
    // The root segment carries the empty-string data-path (PROJECT_ROOT_REL).
    await fileTree.mount(container, PROJECT_ROOT, false);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.bc'))
      .find(el => el.dataset.path === '');

    expect(target).toBeTruthy();
    startRenameByKey(target!);

    expect(container.querySelector('.moss-tree-rename-input')).toBeNull();
  });

  // The prior 'Scrollbar / has-overflow class' describe block tested a class
  // that was toggled by applyWrapperHeight to gate .nav-body's overflow-y.
  // The class is no longer toggled — .nav-body has overflow-y: auto
  // unconditionally now that the breadcrumb moved into .nav-bc-strip (outside
  // the scrollport). Snap-level-0 scrollbar absence is verified live (no
  // tree-row content → scrollHeight === clientHeight → no scrollbar).

  // ── Drag Handle Floor ────────────────────────────────────────────

  test('wrapper height is clamped after window resize reduces available space', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);

    // Expand to max snap level
    enterDirMode(fileTree);
    const heightBefore = parseInt(wrapper.style.height, 10);

    // Shrink the window so that the current height would exceed the new floor.
    // We need the new floor to be less than heightBefore.
    // floor = newInnerHeight - MIN_EDITOR_H (chrome = 0 in jsdom)
    // If newInnerHeight - 144 < heightBefore, height must be clamped.
    const tinyWindow = heightBefore + 144 - 50; // floor = heightBefore - 50
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: tinyWindow });
    window.dispatchEvent(new Event('resize'));

    const heightAfter = parseInt(wrapper.style.height, 10);
    const newFloor = tinyWindow - 144;
    expect(heightAfter).toBeLessThanOrEqual(newFloor);

    // Restore
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 });
  });

  test('wrapper height unchanged after resize if still within floor', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);

    // Park at minimum height — very short tree
    fileTree.parkAtMinHeight();
    const heightBefore = parseInt(wrapper.style.height, 10);

    // Resize to a large window — floor is way larger than current height
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 2000 });
    window.dispatchEvent(new Event('resize'));

    const heightAfter = parseInt(wrapper.style.height, 10);
    // Height should remain unchanged since it's within the floor
    expect(heightAfter).toBe(heightBefore);

    // Restore
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 });
  });

  // ── isEditing() guard ──────────────────────────────────────────

  test('isEditing returns false when no rename or create is in progress', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    expect(fileTree.isEditing()).toBe(false);
  });

  test('isEditing returns true during inline rename', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // Find a sibling file segment and double-click to start rename
    // (the highlighted file is in the breadcrumb; siblings appear in tree body)
    const fileSeg = Array.from(container.querySelectorAll('.seg.leaf'))
      .find(el => (el as HTMLElement).dataset.fullName === 'article-two.md') as HTMLElement;
    expect(fileSeg).toBeTruthy();
    startRenameByKey(fileSeg);

    // Now isEditing should be true
    expect(fileTree.isEditing()).toBe(true);

    // Press Escape to cancel — isEditing should go back to false
    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    expect(input).toBeTruthy();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(fileTree.isEditing()).toBe(false);
  });

  // ── Same-row home file ─────────────────────────────────────────

  test('expanded folder with self-named home file renders both in one inline .seg-pair row', async () => {
    // Tree where "交互" folder contains "交互.md" (self-named folder note)
    const tree = dir('/root', [
        dir('/root/交互', [
            file('/root/交互/交互.md', { is_home: true }),
            file('/root/交互/林中星空.md'),
          ]),
        file('/root/index.md'),
      ]);

    mockListTree.mockResolvedValue({ status: 'ok', data: tree });

    const ft = new FileTree(new EntryRegistry());
    const ctn = document.createElement('div');
    document.body.appendChild(ctn);

    await ft.mount(ctn, '/root', false);
    ft.highlight('/root/index.md');
    enterDirMode(ft);

    // Click "交互" folder to expand it
    const folderSeg = Array.from(ctn.querySelectorAll('.seg:not(.leaf)')).find(
      el => el.textContent?.includes('交互'),
    ) as HTMLElement;
    expect(folderSeg).not.toBeNull();
    (folderSeg.querySelector(".chev") as HTMLElement).click();

    // Folder click defers via the dblclick window; wait for the toggle to land.
    await vi.waitFor(() => {
      const names = Array.from(ctn.querySelectorAll<HTMLElement>('.seg'))
        .map(el => el.dataset.fullName ?? el.textContent?.trim() ?? '');
      expect(names).toContain('交互.md');
    });

    // Both the folder and home file should be present
    const allNames = Array.from(ctn.querySelectorAll<HTMLElement>('.seg'))
      .map(el => el.dataset.fullName ?? el.textContent?.trim() ?? '');
    expect(allNames).toContain('交互');
    expect(allNames).toContain('交互.md');

    // Find the folder seg (not .leaf) and the home file seg (.leaf.home-inline)
    const folderEl = Array.from(ctn.querySelectorAll('.seg:not(.bc):not(.leaf)')).find(
      el => el.textContent?.includes('交互'),
    ) as HTMLElement;
    const homeEl = Array.from(ctn.querySelectorAll('.seg.leaf')).find(
      el => (el as HTMLElement).dataset.fullName === '交互.md',
    ) as HTMLElement;

    expect(folderEl).not.toBeNull();
    expect(homeEl).not.toBeNull();

    // Fixed-indent inline-pair invariant: both must share the SAME parent
    // .seg-pair container (one visual row).
    expect(folderEl.parentElement).toBe(homeEl.parentElement);
    expect(folderEl.parentElement!.classList.contains('seg-pair')).toBe(true);

    // The home file must carry the .home-inline modifier class.
    expect(homeEl.classList.contains('home-inline')).toBe(true);

    // The folder is a folder seg (has .chev, not .dot; aria-expanded set)
    expect(folderEl.querySelector('.chev')).not.toBeNull();

    // Other children should appear OUTSIDE the pair (separate rows in .nav-body)
    const otherEl = Array.from(ctn.querySelectorAll('.seg.leaf')).find(
      el => (el as HTMLElement).dataset.fullName === '林中星空.md',
    ) as HTMLElement;
    expect(otherEl).not.toBeNull();
    // 林中星空.md must NOT be inside the .seg-pair container
    expect(otherEl.parentElement!.classList.contains('seg-pair')).toBe(false);

    ft.unmount();
    ctn.remove();
  });

  // ── renderedRowCount() consistency with the rendered body ────────────
  // The panel height is derived from renderedRowCount(); it MUST equal the
  // number of distinct VISUAL body rows (standalone rows + one per .seg-pair),
  // or the panel is the wrong height. This pins that equality across the home-
  // inline merge AND the active-ancestor exclusion at the same time.

  test('renderedRowCount() equals the distinct visual body rows (home-inline + active ancestor)', async () => {
    // docs/ has a home file (index.md); active file is index.md so docs is
    // an active ancestor (excluded from the body, recursed into). notes/ is a
    // non-active sibling folder we open to exercise the home-inline-less path.
    mockListTree.mockResolvedValue({ status: 'ok', data: treeWithFolders() });
    const ft = new FileTree(new EntryRegistry());
    const ctn = document.createElement('div');
    document.body.appendChild(ctn);

    await ft.mount(ctn, PROJECT_ROOT, false);
    ft.highlight(`${PROJECT_ROOT}/docs/index.md`);
    enterDirMode(ft); // opens root + the active ancestor docs

    // Distinct visual rows = .seg-pair containers (1 each) + standalone placed
    // rows that are NOT inside a .seg-pair (and not the inline home file).
    const countVisualBodyRows = (): number => {
      const body = ctn.querySelector('.nav-body')!;
      const pairs = body.querySelectorAll('.seg-pair').length;
      const standalone = Array.from(
        body.querySelectorAll<HTMLElement>('.seg[data-depth]:not(.bc)'),
      ).filter(el => !el.parentElement?.classList.contains('seg-pair')).length;
      return pairs + standalone;
    };

    const rowCount = priv(ft).render.renderedRowCount();
    expect(rowCount).toBe(countVisualBodyRows());

    // Sanity: the active ancestor docs and the active leaf index.md are
    // NOT body rows; docs' other child guide.md is.
    const bodyPaths = Array.from(
      ctn.querySelectorAll<HTMLElement>('.nav-body .seg[data-path]:not(.bc)'),
    ).map(el => el.dataset.path);
    expect(bodyPaths).not.toContain('docs');
    expect(bodyPaths).not.toContain('docs/index.md');
    expect(bodyPaths).toContain('docs/guide.md');

    ft.unmount();
    ctn.remove();
  });

  // ── Sibling folder click under tight height constraint ──────────

  test('clicking a collapsed sibling folder expands it even when height is constrained', async () => {
    // Tree with a sibling folder containing many children — its rowCost
    // will exceed any reasonable height budget.
    const bigTree = dir('/root', [
        dir('/root/posts', Array.from({ length: 30 }, (_, i) => file(`/root/posts/file-${i}.jpg`))),
        file('/root/index.md'),
      ]);

    mockListTree.mockResolvedValue({ status: 'ok', data: bigTree });

    const ft = new FileTree(new EntryRegistry());
    const ctn = document.createElement('div');
    document.body.appendChild(ctn);

    await ft.mount(ctn, '/root', false);

    // Highlight index.md so activePath = [/root, /root/index.md]. Navigation
    // keeps the window closed (sticky toggle); open it explicitly
    // with enterDirMode so root's children — incl. the collapsed `posts` folder
    // this test opens — surface in the body.
    ft.highlight('/root/index.md');
    enterDirMode(ft);

    // Set up chrome elements and wrapper so setupDragHandle activates floor logic.
    const handle = document.createElement('div');
    const wrapper = document.createElement('div');

    const chipBar = document.createElement('div');
    chipBar.id = 'chip-bar';
    const divider = document.createElement('div');
    divider.id = 'divider';
    document.body.appendChild(chipBar);
    document.body.appendChild(divider);

    // Shrink window so the floor is very tight — just enough for the breadcrumb
    // row plus a handful of sibling rows, but NOT enough for 30 extra children.
    // floor = innerHeight - MIN_EDITOR_H = 300 - 144 = 156px
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 300 });

    ft.setupDragHandle(handle, wrapper);

    // enterDirMode opened the root ancestor, so root's children — including the
    // collapsed `posts` folder — surface in the body.
    {
      const texts = Array.from(ctn.querySelectorAll('.seg'))
        .map(el => el.textContent?.trim() ?? '');
      expect(texts).toContain('posts');
    }

    // Now click the collapsed posts folder to toggle it open.
    // Historical bug: the expanded navH (with 30 children) exceeds the floor,
    // and the old snap-clamp reverted the expansion so the folder never opened.
    // The decoupled model expands regardless of floor (height clamps; content
    // scrolls), which this test guards.
    const postsSeg = Array.from(ctn.querySelectorAll('.seg:not(.leaf)')).find(
      el => el.textContent?.includes('posts'),
    ) as HTMLElement;
    expect(postsSeg).not.toBeNull();
    (postsSeg.querySelector(".chev") as HTMLElement).click();

    // After the click (and the dblclick-window defer), posts children visible.
    await vi.waitFor(() => {
      const texts = Array.from(ctn.querySelectorAll('.seg'))
        .map(el => el.textContent?.trim() ?? '');
      expect(texts).toContain('file-0.jpg');
    });

    // Cleanup
    ft.unmount();
    ctn.remove();
    chipBar.remove();
    divider.remove();
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 });
  });

  // ── OS drag-in public accessors ────────────────────────────────

  test('projectRootPath returns the mounted project root', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    expect(fileTree.projectRootPath).toBe(PROJECT_ROOT);
  });

  test('projectRootPath is empty string before mount', () => {
    expect(fileTree.projectRootPath).toBe('');
  });

  test('pathOf returns path for a known entry id', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Navigate to a file so root children (assets, posts) appear as siblings
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree); // expand to show siblings
    // 'assets' folder is in the default tree; it should be in the registry after mount.
    // Registry stores project-root-relative paths.
    const assetsPath = 'assets';
    const id = registry.getByPath(assetsPath);
    expect(id).not.toBeNull();
    expect(fileTree.pathOf(id!)).toBe(assetsPath);
  });

  test('pathOf returns null for an unknown id', () => {
    const fakeId = 9999 as import('../../entry-registry').EntryId;
    expect(fileTree.pathOf(fakeId)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Row identity across renders
//
// render() reconciles rows by EntryId instead of rebuilding them, so a row
// that still exists after a render is the same DOM object it was before.
// Everything below rests on that. The browser raises a click's `detail` to 2
// only when both clicks land on the SAME element, so a rebuilt row would
// deliver the second click as a fresh detail-1 click and act on it twice —
// which for a folder means toggling shut the folder the first click opened
// ---------------------------------------------------------------------------

describe('FileTree row identity and the double-click', () => {
  let fileTree: FileTree;
  let container: HTMLElement;
  let registry: EntryRegistry;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('a render reuses each row element, and drops only the rows that left the tree', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const idOf = (el: HTMLElement) => el.dataset.entryId;
    const before = new Map(
      Array.from(container.querySelectorAll<HTMLElement>('.seg[data-entry-id]'))
        .map(el => [idOf(el)!, el]),
    );
    expect(before.size).toBeGreaterThan(1);

    // A watcher-driven refresh where the payload lost article-two.md.
    const pruned = defaultTree();
    const posts = pruned.children!.find(c => c.name === 'posts')!;
    posts.children = posts.children!.filter(c => c.name !== 'article-two.md');
    mockListTree.mockResolvedValue({ status: 'ok', data: pruned });
    await fileTree.refreshAndRender();

    const goneId = idOf(
      Array.from(before.values()).find(el => el.dataset.fullName === 'article-two.md')!,
    )!;
    for (const [id, el] of before) {
      const now = container.querySelector<HTMLElement>(`.seg[data-entry-id="${id}"]`);
      if (id === goneId) expect(now).toBeNull();
      else expect(now).toBe(el); // same object, not an equal one
    }
  });

  test('a double-click on a file opens it once and starts the rename', async () => {
    // The second press renames the item and
    // never re-opens. jsdom hands the handler whatever
    // `detail` this test writes, so this covers the POLICY only — the engine
    // pairing that feeds it is file-tree-gestures.spec.ts's.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    const entryId = target.dataset.entryId;

    target.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    expect(listener).toHaveBeenCalledTimes(1);
    // filePath is project-root-relative (the registry domain) after the
    expect((listener.mock.calls[0][0] as CustomEvent).detail.filePath)
      .toBe('posts/article-two.md');

    // A full rebuild between the two clicks — the watcher burst a save fires.
    // The row has to survive it, or the second click arrives as detail 1.
    await fileTree.refreshAndRender();
    expect(container.querySelector<HTMLElement>(`.seg[data-entry-id="${entryId}"]`)).toBe(target);

    target.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 2 }));
    expect(listener).toHaveBeenCalledTimes(1); // the second press is not a re-open
    const input = container.querySelector<HTMLInputElement>('.moss-tree-rename-input');
    expect(input?.value).toBe('article-two');
  });

  test('a double-click on a folder leaves it open rather than toggling it shut', async () => {
    // The destructive case for the detail>1 guard: a folder's click TOGGLES,
    // so acting on both clicks would collapse what the first one opened.
    const nested = defaultTree();
    const posts = nested.children!.find(c => c.name === 'posts')!;
    posts.children!.push(dir(`${PROJECT_ROOT}/posts/drafts`, [file(`${PROJECT_ROOT}/posts/drafts/wip.md`)]) as unknown as NonNullable<typeof posts.children>[number]);
    mockListTree.mockResolvedValue({ status: 'ok', data: nested });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const folder = Array.from(
      container.querySelectorAll<HTMLElement>('.nav-body .seg:not(.leaf)'),
    ).find(el => el.dataset.path === 'posts/drafts')!;
    expect(folder).toBeDefined();

    folder.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    await vi.waitFor(() =>
      expect(container.querySelector('.nav-body .seg[data-path^="posts/drafts/"]')).not.toBeNull());

    folder.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 2 }));
    expect(container.querySelector('.nav-body .seg[data-path^="posts/drafts/"]')).not.toBeNull();
  });

  test('a click inside the open rename input does not act on the row under it', async () => {
    // The input lives INSIDE the row, so without one boundary the delegated
    // click would resolve to that row and open the file the user is renaming.
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    startRenameByKey(target);
    const input = container.querySelector<HTMLInputElement>('.moss-tree-rename-input')!;
    expect(input).not.toBeNull();

    const listener = vi.fn();
    container.addEventListener('editor:file-selected', listener);
    input.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));

    expect(listener).not.toHaveBeenCalled();
    expect(container.querySelector('.moss-tree-rename-input')).toBe(input);
  });

  test('file-changed event during inline rename does NOT destroy the rename input', async () => {
    // Bug: file-tree's FileChanged listener unconditionally calls refreshTree
    // + render(), wiping the .nav-bc-strip + .nav-body contents and any in-progress rename input.
    // Auto-saves and other FS events flowing through the watcher would land
    // mid-rename and the user would see the input vanish.
    let listenCb: Function | null = null;
    mockListen.mockImplementation(async (_event: string, cb: Function) => {
      listenCb = cb;
      return vi.fn();
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    startRenameByKey(target);

    expect(container.querySelector('.moss-tree-rename-input')).not.toBeNull();
    expect(fileTree.isEditing()).toBe(true);

    // Simulate a watcher event arriving mid-rename (e.g. autosave landed).
    expect(listenCb).not.toBeNull();
    listenCb!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });

    // Allow the awaited refreshTree call inside the listener to settle.
    await new Promise(r => setTimeout(r, 0));

    // The input must survive the watcher event.
    expect(container.querySelector('.moss-tree-rename-input')).not.toBeNull();
  });

  test('deferred refresh runs after the rename completes', async () => {
    // The watcher event we swallowed during the rename must not be lost —
    // dropping it would leave dirIndex stale (e.g. a sibling file created
    // by an external editor would be invisible until the next event).
    let listenCb: Function | null = null;
    mockListen.mockImplementation(async (_event: string, cb: Function) => {
      listenCb = cb;
      return vi.fn();
    });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    startRenameByKey(target);

    const callsBeforeWatcherEvent = mockListTree.mock.calls.length;

    // Watcher fires during the rename — refresh must be deferred.
    listenCb!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });
    await new Promise(r => setTimeout(r, 0));
    expect(mockListTree.mock.calls.length).toBe(callsBeforeWatcherEvent);

    // Cancel the rename — deferred refresh must now fire.
    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    await vi.waitFor(() => {
      expect(mockListTree.mock.calls.length).toBeGreaterThan(callsBeforeWatcherEvent);
    });
  });

  test('renaming the currently-viewed file dispatches editor:file-renamed, not editor:file-selected', async () => {
    // Bug: dispatching editor:file-selected after rename routes through
    // loadFile → resolveUrlForFile → emit('editor:navigate-preview', '/')
    // (because the site has not rebuilt yet so the URL is unresolvable).
    // The preview navigates to home, then the preview→editor sync re-loads
    // the home file, kicking the editor off the file the user just renamed.
    // The fix dispatches a dedicated editor:file-renamed event that updates
    // editor state in place without touching the preview.
    //
    // The event detail
    // carries project-root-relative paths (the registry domain), and
    // data-path on rendered DOM does too.
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    await fileTree.mount(container, PROJECT_ROOT, false);
    const oldPath = 'posts/article-one.md';
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const seg = container.querySelector<HTMLElement>(`.seg.bc[data-path="${oldPath}"]`)!;
    startRenameByKey(seg);

    const selected = vi.fn();
    const renamed = vi.fn();
    const renaming = vi.fn();
    container.addEventListener('editor:file-selected', selected);
    container.addEventListener('editor:file-renamed', renamed);
    container.addEventListener('editor:file-renaming', renaming);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    input.value = 'renamed';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    const newPath = 'posts/renamed.md';
    await vi.waitFor(() => {
      expect(renamed).toHaveBeenCalledTimes(1);
    });
    expect((renamed.mock.calls[0][0] as CustomEvent).detail).toEqual({ oldPath, newPath });

    // editor:file-renaming must fire BEFORE the disk rename so editor-main
    // can register pending paths and silence the watcher's file-changed echo.
    expect(renaming).toHaveBeenCalledTimes(1);
    expect((renaming.mock.calls[0][0] as CustomEvent).detail).toEqual({ oldPath, newPath });
    expect(renaming.mock.invocationCallOrder[0]).toBeLessThan(
      renamed.mock.invocationCallOrder[0],
    );

    // The home-jump path: file-selected must NOT fire.
    expect(selected).not.toHaveBeenCalled();
  });

  test('drag-drop move of the ACTIVE file dispatches rename events and updates the breadcrumb (activePath)', async () => {
    // The active file's breadcrumb crumb IS drag-
    // source-wired, so a user can drag the currently-open file onto a folder.
    // Real drags always populate draggedSelection (startMultiRowDrag), so this
    // takes the batch moveEntries path — which used to skip the rename
    // lifecycle entirely, leaving the active EntryId un-remapped and the
    // breadcrumb (activePath) frozen on the OLD location. The batch path now
    // mirrors commitRename's active-file tracking. Here we drag the active file
    // onto the ROOT crumb (a valid move to project root) and assert the events
    // fire and the breadcrumb follows.
    mockMoveEntries.mockResolvedValue({
      status: 'ok',
      data: {
        moved: [{ old_path: 'posts/article-one.md', new_path: 'article-one.md', is_dir: false }],
        edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1',
      },
    });
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // Simulate editor-main's synchronous editor:file-renamed handler, which
    // remaps the active EntryId to the new path so reconcileSnapshot preserves
    // identity (see commitRename's ORDER-REVERSED comment). Without this the
    // active EntryId would be retired and currentFilePath would go null.
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

    // After the move, the tree has article-one.md at the project root.
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: dir(PROJECT_ROOT, [
          dir(`${PROJECT_ROOT}/assets`, []),
          dir(`${PROJECT_ROOT}/posts`, [
            file(`${PROJECT_ROOT}/posts/article-two.md`),
          ]),
          file(`${PROJECT_ROOT}/article-one.md`),
          file(`${PROJECT_ROOT}/index.md`),
          file(`${PROJECT_ROOT}/readme.txt`),
        ], { name: 'my-blog' }),
    });

    const activeSeg = container.querySelector<HTMLElement>('.seg.bc[data-path="posts/article-one.md"]')!;
    expect(activeSeg).toBeTruthy();
    const rootCrumb = container.querySelector<HTMLElement>('.seg.bc[data-path=""]')!;
    expect(rootCrumb).toBeTruthy();

    activeSeg.dispatchEvent(new Event('dragstart', { bubbles: true }));
    rootCrumb.dispatchEvent(new Event('drop', { bubbles: true }));

    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalled());
    await vi.waitFor(() => {
      expect(renamed).toHaveBeenCalledTimes(1);
      const activePath = priv(fileTree).view.anchor;
      expect(activePath[activePath.length - 1]).toBe('article-one.md');
    });
    // Lifecycle: renaming BEFORE renamed (so editor-main can register pending
    // before the move), and the canonical rename-complete broadcast fires.
    expect((renaming.mock.calls[0][0] as CustomEvent).detail).toEqual({ oldPath: 'posts/article-one.md', newPath: 'article-one.md' });
    expect((renamed.mock.calls[0][0] as CustomEvent).detail).toEqual({ oldPath: 'posts/article-one.md', newPath: 'article-one.md' });
    expect(renaming.mock.invocationCallOrder[0]).toBeLessThan(renamed.mock.invocationCallOrder[0]);
    expect(renameComplete).toHaveBeenCalledTimes(1);
  });

  test('drag-drop move of a non-active file does not dispatch rename events', async () => {
    // Single-file drops go through moveEntries (not renameEntry),
    // because startMultiRowDrag sets draggedSelection (size=1). The invariant
    // tested here — no editor:file-renaming / editor:file-renamed events for a
    // NON-active file move — still holds: the batch moveEntries path dispatches
    // the rename lifecycle ONLY when the moved set includes the active file
    // (see the active-file regression test above); readme.txt is not active.
    mockMoveEntries.mockResolvedValue({
      status: 'ok',
      data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' },
    });
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Highlight index.md so root's children (assets, posts, readme.txt) all
    // render as siblings under enterDirMode. readme.txt is the dragged file.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const renaming = vi.fn();
    const renamed = vi.fn();
    container.addEventListener('editor:file-renaming', renaming);
    container.addEventListener('editor:file-renamed', renamed);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf'))
      .find(el => el.dataset.fullName === 'readme.txt');
    expect(target).toBeTruthy();
    const assetsSeg = Array.from(container.querySelectorAll<HTMLElement>('.seg:not(.leaf):not(.bc)'))
      .find(el => el.dataset.path === 'assets');
    expect(assetsSeg).toBeTruthy();

    target!.dispatchEvent(new Event('dragstart', { bubbles: true }));
    assetsSeg!.dispatchEvent(new Event('drop', { bubbles: true }));

    await vi.waitFor(() => expect(mockMoveEntries).toHaveBeenCalled());
    await new Promise(r => setTimeout(r, 10));
    expect(renaming).not.toHaveBeenCalled();
    expect(renamed).not.toHaveBeenCalled();
  });

  test('file-changed while createPending is set defers refresh (no concurrent listTree)', async () => {
    // Earlier bug: `removeTempEntry()` cleared `createPending = null` BEFORE
    // awaiting createFiles, so a watcher event during the IPC would call
    // refreshTree+render and wipe the new-file input mid-typing. The fix
    // moves removeTempEntry into the create-finish `finally` block, keeping
    // createPending non-null across the IPC. We probe via isEditing + the
    // listTree call count without awaiting (rAF + cancelCreate behavior in
    // jsdom is brittle; the synchronous defer guard is what we're testing).
    let listenCb: Function | null = null;
    mockListen.mockImplementation(async (_event: string, cb: Function) => {
      listenCb = cb;
      return vi.fn();
    });

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    // Context-menu create seam (the deleted createInCurrentDir wrapper used
    // to route here); the sync half runs before the first await.
    void (fileTree as unknown as { startCreate(p: string, t: 'file' | 'folder'): Promise<void> })
      .startCreate('', 'file');
    expect(fileTree.isEditing()).toBe(true);

    const listTreeBefore = mockListTree.mock.calls.length;
    expect(listenCb).not.toBeNull();
    // Fire the watcher event — the sync part of refreshAndRender must see
    // isEditing=true, set refreshPending, and return without calling listTree.
    listenCb!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });
    expect(mockListTree.mock.calls.length).toBe(listTreeBefore);
    expect(fileTree.isEditing()).toBe(true);
  });

  test('a watcher event during the rename IPC leaves the confirmed rename standing', async () => {
    // This test used to assert the OPPOSITE — that the input was still in the
    // DOM throughout the rename IPC — because renamingPath was held across it.
    // That is precisely what made the optimistic render invisible on the Enter
    // gesture (render() parks while the flag is held), so on Enter the input
    // now comes down and the new name is painted immediately. What still has
    // to be true is the part the test was really about: a watcher event
    // arriving mid-IPC must not disturb any of it.
    let listenCb: Function | null = null;
    mockListen.mockImplementation(async (_event: string, cb: Function) => {
      listenCb = cb;
      return vi.fn();
    });

    // Hold renameEntry pending so we can fire a watcher event during the await.
    let resolveRename: ((v: { status: 'ok'; data: unknown }) => void) | null = null;
    mockRenameEntry.mockImplementation(() => new Promise(r => { resolveRename = r as typeof resolveRename; }));

    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    startRenameByKey(target);

    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    expect(input).not.toBeNull();
    input.value = 'new-name';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // Yield so finish() begins and awaits renameEntry.
    await new Promise(r => setTimeout(r, 0));

    // The edit is over the moment Enter is pressed: no input, and the row
    // already carries the new name even though the IPC has not returned.
    expect(container.querySelector('.moss-tree-rename-input')).toBeNull();
    const rowNames = () => Array.from(container.querySelectorAll<HTMLElement>('.seg[data-path]'))
      .map(el => el.dataset.path);
    expect(rowNames()).toContain('posts/new-name.md');

    // Now fire a watcher event inside the await window. By the time the
    // watcher can see anything the file has already moved — the command
    // renames on its first line and spends the rest of its time rewriting
    // links — so disk agrees with the prediction, and the refresh it triggers
    // must leave the row alone rather than flicker.
    const renamed = defaultTree();
    renamed.children![1].children![1] = file(`${PROJECT_ROOT}/posts/new-name.md`);
    mockListTree.mockResolvedValue({ status: 'ok', data: renamed });
    expect(listenCb).not.toBeNull();
    listenCb!({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });
    await new Promise(r => setTimeout(r, 0));

    expect(rowNames()).toContain('posts/new-name.md');
    expect(container.querySelector('.moss-tree-rename-input')).toBeNull();

    // Resolve renameEntry so finish completes and frees state for afterEach.
    resolveRename!({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    await vi.waitFor(() => expect(fileTree.isEditing()).toBe(false));
  });

  test('renaming a non-active file does not dispatch editor:file-renamed', async () => {
    // The current file's editor state needs the rename signal; siblings do not.
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    const renamed = vi.fn();
    container.addEventListener('editor:file-renamed', renamed);

    const target = Array.from(container.querySelectorAll<HTMLElement>('.seg.leaf')).find(
      el => el.dataset.fullName === 'article-two.md',
    )!;
    startRenameByKey(target);
    const input = container.querySelector('.moss-tree-rename-input') as HTMLInputElement;
    input.value = 'two-renamed';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() => {
      expect(mockRenameEntry).toHaveBeenCalled();
    });
    // Brief settle window so a misdispatched event would land.
    await new Promise(r => setTimeout(r, 10));
    expect(renamed).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// FIX 4 — download-flash target resolution (deepestVisibleContainerFor) +
// flashRow. These back the editor-main flashDownloadTargets coalescing.
// ---------------------------------------------------------------------------
describe('FileTree deepestVisibleContainerFor / flashRow (download flash)', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('returns the file-tree BODY ROW for a visible expanded folder', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree); // renders root children as body rows, incl. `posts`

    const postsRow = findRowByPath(container, 'posts');
    expect(postsRow).toBeDefined();

    // A new file under `posts/` resolves to the `posts` body row.
    const target = fileTree.deepestVisibleContainerFor('posts');
    expect(target).toBe(postsRow);
  });

  test('walks UP to the deepest visible ancestor row when the exact folder has no row', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsRow = findRowByPath(container, 'posts');
    expect(postsRow).toBeDefined();

    // `posts/sub` has no rendered row (posts is collapsed) → resolves to the
    // deepest visible ancestor, the `posts` row.
    const target = fileTree.deepestVisibleContainerFor('posts/sub');
    expect(target).toBe(postsRow);
  });

  test('falls back to the BREADCRUMB segment when the folder is an active-path ancestor (not a body row)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // Active file lives inside posts/ → posts becomes a breadcrumb crumb, NOT a
    // body row (active-path ancestors render in the breadcrumb).
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);
    enterDirMode(fileTree);

    // No `posts` body row (it's the active ancestor) ...
    expect(findRowByPath(container, 'posts')).toBeUndefined();
    // ... but there IS a breadcrumb segment for it.
    const bcSeg = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc[data-path="posts"]');
    expect(bcSeg).not.toBeNull();

    const target = fileTree.deepestVisibleContainerFor('posts');
    expect(target).toBe(bcSeg);
  });

  test('falls back to the ROOT breadcrumb crumb when nothing deeper is visible (collapsed tree)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    // No expansion gesture at all → overrides stays empty, no body rows. The
    // root crumb is still shown, so it is the deepest visible ancestor of any
    // path → the breadcrumb fallback target.
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    expect(container.querySelectorAll('.nav-body .seg').length).toBe(0);

    const rootCrumb = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc[data-path=""]');
    expect(rootCrumb).not.toBeNull();
    const target = fileTree.deepestVisibleContainerFor('assets');
    expect(target).toBe(rootCrumb);
  });

  test('returns null when the tree has no rendered breadcrumb at all (unmounted/empty)', () => {
    // Never mounted → no container/navBcStrip → null, so editor-main flashes the
    // whole nav-surface.
    expect(fileTree.deepestVisibleContainerFor('assets')).toBeNull();
  });

  test('flashRow applies the shared .moss-just-committed pulse to the matching row', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);

    const postsRow = findRowByPath(container, 'posts')!;
    expect(postsRow.classList.contains('moss-just-committed')).toBe(false);
    fileTree.flashRow('posts');
    expect(postsRow.classList.contains('moss-just-committed')).toBe(true);
  });

  test('flashRow is a no-op when no row matches the path', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    fileTree.highlight(`${PROJECT_ROOT}/index.md`);
    enterDirMode(fileTree);
    // Should not throw.
    expect(() => fileTree.flashRow('does/not/exist')).not.toThrow();
  });
});

// ── FileTree.setSealed ─────────────────────────────────────────
//
// The file-tree lock is keyed on the `editor:seal` event through editor-main.ts.
// FileTree.setSealed(boolean) toggles .moss-tree--sealed on the container.
// It must NOT couple file-tree.ts to deploy-handler.
describe('FileTree.setSealed', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('setSealed(true) adds .moss-tree--sealed to the tree container', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    fileTree.setSealed(true);

    expect(container.classList.contains('moss-tree--sealed')).toBe(true);
  });

  test('setSealed(false) removes .moss-tree--sealed from the tree container', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);

    fileTree.setSealed(true);
    fileTree.setSealed(false);

    expect(container.classList.contains('moss-tree--sealed')).toBe(false);
  });

  test('setSealed(true) before mount is a no-op (no crash)', () => {
    // No container yet — should not throw.
    expect(() => fileTree.setSealed(true)).not.toThrow();
  });

  test('setSealed(false) is idempotent (no crash if called without prior seal)', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    expect(() => fileTree.setSealed(false)).not.toThrow();
    expect(container.classList.contains('moss-tree--sealed')).toBe(false);
  });

  // The root is a folder like any other for id-keyed state: it is a key in
  // `overrides` and a parent in `dirIndex`, so it has to have an EntryId or
  // every id lookup needs a root special case.
  test('the project root has a registry EntryId after mount, and it is a directory', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const rootId = registry.getByPath('');
    expect(rootId).not.toBeNull();
    expect(registry.getById(rootId!)?.isDir).toBe(true);
  });
});

// ── Versions mode: FileTree.setTitleBar (title bar as navigation stack) ────
//
// The
// strip is REPLACED, not relabeled (round five relabeled the root crumb in
// place). Whoever owns the screen builds its own content (mkBackCrumb +
// subject + actions, tree/title-bar.ts) and hands it to
// FileTree.setTitleBar, which mounts it into `.nav-bc-strip__mode` and
// toggles `is-mode` — no more click-routing lives at this layer. This drives
// the REAL TreeRender-built breadcrumb DOM; editor-main-versions.test.ts
// mocks FileTree wholesale and can only pin the wiring (setTitleBar called
// with an element/null).
describe('FileTree.setTitleBar (title bar as navigation stack)', () => {
  let fileTree: FileTree;
  let registry: EntryRegistry;
  let container: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListTree.mockResolvedValue({ status: 'ok', data: defaultTree() });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('mounts content into the mode layer and sets is-mode on the strip', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const strip = container.querySelector<HTMLElement>('.nav-bc-strip')!;
    const modeLayer = container.querySelector<HTMLElement>('.nav-bc-strip__mode')!;
    expect(strip.classList.contains('is-mode')).toBe(false);

    const content = document.createElement('div');
    content.textContent = 'Editor';
    fileTree.setTitleBar(content);

    expect(strip.classList.contains('is-mode')).toBe(true);
    expect(modeLayer.contains(content)).toBe(true);
  });

  test('releasing with null removes is-mode', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const strip = container.querySelector<HTMLElement>('.nav-bc-strip')!;
    fileTree.setTitleBar(document.createElement('div'));

    fileTree.setTitleBar(null);

    expect(strip.classList.contains('is-mode')).toBe(false);
  });

  test('every original crumb stays in the DOM under is-mode — inertness is CSS (file-tree.css), not removal', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const before = container.querySelectorAll('.nav-bc-strip > .seg.bc').length;
    expect(before).toBeGreaterThan(0);

    fileTree.setTitleBar(document.createElement('div'));

    expect(container.querySelectorAll('.nav-bc-strip > .seg.bc').length).toBe(before);
  });

  test('swapping content while already active replaces the mode layer\'s children', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const modeLayer = container.querySelector<HTMLElement>('.nav-bc-strip__mode')!;
    const first = document.createElement('div');
    fileTree.setTitleBar(first);

    const second = document.createElement('div');
    fileTree.setTitleBar(second);

    // jsdom has no Element.animate, so crossfadeSwap (shared/ui/morph.ts)
    // replaces the children synchronously instead of after a real
    // animation — the fade itself is covered by the render gate.
    expect(modeLayer.contains(first)).toBe(false);
    expect(modeLayer.contains(second)).toBe(true);
  });

  test('a render() while a title bar is active rebuilds an EMPTY mode layer — a known gap, same as round five\'s crossfaded root label', async () => {
    await fileTree.mount(container, PROJECT_ROOT, false);
    const content = document.createElement('div');
    fileTree.setTitleBar(content);

    // A different active file re-renders the breadcrumb's crumbs wholesale
    // (render()'s own strip.replaceChildren()), which just as wholesale
    // rebuilds the mode layer empty (tree/title-bar.ts's own file header).
    fileTree.highlight(`${PROJECT_ROOT}/posts/article-one.md`);

    const modeLayerAfter = container.querySelector<HTMLElement>('.nav-bc-strip__mode');
    expect(modeLayerAfter).not.toBeNull();
    expect(modeLayerAfter!.contains(content)).toBe(false);
    // is-mode itself is a class on the strip, untouched by replaceChildren().
    expect(container.querySelector('.nav-bc-strip')!.classList.contains('is-mode')).toBe(true);
  });

  test('setTitleBar before mount does not throw (no strip to attach to yet)', () => {
    expect(() => fileTree.setTitleBar(document.createElement('div'))).not.toThrow();
    expect(() => fileTree.setTitleBar(null)).not.toThrow();
  });
});

describe('mkBackCrumb (tree/title-bar.ts)', () => {
  test('click and Enter/Space keydown both fire onClick', () => {
    const onClick = vi.fn();
    const el = mkBackCrumb('Editor', onClick);
    document.body.appendChild(el);

    el.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    el.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true }));
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));

    expect(onClick).toHaveBeenCalledTimes(3);
    expect(el.className).toBe('seg bc');
    expect(el.querySelector('.chev.chev--back')).not.toBeNull();
    expect(el.querySelector('.seg-name')!.textContent).toBe('Editor');
    el.remove();
  });
});
