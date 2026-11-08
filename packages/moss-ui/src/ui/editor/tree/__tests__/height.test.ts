/**
 * Tests for the tree-window height contract (`view.park`).
 *
 * Four rules:
 *   (1) breadcrumb expand → content-fit grow  (park cleared by
 *       handleToggleTop's OPEN branch)
 *   (2) in-tree expand (via toggleFolderOpen) → height FROZEN, does not grow even
 *       without a prior drag. The park is set to pre-expand content height.
 *   (3) drag-up-to-shrink → the park is the height the drag applied;
 *       subsequent applyWrapperHeight honours it (no bounce back to
 *       content-fit — release commits nothing but the height, sash model rule 5).
 *   (4) closing a folder → desiredH = min(park, smaller contentH) → wrapper shrinks
 *       below park (rule 4).
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, stubPointerCapture, installCSSEscapePolyfill, priv, overrideAt, dir, file, setView } from './test-helpers';

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
import { navH } from '../spine-projection';

const ROOT = '/Users/me/Sites/blog';

/**
 * Tree with a root-level folder "posts" containing 2 articles,
 * plus a root-level file "index.md".
 */
function makeTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
      dir(`${ROOT}/posts`, [
          file(`${ROOT}/posts/article-one.md`),
          file(`${ROOT}/posts/article-two.md`),
        ]),
    ], { name: 'blog' });
}

/** Read the park from the controller (private, via priv()). */
function getPreferredH(ft: FileTree): number | null {
  return priv(ft).view.park;
}

