/**
 * Tests for drag-and-drop helpers in editor-main.ts:
 * - showDropZone: toggles CSS classes on nav-body and editor elements
 * - routeDropIndicator: highlights correct drop zone based on Y coordinate
 * - handleEditorFileDrop: filters by registry, copies to project, inserts ![](path) embeds written from the page
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { savedWithHash } from './disk-hash';

// As editor.html does: the desktop's host record, then the editor. After a
// module reset both are fresh, so they load together.
async function bootEditorMain() {
  // A fresh host per boot, as the editor webview loads it; the bus records emits.
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit, (name, handler) => listenCallbacks.set(name, handler as Function));
  host.onFileDrop = mockOnWebviewDragDrop;
  host.copyIn = mockCopyIn;
  host.isDirectory = mockIsDirectory;
  host.assetRefForPath = mockAssetRefForPath;
  return import('../editor-main');
}

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const {
  mockEmit, mockCommands, listenCallbacks, mockCopyIn, mockIsDirectory, mockAssetRefForPath,
  MockChipBar, MockCmEditor, MockFileTree,
  mockOnWebviewDragDrop, dragDropCallback,
  mockDispatch, mockPosAtCoords, mockToast,
} = vi.hoisted(() => {
  const listenCallbacks = new Map<string, Function>();
  // Capture the onDragDropEvent callback so tests can invoke it
  let _dragDropCb: Function | null = null;
  const mockEmit = vi.fn().mockResolvedValue(undefined);
  const mockCommands = {
    pageFacts: vi.fn().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } }),
    getSchema: vi.fn(),
    getProjectPath: vi.fn(),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    getFileInfo: vi.fn().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn(),
    // Default: no setting → images land next to the page (attachmentDirFor's fallback).
    resolveAttachmentDir: vi.fn().mockResolvedValue({ status: 'error', error: 'unset' }),
  };
  const MockChipBar = vi.fn().mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    revealField: vi.fn(),
  }));
  const mockDispatch = vi.fn();
  const mockPosAtCoords = vi.fn().mockReturnValue(null);
  const mockToast = vi.fn();
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({
      scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
      state: { selection: { main: { head: 0 } }, doc: { length: 99 }, field: () => undefined },
      dispatch: mockDispatch,
      posAtCoords: mockPosAtCoords,
    }),
  }));
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null),
    collapse: vi.fn(),
    collapseForReopen: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
    // os-drag-in methods
    setDropHighlight: vi.fn(),
    springLoadIfHoverIs: vi.fn(),
    getFolderAtPosition: vi.fn().mockReturnValue(null),
    pathOf: vi.fn().mockReturnValue(null),
    projectRootPath: '/Users/test/blog',
    refreshAndRender: vi.fn().mockResolvedValue(undefined),
    // in-app drag routing (onDragDropEvent intercepts the native session for
    // rows too — see hasActiveInternalDrag's doc in file-tree.ts). Defaults
    // to false/no-op here since this test file exercises the OS-drag-in
    // (real Finder file) path, not in-app row drags. The internal-drop
    // routing tests below flip these per-instance after init.
    hasActiveInternalDrag: vi.fn().mockReturnValue(false),
    handleInternalDragOver: vi.fn(),
    handleInternalDrop: vi.fn(),
    activeInternalDragEntryIds: vi.fn().mockReturnValue([]),
    resolveDropEntries: vi.fn().mockReturnValue([]),
    clearInternalDragSession: vi.fn(),
    // Edge-scroll methods are public so editor-main can call them
    updateEdgeScroll: vi.fn(),
    stopEdgeScroll: vi.fn(),
  }));
  const mockCopyIn = vi.fn();
  const mockIsDirectory = vi.fn(async () => false);
  // Stands in for the host's asset reference (the engine's asset_ref_in), for a page in `posts/` of
  // /Users/test/blog: the encoded path from the page.
  const mockAssetRefForPath = vi.fn(async (abs: string, _fromPage: string) => {
    const rel = abs.replace('/Users/test/blog/', '');
    const insert = (rel.startsWith('posts/') ? rel.slice(6) : `../${rel}`).replace(/ /g, '%20');
    return { insert, label: rel, detail: null, kind: 'asset' as const };
  });
  // The host supplies the boot-path OS drag-drop seam. Capture the handler
  // editor-main registers so tests can drive native drops directly.
  const mockOnWebviewDragDrop = vi.fn().mockImplementation(async (cb: Function) => {
    _dragDropCb = cb;
    return vi.fn();
  });

  return {
    mockEmit, mockCommands, listenCallbacks, mockCopyIn, mockIsDirectory, mockAssetRefForPath,
    MockChipBar, MockCmEditor, MockFileTree,
    mockOnWebviewDragDrop,
    dragDropCallback: { get: () => _dragDropCb, set: (v: Function | null) => { _dragDropCb = v; } },
    mockDispatch, mockPosAtCoords, mockToast,
  };
});




vi.mock('../../engine', async () => ({ engine: (await import('./disk-hash')).withContentHash(mockCommands) }));

vi.mock('../chip-bar', () => ({
  ChipBar: MockChipBar,
}));

vi.mock('../cm-editor', () => ({
  CmEditor: MockCmEditor,
}));

vi.mock('../file-viewer', () => ({
  showFileViewer: vi.fn(),
  destroyFileViewer: vi.fn(),
}));

vi.mock('../tree', () => ({
  FileTree: MockFileTree,
}));

vi.mock("../../shared/ui/toast/toast-manager", () => ({
  showToast: mockToast,
  showAck: vi.fn(),
  configureToastRuntime: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const DOC_LEN = 99;

/**
 * editorBootstrap delegate — routes through the existing getSchema /
 * getProjectPath mocks so per-test overrides keep working unchanged.
 * Struct fields are snake_case (specta does not camelCase struct fields);
 * `initial_file: null` makes loadFile fall back to the mocked parseFrontmatter.
 */
