/**
 * Tests for the sash-model drag-fill mechanics.
 *
 * The sash rules 3/4/5/6 and the setupDragHandle() header comment in file-tree.ts.
 *
 * Covered here (unit-level, via priv() + synthesized pointer events):
 *   - fillOrder() on a deep path: nearest ancestor first, root last
 *   - the half-row slack threshold (DRAG_FILL_SLACK_PX): no fill until the
 *     cursor pushes past content-fit by more than half a row
 *   - one-way opening within a single gesture: dragging back up after a fill
 *     does not re-close what the fill opened (rule 6 — clip, never close)
 *   - release inertness at 30/50/57px: release never mutates `overrides`,
 *     regardless of where the drag lands (rule 5 — no threshold, no bounce)
 *   - drag up never mutates `overrides`
 *   - folder-valued-leaf fill: activePath=[''] (generated home) can still
 *     drag-fill root's children
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
import { EntryRegistry, type EntryId } from '../../entry-registry';
import { navH } from '../spine-projection';

const ROOT = '/Users/me/Sites/blog';

/**
 * A 3-level-deep tree: root > docs > sub > page.md, plus a root index.md so
 * the active path for page.md is the root, docs, sub, page.md chain
 * — deep enough to distinguish fillOrder()'s nearest-first ordering from a
 * naive root-first walk.
 */
function deepTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
      dir(`${ROOT}/docs`, [
          dir(`${ROOT}/docs/sub`, [
              file(`${ROOT}/docs/sub/page.md`),
            ]),
        ]),
    ], { name: 'blog' });
}