describe('height contract', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;
  let handle: HTMLElement;
  let wrapper: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree() });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    handle = document.createElement('div');
    wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);

    // Highlight a file so activePath is set
    fileTree.highlight(`${ROOT}/index.md`);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    handle.remove();
    wrapper.remove();
  });

  // ── Rule 1: breadcrumb expand → content-fit (no park) ────────────────────

  test('Rule 1: handleToggleTop(0) OPEN clears the park (breadcrumb expand = content-fit grow)', () => {
    // Simulate: user had parked a height via drag
    setView(fileTree, { park: 200 });
    // Root not open — the chevron click takes the OPEN branch.
    expect(overrideAt(fileTree, '')).not.toBe('open');

    priv(fileTree).handleToggleTop(0);

    expect(getPreferredH(fileTree)).toBeNull();
    // Wrapper height should now be content-fit, not the old parked value
    const h = parseInt(wrapper.style.height, 10);
    expect(h).toBeGreaterThanOrEqual(navH(0));
  });

  test('Rule 1: breadcrumb OPEN via handleToggleTop on a middle segment clears a stale park', () => {
    // Highlight a file INSIDE posts so the breadcrumb is ['', 'posts', article].
    // posts is then a middle breadcrumb folder we can open via handleToggleTop(1) —
    // the branch the Rule 1 park-clear fix lives in.
    fileTree.highlight(`${ROOT}/posts/article-one.md`);
    // Ensure posts is currently CLOSED so the toggle takes the open branch.
    setView(fileTree, { overrides: new Map() });
    fileTree.highlight(`${ROOT}/posts/article-one.md`);

    // Simulate a stale park left over from an earlier in-tree expand / drag.
    setView(fileTree, { park: 60 });

    // OPEN posts via the breadcrumb (handleToggleTop index 1 → else-open branch).
    priv(fileTree).handleToggleTop(1);

    // Rule 1: the breadcrumb expand cleared the park and grew to content-fit.
    expect(getPreferredH(fileTree)).toBeNull();
    const heightAfter = parseInt(wrapper.style.height, 10);
    expect(heightAfter).toBe(navH(priv(fileTree).render.renderedRowCount()));
  });

  // ── Rule 2: in-tree expand → height frozen at pre-expand size ────────────

  test('Rule 2a: toggleFolderOpen (open) sets the park when null', () => {
    // Start with no park
    expect(getPreferredH(fileTree)).toBeNull();

    // Simulate in-tree expand of "posts" (toggleFolderOpen)
    // Access the private method directly to unit-test the freeze logic
    priv(fileTree).toggleFolderOpen('posts');

    // the park should now be set to the pre-expand content height
    const parkedH = getPreferredH(fileTree);
    expect(parkedH).not.toBeNull();
    // The freeze happens BEFORE applying the override, so parkedH should be
    // the navH of the row count BEFORE opening posts.
    // Before opening: active path = ['', 'index.md'], only root siblings visible
    // when root is open. With index.md active, root has 2 siblings: index.md + posts.
    // However the breadcrumb excludes active-path nodes, so rendered rows are
    // the sibling rows not on the active path. With root open: siblings = index.md + posts.
    // index.md is in activePath (it IS the active file), posts is not → 1 visible row (posts).
    expect(typeof parkedH).toBe('number');
    // After the freeze, wrapper height must equal the parked value (not larger)
    const wrapperH = parseInt(wrapper.style.height, 10);
    expect(wrapperH).toBe(parkedH);
  });

  test('Rule 2b: in-tree expand does NOT grow the wrapper beyond the parked pre-expand height', () => {
    // First open the root so siblings are visible
    priv(fileTree).handleToggleTop(0);
    // Clear the park that handleToggleTop(0) set (it clears it, confirms Rule 1)
    expect(getPreferredH(fileTree)).toBeNull();

    // Record the wrapper height before opening posts
    const heightBefore = parseInt(wrapper.style.height, 10);

    // In-tree expand of posts (toggleFolderOpen)
    priv(fileTree).toggleFolderOpen('posts');

    // Wrapper height must NOT grow beyond the pre-expand height
    const heightAfter = parseInt(wrapper.style.height, 10);
    expect(heightAfter).toBeLessThanOrEqual(heightBefore);
  });

  test('Rule 2c: second in-tree expand does NOT re-freeze (park is already set)', () => {
    // Open root
    priv(fileTree).handleToggleTop(0);
    expect(getPreferredH(fileTree)).toBeNull();

    // First in-tree expand: posts
    priv(fileTree).toggleFolderOpen('posts');
    const parkAfterFirst = getPreferredH(fileTree);
    expect(parkAfterFirst).not.toBeNull();

    // Collapse posts
    priv(fileTree).toggleFolderOpen('posts');
    // Park survives a close (only breadcrumb-open (Rule 1) or parkAtMinHeight clear it)

    // Reopen posts — park should remain at the first-set value, NOT re-freeze to
    // the current (smaller) content height
    priv(fileTree).toggleFolderOpen('posts');
    const parkAfterSecond = getPreferredH(fileTree);
    // Park is already set, so toggleFolderOpen does NOT overwrite it
    expect(parkAfterSecond).toBe(parkAfterFirst);
  });

  // ── Rule 3: drag-up-to-shrink stays (no bounce) ───────────────────────────

  test('Rule 3: drag-release sets the park and wrapper keeps that height', () => {
    stubPointerCapture(handle);

    // Simulate: user expands the root so rows are visible
    priv(fileTree).handleToggleTop(0);
    expect(getPreferredH(fileTree)).toBeNull();

    // The sash model's endDrag has no threshold and no collapse branch (rule 5:
    // release commits nothing but the height) — whatever height the drag
    // applied parks. Content-fit after handleToggleTop(0) is navH(1) = 77; a
    // 20px drag up lands at 57, above the 48px minimum.
    const contentH = parseInt(wrapper.style.height, 10);
    expect(contentH).toBe(navH(1));
    const parkTarget = contentH - 20;

    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 200, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 180, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 180, bubbles: true }));

    // endDrag parks the height the gesture applied — the same number the
    // wrapper shows, read from the drag's own state and not back out of the DOM.
    expect(getPreferredH(fileTree)).toBe(parkTarget);
    const h = parseInt(wrapper.style.height, 10);
    expect(h).toBe(parkTarget);
  });

  test('Rule 3: applyWrapperHeight after drag honours the park (no bounce)', () => {
    stubPointerCapture(handle);

    priv(fileTree).handleToggleTop(0);
    expect(getPreferredH(fileTree)).toBeNull();

    const contentH = parseInt(wrapper.style.height, 10);
    // Park manually at a shrunken height
    const parkedH = contentH - 15;
    setView(fileTree, { park: parkedH });

    // Re-apply wrapper height — should respect the park
    priv(fileTree).render.applyWrapperHeight();
    const h = parseInt(wrapper.style.height, 10);
    // desiredH = min(park, contentH) = parkedH (since parkedH < contentH)
    // finalH = clamp(parkedH, minH, floor) = parkedH (assuming parkedH > minH and < floor)
    expect(h).toBe(parkedH);
  });

  // ── Rule 4: collapse a folder shrinks below park ──────────────────────────

  test('Rule 4: closing a folder via the REAL toggle path shrinks below park (min())', () => {
    // Open root, then in-tree open posts (freezes park at the pre-expand height).
    priv(fileTree).handleToggleTop(0);
    expect(getPreferredH(fileTree)).toBeNull();
    priv(fileTree).toggleFolderOpen('posts');
    expect(getPreferredH(fileTree)).not.toBeNull();

    // Open posts shows 2 child rows; with the park capping height, the wrapper is
    // scroll-hidden at the pre-expand size. Now force a HIGH park so that closing
    // posts produces a contentH strictly BELOW park — that's the Rule 4 scenario.
    setView(fileTree, { park: 999 });

    // Close posts via the REAL path (toggleFolderOpen close
    // branch), which re-renders + calls applyWrapperHeight. Park is preserved on
    // close (only breadcrumb-open or parkAtMinHeight clear it); applyWrapperHeight
    // then does desiredH = min(999, contentH) → shrinks to the now-smaller contentH.
    priv(fileTree).toggleFolderOpen('posts');

    const contentH = navH(priv(fileTree).render.renderedRowCount());
    const h = parseInt(wrapper.style.height, 10);
    // Wrapper shrank to the content height (below the 999 park), proving min() bites.
    expect(h).toBe(contentH);
    expect(h).toBeLessThan(999);
    // The park itself survives the close (Rule 4 only caps the rendered height).
    expect(getPreferredH(fileTree)).toBe(999);
  });

  // ── parkAtMinHeight() parks at the minimum, doesn't clear (rule 2) ────────

  test('parkAtMinHeight() parks at navH(0), never null (overrides untouched)', () => {
    setView(fileTree, { park: 300 });
    const overridesBefore = new Map(priv(fileTree).view.overrides);
    fileTree.parkAtMinHeight();
    // Sash model rule 2: nothing is wiped — parking sets an explicit minimum
    // height park, it does not fall back to content-fit (null).
    expect(getPreferredH(fileTree)).toBe(navH(0));
    expect(priv(fileTree).view.overrides).toEqual(overridesBefore);
  });

  // ── unmount() resets park ────────────────────────────────────────────────

  test('unmount() clears the park', () => {
    setView(fileTree, { park: 300 });
    fileTree.unmount();
    // unmount resets state; a subsequent re-read must show null
    expect(getPreferredH(fileTree)).toBeNull();
  });
});