function applyEditorBootstrapDelegate(): void {
  mockCommands.editorBootstrap.mockImplementation(async (_src: string | null) => {
    const s = await mockCommands.getSchema();
    if (s?.status !== 'ok') return { status: 'error', error: s?.error };
    const p = await mockCommands.getProjectPath();
    return {
      status: 'ok',
      data: {
        schema: s.data,
        project_path: p?.status === 'ok' ? p.data : null,
        initial_file: null,
      },
    };
  });
}
applyEditorBootstrapDelegate();

function resetMocks(): void {
  listenCallbacks.clear();
  dragDropCallback.set(null);
  mockEmit.mockReset().mockResolvedValue(undefined);
  mockCommands.getSchema.mockReset().mockResolvedValue({
    status: 'ok',
    data: { fields: [] },
  });
  mockCommands.getProjectPath.mockReset().mockResolvedValue({
    status: 'error',
    error: 'no project',
  });
  mockCommands.editorBootstrap.mockReset();
  applyEditorBootstrapDelegate();
  mockCommands.parseFrontmatter.mockReset();
  mockCommands.getFileInfo.mockReset().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } });
  mockCommands.saveEditorContent.mockReset();
  // Default to "null result" (Ok(None)) so PreviewFollower's active-path
  // subscriber doesn't blow up on unhandled-undefined when drag-drop tests
  // don't bother stubbing the URL resolver. Individual tests override.
  mockCommands.resolveUrlForFile.mockReset().mockResolvedValue({ status: 'ok', data: null });
  mockCopyIn.mockReset();
  mockOnWebviewDragDrop.mockReset().mockImplementation(async (cb: Function) => {
    dragDropCallback.set(cb);
    return vi.fn();
  });
  mockDispatch.mockReset();
  mockPosAtCoords.mockReset().mockReturnValue(null);
  mockToast.mockReset();
  mockCommands.pageFacts.mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } });
  MockChipBar.mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    revealField: vi.fn(),
  }));
  MockCmEditor.mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({
      scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
      state: { selection: { main: { head: 0 } }, doc: { length: DOC_LEN }, field: () => undefined },
      dispatch: mockDispatch,
      posAtCoords: mockPosAtCoords,
    }),
  }));
  MockFileTree.mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null),
    collapse: vi.fn(),
    collapseForReopen: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
    // os-drag-in methods
    setDropHighlight: vi.fn(),
    springLoadIfHoverIs: vi.fn(),
    getFolderAtPosition: vi.fn().mockReturnValue(null),
    pathOf: vi.fn().mockReturnValue(null),
    projectRootPath: '/Users/test/blog',
    refreshAndRender: vi.fn().mockResolvedValue(undefined),
    // in-app drag routing (onDragDropEvent intercepts the native session for
    // rows too — see hasActiveInternalDrag's doc in file-tree.ts). Defaults
    // to false/no-op here since this test file exercises the OS-drag-in
    // (real Finder file) path, not in-app row drags. The internal-drop
    // routing tests below flip these per-instance after init.
    hasActiveInternalDrag: vi.fn().mockReturnValue(false),
    handleInternalDragOver: vi.fn(),
    handleInternalDrop: vi.fn(),
    activeInternalDragEntryIds: vi.fn().mockReturnValue([]),
    resolveDropEntries: vi.fn().mockReturnValue([]),
    clearInternalDragSession: vi.fn(),
    // Edge-scroll methods are public so editor-main can call them
    updateEdgeScroll: vi.fn(),
    stopEdgeScroll: vi.fn(),
  }));
}

/** DOM that includes elements expected by showDropZone and routeDropIndicator. */
function setupDropZoneDOM(): void {
  document.body.innerHTML = `
    <div id="nav-surface">
      <div id="nav-grid"><div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div></div>
    </div>
    <div id="divider"></div>
    <div class="nav-body"></div>
    <div id="chip-bar"></div>
    <div id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    <div id="moss-save-indicator"></div>
  `;
}

// ---------------------------------------------------------------------------
// Tests: showDropZone
// ---------------------------------------------------------------------------

