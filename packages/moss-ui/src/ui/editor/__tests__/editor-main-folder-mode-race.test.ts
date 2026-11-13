/**
 * Race-guard test for editor-main's empty-folder branch.
 *
 * Scenario: editor-main's `init()` resolves `getProjectPath()`, then asks
 * `fileTree.getFirstFile()` whether the folder has any markdown files,
 * and — if not — calls `showFolderMode(true)` to render the four-card
 * empty state. Between those two `await`s, an external `editor:load-source`
 * event (fired by the panel manager when the user clicks a file in the
 * tree, or by a deep link) can land, run `loadFile()`, and mount CM6 on
 * a real file. Without a re-check, the subsequent `showFolderMode` would
 * tear that fresh editor down and replace it with the empty-folder UI —
 * a visible flash of cards on top of a successfully loaded file.
 *
 * The guard in editor-main.ts (`else if (projectRoot && currentEntryId()
 * === null)`) protects against this by re-reading `currentEntryId` at
 * the moment of the empty-branch decision; if a concurrent load already
 * populated it, we leave the editor alone.
 *
 * This test exercises that re-check directly: it lets init() reach the
 * empty branch, but fires `editor:load-source` mid-flight so a file is
 * loaded BEFORE the branch executes, and asserts `mountFolderMode` is
 * never called.
 *
 * The test lives in its own file because it needs to mock `folder-mode`
 * to spy on `mountFolderMode`, and the existing editor-main.test.ts
 * suite intentionally does NOT mock it (so the real component renders
 * for other branches). Separate file = independent mock surface.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

// As editor.html does: the desktop's host record, then the editor. After a
// module reset both are fresh, so they load together.
async function bootEditorMain() {
  // A fresh host per boot, as the editor webview loads it; the bus records emits.
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit, (name, handler) => listenCallbacks.set(name, handler as Function));
  host.onFileDrop = mockOnWebviewDragDrop;
  return import('../editor-main');
}

// ── Hoisted mocks ────────────────────────────────────────────────────────
const {
  mockEmit,
  mockCommands,
  listenCallbacks,
  MockChipBar,
  MockCmEditor,
  MockFileTree,
  mockOnWebviewDragDrop,
  mockMountFolderMode,
  mockUnmountFolderMode,
} = vi.hoisted(() => {
  const listenCallbacks = new Map<string, Function>();
  const mockEmit = vi.fn().mockResolvedValue(undefined);
  const mockCommands = {
    pageFacts: vi.fn().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } }),
    // The launcher's New from starter… intent: none pending in these tests.
    takePendingStarterPicker: vi.fn().mockResolvedValue(false),
    getSchema: vi.fn().mockResolvedValue({ status: 'ok', data: { fields: [] } }),
    getProjectPath: vi.fn(),
    resolvePageSource: vi.fn(),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    computeHeadingState: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { visible: false, text: '', source: 'Filename' },
    }),
    getFileInfo: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { size: '0', modified: null, width: null, height: null },
    }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    copyFilesToProject: vi.fn(),
  };
  const mockOnWebviewDragDrop = vi.fn().mockResolvedValue(vi.fn());
  const MockChipBar = vi.fn().mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
  }));
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
  }));
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null), // empty folder by default
    collapse: vi.fn(),
    collapseForReopen: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
  }));
  const mockMountFolderMode = vi.fn();
  const mockUnmountFolderMode = vi.fn();
  return {
    mockEmit,
    mockCommands,
    listenCallbacks,
    MockChipBar,
    MockCmEditor,
    MockFileTree,
    mockOnWebviewDragDrop,
    mockMountFolderMode,
    mockUnmountFolderMode,
  };
});


// Some modules in this graph are
// NOT on the platform seam yet — they import listen/emit from
// '@tauri-apps/api/event' directly. Mock the raw module too (shared spies) so
// their subscriptions are intercepted alongside the seam ones.



vi.mock('../../engine', () => ({ engine: mockCommands }));

vi.mock('../chip-bar', () => ({
  ChipBar: MockChipBar,
}));

vi.mock('../cm-editor', () => ({
  CmEditor: MockCmEditor,
}));

vi.mock('../file-viewer');

vi.mock('../tree', () => ({
  FileTree: MockFileTree,
}));

vi.mock('../folder-mode/folder-mode', () => ({
  mountFolderMode: mockMountFolderMode,
  unmountFolderMode: mockUnmountFolderMode,
  isFolderModeMounted: vi.fn().mockReturnValue(false),
}));

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
        root_name: p?.status === 'ok' && p.data ? (p.data.split('/').pop() || null) : null,
      },
    };
  });
}
applyEditorBootstrapDelegate();

function resetAll(): void {
  listenCallbacks.clear();
  mockEmit.mockReset().mockResolvedValue(undefined);
  mockMountFolderMode.mockClear();
  mockUnmountFolderMode.mockClear();
  MockCmEditor.mockClear();
  // Re-apply CmEditor default impl — vi.restoreAllMocks() in prior
  // afterEach strips the .mockImplementation set at hoist time.
  MockCmEditor.mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
  }));
  mockCommands.pageFacts.mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } });
  MockChipBar.mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
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
  }));
  // Re-apply command defaults that init() relies on.
  mockCommands.getSchema
    .mockReset()
    .mockResolvedValue({ status: 'ok', data: { fields: [] } });
  mockCommands.parseFrontmatter.mockReset();
  mockCommands.getProjectPath.mockReset();
  mockCommands.editorBootstrap.mockReset();
  applyEditorBootstrapDelegate();
  mockCommands.computeHeadingState
    .mockReset()
    .mockResolvedValue({
      status: 'ok',
      data: { visible: false, text: '', source: 'Filename' },
    });
  mockCommands.getFileInfo
    .mockReset()
    .mockResolvedValue({
      status: 'ok',
      data: { size: '0', modified: null, width: null, height: null },
    });
  mockCommands.resolveUrlForFile
    .mockReset()
    .mockResolvedValue({ status: 'ok', data: null });
  mockOnWebviewDragDrop.mockReset().mockResolvedValue(vi.fn());
}

/** Minimal DOM that editor-main expects. Mirrors editor-main.test.ts. */
function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div id="nav-surface">
      <div id="nav-grid"><div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div></div>
    </div>
    <div id="divider"></div>
    <div class="moss-editor-scroll">
      <div id="chip-bar"></div>
      <div id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    </div>
    <div id="moss-save-indicator"></div>
  `;
}

describe('editor-main empty-folder race guard', () => {
  const originalLocation = window.location;

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetAll();
    // is_root=1, no ?source= — init() takes the empty-branch path that mounts
    // folder-mode unless a concurrent load wins. open_editor always bakes
    // is_root=1 when opening the site root, so the boot block needs the param.
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: '?is_root=1' },
      writable: true,
    });
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      writable: true,
    });
    vi.restoreAllMocks();
  });

  test('shows folder mode when no concurrent load happens (baseline)', async () => {
    // Empty folder, no source, no concurrent load. The else-if branch
    // sees currentEntryId() === null and mounts folder mode.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/empty-blog',
    });
    mockCommands.resolvePageSource = vi.fn().mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { kind: 'root-home', display: 'empty-blog', field: null, folder_empty: true, files: [{ dir: '/Users/test/empty-blog', name: 'empty-blog.md', frontmatter: { home: true } }] } } });

    await bootEditorMain();

    // Wait for init to register listeners (proxy for "init has run").
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Microtasks need to flush so the post-await empty-branch runs.
    await vi.waitFor(() => {
      expect(mockMountFolderMode).toHaveBeenCalledTimes(1);
    });
    expect(mockMountFolderMode.mock.calls[0][1]).toMatchObject({
      folderName: 'empty-blog',
      isEmpty: true,
    });
  });

  test('does NOT show folder mode when editor:load-source mounts CM6 first', async () => {
    // Same empty-folder setup, but fire editor:load-source DURING the
    // init await chain so loadFile completes (and currentEntryId is
    // populated) before init reaches the empty-branch check.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/empty-blog',
    });
    mockCommands.resolvePageSource = vi.fn().mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { kind: 'root-home', display: 'empty-blog', field: null, folder_empty: true, files: [{ dir: '/Users/test/empty-blog', name: 'empty-blog.md', frontmatter: { home: true } }] } } });
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hi' }, body: '# Hi' },
    });

    // Park init at its listener-registration batch: onWebviewDragDrop's promise
    // is part of init's single Promise.all, so init cannot reach the
    // empty-branch re-check until the test releases it. With batched
    // registration the editor:load-source callback registers synchronously,
    // so "listener observable" no longer implies "init still mid-chain" —
    // this deferred promise recreates the mid-init window deterministically.
    let releaseDragDrop!: (unlisten: () => void) => void;
    mockOnWebviewDragDrop.mockReturnValue(
      new Promise<() => void>((resolve) => { releaseDragDrop = resolve; }),
    );

    await bootEditorMain();

    // Wait until init has registered editor:load-source — init is parked at
    // the listener batch (deferred drag-drop above), poised to enter the
    // empty branch only after the test releases it.
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Fire editor:load-source synchronously before yielding back to init.
    // loadFile() calls parseFrontmatter (mocked above) and constructs a
    // CmEditor; once that resolves, currentEntryId is non-null, so the
    // re-check inside init's else-if (currentEntryId() === null) is
    // false and mountFolderMode must NOT fire.
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({
        sourcePath: '/Users/test/empty-blog/index.md',
        isDir: false,
      });

    // CM6 was mounted (proves loadFile completed).
    expect(MockCmEditor).toHaveBeenCalled();

    // Release init from the listener batch; its empty-branch re-check must
    // now see the mounted file (currentEntryId non-null) and skip folder mode.
    releaseDragDrop(vi.fn());

    // Give init's else-if branch every chance to run.
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));

    // The race guard held: folder mode never mounted on top of CM6.
    expect(mockMountFolderMode).not.toHaveBeenCalled();
  });

  test('editor:load-source for an empty root preserves folder-mode (root-empty, not pane-state)', async () => {
    // Empty folder: getFirstFile returns null. Firing editor:load-source with
    // { sourcePath: null, isRoot: true } should route to root-empty →
    // applyEditorSurface → showFolderMode(true). It must NOT replace
    // folder-mode with a pane-state (old buggy behavior).
    //
    // With the idempotency guard in applyEditorSurface, a second editor:load-source
    // for the SAME root-empty surface is a no-op (data-editor-surface already set).
    // The test verifies the structural marker is correct, not a redundant remount.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/empty-blog',
    });
    mockCommands.resolvePageSource = vi.fn().mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { kind: 'root-home', display: 'empty-blog', field: null, folder_empty: true, files: [{ dir: '/Users/test/empty-blog', name: 'empty-blog.md', frontmatter: { home: true } }] } } });

    await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Wait for init's own showFolderMode call to settle (baseline).
    await vi.waitFor(() => {
      expect(mockMountFolderMode).toHaveBeenCalled();
    });
    mockMountFolderMode.mockClear();

    // Now fire editor:load-source for the same empty root via preview-nav.
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: null, isDir: true, isRoot: true });

    // The cm container must have data-editor-surface='root-empty' (not a pane-state).
    // The idempotency guard skips remounting when the surface is already shown,
    // so mockMountFolderMode may not be called again — that's correct behavior.
    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.getAttribute('data-editor-surface')).toBe('root-empty');
    // Confirm it was NOT converted to a pane-state (which would have a different structure).
    expect(cm.querySelector('.moss-pane-state')).toBeNull();
  });
});
