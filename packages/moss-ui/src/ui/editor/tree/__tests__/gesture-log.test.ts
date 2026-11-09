/**
 * Tests for gesture-outcome logging.
 *
 * The logging rule:
 * "Gestures log outcomes at INFO. One line per user gesture, in production:
 * `[tree] gesture=drag-fill outcome=opened-level path=docs rows=8`,
 * `[tree] gesture=open-root outcome=no-rows cause=empty-dirIndex`."
 *
 * Covered here: click-toggle open/close/no-rows, open-root vs.
 * breadcrumb-toggle naming, and drag-fill opened-level lines.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, stubPointerCapture, installCSSEscapePolyfill, priv, dir, file, setView } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath, mockInfo } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockInfo: vi.fn().mockResolvedValue(undefined),
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

vi.mock("../../../log", () => ({
  uiEvent: vi.fn(),
  debug: vi.fn().mockResolvedValue(undefined),
  info: mockInfo,
  warn: vi.fn().mockResolvedValue(undefined),
}));

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/blog';

function makeTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
      dir(`${ROOT}/empty`, []),
      dir(`${ROOT}/posts`, [
          file(`${ROOT}/posts/article-one.md`),
        ]),
    ], { name: 'blog' });
}

function gestureLines(): string[] {
  return mockInfo.mock.calls.map((c) => String(c[0]));
}

describe('gesture-outcome logging', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree() });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);
    mockInfo.mockClear();
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('opening the root breadcrumb logs gesture=open-root outcome=opened', () => {
    priv(fileTree).handleToggleTop(0);
    expect(gestureLines()).toContainEqual(expect.stringMatching(/^\[tree\] gesture=open-root outcome=opened path=/));
  });

  test('closing the root breadcrumb logs gesture=open-root outcome=closed', () => {
    priv(fileTree).handleToggleTop(0);
    mockInfo.mockClear();
    priv(fileTree).handleToggleTop(0);
    expect(gestureLines()).toContainEqual(expect.stringMatching(/^\[tree\] gesture=open-root outcome=closed path=/));
  });

  test('a click-toggle on a tree-body folder with children logs outcome=opened with a row count', () => {
    priv(fileTree).handleToggleTop(0); // open root so "posts" renders as a row
    mockInfo.mockClear();

    priv(fileTree).toggleFolderOpen('posts');

    expect(gestureLines()).toContainEqual(expect.stringMatching(/^\[tree\] gesture=click-toggle outcome=opened path=posts rows=\d+$/));
  });

  test('a click-toggle on a genuinely empty folder logs outcome=no-rows cause=empty-dirIndex', () => {
    priv(fileTree).handleToggleTop(0); // open root so "empty" renders as a row
    mockInfo.mockClear();

    priv(fileTree).toggleFolderOpen('empty');

    expect(gestureLines()).toContainEqual('[tree] gesture=click-toggle outcome=no-rows path=empty cause=empty-dirIndex');
  });

  test('a click-toggle close logs outcome=closed', () => {
    priv(fileTree).handleToggleTop(0);
    priv(fileTree).toggleFolderOpen('posts');
    mockInfo.mockClear();

    priv(fileTree).toggleFolderOpen('posts');

    expect(gestureLines()).toContainEqual('[tree] gesture=click-toggle outcome=closed path=posts');
  });

  test('drag-fill opening a level logs gesture=drag-fill outcome=opened-level with a row count', () => {
    fileTree.highlight(`${ROOT}/posts/article-one.md`);
    mockInfo.mockClear();

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
    stubPointerCapture(handle);

    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 100, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 100, bubbles: true }));

    expect(gestureLines()).toContainEqual(expect.stringMatching(/^\[tree\] gesture=drag-fill outcome=opened-level path=posts rows=\d+$/));

    handle.remove();
    wrapper.remove();
  });

  test('releasing a drag logs gesture=drag-release outcome=parked with the released height', () => {
    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
    stubPointerCapture(handle);
    mockInfo.mockClear();

    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 40, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 40, bubbles: true }));

    expect(gestureLines()).toContainEqual(expect.stringMatching(/^\[tree\] gesture=drag-release outcome=parked height=\d+(\.\d+)?$/));

    handle.remove();
    wrapper.remove();
  });

  test('double-click on the divider (parkAtMinHeight) logs gesture=park-min-height outcome=parked', () => {
    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
    mockInfo.mockClear();

    priv(fileTree).parkAtMinHeight();

    expect(gestureLines()).toContainEqual('[tree] gesture=park-min-height outcome=parked');

    handle.remove();
    wrapper.remove();
  });

  test('a drag-fill gesture over a fully-degenerate index (no openable levels at all) still leaves a trace', () => {
    // No active file at all — fillOrder() has nothing to offer (a fix narrows
    // this case but doesn't eliminate it: a session before any file is
    // active has activePath = []). The while loop in the pointermove handler
    // used to break silently here with no INFO line, the exact "gesture left
    // no trace" failure Finding 2 flagged.
    setView(fileTree, { anchor: [] });

    const handle = document.createElement('div');
    const wrapper = document.createElement('div');
    fileTree.setupDragHandle(handle, wrapper);
    stubPointerCapture(handle);
    mockInfo.mockClear();

    handle.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, clientY: 0, bubbles: true }));
    handle.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientY: 100, bubbles: true }));

    expect(gestureLines()).toContainEqual('[tree] gesture=drag-fill outcome=none cause=empty-dirIndex');

    handle.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientY: 100, bubbles: true }));
    handle.remove();
    wrapper.remove();
  });
});