describe('showDropZone', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupDropZoneDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('showDropZone(true) adds moss-drop-active to editor (no longer adds whole-tree outline to nav-body)', async () => {
    const { showDropZone } = await bootEditorMain();

    const navBody = document.querySelector('.nav-body')!;
    const cmEl = document.getElementById('moss-editor-cm')!;

    showDropZone(true);

    // Only the editor is outlined; the tree's body never is.
    expect(navBody.classList.contains('moss-external-drop-active')).toBe(false);
    expect(cmEl.classList.contains('moss-drop-active')).toBe(true);
  });

  test('showDropZone(false) removes CSS classes', async () => {
    const { showDropZone } = await bootEditorMain();

    const navBody = document.querySelector('.nav-body')!;
    const cmEl = document.getElementById('moss-editor-cm')!;

    // Add first, then remove
    showDropZone(true);
    showDropZone(false);

    expect(navBody.classList.contains('moss-external-drop-active')).toBe(false);
    expect(cmEl.classList.contains('moss-drop-active')).toBe(false);
  });

  test('showDropZone does not throw when elements are missing', async () => {
    document.body.innerHTML = ''; // no nav-body or editor
    const { showDropZone } = await bootEditorMain();

    expect(() => showDropZone(true)).not.toThrow();
    expect(() => showDropZone(false)).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// Tests: OS-drop hint pill
// ---------------------------------------------------------------------------

describe('OS-drop hint pill', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupDropZoneDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('routeDropIndicator: hint pill appears in document.body (fixed, not inside cmEl) when over editor', async () => {
    // The pill must be appended to document.body with position:fixed so it
    // escapes cmEl's overflow-y:auto scroll container — an absolute child of
    // a scrollable element is clipped to the content bottom on long documents.
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    routeDropIndicator(400); // over editor

    // Pill must be in document.body, NOT inside cmEl (overflow container).
    const pill = document.querySelector('.moss-drop-hint') as HTMLElement | null;
    expect(pill).not.toBeNull();
    expect(pill?.textContent).toBe('Drop to embed here');
    expect(cmEl.querySelector('.moss-drop-hint')).toBeNull(); // NOT inside cmEl

    // Verify fixed coordinates derived from getBoundingClientRect():
    // left = rect.left + rect.width / 2 = 0 + 400/2 = 200
    // bottom = window.innerHeight - rect.bottom + 16 = 768 - 600 + 16 = 184
    expect(pill?.style.left).toBe('200px');
    expect(pill?.style.bottom).toBe('184px');
  });

  test('routeDropIndicator: hint pill is removed from document.body when over nav-body (not editor)', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    // First, put the hint in by routing over editor
    routeDropIndicator(400);
    expect(document.querySelector('.moss-drop-hint')).not.toBeNull();

    // Then route away from editor → hint should disappear
    routeDropIndicator(100);
    expect(document.querySelector('.moss-drop-hint')).toBeNull();
  });

  test('showDropZone(false): removes hint pill from document.body on drop/leave', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator, showDropZone } = await bootEditorMain();

    // Create hint via routing over editor
    routeDropIndicator(400);
    expect(document.querySelector('.moss-drop-hint')).not.toBeNull();

    // drop/leave calls showDropZone(false) — hint must be removed
    showDropZone(false);
    expect(document.querySelector('.moss-drop-hint')).toBeNull();
  });

  test('routeDropIndicator: only one hint element is created even if called repeatedly', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    routeDropIndicator(400);
    routeDropIndicator(400);
    routeDropIndicator(400);

    expect(document.querySelectorAll('.moss-drop-hint').length).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Tests: routeDropIndicator
// ---------------------------------------------------------------------------

describe('routeDropIndicator', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupDropZoneDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('highlights editor when Y is over editor rect', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    // Mock getBoundingClientRect to return a known rect
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    const navBody = document.querySelector('.nav-body')!;

    // Y = 400 is within [200, 600]
    routeDropIndicator(400);

    expect(cmEl.classList.contains('moss-drop-active')).toBe(true);
    expect(navBody.classList.contains('moss-external-drop-active')).toBe(false);
  });

  test('does not activate editor when Y is above editor rect (no whole-tree outline)', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    const navBody = document.querySelector('.nav-body')!;

    // Y = 100 is above [200, 600] — cursor is over file tree, not editor
    routeDropIndicator(100);

    // The tree's body never gets moss-external-drop-active.
    expect(navBody.classList.contains('moss-external-drop-active')).toBe(false);
    // Editor is NOT active either — cursor is not over it
    expect(cmEl.classList.contains('moss-drop-active')).toBe(false);
  });

  test('does not activate editor when Y is below editor rect (no whole-tree outline)', async () => {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 200, bottom: 600, left: 0, right: 400, width: 400, height: 400,
    });

    const { routeDropIndicator } = await bootEditorMain();

    const navBody = document.querySelector('.nav-body')!;

    // Y = 700 is below [200, 600] — cursor is below editor
    routeDropIndicator(700);

    // The tree's body never gets moss-external-drop-active.
    expect(navBody.classList.contains('moss-external-drop-active')).toBe(false);
    // Editor is NOT active — cursor is not over it
    expect(cmEl.classList.contains('moss-drop-active')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Tests: handleEditorFileDrop (renamed from handleImageDrop)
// ---------------------------------------------------------------------------

describe('handleEditorFileDrop', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupDropZoneDOM();
    resetMocks();

    // Enable project path so file tree gets initialized
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Import editor-main, wait for init, load a file to set currentFilePath + cmEditor. */
  async function initWithFile(filePath = 'posts/hello.md') {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });

    const mod = await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Load a file so currentFilePath and cmEditor are set
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: filePath, isDir: false });
    mockDispatch.mockClear();

    return mod;
  }

  test('filters and processes only accepted types (image/video/audio/etc) as embeds', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [
        { original_name: 'photo.png', final_name: 'photo.png', final_path: '/Users/test/blog/posts/photo.png', relative_path: 'posts/photo.png' },
      ], skipped: [] },
    });

    await mod.handleEditorFileDrop(['/external/photo.png', '/external/data.zip']);

    // Only the accepted (acceptOnDrop) image should be passed to copyFilesToProject for embed
    const calls = mockCopyIn.mock.calls;
    const imageCall = calls.find((c: string[][]) => (c[0] as string[]).includes('/external/photo.png'));
    expect(imageCall).toBeDefined();
    expect(imageCall![0]).toEqual(['/external/photo.png']);

    // Non-accepted files are simply dropped (no file-tree forwarding path).
  });

  test('inserts the standard form, the name encoded as a Markdown destination', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [
        { original_name: 'my photo.png', final_name: 'my photo.png', final_path: '/Users/test/blog/posts/my photo.png', relative_path: 'posts/my photo.png' },
      ], skipped: [] },
    });

    await mod.handleEditorFileDrop(['/external/my photo.png']);

    expect(mockDispatch).toHaveBeenCalled();
    const insertedText = mockDispatch.mock.calls[0][0].changes.insert;
    // Standard form; the space is percent-encoded, nothing else is.
    expect(insertedText).toBe('![](my%20photo.png)\n');
  });

  test('an image the attachment folder puts elsewhere is written as the path from the page', async () => {
    const mod = await initWithFile('posts/hello.md');
    mockCommands.resolveAttachmentDir.mockResolvedValueOnce({ status: 'ok', data: 'assets/图片' });
    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [
        { original_name: '照片 1.png', final_name: '照片 1.png', final_path: '/Users/test/blog/assets/图片/照片 1.png', relative_path: 'assets/图片/照片 1.png' },
      ], skipped: [] },
    });

    await mod.handleEditorFileDrop(['/external/照片 1.png']);

    expect(mockCopyIn).toHaveBeenCalledWith(['/external/照片 1.png'], '/Users/test/blog/assets/图片');
    // The destination is asked of the engine for the file where it landed.
    expect(mockAssetRefForPath).toHaveBeenCalledWith('/Users/test/blog/assets/图片/照片 1.png', 'posts/hello.md');
    expect(mockDispatch.mock.calls[0][0].changes.insert).toBe('![](../assets/图片/照片%201.png)\n');
  });

  test('inserts at the DROP POINT when coordinates are provided', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [
        { original_name: 'p.png', final_name: 'p.png', final_path: '/p/p.png', relative_path: 'p.png' },
      ], skipped: [] },
    });

    // The view maps the drop coordinates to position 7.
    mockPosAtCoords.mockReturnValueOnce(7);

    await mod.handleEditorFileDrop(['/external/p.png'], { x: 120, y: 80 });

    expect(mockDispatch).toHaveBeenCalledWith(expect.objectContaining({
      changes: expect.objectContaining({ from: 7 }),
    }));
  });

  test('does not insert wikilink when copy fails', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'error',
      error: 'Disk full',
    });

    await mod.handleEditorFileDrop(['/external/photo.png']);

    expect(mockDispatch).not.toHaveBeenCalled();
  });

  // An `ok` result carrying an empty `copied` list is the shape that produced
  // the reported silence: the user dragged an iCloud-evicted photo in, the copy
  // failed with EDEADLK, and the command still returned success. Nothing was
  // embedded and nothing was said.
  test('an ok result that landed nothing still tells the user', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [], skipped: [{ original_name: '可愛！.jpg', reason: 'still_in_cloud' }] },
    });

    await mod.handleEditorFileDrop(['/external/可愛！.jpg']);

    expect(mockDispatch).not.toHaveBeenCalled();
    expect(mockToast).toHaveBeenCalledWith(expect.objectContaining({
      severity: 'NeedsAction',
      what: expect.stringContaining('可愛！.jpg'),
    }));
  });

  test('handles multiple embeddable files — inserts all as embeds', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [
        { original_name: 'a.png', final_name: 'a.png', final_path: '/p/a.png', relative_path: 'a.png' },
        { original_name: 'b.jpg', final_name: 'b.jpg', final_path: '/p/b.jpg', relative_path: 'b.jpg' },
      ], skipped: [] },
    });

    await mod.handleEditorFileDrop(['/external/a.png', '/external/b.jpg']);

    const insertedText = mockDispatch.mock.calls[0][0].changes.insert;

    // One embed per image, written from the page (both landed beside it)
    expect(insertedText).toBe('![](a.png)\n![](b.jpg)\n');
  });


  test('embeds a video in the standard form at the drop point', async () => {
    const mod = await initWithFile('posts/hello.md');
    mockCopyIn.mockResolvedValue({ status: 'ok',
      data: { copied: [{ original_name:'a.mp4', final_name:'a.mp4', final_path:'/p/posts/a.mp4', relative_path:'posts/a.mp4' }], skipped: [] } });
    mockPosAtCoords.mockReturnValue(12);
    await mod.handleEditorFileDrop(['/abs/a.mp4'], { x: 5, y: 5 });
    const call = mockDispatch.mock.calls[0][0];
    expect(call.changes.insert).toBe('![](a.mp4)\n');
    expect(call.changes.from).toBe(12);                    // at drop pos
  });

  test('unsupported type imports + toasts, does not silently forward', async () => {
    const mod = await initWithFile('posts/hello.md');
    mockCopyIn.mockResolvedValue({ status:'ok',
      data: { copied: [{ original_name:'a.zip', final_name:'a.zip', final_path:'/p/posts/a.zip', relative_path:'posts/a.zip' }], skipped: [] } });
    await mod.handleEditorFileDrop(['/abs/a.zip'], { x: 5, y: 5 });
    expect(mockToast).toHaveBeenCalledWith(expect.objectContaining({ what: expect.stringContaining('not embeddable') }));
    expect(mockDispatch).not.toHaveBeenCalled();
  });

  test('posAtCoords null falls back to document end (not cursor) — a deliberate change from current code', async () => {
    const mod = await initWithFile('posts/hello.md');
    mockCopyIn.mockResolvedValue({ status:'ok', data: { copied: [{ original_name:'a.png', final_name:'a.png', final_path:'/p/posts/a.png', relative_path:'posts/a.png' }], skipped: [] } });
    mockPosAtCoords.mockReturnValue(null);
    await mod.handleEditorFileDrop(['/abs/a.png'], { x: 5, y: 5 });
    expect(mockDispatch.mock.calls[0][0].changes.from).toBe(DOC_LEN); // = 99 from the mock doc
  });

  // --------------- Double-fire coalescing test ---------------

  test('double-fire coalesce — same paths+position fires copyFilesToProject only once', async () => {
    await initWithFile('posts/hello.md');

    // We need to set up the editor rect so isOverEditor is true
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 0, bottom: 1000, left: 0, right: 1000, width: 1000, height: 1000,
    });

    mockCopyIn.mockResolvedValue({ status: 'ok',
      data: { copied: [{ original_name: 'img.png', final_name: 'img.png', final_path: '/p/posts/img.png', relative_path: 'posts/img.png' }], skipped: [] } });

    // Fire the captured drag-drop callback with 'drop' payload twice synchronously
    const cb = dragDropCallback.get();
    expect(cb).not.toBeNull();

    const dropPayload = {
      payload: {
        type: 'drop',
        paths: ['/abs/img.png'],
        x: 10, y: 10,
      },
    };

    // Fire twice synchronously (both started before any microtasks run) — the second should be a no-op (same key).
    // Do NOT await between calls; Tauri's double-fire happens in the same tick.
    const p1 = cb!(dropPayload);
    const p2 = cb!(dropPayload);
    await Promise.all([p1, p2]);

    // copyFilesToProject should have been called at most once (the dedup guard prevents double-import).
    expect(mockCopyIn.mock.calls.length).toBeLessThanOrEqual(1);
    // dispatch is called at most twice per real drop: once for setOsDropCaret.of(null)
    // and once for the embed insertion in handleEditorFileDrop.
    // A deduped second drop should not add more dispatches (it returns early before any dispatch).
    expect(mockDispatch.mock.calls.length).toBeLessThanOrEqual(2);
  });

  // Tauri takes the drop before the DOM sees it, so the pane's lead listeners
  // never fire; the native handler has to hand the editor the lead itself.
  test('a drop onto the editor hands it the lead, so the preview follows the embed', async () => {
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({
        scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
        state: {
          selection: { main: { head: 0 } },
          doc: { length: DOC_LEN, lineAt: () => ({ number: 5 }), toString: () => '# Hello' },
          field: () => undefined,
        },
        dispatch: mockDispatch,
        posAtCoords: mockPosAtCoords,
      }),
    }));
    await initWithFile('posts/hello.md');
    const { onCaretMove } = MockCmEditor.mock.calls.at(-1)![0] as { onCaretMove: () => void };
    document.getElementById('moss-editor-cm')!.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 0, bottom: 1000, left: 0, right: 1000, width: 1000, height: 1000,
    });
    mockCopyIn.mockResolvedValue({ status: 'ok',
      data: { copied: [{ original_name: 'img.png', final_name: 'img.png', final_path: '/p/posts/img.png', relative_path: 'posts/img.png' }], skipped: [] } });

    await dragDropCallback.get()!({ type: 'drop', paths: ['/abs/img.png'], x: 10, y: 10 });
    mockEmit.mockClear();
    vi.useFakeTimers();
    onCaretMove();
    vi.advanceTimersByTime(200);
    vi.useRealTimers();

    expect(mockEmit).toHaveBeenCalledWith(
      'scroll-sync:editor-to-preview',
      expect.objectContaining({ line: 5, onlyIfOffscreen: true }),
    );
  });

  // --------------- OS folder drop → ![](name/) embed ---------------

  test('mixed drop: embeddable file embed is still inserted when folder copy fails (partial-success fix)', async () => {
    const mod = await initWithFile('posts/hello.md');

    // Stat: first path is a file (isDirectory:false), second is a folder (isDirectory:true).
    // The global mock defaults to { isDirectory: false }; we use mockResolvedValueOnce to
    // return a different value for the folder call.
    mockIsDirectory
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);

    // First copyFilesToProject call (embeddable file) succeeds.
    // Second call (folder) fails.
    mockCopyIn
      .mockResolvedValueOnce({
        status: 'ok',
        data: { copied: [{ original_name: 'photo.png', final_name: 'photo.png', final_path: '/p/posts/photo.png', relative_path: 'posts/photo.png' }], skipped: [] },
      })
      .mockResolvedValueOnce({ status: 'error', error: 'Permission denied' });

    mockPosAtCoords.mockReturnValue(5);
    await mod.handleEditorFileDrop(['/external/photo.png', '/external/webapp'], { x: 5, y: 5 });

    // The embeddable file wikilink must still be inserted even though the folder copy failed.
    const insertCall = mockDispatch.mock.calls.find(
      (call: Array<{ changes?: { insert?: string } }>) => call[0]?.changes?.insert?.includes('![](photo.png)'),
    );
    expect(insertCall).toBeDefined();
    expect(insertCall![0].changes.insert).toContain('![](photo.png)');

    // The folder wikilink must NOT be inserted (folder copy failed).
    const badCall = mockDispatch.mock.calls.find(
      (call: Array<{ changes?: { insert?: string } }>) => call[0]?.changes?.insert?.includes('![[webapp/]]'),
    );
    expect(badCall).toBeUndefined();

    // A toast must have been shown for the folder failure.
    expect(mockToast).toHaveBeenCalledWith(expect.objectContaining({ what: expect.stringContaining('folder') }));
  });

  test('flushes the save immediately after inserting the embed (no 800ms debounce wait)', async () => {
    // The drop is a user edit that must persist NOW: the just-copied asset
    // already triggered a rebuild, and we want that single rebuild to see the
    // embed too. Wire the mock editor's dispatch to fire onChange (as the real
    // CM6 updateListener does) so the insert marks the doc dirty, then assert
    // the write command ran synchronously within handleEditorFileDrop rather
    // than after the autosave debounce.
    let capturedOnChange: (() => void) | null = null;
    MockCmEditor.mockImplementation((opts: { onChange: () => void }) => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue('# Hello'),
      getView: vi.fn().mockReturnValue({
        scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
        state: { selection: { main: { head: 0 } }, doc: { length: DOC_LEN }, field: () => undefined },
        dispatch: vi.fn().mockImplementation((transaction) => {
          if (transaction.changes) {
            capturedOnChange = opts.onChange;
            capturedOnChange();
          }
        }),
        posAtCoords: mockPosAtCoords,
      }),
    }));
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);

    const mod = await initWithFile('posts/hello.md');
    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [{ original_name: 'a.png', final_name: 'a.png', final_path: '/p/posts/a.png', relative_path: 'posts/a.png' }], skipped: [] },
    });
    mockPosAtCoords.mockReturnValue(5);

    await mod.handleEditorFileDrop(['/abs/a.png'], { x: 5, y: 5 });

    // The embed was persisted immediately (flushPendingSave → saveActiveDocument
    // → saveEditorContent), not deferred to the 800ms debounce.
    expect(mockCommands.saveEditorContent).toHaveBeenCalled();
  });

  // --------------- Tree refresh after editor drops (missing-refresh fix) ---------------
  // Dropping onto the EDITOR copies the file into the vault just like dropping
  // onto the TREE does — so it must run the same afterVaultImport follow-through
  // (fileTree.refreshAndRender), or the import stays invisible in the tree.

  /** The FileTree instance created by the current test's init (last constructed). */
  function currentTreeInstance() {
    return MockFileTree.mock.results[MockFileTree.mock.results.length - 1]?.value;
  }

  test('refreshes the file tree (awaited) after an embeddable drop copies a file', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [{ original_name: 'photo.png', final_name: 'photo.png', final_path: '/Users/test/blog/posts/photo.png', relative_path: 'posts/photo.png' }], skipped: [] },
    });

    const treeInstance = currentTreeInstance();
    treeInstance.refreshAndRender.mockClear();

    // Resolve refreshAndRender on a macrotask and record completion, so the
    // assertion below proves the handler AWAITED it (a fire-and-forget call
    // would leave refreshCompleted false when handleEditorFileDrop resolves).
    let refreshCompleted = false;
    treeInstance.refreshAndRender.mockImplementation(
      () => new Promise<void>((resolve) => setTimeout(() => { refreshCompleted = true; resolve(); }, 0)),
    );

    await mod.handleEditorFileDrop(['/external/photo.png']);

    expect(treeInstance.refreshAndRender).toHaveBeenCalled();
    expect(refreshCompleted).toBe(true);
  });

  test('refreshes the file tree after a non-embeddable (import-only) drop that copies but skips the embed', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [{ original_name: 'a.zip', final_name: 'a.zip', final_path: '/p/posts/a.zip', relative_path: 'posts/a.zip' }], skipped: [] },
    });

    const treeInstance = currentTreeInstance();
    treeInstance.refreshAndRender.mockClear();

    await mod.handleEditorFileDrop(['/abs/a.zip']);

    // The import-only branch returns before any embed insert (allEmbeddable is
    // empty) — but it COPIED a file, so the tree must still refresh.
    expect(mockDispatch).not.toHaveBeenCalled();
    expect(treeInstance.refreshAndRender).toHaveBeenCalled();
  });

  test('does not refresh the tree when the copy fails (nothing landed in the vault)', async () => {
    const mod = await initWithFile();

    mockCopyIn.mockResolvedValue({ status: 'error', error: 'Disk full' });

    const treeInstance = currentTreeInstance();
    treeInstance.refreshAndRender.mockClear();

    await mod.handleEditorFileDrop(['/external/photo.png']);

    expect(treeInstance.refreshAndRender).not.toHaveBeenCalled();
  });

  test('refreshes the tree when the folder copy fails but the embeddable file copy landed (partial success)', async () => {
    const mod = await initWithFile();

    // First path is a file, second is a folder (same shape as the
    // partial-success test above).
    mockIsDirectory
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);

    mockCopyIn
      .mockResolvedValueOnce({
        status: 'ok',
        data: { copied: [{ original_name: 'photo.png', final_name: 'photo.png', final_path: '/p/posts/photo.png', relative_path: 'posts/photo.png' }], skipped: [] },
      })
      .mockResolvedValueOnce({ status: 'error', error: 'Permission denied' });

    const treeInstance = currentTreeInstance();
    treeInstance.refreshAndRender.mockClear();

    await mod.handleEditorFileDrop(['/external/photo.png', '/external/webapp'], { x: 5, y: 5 });

    // The file copy landed on disk even though the folder copy failed —
    // the tree must reflect it.
    expect(treeInstance.refreshAndRender).toHaveBeenCalled();
  });

  test('OS folder drop (Finder→editor) inserts ![](name/) after importing', async () => {
    const mod = await initWithFile('posts/hello.md');

    // Make stat() report the dropped path as a directory
    mockIsDirectory.mockResolvedValue(true);

    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [{ original_name: 'webapp', final_name: 'webapp', final_path: '/Users/test/blog/posts/webapp', relative_path: 'posts/webapp' }], skipped: [] },
    });

    mockPosAtCoords.mockReturnValue(5);
    await mod.handleEditorFileDrop(['/external/webapp'], { x: 5, y: 5 });

    // Should embed the folder with a trailing slash
    const insertedText = mockDispatch.mock.calls.find(
      (call: Array<{ changes?: { insert?: string } }>) => call[0]?.changes?.insert?.includes('![](webapp/)'),
    );
    expect(insertedText).toBeDefined();
    expect(insertedText![0].changes.insert).toContain('![](webapp/)');
  });
});