describe('sash-model drag-fill', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;
  let handle: HTMLElement;
  let wrapper: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: deepTree() });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    handle = document.createElement('div');
    wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    handle.remove();
    wrapper.remove();
  });

  test('fillOrder() on a deep path opens nearest ancestor first, root last', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    expect(priv(fileTree).fillOrder()).toEqual(['docs/sub', 'docs', '']);
  });

  test('half-row slack threshold: a target height within DRAG_FILL_SLACK_PX of content-fit does not fill', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    expect(priv(fileTree).view.overrides.size).toBe(0);

    stubPointerCapture(handle);
    // DRAG_FILL_SLACK_PX is ROW_H / 2 = 14 (ROW_H is 28). startHeight is the
    // current content-fit height (highlight() already applied it), so a dy of
    // exactly 14 lands the target height AT content-fit + slack — the fill
    // condition is a strict `>`, so landing exactly on it must not open
    // anything.
    const dy = 14;
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: dy, bubbles: true }));

    expect(priv(fileTree).view.overrides.size).toBe(0);

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: dy, bubbles: true }));
  });

  test('past the slack threshold, drag-down opens the next fillOrder() level', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    expect(priv(fileTree).view.overrides.size).toBe(0);

    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    // Push well past content-fit + slack so the nearest fillOrder() level opens.
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 100, bubbles: true }));

    expect(overrideAt(fileTree, 'docs/sub')).toBe('open');

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 100, bubbles: true }));
  });

  test('one-way opening within a gesture: dragging back up after a fill does not re-close it', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);

    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    // Drag down far enough to open the sub folder (and possibly docs).
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 200, bubbles: true }));
    expect(overrideAt(fileTree, 'docs/sub')).toBe('open');
    const openedAfterFill = new Map(priv(fileTree).view.overrides);

    // Same gesture, drag back up toward the start — rule 6: clip, never close.
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 10, bubbles: true }));

    expect(priv(fileTree).view.overrides).toEqual(openedAfterFill);

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 10, bubbles: true }));
  });

  test('drag up never mutates overrides', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).handleToggleTop(0); // pre-populate overrides so there's something to (not) close
    const overridesBefore = new Map(priv(fileTree).view.overrides);
    expect(overridesBefore.size).toBeGreaterThan(0);

    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 500, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 0, bubbles: true }));

    expect(priv(fileTree).view.overrides).toEqual(overridesBefore);

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 0, bubbles: true }));
  });

  test.each([30, 50, 57])('release at %ipx of downward drag mutates only height, never overrides', (px) => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    const overridesBefore = new Map(priv(fileTree).view.overrides);

    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: px, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: px, bubbles: true }));

    // Release commits nothing but the height (rule 5) — any level(s) opened
    // during the drag stay open (fill is a pointermove effect, not a release
    // effect), and no OTHER override is touched by pointerup itself.
    for (const [k, v] of overridesBefore) {
      expect(priv(fileTree).view.overrides.get(k)).toBe(v);
    }
    expect(priv(fileTree).view.park).not.toBeNull();
  });

  test('folder-valued-leaf fill: activePath=[\'\'] (generated home) can drag-fill root\'s children', () => {
    // Simulate a generated home (no root index.md): the active path is just
    // the root itself, a folder-valued leaf per tree/spine-projection.ts's spineBody().
    setView(fileTree, { anchor: [''] });
    expect(priv(fileTree).view.overrides.size).toBe(0);
    expect(priv(fileTree).fillOrder()).toEqual(['']);

    stubPointerCapture(handle);
    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 100, bubbles: true }));

    expect(overrideAt(fileTree, '')).toBe('open');

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 100, bubbles: true }));
  });

  test('folder-valued-leaf fill survives a failed boot refresh: fillOrder() still yields the root level', async () => {
    // listTree never populates the registry,
    // yet a generated-home session must keep one openable level or every
    // drag/toggle gesture is silently dead. mount() registers the root before
    // the first listing, and a failed refresh leaves the registry untouched.
    mockListTree.mockRejectedValue(new Error('boot listing failed'));
    const bootRegistry = new EntryRegistry();
    const bootTree = new FileTree(bootRegistry);
    const bootContainer = document.createElement('div');
    document.body.appendChild(bootContainer);
    try {
      await bootTree.mount(bootContainer, ROOT, false);
      expect(bootRegistry.allIds().map(id => bootRegistry.getById(id)!.path)).toEqual(['']);
      setView(bootTree, { anchor: [''] });
      expect(priv(bootTree).fillOrder()).toEqual(['']);
    } finally {
      bootTree.unmount();
      bootContainer.remove();
    }
  });

  test('folder-valued-leaf toggle survives an empty dirIndex: handleToggleTop(0) surfaces the unavailable row', () => {
    setView(fileTree, { anchor: [''] });
    priv<{ dirIndex: Map<EntryId, unknown[]> }>(fileTree).dirIndex.clear();

    priv(fileTree).handleToggleTop(0);

    const stateRow = container.querySelector<HTMLElement>('.nav-state-row');
    expect(stateRow).not.toBeNull();
    expect(stateRow!.classList.contains('nav-state-row--unavailable')).toBe(true);
  });

  // ── Divider double-click is a real toggle, and a parked sash does not let
  //    its own controls lie ──────────────────────────────────
  //
  // Parking must close `overrides` too, or the breadcrumb chevron paints OPEN
  // over an empty tree and its first click runs the CLOSE branch against
  // invisible content (a silent no-op).
  //
  // The gesture is the native `dblclick`: Chromium-based webviews (WebView2
  // included) always report PointerEvent.detail as 0, so a `detail >= 2` check
  // on pointerdown never fires there. These tests dispatch `dblclick`, the
  // event a real double-click produces in every engine.

  /** Synthesize the divider double-click the way the browser delivers it. */
  function dividerDblClick(): void {
    handle.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, detail: 2 }));
  }

  /** The breadcrumb segment for `path`, as rendered in the strip. */
  function bcSeg(path: string): HTMLElement | null {
    return container.querySelector<HTMLElement>(`.nav-bc-strip .seg.bc[data-path="${path}"]`);
  }

  test('double-click parks a showing tree, and double-click again reveals it', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).handleToggleTop(0); // open root so there is something to hide
    expect(priv(fileTree).render.renderedRowCount()).toBeGreaterThan(0);

    dividerDblClick();
    expect(priv(fileTree).view.park).toBe(navH(0));

    // The inverse now lives on the same gesture, at the same site.
    dividerDblClick();
    expect(priv(fileTree).view.park).toBeNull();
    expect(priv(fileTree).render.renderedRowCount()).toBeGreaterThan(0);
  });

  test('reveal releases the cap without re-opening anything the user already had open', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).handleToggleTop(0);
    const openedBefore = new Map(priv(fileTree).view.overrides);

    dividerDblClick(); // park
    dividerDblClick(); // reveal

    // Releasing the cap alone was enough — the forest was never wiped, so the
    // reveal must not need to open a single extra level (rule 2 paying off).
    expect(priv(fileTree).view.overrides).toEqual(openedBefore);
  });

  test("reveal opens the breadcrumb's deepest folder when nothing is open yet", () => {
    // The case that motivated the whole review: at mount `overrides` is empty,
    // so content-fit IS the minimum height and releasing the cap reveals
    // nothing. Revealing has to be a CONTENT operation or it is a no-op.
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).parkAtMinHeight();
    expect(priv(fileTree).view.overrides.size).toBe(0);

    dividerDblClick();

    // Identical to clicking the deepest breadcrumb chevron — handleToggleTop
    // opens that folder and the chain above it, so the two can never drift.
    expect(overrideAt(fileTree, 'docs/sub')).toBe('open');
    expect(overrideAt(fileTree, 'docs')).toBe('open');
    expect(overrideAt(fileTree, '')).toBe('open');
    expect(priv(fileTree).render.renderedRowCount()).toBeGreaterThan(0);
  });

  test('a parked sash paints its breadcrumb chevrons closed, in class and in ARIA', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).handleToggleTop(0);
    expect(bcSeg('')!.classList.contains('open')).toBe(true);
    expect(bcSeg('')!.getAttribute('aria-expanded')).toBe('true');

    priv(fileTree).parkAtMinHeight();

    // Root is still 'open' in `overrides` — nothing is wiped — but nothing can
    // be seen, so the control must not claim otherwise.
    expect(overrideAt(fileTree, '')).toBe('open');
    expect(bcSeg('')!.classList.contains('open')).toBe(false);
    expect(bcSeg('')!.getAttribute('aria-expanded')).toBe('false');
  });

  test('ONE breadcrumb chevron click reveals a parked tree (toggle-parity trap)', () => {
    fileTree.highlight(`${ROOT}/docs/sub/page.md`);
    priv(fileTree).handleToggleTop(0);
    priv(fileTree).parkAtMinHeight();
    // Note what "hidden" means here: the rows still EXIST (parking never wipes
    // content — renderedRowCount stays > 0), they are simply capped out of
    // sight. That gap between "open" and "visible" is the whole bug: a control
    // reading `overrides` alone cannot see it.
    expect(priv(fileTree).render.renderedRowCount()).toBeGreaterThan(0);
    expect(wrapper.style.height).toBe(`${navH(0)}px`);

    // Exactly one click on the chevron the user can actually see. Before the
    // fix this ran the CLOSE branch against invisible content and changed
    // nothing on screen; the reveal needed a second click.
    bcSeg('')!.querySelector<HTMLElement>('.chev')!.click();

    expect(priv(fileTree).view.park).toBeNull();
    expect(priv(fileTree).render.renderedRowCount()).toBeGreaterThan(0);
    expect(parseFloat(wrapper.style.height)).toBeGreaterThan(navH(0));
  });
});