// ---------------------------------------------------------------------------
// Tests: internal tree→editor drop routing (Flow 3)
//
// Tauri's drag_drop_handler intercepts the ENTIRE native drag session — an
// in-app row drag released over the editor arrives ONLY as an onDragDropEvent
// 'drop' with payload.paths = [] (no real OS file). The routing must detect
// the internal session via fileTree.hasActiveInternalDrag() and insert the
// `![[name]]` embed via the shared cm-drop helper, NOT fall through to
// handleEditorFileDrop's copy path (which silently no-ops on empty paths —
// the pre-existing gap this closes).
// ---------------------------------------------------------------------------

describe('internal tree→editor drop routing (Flow 3)', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupDropZoneDOM();
    resetMocks();

    // Enable project path so file tree gets initialized
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Import editor-main, wait for init, load a file to set currentFilePath + cmEditor. */
  async function initWithFile(filePath = 'posts/hello.md') {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });

    const mod = await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: filePath, isDir: false });

    return mod;
  }

  /** The FileTree instance created by the current test's init (last constructed). */
  function currentTreeInstance() {
    return MockFileTree.mock.results[MockFileTree.mock.results.length - 1]?.value;
  }

  /** Make the editor rect cover the drop point so isOverEditor is true. */
  function coverEditorRect() {
    const cmEl = document.getElementById('moss-editor-cm')!;
    cmEl.getBoundingClientRect = vi.fn().mockReturnValue({
      top: 0, bottom: 1000, left: 0, right: 1000, width: 1000, height: 1000,
    });
  }

  /** Arm the mocked tree with an active internal drag carrying entry id 5. */
  function armInternalDrag(tree: ReturnType<typeof currentTreeInstance>) {
    tree.hasActiveInternalDrag.mockReturnValue(true);
    tree.activeInternalDragEntryIds.mockReturnValue([5]);
    tree.resolveDropEntries.mockReturnValue(['img/over-hell-gate.jpg']);
  }

  test('internal drag dropped over the editor inserts the embed, flushes the save, and clears the session', async () => {
    // Wire the mock view's dispatch → onChange for changes-bearing
    // transactions (as the real CM6 updateListener does) so the insert marks
    // the doc dirty and flushPendingSave actually routes a write. Effects-only
    // dispatches (the setOsDropCaret clear) must NOT mark dirty.
    const dispatches: Array<{ changes?: { from?: number; insert?: string } }> = [];
    MockCmEditor.mockImplementation((opts: { onChange: () => void }) => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue('# Hello'),
      getView: vi.fn().mockReturnValue({
        scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
        state: { selection: { main: { head: 0 } }, doc: { length: DOC_LEN }, field: () => undefined },
        dispatch: vi.fn().mockImplementation((tx: { changes?: object }) => {
          dispatches.push(tx);
          if (tx?.changes) opts.onChange();
        }),
        posAtCoords: mockPosAtCoords,
      }),
    }));
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);

    await initWithFile('posts/hello.md');
    coverEditorRect();

    const tree = currentTreeInstance();
    armInternalDrag(tree);
    mockPosAtCoords.mockReturnValue(7);

    const cb = dragDropCallback.get();
    expect(cb).not.toBeNull();
    // Internal drag: payload.paths is EMPTY — no real OS file behind it.
    await cb!({ type: 'drop', paths: [], x: 40, y: 40 });

    // The embed was inserted at the drop position (snapToBlockBoundary
    // passes the raw pos through for mocked docs without lineAt)...
    const insertTx = dispatches.find((tx) => tx?.changes?.insert?.includes('over-hell-gate.jpg'));
    expect(insertTx).toBeDefined();
    expect(insertTx!.changes!.from).toBe(7);
    // ...written as the path from the open page (posts/hello.md)...
    expect(insertTx!.changes!.insert).toBe('![](../img/over-hell-gate.jpg)\n');
    // ...resolved through the tree's accessor pair...
    expect(tree.activeInternalDragEntryIds).toHaveBeenCalled();
    expect(tree.resolveDropEntries).toHaveBeenCalledWith([5]);
    // ...persisted immediately (flushPendingSave → saveEditorContent),
    // not left to the autosave debounce...
    expect(mockCommands.saveEditorContent).toHaveBeenCalled();
    // ...and the drag session was consumed.
    expect(tree.clearInternalDragSession).toHaveBeenCalled();

    // The gesture must NOT reach handleEditorFileDrop's copy path (paths=[]
    // silently no-ops there — the old gap) nor the tree-target internal drop.
    expect(mockCopyIn).not.toHaveBeenCalled();
    expect(tree.handleInternalDrop).not.toHaveBeenCalled();
  });

  test('non-markdown document: no insert, no flush, session still cleared', async () => {
    await initWithFile('styles/site.css');
    coverEditorRect();

    const tree = currentTreeInstance();
    armInternalDrag(tree);
    mockPosAtCoords.mockReturnValue(7);
    mockDispatch.mockClear();

    const cb = dragDropCallback.get();
    await cb!({ type: 'drop', paths: [], x: 40, y: 40 });

    // Nothing inserted into the .css document (only the effects-only
    // setOsDropCaret clear dispatch is allowed)...
    const changeTx = mockDispatch.mock.calls.find(
      (call: Array<{ changes?: object }>) => call[0]?.changes,
    );
    expect(changeTx).toBeUndefined();
    // ...the markdown gate short-circuits before resolving names...
    expect(tree.resolveDropEntries).not.toHaveBeenCalled();
    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();
    // ...but the session is still consumed so the finished gesture can't
    // misroute a later drop inside the dragend grace window.
    expect(tree.clearInternalDragSession).toHaveBeenCalled();
  });

  test('an OS drop (real paths, no internal drag) still routes to handleEditorFileDrop', async () => {
    await initWithFile('posts/hello.md');
    coverEditorRect();

    const tree = currentTreeInstance();
    // hasActiveInternalDrag stays at its default false — a real Finder drop.
    mockCopyIn.mockResolvedValue({
      status: 'ok',
      data: { copied: [{ original_name: 'img.png', final_name: 'img.png', final_path: '/p/posts/img.png', relative_path: 'posts/img.png' }], skipped: [] },
    });
    mockPosAtCoords.mockReturnValue(3);

    const cb = dragDropCallback.get();
    await cb!({ type: 'drop', paths: ['/abs/img.png'], x: 40, y: 40 });

    // Copy path ran and the embed was inserted by handleEditorFileDrop.
    expect(mockCopyIn).toHaveBeenCalledWith(['/abs/img.png'], expect.any(String));
    const insertTx = mockDispatch.mock.calls.find(
      (call: Array<{ changes?: { insert?: string } }>) => call[0]?.changes?.insert?.includes('![](img.png)'),
    );
    expect(insertTx).toBeDefined();
    // The internal-drop consumption path must NOT have run.
    expect(tree.clearInternalDragSession).not.toHaveBeenCalled();
  });
});
