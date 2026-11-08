/**
 * Tests for editor-main.ts — the editor webview entry point.
 *
 * Covers:
 * - Same-file guard: editor:load-source with same path skips reload
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { savedWithHash, textHash } from './disk-hash';
import { Text, type ChangeSet } from '@codemirror/state';
import { folderHomeTakeover } from '../session/node-class';
import { info as mockedLogInfo } from "../../log";

// The desktop's own stubs for the host members these tests drive; tests assert them by name.
const mockStartWriting = vi.fn();
const mockStarterTake = vi.fn().mockResolvedValue(false);
const mockToastAction = vi.fn().mockResolvedValue(null);
// The host's composer, as a stub that draws a Send button and records its flush.
const mockComposer = { destroy: vi.fn(), flush: vi.fn(async () => {}) };
// The flush request the editor registered with the host (the host's onSaveRequest).
let saveRequest: ((token: string) => Promise<void>) | undefined;

// Fills the host record for a test: stubs for every member, with the bus and the
// engine's subscriptions routed through the listener mocks below.
async function fillHost(): Promise<void> {
  saveRequest = undefined;
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit, mockListen);
  host.engine.on = mockListen;
  host.focusView = mockFocusThisWebview;
  host.startWriting = mockStartWriting;
  host.starterPicker = { open: async () => {}, takeRequest: mockStarterTake };
  host.toasts = { take: async () => [], onAction: mockToastAction };
  host.composer = async (container) => {
    container.innerHTML = '<button data-action="send-now"></button>';
    return mockComposer;
  };
  host.onSaveRequest = (handler) => {
    saveRequest = async (token: string) => {
      const ok = await handler();
      host.bus.emit('editor:flush-save-done', { token, ok });
    };
    return Promise.resolve(() => {});
  };
}

// As editor.html does: the host record, then the editor. After a module reset both are fresh, so they load together.
async function bootEditorMain() {
  await fillHost();
  return import('../editor-main');
}

// ---------------------------------------------------------------------------
// Hoisted mocks — vi.mock factories are hoisted above imports, so we must
// use vi.hoisted() for any variables referenced inside them.
// ---------------------------------------------------------------------------
const { mockListen, mockEmit, mockCommands, listenCallbacks, listenCallbacksArrays, MockChipBar, MockCmEditor, MockFileTree, MockEditorFileEvents, MockPublishPreflightStore, mockPrimePublishPreflight, mockGetFirstFile, mockRefreshAndRender, mockDeepestVisibleContainerFor, mockFlashRow, mockSetSealed } = vi.hoisted(() => {
  // Map<eventName, dispatcher>. For most events only one listener is ever
  // registered so the dispatcher just calls the single callback. For
  // 'moss-event' MULTIPLE callers register (e.g. dispatchEditorFileChanged
  // handler + PreviewFollower), so we store ALL of them and the dispatcher
  // fans the payload out to every registered callback. Callers that need
  // to fire an event use `listenCallbacks.get(name)!(payload)`.
  const listenCallbacksArrays = new Map<string, Function[]>();
  const listenCallbacks = new Map<string, Function>();
  const makeMossEventImpl = (event: string, cb: Function) => {
    // Accumulate all callbacks per event name.
    const arr = listenCallbacksArrays.get(event) ?? [];
    arr.push(cb);
    listenCallbacksArrays.set(event, arr);
    // Expose a fan-out dispatcher so tests can fire the event to ALL listeners.
    listenCallbacks.set(event, async (payload: unknown) => {
      await Promise.all(arr.map((fn) => fn(payload)));
    });
    return Promise.resolve(vi.fn());
  };
  const mockListen = vi.fn().mockImplementation(makeMossEventImpl);

  // ── MockEditorFileEvents ────────────────────────────────────────────────────
  // Must stay in sync with EditorFileEvents's public interface: start/onFileChanged/onAssetReady/onRawFileCreated/stop + priority fan-out.
  // A synchronous-start stand-in for the real EditorFileEvents coordinator.
  // start() registers a 'moss-event' listener (via the same fan-out logic as
  // makeMossEventImpl) that fans out to all onFileChanged/onAssetReady/onRawFileCreated handlers
  // — just like the real class — but resolves synchronously (no listen() await
  // on the critical path) so init() does NOT yield extra microtask ticks.
  // Without this, the extra await from the real fileEvents.start() shifts init's
  // async timing so the boot's host calls land after afterEach()'s
  // vi.restoreAllMocks() strips the mocks, producing unhandled rejections.
  //
  // makeFileEventsImpl is a plain function (not a vi.fn factory) so resetMocks()
  // can call .mockImplementation(makeFileEventsImpl) to re-apply it after
  // vi.restoreAllMocks() strips the original implementation.
  const makeFileEventsImpl = () => {
    const fileChangedHandlers: Array<{ handler: Function; priority: number }> = [];
    const assetReadyHandlers: Function[] = [];
    const rawFileCreatedHandlers: Function[] = [];
    return {
      start() {
        // Replicate makeMossEventImpl for 'moss-event' directly.
        const arr = listenCallbacksArrays.get('moss-event') ?? [];
        const cb = (inner: { kind?: string; payload?: unknown } | undefined) => {
          if (inner?.kind === 'FileChanged') {
            const sorted = [...fileChangedHandlers].sort((a, b) => a.priority - b.priority);
            return Promise.all(sorted.map((r) => r.handler(inner.payload)));
          }
          if (inner?.kind === 'AssetReady') {
            assetReadyHandlers.forEach((h) => h(inner.payload));
          }
          if (inner?.kind === 'RawFileCreated') {
            rawFileCreatedHandlers.forEach((h) => h(inner.payload));
          }
        };
        arr.push(cb);
        listenCallbacksArrays.set('moss-event', arr);
        listenCallbacks.set('moss-event', async (payload: unknown) => {
          await Promise.all(arr.map((fn) => fn(payload)));
        });
        return Promise.resolve();
      },
      stop() { return Promise.resolve(); },
      onFileChanged(handler: Function, priority = 50) {
        const reg = { handler, priority };
        fileChangedHandlers.push(reg);
        return () => {
          const idx = fileChangedHandlers.indexOf(reg);
          if (idx >= 0) fileChangedHandlers.splice(idx, 1);
        };
      },
      onAssetReady(handler: Function) {
        assetReadyHandlers.push(handler);
        return () => {
          const idx = assetReadyHandlers.indexOf(handler);
          if (idx >= 0) assetReadyHandlers.splice(idx, 1);
        };
      },
      onRawFileCreated(handler: Function) {
        rawFileCreatedHandlers.push(handler);
        return () => {
          const idx = rawFileCreatedHandlers.indexOf(handler);
          if (idx >= 0) rawFileCreatedHandlers.splice(idx, 1);
        };
      },
    };
  };
  // Expose the factory so resetMocks() can re-apply it after vi.restoreAllMocks().
  const MockEditorFileEvents = vi.fn().mockImplementation(makeFileEventsImpl);
  (MockEditorFileEvents as any).__impl = makeFileEventsImpl;
  const mockEmit = vi.fn().mockResolvedValue(undefined);
  const mockCommands = {
    // The launcher's New from starter… intent: none pending in these tests.
    getSchema: vi.fn(),
    getProjectPath: vi.fn(),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    computeHeadingState: vi.fn().mockResolvedValue({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } }),
    getFileInfo: vi.fn().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn(),
    resolveExpectedUrlForFile: vi.fn(),
    describeSourceRole: vi.fn().mockResolvedValue({ status: 'ok', data: { slot: null, language: null } }),
    pageFacts: vi.fn().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } }),
    resolveUrlForFolder: vi.fn(),
    copyFilesToProject: vi.fn(),
    createFiles: vi.fn(),
    // resolvePageSource('/') is the source-truth root resolve. Default to
    // "no home file" (source_path: null) with the root-home takeover the backend
    // always computes for `/`, so the root surface decision falls to
    // root-empty / takeover unless a test overrides it.
    resolvePageSource: vi.fn().mockResolvedValue({ status: 'ok', data: { source_path: null, is_dir: true, is_article: false, syndicated: [], takeover: { kind: 'root-home', display: 'blog', files: [{ dir: '/Users/test/blog', name: 'blog.md', frontmatter: { home: true } }] } } }),
    getPublishPreflight: vi.fn(),
    readSourceText: vi.fn(),
    emailListSubscribers: vi.fn(),
  };
  const MockChipBar = vi.fn().mockImplementation(() => ({
    render: vi.fn(),
    setPageFacts: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    closeAnyOverlay: vi.fn(),
    hasOpenEditOverlay: vi.fn().mockReturnValue(false),
  }));
  // getView() carries a focus() stub by default: the create-home flows now
  // route through the editor:file-selected isNewFile handler, which calls
  // view.focus() after loadFile — without the stub those tests would throw
  // (unhandled) TypeErrors inside the async handler. Focus-behavior tests
  // override with their own spy.
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    setContent: vi.fn(),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() }),
  }));
  const mockGetFirstFile = vi.fn().mockReturnValue(null);
  const mockRefreshAndRender = vi.fn().mockResolvedValue(undefined);
  // FIX 4: deepest-visible-container resolver + row flasher. Default "no visible
  // container" (null) so flashDownloadTargets falls back to the nav-surface;
  // tests that exercise row/breadcrumb targeting override per-call.
  const mockDeepestVisibleContainerFor = vi.fn().mockReturnValue(null);
  const mockFlashRow = vi.fn();
  const mockSetSealed = vi.fn();
  const MockPublishPreflightStore = vi.fn(function(this: any, read: Function) {
    this.read = read;
    this.listeners = new Set<Function>();
    this.subscribe = (listener: Function) => {
      this.listeners.add(listener);
      return () => this.listeners.delete(listener);
    };
    this.emit = (folder: string, projection: unknown) => {
      for (const listener of this.listeners) listener(folder, projection);
    };
    this.refresh = async (folder: string) => {
      const projection = await this.read(folder);
      this.emit(folder, projection);
      return projection;
    };
  });
  const mockPrimePublishPreflight = vi.fn((store: any, folder: string) => {
    void store.refresh(folder).catch(() => {});
  });
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    revealNewFile: vi.fn(),
    getFirstFile: mockGetFirstFile,
    refreshAndRender: mockRefreshAndRender,
    deepestVisibleContainerFor: mockDeepestVisibleContainerFor,
    flashRow: mockFlashRow,
    parkAtMinHeight: vi.fn(),
    collapseForReopen: vi.fn(),
    // Reached when an open FAILS with no document mounted: the tree must not
    // keep a selection the editor never honoured.
    clearSelection: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
    setSealed: mockSetSealed,
    setPublishPreflight: vi.fn(),
  }));
  return { mockListen, mockEmit, mockCommands, listenCallbacks, listenCallbacksArrays, MockChipBar, MockCmEditor, MockFileTree, MockEditorFileEvents, MockPublishPreflightStore, mockPrimePublishPreflight, mockGetFirstFile, mockRefreshAndRender, mockDeepestVisibleContainerFor, mockFlashRow, mockSetSealed };
});

const { mockFocusThisWebview } = vi.hoisted(() => ({ mockFocusThisWebview: vi.fn() }));


vi.mock("../../log", () => ({
  uiEvent: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
  trace: vi.fn(),
}));

// This factory runs once per file (vi.resetModules() does not re-evaluate a
// mocked module), so every editor-main graph shares one engine and its spy
// logs: assert invariants over them (toContain, the last call), never totality,
// and the file-wide afterEach drains the live graph's pending saves.
vi.mock('../../engine', async () => ({ engine: (await import('./disk-hash')).withContentHash(mockCommands) }));

vi.mock('../chip-bar', () => ({
  ChipBar: MockChipBar,
}));

vi.mock('../cm-editor', async () => ({
  CmEditor: (await import('./disk-hash')).acceptingExternalChanges(MockCmEditor),
}));

vi.mock('../file-viewer', () => ({
  showFileViewer: vi.fn(),
  destroyFileViewer: vi.fn(),
}));

vi.mock('../tree', () => ({
  FileTree: MockFileTree,
}));

vi.mock('../editor-file-events', () => ({
  EditorFileEvents: MockEditorFileEvents,
}));

const { mockMountToasts } = vi.hoisted(() => ({
  mockMountToasts: vi.fn(() => Promise.resolve([] as unknown[])),
}));
vi.mock("../../shared/ui/toast/app-toast-render", () => ({ mountToasts: mockMountToasts }));

vi.mock("../../shared/preflight-store", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/preflight-store")>()),
  PublishPreflightStore: MockPublishPreflightStore,
  primePublishPreflight: mockPrimePublishPreflight,
}));

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Shared setup helper
// ---------------------------------------------------------------------------

/**
 * editorBootstrap delegate — routes through the existing getSchema /
 * getProjectPath mocks so every per-test override of those keeps working
 * unchanged. Struct fields are snake_case (specta does not camelCase struct
 * fields); `initial_file: null` makes loadFile fall back to the already-mocked
 * parseFrontmatter, so existing assertions hold.
 */
function applyEditorBootstrapDelegate(): void {
  mockCommands.editorBootstrap.mockImplementation(async (_src: string | null) => {
    const s = await mockCommands.getSchema();
    if (s?.status !== 'ok') return { status: 'error', error: s?.error };
    const p = await mockCommands.getProjectPath();
    const projectPath = p?.status === 'ok' ? p.data : null;
    return {
      status: 'ok',
      data: {
        schema: s.data,
        project_path: projectPath,
        initial_file: null,
        // The real command emits VaultRoot::name; the mock's project paths are all
        // plain absolute, where the last segment IS the resolved name.
        root_name: projectPath ? (projectPath.split('/').pop() || null) : null,
      },
    };
  });
}
applyEditorBootstrapDelegate();

function resetMocks(): void {
  listenCallbacks.clear();
  listenCallbacksArrays.clear();
  mockListen.mockReset().mockImplementation((event: string, cb: Function) => {
    // Fan-out implementation — same as the hoisted version. Multiple callers
    // may register for the same event (e.g. 'moss-event' is used by both
    // dispatchEditorFileChanged handler and PreviewFollower). The dispatcher
    // stored in listenCallbacks calls all registered callbacks in parallel so
    // tests firing listenCallbacks.get('moss-event')! reach ALL handlers.
    const arr = listenCallbacksArrays.get(event) ?? [];
    arr.push(cb);
    listenCallbacksArrays.set(event, arr);
    listenCallbacks.set(event, async (payload: unknown) => {
      await Promise.all(arr.map((fn) => fn(payload)));
    });
    return Promise.resolve(vi.fn());
  });
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
  mockCommands.describeSourceRole.mockReset().mockResolvedValue({ status: 'ok', data: { slot: null, language: null, can_be_page: true } });
  mockCommands.getFileInfo.mockReset().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } });
  mockCommands.saveEditorContent.mockReset().mockImplementation(savedWithHash);
  // Default the URL resolver to "null result" (Ok(None)) so PreviewFollower's
  // active-path subscriber doesn't crash when tests don't bother stubbing it.
  // Individual tests that care about the value override this.
  mockStartWriting.mockReset().mockResolvedValue({ status: 'ok', data: '/Users/test/blog/blog.md' });
  mockCommands.resolveUrlForFile.mockReset().mockResolvedValue({ status: 'ok', data: null });
  // Build-independent expected-URL resolver: default to "null result" (Ok(None))
  // so a non-new-file selection stays a no-op for the immediate-nav path unless
  // a test stubs a concrete URL. New-file tests override this with the expected
  // slug URL to assert the synchronous emit.
  mockCommands.resolveExpectedUrlForFile.mockReset().mockResolvedValue({ status: 'ok', data: null });
  // Folder-URL resolver: default to "null result" (Ok(None)) so the
  // editor:folder-navigate handler is a no-op for preview nav unless a test
  // overrides it. Individual tests that assert the folder-page navigation
  // stub a concrete URL.
  mockCommands.resolveUrlForFolder.mockReset().mockResolvedValue({ status: 'ok', data: null });
  mockCommands.copyFilesToProject.mockReset();
  mockCommands.resolvePageSource.mockReset().mockResolvedValue({ status: 'ok', data: { source_path: null, is_dir: true, is_article: false, syndicated: [], takeover: { kind: 'root-home', display: 'blog', files: [{ dir: '/Users/test/blog', name: 'blog.md', frontmatter: { home: true } }] } } });
  mockGetFirstFile.mockReset().mockReturnValue(null);
  mockRefreshAndRender.mockReset().mockResolvedValue(undefined);
  mockDeepestVisibleContainerFor.mockReset().mockReturnValue(null);
  mockFlashRow.mockReset();
  mockCommands.getPublishPreflight.mockReset().mockResolvedValue({ status: 'ok', data: null });
  mockCommands.readSourceText.mockReset().mockResolvedValue({ status: 'ok', data: '' });
  mockCommands.emailListSubscribers.mockReset();
  mockCommands.pageFacts.mockReset().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } });
  // Re-apply mock implementations (vi.restoreAllMocks() in prior afterEach strips them)
  MockChipBar.mockImplementation(() => ({
    render: vi.fn(),
    setPageFacts: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    closeAnyOverlay: vi.fn(),
    hasOpenEditOverlay: vi.fn().mockReturnValue(false),
  }));
  MockCmEditor.mockImplementation((options: any) => {
    // CodeMirror stores line breaks as LF.
    const json = { doc: String(options.content).replace(/\r\n?/g, '\n') };
    // The buffer holds the text it was opened with until something changes it.
    let text = json.doc;
    return {
      destroy: vi.fn(),
      getContent: vi.fn(() => text),
      setContent: vi.fn((t: string) => { text = t; }),
      // Default focus() stub — see the hoisted MockCmEditor comment.
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() }),
      toJSON: () => json,
    };
  });
  mockSetSealed.mockReset();
  MockFileTree.mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    revealNewFile: vi.fn(),
    getFirstFile: mockGetFirstFile,
    refreshAndRender: mockRefreshAndRender,
    deepestVisibleContainerFor: mockDeepestVisibleContainerFor,
    flashRow: mockFlashRow,
    parkAtMinHeight: vi.fn(),
    collapseForReopen: vi.fn(),
    // Reached when an open FAILS with no document mounted: the tree must not
    // keep a selection the editor never honoured.
    clearSelection: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
    setSealed: mockSetSealed,
    setPublishPreflight: vi.fn(),
  }));
  MockPublishPreflightStore.mockClear();
  mockPrimePublishPreflight.mockClear();
  // Re-apply the EditorFileEvents mock implementation after vi.restoreAllMocks()
  // strips it. The __impl reference points to the factory defined in vi.hoisted().
  MockEditorFileEvents.mockReset().mockImplementation((MockEditorFileEvents as any).__impl);
}

/** Minimal DOM that editor-main.ts expects for the editor UI. Mirrors
 *  editor.html — including the static cold-start skeleton nodes. */
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
    <div id="moss-toast-container"></div>
  `;
}

// applyResolvedNode re-bakes the resolved node into the document
// query via history.replaceState — reset the SHARED jsdom URL before every
// test so one test's open doesn't leak ?source=/is_root= into the next
// test's init() (which reads location.search as its boot params). Top-level
// hook: runs before each describe's own beforeEach, file-wide.
beforeEach(() => {
  window.history.replaceState(null, '', '/');
});

// A test that leaves a debounced (real 800ms autosave timer)
// or in-flight save behind would fire it into a LATER test's shared mock log:
// vi.resetModules() abandons the module graph but does not stop its timers or
// async chains. Settle the live graph's save machinery inside the test that
// armed it. Runs outer-last, i.e. after a describe's vi.restoreAllMocks(): a
// stripped saveEditorContent then resolves undefined and saveActiveDocument's
// own try/catch absorbs it — the chain still ENDS here, which is what the
// next test needs.
afterEach(async () => {
  const dbg = (window as { __moss_debug?: { flushPendingSave?: () => Promise<boolean> } }).__moss_debug;
  if (dbg?.flushPendingSave) await dbg.flushPendingSave();
  // Each import owns a webview lifetime; retire its DOM listeners before resetModules.
  window.dispatchEvent(new Event('beforeunload'));
});

describe('editor-main folder surface request ownership', () => {
  const root = '/Users/test/blog';
  const folder = (name: string) => ({ ...folderHomeTakeover(`${root}/${name}`, name), folder_empty: true });

  beforeEach(async () => {
    window.dispatchEvent(new Event('beforeunload'));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    vi.resetModules();
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: root });
    mockCommands.describeSourceRole.mockResolvedValue({ status: 'ok', data: { slot: null, language: null } });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: '# Current' } });
    mockCommands.resolveUrlForFolder.mockImplementation(async (path: string) => ({ status: 'ok', data: `/${path.slice(root.length + 1)}/` }));
    // The desktop's editor has a starter picker, so an empty folder shows four cards.
    (await import("../../host")).host.starterPicker = { open: async () => {}, takeRequest: async () => false };
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await vi.waitFor(() => expect(document.getElementById('moss-editor-cm')!.getAttribute('data-editor-surface')).not.toBeNull());
  });

  afterEach(() => vi.restoreAllMocks());

  test('a duplicate content echo cannot invalidate an explicit same-file open already in flight', async () => {
    const load = listenCallbacks.get('editor:load-source')!;
    await load({ sourcePath: `${root}/current.md`, isRoot: false });
    expect(MockCmEditor).toHaveBeenCalledTimes(1);
    let finish!: (value: unknown) => void;
    mockCommands.parseFrontmatter.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    document.dispatchEvent(new CustomEvent('editor:file-selected', { detail: { filePath: 'current.md' } }));
    await vi.waitFor(() => expect(finish).toBeDefined());
    await load({ sourcePath: `${root}/current.md`, isRoot: false });
    finish({ status: 'ok', data: { frontmatter: {}, body: '# Updated' } });
    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalledTimes(2));
  });

  test('a deferred folder verdict cannot tear down a newer file', async () => {
    let finish!: (value: unknown) => void;
    mockCommands.resolvePageSource.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const load = listenCallbacks.get('editor:load-source')!;
    const obsolete = load({ sourcePath: null, isRoot: false, takeover: folder('A') });
    await vi.waitFor(() => expect(finish).toBeDefined());
    await load({ sourcePath: `${root}/current.md`, isRoot: false });
    const mounted = MockCmEditor.mock.results.at(-1)!.value;
    finish({ status: 'ok', data: { source_path: null, takeover: folder('A') } });
    await obsolete;
    expect(mounted.destroy).not.toHaveBeenCalled();
    expect(document.querySelectorAll('[data-folder-mode-card]')).toHaveLength(0);
    expect(location.search).toContain('current.md');
  });

  test('a deferred folder verdict cannot replace a newer folder', async () => {
    let finish!: (value: unknown) => void;
    mockCommands.resolvePageSource.mockImplementation((url: string) => url === '/A/'
      ? new Promise(resolve => { finish = resolve; })
      : Promise.resolve({ status: 'ok', data: { source_path: null, takeover: folder('B') } }));
    const load = listenCallbacks.get('editor:load-source')!;
    const obsolete = load({ sourcePath: null, isRoot: false, takeover: folder('A') });
    await vi.waitFor(() => expect(finish).toBeDefined());
    await load({ sourcePath: null, isRoot: false, takeover: folder('B') });
    const cm = document.getElementById('moss-editor-cm')!;
    const newerSurface = cm.getAttribute('data-editor-surface-key');
    expect(newerSurface).toContain('/B');
    finish({ status: 'ok', data: { source_path: null, takeover: folder('A') } });
    await obsolete;
    expect(cm.getAttribute('data-editor-surface-key')).toBe(newerSurface);
    expect(document.querySelectorAll('[data-folder-mode-card]')).toHaveLength(4);
  });

  test.each(['', 'posts'])('deleting the last open source in "%s" restores the four cards from disk truth', async (dir) => {
    const load = listenCallbacks.get('editor:load-source')!;
    const relativePath = dir ? `${dir}/last.md` : 'last.md';
    await load({ sourcePath: `${root}/${relativePath}`, isRoot: false });
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: {
      source_path: null, takeover: dir ? folder(dir) : { ...folderHomeTakeover(root, 'blog', true), folder_empty: true },
    } });
    await listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: {
      source_deletes: [relativePath], changed_output_files: [dir ? `${dir}/index.html` : 'index.html'],
    } });
    expect(document.querySelectorAll('[data-folder-mode-card]')).toHaveLength(4);
    expect(document.getElementById('moss-editor-cm')!.getAttribute('data-editor-surface')).toBe('root-empty');
    if (dir) expect(document.getElementById('moss-editor-cm')!.getAttribute('data-editor-surface-key')).toContain(`${root}/${dir}`);
  });

  test('Start writing in a nested empty folder creates and opens its own home', async () => {
    const recipe = folder('posts');
    mockCommands.describeSourceRole.mockResolvedValue({ status: 'ok', data: { slot: null, language: null, can_be_page: true } });
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/' });
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: recipe } });
    mockCommands.createFiles.mockResolvedValue({ status: 'ok', data: [`${root}/posts/posts.md`] });
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isRoot: false, takeover: recipe });
    (document.querySelector('[data-folder-mode-card="start-writing"]') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(mockCommands.createFiles).toHaveBeenCalledWith(recipe.files));
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/posts/posts.md`));
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/', awaitingBuild: true }));
    expect(MockFileTree.mock.results[0].value.revealNewFile).toHaveBeenCalledWith('posts/posts.md');
    expect(mockStartWriting).not.toHaveBeenCalled();
  });
});

describe('editor-main same-file guard (editor:load-source)', () => {
  beforeEach(async () => {
    vi.resetModules();
    // The desktop's host record, as editor.html loads it before editor-main.
    await fillHost();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    // Provide a project root so toRelative() strips absolute paths before
    // handing them to registry.upsert(), silencing guardRelative warnings.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('the editor panel mounts the action-panel toast surface so a document window can show toasts', async () => {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(mockMountToasts).toHaveBeenCalled();
    });
    const [surface] = mockMountToasts.mock.calls.at(-1) as unknown as [{ label: string }];
    expect(surface.label).toBe('action-panel');
  });

  test('a toast button clicked in the editor panel runs its action in Rust', async () => {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(mockMountToasts).toHaveBeenCalled();
    });
    const [surface] = mockMountToasts.mock.calls.at(-1) as unknown as [{ onAction: (key: string) => void }];

    surface.onAction('update:check');

    expect(mockToastAction).toHaveBeenCalledWith('update:check');
  });

  test('editor:load-source skips loadFile when sourcePath matches current file', async () => {
    // Set up parseFrontmatter to succeed (needed for the initial loadFile)
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });

    const filePath = '/Users/test/blog/content/posts/hello.md';

    // Import triggers init()
    await bootEditorMain();

    // Wait for init to complete
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Get the editor:load-source callback
    const loadSourceCb = listenCallbacks.get('editor:load-source');
    expect(loadSourceCb).toBeDefined();

    // First call: loads the file (should call parseFrontmatter)
    await loadSourceCb!({ sourcePath: filePath, isDir: false });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(1);
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(filePath);

    // Reset to track subsequent calls
    mockCommands.parseFrontmatter.mockClear();

    // Second call: same path — should NOT call parseFrontmatter
    await loadSourceCb!({ sourcePath: filePath, isDir: false });
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });

  test('editor:load-source loads new file when sourcePath differs', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Test' }, body: 'body' },
    });

    const fileA = '/Users/test/blog/content/posts/hello.md';
    const fileB = '/Users/test/blog/content/posts/world.md';

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const loadSourceCb = listenCallbacks.get('editor:load-source');
    expect(loadSourceCb).toBeDefined();

    // Load first file
    await loadSourceCb!({ sourcePath: fileA, isDir: false });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(1);
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(fileA);

    mockCommands.parseFrontmatter.mockClear();

    // Load different file — should call parseFrontmatter
    await loadSourceCb!({ sourcePath: fileB, isDir: false });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(1);
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(fileB);
  });

  test('going back to an unchanged file hands its undo history to the new editor', async () => {
    const bodies: Record<string, string> = { 'a.md': 'alpha', 'b.md': 'beta' };
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => ({
      status: 'ok',
      data: { frontmatter: { title: 'T' }, body: bodies[p.split('/').pop()!] },
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const fileA = '/Users/test/blog/content/a.md';
    const fileB = '/Users/test/blog/content/b.md';

    await loadSourceCb({ sourcePath: fileA, isDir: false });
    const editorA = MockCmEditor.mock.results[0].value;
    await loadSourceCb({ sourcePath: fileB, isDir: false });
    await loadSourceCb({ sourcePath: fileA, isDir: false });

    const third = MockCmEditor.mock.calls.at(-1)![0] as { restore?: unknown };
    expect(third.restore).toBe(editorA.toJSON());
  });

  test('going back to a file brings back the undo of its chip edits', async () => {
    mockCommands.parseFrontmatter.mockImplementation(async () => ({
      status: 'ok',
      data: { frontmatter: { title: 'T' }, body: 'body' },
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const undoButton = () => document.querySelector<HTMLButtonElement>('button[data-i18n-tooltip="editor.undo"]');

    await loadSourceCb({ sourcePath: '/Users/test/blog/content/a.md', isDir: false });
    const chipOptions = MockChipBar.mock.calls.at(-1)![2] as { onFieldEdit: (k: string, b: unknown, a: unknown) => void };
    chipOptions.onFieldEdit('title', 'T', 'T2');
    expect(undoButton()?.disabled).toBe(false);

    await loadSourceCb({ sourcePath: '/Users/test/blog/content/b.md', isDir: false });
    expect(undoButton()?.disabled).toBe(true);

    await loadSourceCb({ sourcePath: '/Users/test/blog/content/a.md', isDir: false });
    expect(undoButton()?.disabled).toBe(false);
  });

  test('going back to a file whose frontmatter changed on disk starts with no undo history', async () => {
    let title = 'T';
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => ({
      status: 'ok',
      data: { frontmatter: { title: p.endsWith('a.md') ? title : 'other' }, body: 'body' },
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const undoButton = () => document.querySelector<HTMLButtonElement>('button[data-i18n-tooltip="editor.undo"]');
    const fileA = '/Users/test/blog/content/a.md';
    const fileB = '/Users/test/blog/content/b.md';

    await loadSourceCb({ sourcePath: fileA, isDir: false });
    const chipOptions = MockChipBar.mock.calls.at(-1)![2] as { onFieldEdit: (k: string, b: unknown, a: unknown) => void };
    chipOptions.onFieldEdit('title', 'T', 'T2');
    expect(undoButton()?.disabled).toBe(false);
    await loadSourceCb({ sourcePath: fileB, isDir: false });
    title = 'Edited elsewhere';
    await loadSourceCb({ sourcePath: fileA, isDir: false });

    const third = MockCmEditor.mock.calls.at(-1)![0] as { restore?: unknown };
    expect(third.restore).toBeUndefined();
    expect(undoButton()?.disabled).toBe(true);
  });

  test('going back to a file whose body changed on disk starts with no undo history', async () => {
    let body = 'body';
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => ({
      status: 'ok',
      data: { frontmatter: { title: 'T' }, body: p.endsWith('a.md') ? body : 'other' },
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const undoButton = () => document.querySelector<HTMLButtonElement>('button[data-i18n-tooltip="editor.undo"]');
    const fileA = '/Users/test/blog/content/a.md';
    const fileB = '/Users/test/blog/content/b.md';

    await loadSourceCb({ sourcePath: fileA, isDir: false });
    const chipOptions = MockChipBar.mock.calls.at(-1)![2] as { onFieldEdit: (k: string, b: unknown, a: unknown) => void };
    chipOptions.onFieldEdit('title', 'T', 'T2');
    await loadSourceCb({ sourcePath: fileB, isDir: false });
    body = 'Edited elsewhere';
    await loadSourceCb({ sourcePath: fileA, isDir: false });

    const third = MockCmEditor.mock.calls.at(-1)![0] as { restore?: unknown };
    expect(third.restore).toBeUndefined();
    expect(undoButton()?.disabled).toBe(true);
  });

  test('going back to a file with CRLF line breaks keeps its undo history', async () => {
    const bodies: Record<string, string> = { 'a.md': 'a\r\nb\r\n', 'b.md': 'beta' };
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => ({
      status: 'ok',
      data: { frontmatter: { title: 'T' }, body: bodies[p.split('/').pop()!] },
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const fileA = '/Users/test/blog/content/a.md';
    const fileB = '/Users/test/blog/content/b.md';

    await loadSourceCb({ sourcePath: fileA, isDir: false });
    const editorA = MockCmEditor.mock.results[0].value;
    await loadSourceCb({ sourcePath: fileB, isDir: false });
    await loadSourceCb({ sourcePath: fileA, isDir: false });

    const third = MockCmEditor.mock.calls.at(-1)![0] as { restore?: unknown };
    expect(third.restore).toBe(editorA.toJSON());
  });

  test('malformed frontmatter shows the invalid-YAML banner; a clean file clears it', async () => {
    // BUG 3 editor surface: a malformed `---...---` block parses to zero fields,
    // so the chip form is blank — the banner is the visible signal it is broken.
    mockCommands.parseFrontmatter.mockResolvedValueOnce({
      status: 'ok',
      data: {
        frontmatter: {},
        body: '---\nuid: blk-europecover: "006.jpg"\n---\n\nbody\n',
        has_frontmatter: true,
        frontmatter_error: 'mapping values are not allowed in this context',
      },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    const bad = '/Users/test/blog/content/europe.md';
    await loadSourceCb({ sourcePath: bad, isDir: false });

    const banner = document.getElementById('fm-error-banner');
    expect(banner).not.toBeNull();
    expect(banner!.textContent).toContain('Invalid YAML frontmatter');
    // Placed above the chip form so it explains the blank chips.
    expect(banner!.nextElementSibling?.id).toBe('chip-bar');

    // Switching to a clean file must clear the stale banner.
    mockCommands.parseFrontmatter.mockResolvedValueOnce({
      status: 'ok',
      data: { frontmatter: { title: 'Ok' }, body: 'body', has_frontmatter: true },
    });
    const good = '/Users/test/blog/content/ok.md';
    await loadSourceCb({ sourcePath: good, isDir: false });
    expect(document.getElementById('fm-error-banner')).toBeNull();
  });

  test('editor:load-source loads file even when isDir is true', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const loadSourceCb = listenCallbacks.get('editor:load-source');
    expect(loadSourceCb).toBeDefined();

    // Directory page — isDir is a content-model concept, editor ignores it
    const filePath = '/Users/test/blog/posts/index.md';
    await loadSourceCb!({ sourcePath: filePath, isDir: true });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(filePath);
  });

  test('editor:load-source skips loadFile when sourcePath is null', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const loadSourceCb = listenCallbacks.get('editor:load-source');
    expect(loadSourceCb).toBeDefined();

    // No source path — should NOT load file
    await loadSourceCb!({ sourcePath: null, isDir: false });
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });

  test('root home (no source, isRoot) mounts a "Create home file" CTA that creates the self-named <root>.md', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    // Root folder is 'blog' → self-named home note is 'blog.md' (the canonical
    // folder-note convention, same as subfolders).
    mockStartWriting.mockResolvedValue({ status: 'ok', data: '/Users/test/blog/blog.md' });
    mockGetFirstFile.mockReturnValue('about.md'); // non-empty folder → the root-home takeover (not root-empty)

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    // Generated home: no source + isRoot + non-empty folder → the root-home
    // takeover the resolver answers for `/`.
    await loadSourceCb({ sourcePath: null, isDir: true, isRoot: true });

    const cm2 = document.getElementById('moss-editor-cm')!;
    expect(cm2.getAttribute('data-editor-surface')).toBe('takeover');
    const cta = cm2.querySelector('.moss-pane-state__cta') as HTMLButtonElement | null;
    expect(cta?.textContent).toContain('Create home file');

    // Clicking the CTA creates the self-named <rootName>.md at the project root
    // (root folder 'blog' → 'blog.md') with home:true already in it, then loads it.
    cta!.click();
    await vi.waitFor(() => {
      expect(mockStartWriting).toHaveBeenCalledWith('/Users/test/blog');
      expect(mockCommands.createFiles).not.toHaveBeenCalled();
    });
    // takeOver then loads the file (mounts CM6); await that so the
    // fire-and-forget chain settles before afterEach tears the mocks down.
    await vi.waitFor(() => {
      expect(MockCmEditor).toHaveBeenCalled();
    });
  });

  test.each(['starter-card', 'root-context-menu'])('%s creates and reveals the same root home, focuses it and follows /', async (route) => {
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { ...folderHomeTakeover('/Users/test/blog', 'blog', true), folder_empty: true } } });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { home: true }, body: '' } });
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({ status: 'ok', data: '/' });
    const focus = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(), getContent: vi.fn().mockReturnValue(''), setContent: vi.fn(),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus }),
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isDir: true, isRoot: true });
    if (route === 'starter-card') {
      document.querySelector<HTMLButtonElement>('[data-folder-mode-card="start-writing"]')!.click();
    } else {
      document.dispatchEvent(new CustomEvent('editor:create-home', { detail: { folderPath: '/Users/test/blog', folderName: 'blog' } }));
    }
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/', awaitingBuild: true }));
    expect(mockStartWriting).toHaveBeenCalledWith('/Users/test/blog');
    expect(mockCommands.createFiles).not.toHaveBeenCalled();
    expect(mockRefreshAndRender).toHaveBeenCalled();
    expect(MockFileTree.mock.results[0].value.revealNewFile).toHaveBeenCalledWith('blog.md');
    expect(focus).toHaveBeenCalled();
    expect(document.querySelector('[data-folder-mode-card="start-writing"]')).toBeNull();
    // A warm reopen at the canonical home route retains the editable document.
    const count = MockCmEditor.mock.calls.length;
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/blog.md', isDir: true, isRoot: true });
    expect(MockCmEditor.mock.calls.length).toBe(count);
  });

  test('root context creation shares the card guard until the home is opened', async () => {
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { ...folderHomeTakeover('/Users/test/blog', 'blog', true), folder_empty: true } } });
    let settle!: (value: { status: 'ok'; data: string }) => void;
    mockStartWriting.mockImplementation(() => new Promise((resolve) => { settle = resolve; }));
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { home: true }, body: '' } });
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({ status: 'ok', data: '/' });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isDir: true, isRoot: true });
    const event = () => document.dispatchEvent(new CustomEvent('editor:create-home', { detail: { folderPath: '/Users/test/blog', folderName: 'blog' } }));
    event();
    const card = document.querySelector<HTMLButtonElement>('[data-folder-mode-card="start-writing"]')!;
    expect(card.getAttribute('aria-busy')).toBe('true');
    card.click();
    event();
    expect(mockStartWriting).toHaveBeenCalledTimes(1);
    settle({ status: 'ok', data: '/Users/test/blog/blog.md' });
    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/', awaitingBuild: true }));
  });

  test('folder CTA writes the home note with home:true in one create, then loads it', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    // Subfolder 'Essays' → self-named home note is 'Essays.md'.
    mockCommands.createFiles.mockResolvedValue({ status: 'ok', data: ['/Users/test/blog/Essays/Essays.md'] });

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    // Navigate to a subfolder with no home file → its takeover surface.
    await loadSourceCb({
      sourcePath: null, isDir: true, isRoot: false,
      takeover: folderHomeTakeover('/Users/test/blog/Essays', 'Essays'),
    });

    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.getAttribute('data-editor-surface')).toBe('takeover');

    // Click the "Create folder page" CTA.
    const cta = cm.querySelector('.moss-pane-state__cta') as HTMLButtonElement | null;
    expect(cta?.textContent).toContain('Create folder page');
    cta!.click();

    // One call writes the file with its frontmatter; no second save pass.
    await vi.waitFor(() => {
      expect(mockCommands.createFiles).toHaveBeenCalledWith([
        { dir: '/Users/test/blog/Essays', name: 'Essays.md', frontmatter: { home: true } },
      ]);
    });
    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();
    // The file is then loaded into the editor (CM6 mounted).
    await vi.waitFor(() => {
      expect(MockCmEditor).toHaveBeenCalled();
    });
  });

  // One term-page takeover covers every namespace a site declares, so the
  // wording comes from the claim FIELD's own chip label rather than from a
  // kind per namespace. Both cases below run the same code path.
  test.each([
    ['author', 'Author'],
    ['editor', 'Editor'],
  ])('a %s takeover is titled by the name and worded from that field', async (field, label) => {
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({
      sourcePath: null, isDir: false, isRoot: false,
      takeover: {
        kind: 'term-page', display: '馬欣宜', field,
        files: [{ dir: '/Users/test/blog/people', name: '馬欣宜.md', frontmatter: { [`${field}_page`]: '馬欣宜' } }],
      },
    });
    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.querySelector('.moss-pane-state__title')?.textContent).toBe('馬欣宜');
    expect(cm.querySelector('.moss-pane-state__desc')?.textContent).toContain(`${label} page`);
    expect(cm.querySelector('.moss-pane-state__desc')?.textContent).toContain('馬欣宜');
    expect(cm.querySelector('.moss-pane-state__cta')?.textContent).toBe(`Create the ${label} page`);
  });

  test('creating a folder home via the Create-folder-page CTA ends with CM6 FOCUSED (routes through editor:file-selected isNewFile)', async () => {
    // Regression for the "editable-yet-unfocused" bug: createHomeNoteAndReveal
    // called loadFile() directly and never focused the editor, so after the
    // CTA the new home page mounted but clicks on the empty cm-scroller did
    // not place the cursor until the user navigated away and back. The flow
    // now dispatches the SAME editor:file-selected { isNewFile: true } event
    // the file-tree inline create uses; its handler focuses CM6 once loaded.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { home: true }, body: '' },
    });
    mockCommands.createFiles.mockResolvedValue({ status: 'ok', data: ['/Users/test/blog/Essays/Essays.md'] });
    // Build-independent expected URL — consolidating onto the shared channel
    // also buys create-home the immediate new-file preview navigate the
    // hand-rolled path lacked (it only armed the PreviewFollower retry).
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({ status: 'ok', data: '/essays/' });

    const focusSpy = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      setContent: vi.fn(),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: focusSpy }),
    }));

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    // Navigate to a home-less subfolder → its takeover surface.
    await loadSourceCb({
      sourcePath: null, isDir: true, isRoot: false,
      takeover: folderHomeTakeover('/Users/test/blog/Essays', 'Essays'),
    });

    const cm = document.getElementById('moss-editor-cm')!;
    const cta = cm.querySelector('.moss-pane-state__cta') as HTMLButtonElement | null;
    expect(cta).not.toBeNull();
    mockEmit.mockClear();
    cta!.click();

    // THE fix: the new home page ends focused.
    await vi.waitFor(() => expect(focusSpy).toHaveBeenCalled());
    // Breadcrumb re-anchor is preserved (create-reveal, relative path).
    const treeInstance = MockFileTree.mock.results[0]?.value;
    expect(treeInstance.revealNewFile).toHaveBeenCalledWith('Essays/Essays.md');
    // And the immediate build-independent preview navigate fired — proof the
    // open went through the shared isNewFile channel, not a bespoke loadFile.
    await vi.waitFor(() => {
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', {
        urlPath: '/essays/', awaitingBuild: true,
      });
    });
  });

  test('creating a home via the editor:create-home context-menu event ends with CM6 FOCUSED', async () => {
    // Same consolidation, third entry point: the file-tree folder context-menu
    // "Create home file" item dispatches editor:create-home → takeOver, which
    // must land focused like the CTA flows.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { home: true }, body: '' },
    });
    mockCommands.createFiles.mockResolvedValue({ status: 'ok', data: ['/Users/test/blog/Poems/Poems.md'] });

    const focusSpy = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      setContent: vi.fn(),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: focusSpy }),
    }));

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));

    document.dispatchEvent(new CustomEvent('editor:create-home', {
      detail: { folderPath: '/Users/test/blog/Poems', folderName: 'Poems' },
    }));

    await vi.waitFor(() => {
      expect(mockCommands.createFiles).toHaveBeenCalledWith([
        { dir: '/Users/test/blog/Poems', name: 'Poems.md', frontmatter: { home: true } },
      ]);
    });
    await vi.waitFor(() => expect(focusSpy).toHaveBeenCalled());
  });

  test('an ORDINARY loadFile via editor:load-source does NOT auto-focus CM6', async () => {
    // Scope guard for the consolidation above: loadFile itself must keep its
    // no-steal-focus behavior — external navigations (preview → editor sync)
    // and file-changed reloads route through loadFile without isNewFile, and
    // must never yank focus from wherever the user is typing.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'existing body' },
    });

    const focusSpy = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue('existing body'),
      setContent: vi.fn(),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: focusSpy }),
    }));

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    await loadSourceCb({ sourcePath: '/Users/test/blog/existing.md', isDir: false });

    expect(MockCmEditor).toHaveBeenCalled();
    expect(focusSpy).not.toHaveBeenCalled();
  });

  test('preview-nav to a no-home subfolder mounts the Create-folder-page CTA', async () => {
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;

    await loadSourceCb({
      sourcePath: null, isDir: true, isRoot: false,
      takeover: folderHomeTakeover('/Users/test/blog/My Section', 'My Section'),
    });

    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.getAttribute('data-editor-surface')).toBe('takeover');
    const title = cm.querySelector('.moss-pane-state__title');
    expect(title?.textContent).toBe('My Section');
  });

  test('preview-nav to / on an EMPTY folder yields the four-card starter (regression)', async () => {
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { ...folderHomeTakeover('/Users/test/blog', 'blog', true), folder_empty: true } } });
    mockGetFirstFile.mockReturnValue(null); // empty folder
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: null, isDir: true, isRoot: true });
    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.getAttribute('data-editor-surface')).toBe('root-empty');
  });

  test('an empty folder asks whether New from starter made it, for that folder', async () => {
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { ...folderHomeTakeover('/Users/test/blog', 'blog', true), folder_empty: true } } });
    mockGetFirstFile.mockReturnValue(null);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isDir: true, isRoot: true });
    expect(mockStarterTake).toHaveBeenCalledWith('/Users/test/blog');
  });

  test('editor:folder-page-missing event is inert', async () => {
    // The editor:folder-page-missing listener is gone. The "create folder
    // page" CTA now only appears via preview/URL navigation (applyResolvedNode).
    // Dispatching the old event must be a no-op — it must NOT mount the 'takeover' surface.
    await bootEditorMain();
    const cm = document.getElementById('moss-editor-cm')!;
    // Capture whatever surface init() may have set (e.g. 'generated').
    const surfaceBefore = cm.getAttribute('data-editor-surface');

    document.dispatchEvent(new CustomEvent('editor:folder-page-missing', {
      detail: { folderPath: '/Users/test/blog/Essays', folderName: 'Essays', isRoot: false },
    }));
    // The listener is gone — surface must NOT become 'takeover'.
    await new Promise((r) => setTimeout(r, 50));
    expect(cm.getAttribute('data-editor-surface')).toBe(surfaceBefore);
    expect(cm.getAttribute('data-editor-surface')).not.toBe('takeover');
  });
});

// ---------------------------------------------------------------------------
// Editor → Preview sync: file selection in tree emits editor:navigate-preview
// ---------------------------------------------------------------------------

/** DOM that includes file-tree-wrapper and drag-handle for tree init. */
function setupEditorDOMWithTree(): void {
  document.body.innerHTML = `
    <div id="nav-surface">
      <div id="nav-grid"></div>
    </div>
    <div id="divider"></div>
    <div id="chip-bar"></div>
    <div id="moss-editor-cm"></div>
    <div id="moss-save-indicator"></div>
  `;
}

describe('editor-main Editor → Preview sync', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMWithTree();
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

  test('file selection from tree emits editor:navigate-preview with resolved URL', async () => {
    const filePath = '/Users/test/blog/content/posts/hello.md';

    // parseFrontmatter succeeds so loadFile completes
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });

    // resolveUrlForFile returns the article's URL path (with leading slash, as Rust does)
    mockCommands.resolveUrlForFile.mockResolvedValue({
      status: 'ok',
      data: '/posts/hello/',
    });

    // Import triggers init()
    await bootEditorMain();

    // Wait for init to complete
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // Clear emit calls from init so we can check new ones
    mockEmit.mockClear();

    // Dispatch editor:file-selected on document (listener is on document)
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath },
    }));

    // Wait for the async handler to complete
    await vi.waitFor(() => {
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
    });

    // Regression guard: the in-tree click path must NOT force-anchor the file
    // tree breadcrumb — only external
    // navigation (editor:load-source) does. See the forceAnchor test below.
    const treeInstance = MockFileTree.mock.results[0]?.value;
    expect(treeInstance).toBeDefined();
    const call = treeInstance.highlight.mock.calls.at(-1);
    expect(call?.[0]).toBe('content/posts/hello.md');
    expect(call?.[1]?.forceAnchor).not.toBe(true);
  });

  test('editor:load-source (external preview-nav) force-anchors the file tree breadcrumb', async () => {
    // Confirmed mechanism: PREVIEW_URL_CHANGED (app.ts) emits
    // editor:load-source, whose listener calls applyResolvedNode →
    // loadContentPageDeduped → loadFile. That is an external navigation (the
    // preview webview jumped to a different page) — a bigger shift in
    // attention than in-tree browsing, so the breadcrumb must re-anchor even
    // while the tree is expanded.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const treeInstance = MockFileTree.mock.results[0]?.value;
    expect(treeInstance).toBeDefined();
    treeInstance.highlight.mockClear();

    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const filePath = '/Users/test/blog/content/posts/hello.md';
    await loadSourceCb({ sourcePath: filePath, isDir: false });

    expect(treeInstance.highlight).toHaveBeenCalled();
    const call = treeInstance.highlight.mock.calls.at(-1);
    expect(call?.[0]).toBe('content/posts/hello.md');
    expect(call?.[1]?.forceAnchor).toBe(true);
  });

  test('creating a new file focuses the CM6 body (isNewFile flag on editor:file-selected)', async () => {
    // Regression for the "focus vacuum" bug: FileTree.startCreate()'s finish()
    // destroys the temp rename <input>'s focus synchronously (removeTempEntry
    // + render()) before loadFile's awaited RPC chain even progresses, and
    // nothing in loadFile/applyHeadingState/CmEditor ever called .focus() —
    // leaving document.activeElement === body once the new file finished
    // loading. The file-tree now marks the dispatched event isNewFile: true
    // for exactly this path; editor-main must focus the CM6 view once
    // loadFile resolves.
    const filePath = '/Users/test/blog/content/posts/new-page.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    const focusSpy = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({
        scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
        focus: focusSpy,
      }),
      setContent: vi.fn(),
    }));

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath, isNewFile: true },
    }));

    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalled());
    await vi.waitFor(() => expect(focusSpy).toHaveBeenCalled());
  });

  test('a NEW file selection navigates the preview immediately via the build-independent expected URL', async () => {
    // A brand-new page must move the preview the moment it is created —
    // build-independently, exactly like folders — instead of waiting for a
    // build to make resolveUrlForFile (the accurate post-build resolve) return a
    // URL. The new-file branch resolves resolveExpectedUrlForFile (a pure slug
    // transform) and emits editor:navigate-preview synchronously.
    const filePath = '/Users/test/blog/Auguries of Innocence.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    // Build-DEPENDENT resolve returns null (page not built yet) — proving the
    // immediate emit does NOT come from resolveUrlForFile.
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });
    // Build-INDEPENDENT resolve returns the expected slug URL.
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({
      status: 'ok',
      data: '/auguries-of-innocence/',
    });

    // isNewFile also focuses the CM6 body, so the mock's getView() must expose
    // a focus() (the default mock omits it).
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() }),
      setContent: vi.fn(),
    }));

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    mockEmit.mockClear();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath, isNewFile: true },
    }));

    await vi.waitFor(() => {
      expect(mockCommands.resolveExpectedUrlForFile).toHaveBeenCalledWith(filePath);
      // `awaitingBuild` rides along: the page was created moments ago, so the
      // shell covers the 404 it is about to land on instead of showing it.
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', {
        urlPath: '/auguries-of-innocence/', awaitingBuild: true,
      });
    });
  });

  test('an EXISTING (non-new) file selection does NOT call the build-independent resolver', async () => {
    // Scope guard: only isNewFile selections take the immediate build-independent
    // navigation. An ordinary tree click of an already-built file continues to go
    // exclusively through PreviewFollower.navigate (resolveUrlForFile), so the
    // expected-URL resolver must never be consulted for it.
    const filePath = '/Users/test/blog/content/posts/hello.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/hello/' });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    mockCommands.resolveExpectedUrlForFile.mockClear();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath },
    }));

    await vi.waitFor(() => {
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
    });
    expect(mockCommands.resolveExpectedUrlForFile).not.toHaveBeenCalled();
  });

  test('opening an EXISTING file via a plain tree click does NOT force-focus CM6 (no isNewFile flag)', async () => {
    // Scope guard: ordinary tree-click opens of existing files must NOT have
    // their focus silently redirected — that would surprise keyboard
    // tree-navigation users. Only isNewFile: true (file creation) triggers
    // the focus call.
    const filePath = '/Users/test/blog/content/posts/hello.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });
    mockCommands.resolveUrlForFile.mockResolvedValue({
      status: 'ok',
      data: '/posts/hello/',
    });

    const focusSpy = vi.fn();
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue('# Hello'),
      getView: vi.fn().mockReturnValue({
        scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() },
        focus: focusSpy,
      }),
      setContent: vi.fn(),
    }));

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath },
    }));

    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalled());
    await vi.waitFor(() => {
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/posts/hello/' });
    });
    expect(focusSpy).not.toHaveBeenCalled();
  });

  // NOTE: a "stale cmEditor after a superseding concurrent load" scenario
  // (loadFile() has no per-call identity/generation guard of its own, unlike
  // applyHeadingState's isStaleResult check) was manually verified against
  // the code above — added `currentFilePath() === loadedPath` (path
  // normalized via toRelative, since e.detail.filePath is not guaranteed
  // relative) as a defensive guard so the isNewFile focus call never fires
  // against a cmEditor instance that no longer corresponds to the file this
  // handler loaded. An automated regression test for this exact interleaving
  // was attempted but dropped: this file's `editor:file-selected` listener is
  // registered on the shared `document` by every test's `init()` call and is
  // never torn down between tests, so a test asserting exact call counts
  // across two concurrent loadFile() calls is contaminated by the (harmless,
  // pre-existing) accumulation of stale listeners from earlier tests once
  // run as part of the full suite — it passes in isolation but is flaky
  // as part of the file. The two focus tests above cover the primary
  // (non-concurrent) behavior this guard must not regress.

  test('editor:folder-navigate resolves the folder URL and emits editor:navigate-preview', async () => {
    // Empty folders are now published as their own index page, so a forced
    // folder navigation (folder-create or the delete-fallback) must move the
    // preview to that folder's page — reusing the same navigate-preview seam.
    const folderPath = '/Users/test/blog/my-1st-folder';
    mockCommands.resolveUrlForFolder.mockResolvedValue({
      status: 'ok',
      data: '/my-1st-folder/',
    });

    await bootEditorMain();
    // editor:load-source registers AFTER the folder-navigate handler, so its
    // presence proves the folder-navigate listener is already wired.
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    mockEmit.mockClear();
    document.dispatchEvent(new CustomEvent('editor:folder-navigate', {
      detail: { folderPath, folderName: 'my-1st-folder' },
    }));

    await vi.waitFor(() => {
      expect(mockCommands.resolveUrlForFolder).toHaveBeenCalledWith(folderPath);
      // No `justCreated` on the event (this is the delete-fallback shape), so
      // no wait is asked for: the folder's page is already in the build output
      // and a "preparing" screen over it would be one flash traded for another.
      expect(mockEmit).toHaveBeenCalledWith('editor:navigate-preview', {
        urlPath: '/my-1st-folder/', awaitingBuild: false,
      });
    });
  });

  test('editor:folder-navigate with a null-URL folder does NOT emit navigate-preview', async () => {
    // Null-safe: a folder that still resolves to no URL (Ok(None)) leaves the
    // preview where it is — no throw, no "/" fallback.
    const folderPath = '/Users/test/blog/orphan-folder';
    mockCommands.resolveUrlForFolder.mockResolvedValue({ status: 'ok', data: null });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    mockEmit.mockClear();
    document.dispatchEvent(new CustomEvent('editor:folder-navigate', {
      detail: { folderPath, folderName: 'orphan-folder' },
    }));

    // Let the async resolve settle, then assert no navigate-preview emit.
    await vi.waitFor(() => {
      expect(mockCommands.resolveUrlForFolder).toHaveBeenCalledWith(folderPath);
    });
    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', expect.anything());
  });

  // REMOVED: 'file selection does NOT emit "/" fallback when resolveUrlForFile
  // rejects'. The no-'/'-fallback rule belongs to PreviewFollower and is pinned
  // there twice, on both shapes of dead end — 'navigate() with unresolvable URL
  // does NOT emit "/" fallback' and 'a navigation that blows up is contained'
  // (preview-follower.test.ts). All this test added on top was the wiring from
  // editor:file-selected into the follower, which the sibling test above ('file
  // selection from tree emits editor:navigate-preview with resolved URL')
  // already proves. The rename case below is NOT a duplicate: it pins that a
  // rename does not reach the follower by a second, older route.

  test('editor:rename-complete with null-URL resolution does NOT emit navigate-preview "/" fallback', async () => {
    // Regression for the May 26 home-jump bug (commit 7f7c80725). The
    // editor:rename-complete listener used to call resolveUrlForFile(newPath)
    // and fall back to '/' if null, which routed through preview→editor
    // sync and re-loaded the home file. The PreviewFollower replaces that
    // listener and must never emit '/'.
    const OLD = 'content/posts/old.md';
    const NEW = 'content/posts/new.md';
    const OLD_ABS = '/Users/test/blog/content/posts/old.md';

    MockChipBar.mockImplementation(() => ({
      setPageFacts: vi.fn(),
      render: vi.fn(),
      destroy: vi.fn(),
      getValues: vi.fn().mockReturnValue({}),
      setValues: vi.fn(),
    }));
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
      setContent: vi.fn(),
    }));

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Old' }, body: '# Old' },
    });
    // Pre-build state: resolveUrlForFile(NEW) returns null. This is the
    // failure mode the May 26 listener mishandled by falling back to '/'.
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: null });

    await bootEditorMain();

    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: OLD_ABS, isDir: false });
    mockEmit.mockClear();

    // Dispatch the LEGACY event that the old broken listener consumed. After
    // the refactor the listener is gone; PreviewFollower's path-change
    // subscriber handles rename via registry.rename instead. Either way,
    // the forbidden emit is '/'.
    document.dispatchEvent(new CustomEvent('editor:rename-complete', {
      detail: { oldPath: OLD, newPath: NEW },
    }));

    // Drain microtasks deterministically rather than via setTimeout. The legacy
    // listener (if present) does: await resolveUrlForFile → await emit, so we
    // need enough microtask drains for both awaits to settle.
    for (let i = 0; i < 20; i++) await Promise.resolve();

    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/' });
  });
});

// ---------------------------------------------------------------------------
// URL query parameter: init reads ?source= and loads file
// ---------------------------------------------------------------------------

describe('editor-main URL query parameter loading', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    // Provide a project root so loadFile can relativize the ?source= path.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    // Restore window.location
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      writable: true,
    });
    vi.restoreAllMocks();
  });

  const originalLocation = window.location;

  test('init loads file from ?source= URL parameter', async () => {
    const filePath = '/Users/test/blog/content/index.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Home' }, body: '# Home' },
    });

    // Set URL with source query param
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?source=${encodeURIComponent(filePath)}` },
      writable: true,
    });

    await bootEditorMain();

    await vi.waitFor(() => {
      expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(filePath);
    });
  });

  test('init does not load file when no ?source= parameter', async () => {
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: '' },
      writable: true,
    });

    await bootEditorMain();

    // Wait for init to complete
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });

  test('boot on non-empty root with no index.md shows the home CTA (not an arbitrary file)', async () => {
    // Editor URL carries is_root=1, no source. Folder is non-empty → the root-home takeover.
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
    mockGetFirstFile.mockReturnValue('about.md');
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: '?is_root=1' }, writable: true, configurable: true,
    });
    await bootEditorMain();
    const cm = document.getElementById('moss-editor-cm')!;
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('takeover'));
    expect(MockCmEditor).not.toHaveBeenCalled(); // did NOT open getFirstFile()
  });

  test('boot on a no-home subfolder shows the Create-folder-page CTA', async () => {
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
    mockGetFirstFile.mockReturnValue('about.md'); // folder has content; this node is a subfolder
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: '?takeover=' + encodeURIComponent(JSON.stringify(folderHomeTakeover('/Users/test/blog/Notes', 'Notes'))) },
      writable: true, configurable: true,
    });
    await bootEditorMain();
    const cm = document.getElementById('moss-editor-cm')!;
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('takeover'));
    expect(cm.querySelector('.moss-pane-state__title')?.textContent).toBe('Notes');
  });
});

// ---------------------------------------------------------------------------
// Bootstrap RPC: init boots from ONE editor_bootstrap round trip
// ---------------------------------------------------------------------------

describe('editor-main bootstrap RPC', () => {
  const originalLocation = window.location;

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      writable: true,
    });
    vi.restoreAllMocks();
  });

  test('init makes a single editorBootstrap RPC — getSchema/getProjectPath are never called', async () => {
    // Literal override (bypassing the delegate) pins the production code path:
    // getSchema/getProjectPath are now only reachable through the delegate.
    mockCommands.editorBootstrap.mockReset().mockResolvedValue({
      status: 'ok',
      data: { schema: { fields: [] }, project_path: '/Users/test/blog', initial_file: null, root_name: 'blog' },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    expect(mockCommands.editorBootstrap).toHaveBeenCalledTimes(1);
    expect(mockCommands.getSchema).not.toHaveBeenCalled();
    expect(mockCommands.getProjectPath).not.toHaveBeenCalled();
  });

  test('pre-parsed initial_file skips the parseFrontmatter round trip', async () => {
    const filePath = '/Users/test/blog/posts/hello.md';
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?source=${encodeURIComponent(filePath)}` },
      writable: true,
      configurable: true,
    });
    mockCommands.editorBootstrap.mockReset().mockResolvedValue({
      status: 'ok',
      data: {
        schema: { fields: [] },
        project_path: '/Users/test/blog',
        root_name: 'blog',
        initial_file: { frontmatter: { title: 'Hello' }, body: '# Hello', has_frontmatter: true },
      },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(MockCmEditor).toHaveBeenCalled();
    });

    expect(mockCommands.editorBootstrap).toHaveBeenCalledWith(filePath);
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
    expect(MockCmEditor.mock.calls[0][0]).toMatchObject({ content: '# Hello' });
  });

  test('bootstrap parse-miss (initial_file null) falls back to parseFrontmatter', async () => {
    const filePath = '/Users/test/blog/posts/hello.md';
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?source=${encodeURIComponent(filePath)}` },
      writable: true,
      configurable: true,
    });
    // Delegate default: initial_file null → loadFile fetches the parse itself.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello', has_frontmatter: true },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(1);
    });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(filePath);
  });

  test('bootstrap error short-circuits init (no file tree, no listeners)', async () => {
    mockCommands.editorBootstrap.mockReset().mockResolvedValue({
      status: 'error',
      error: 'boom',
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(mockCommands.editorBootstrap).toHaveBeenCalledTimes(1);
    });
    // Let init's post-bootstrap microtasks (if any) settle.
    await Promise.resolve();
    await Promise.resolve();

    expect(MockFileTree).not.toHaveBeenCalled();
    expect(listenCallbacks.has('editor:load-source')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Cold-start skeleton: removed on every boot path
// ---------------------------------------------------------------------------

describe('editor-main cold-start skeleton', () => {
  const originalLocation = window.location;

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      writable: true,
    });
    vi.restoreAllMocks();
  });

  test('successful boot with an initial source removes both skeleton nodes', async () => {
    const filePath = '/Users/test/blog/posts/hello.md';
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?source=${encodeURIComponent(filePath)}` },
      writable: true,
      configurable: true,
    });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello', has_frontmatter: true },
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(MockCmEditor).toHaveBeenCalled();
    });

    // Content skeleton: structurally gone (CM6 mount clears #moss-editor-cm).
    expect(document.getElementById('moss-editor-skeleton')).toBeNull();
    // Nav skeleton: explicitly dismissed before fileTree.mount.
    expect(document.getElementById('moss-nav-skeleton')).toBeNull();
  });

  test('bootstrap error → the .finally failsafe removes both skeleton nodes', async () => {
    mockCommands.editorBootstrap.mockReset().mockResolvedValue({
      status: 'error',
      error: 'boom',
    });

    await bootEditorMain();
    await vi.waitFor(() => {
      expect(document.getElementById('moss-editor-skeleton')).toBeNull();
    });
    expect(document.getElementById('moss-nav-skeleton')).toBeNull();
  });

  test('no-project + no-source boot: the failsafe is the SOLE nav-skeleton remover', async () => {
    // resetMocks default: getProjectPath errors → the delegate yields
    // project_path null. No URL source either. The explicit
    // dismissNavSkeleton() is gated behind project_path and never runs —
    // pin that by asserting the tree never mounted while the skeleton is
    // still removed (by init().finally(dismissEditorSkeleton) ONLY).
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(document.getElementById('moss-nav-skeleton')).toBeNull();
    });
    expect(document.getElementById('moss-editor-skeleton')).toBeNull();
    // The tree was constructed but never mounted (no project) — so the
    // explicit pre-mount dismissal cannot have run.
    const treeInstance = MockFileTree.mock.results[0]?.value;
    expect(treeInstance).toBeDefined();
    expect(treeInstance.mount).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Scroll sync always-on: toggle button absent, no scrollSyncEnabled guard
// ---------------------------------------------------------------------------

/** Minimal editor DOM. Used to verify the scroll-sync toggle button is
 *  never created during init. */
function setupEditorDOMForScrollSync(): void {
  document.body.innerHTML = `
    <div id="nav-surface">
      <div id="nav-grid"></div>
    </div>
    <div id="divider"></div>
    <div id="chip-bar"></div>
    <div id="moss-editor-cm"></div>
    <div id="moss-save-indicator"></div>
  `;
}

describe('scroll sync always-on', () => {
  beforeEach(() => {
    // Retire every editor-main instance an earlier test imported: their window
    // listeners (resize, blur) would otherwise answer this test's events too.
    window.dispatchEvent(new Event('beforeunload'));
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMForScrollSync();
    resetMocks();
    localStorage.clear();
    // Provide a project root so loadFile can relativize absolute paths,
    // silencing guardRelative warnings from registry.upsert().
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('scroll-sync-toggle button is NOT created after init', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();

    // Wait for init to complete
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    const toggleBtn = document.querySelector('.scroll-sync-toggle');
    expect(toggleBtn).toBeNull();
  });

  test('moss-scroll-sync localStorage key is never written during init', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // The key must never be written — scroll sync is always active
    expect(localStorage.getItem('moss-scroll-sync')).toBeNull();
  });

  /** Load a file into a mock CM6 whose scroller is a real element, so the
   *  tests below can dispatch the user's input and the scroll it causes. */
  async function loadWithRealScroller(): Promise<{ scroller: HTMLElement; caretMoved: () => void; historyKey: (d: 'undo' | 'redo') => void; dispatch: ReturnType<typeof vi.fn> }> {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '# Hello\n\nSome content.' },
    });
    const scroller = document.createElement('div');
    const dispatch = vi.fn();
    let onCaretMove: (() => void) | null = null;
    let onHistoryKey: ((d: 'undo' | 'redo') => void) | null = null;
    MockCmEditor.mockImplementation((opts: { onCaretMove?: () => void; onHistoryKey?: (d: 'undo' | 'redo') => void }) => {
      onCaretMove = opts.onCaretMove ?? null;
      onHistoryKey = opts.onHistoryKey ?? null;
      return {
        destroy: vi.fn(),
        getContent: vi.fn().mockReturnValue('# Hello\n\nSome content.'),
        getView: vi.fn().mockReturnValue({
          scrollDOM: scroller,
          contentDOM: { offsetTop: 0 },
          dispatch,
          focus: vi.fn(),
          // The pane's click-focus probe checks this after a press; a focused
          // editor is the healthy case it leaves alone.
          hasFocus: true,
          lineBlockAtHeight: vi.fn().mockReturnValue({ from: 0 }),
          coordsAtPos: vi.fn().mockReturnValue({ top: 0 }),
          state: {
            selection: { main: { head: 0 } },
            doc: {
              lineAt: vi.fn(() => ({ number: 7 })),
              toString: vi.fn().mockReturnValue('# Hello\n\nSome content.'),
              lines: 3,
              line: vi.fn().mockReturnValue({ from: 0 }),
            },
          },
        }),
      };
    });

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/content/posts/hello.md', isDir: false });
    dispatch.mockClear();
    // The editor was handed the callback — this is the wiring the boot mocks
    // used to make untestable when it went through view.dispatch.
    expect(onCaretMove).not.toBeNull();
    // Where CM6 puts its scroller: inside the editor's pane.
    document.getElementById('moss-editor-cm')!.appendChild(scroller);
    return { scroller, caretMoved: () => onCaretMove!(), historyKey: (d) => onHistoryKey!(d), dispatch };
  }

  /** Run `act` and report every editor→preview sync it produced, past both
   *  debounces (100ms scroll, 120ms caret). */
  function syncsDuring(act: () => void): unknown[] {
    mockEmit.mockClear();
    vi.useFakeTimers();
    try {
      act();
      vi.advanceTimersByTime(200);
    } finally {
      vi.useRealTimers();
    }
    return mockEmit.mock.calls
      .filter(([name]) => name === 'scroll-sync:editor-to-preview')
      .map(([, payload]) => payload);
  }

  test('a scroll the user makes in the editor moves the preview', async () => {
    const { scroller } = await loadWithRealScroller();

    const syncs = syncsDuring(() => {
      scroller.dispatchEvent(new Event('wheel'));
      scroller.dispatchEvent(new Event('scroll'));
    });

    expect(syncs).toEqual([expect.objectContaining({ type: 'source-line', line: 7 })]);
  });

  // The insert bar is the pane's, not the scroller's: a click there is work in
  // the editor even though it never touches CM6's DOM.
  test('a click anywhere in the editor pane hands it the lead', async () => {
    const { scroller, caretMoved } = await loadWithRealScroller();
    const bar = document.createElement('div');
    document.getElementById('moss-editor-cm')!.appendChild(bar);

    const syncs = syncsDuring(() => {
      bar.dispatchEvent(new Event('pointerdown'));
      caretMoved();
      scroller.dispatchEvent(new Event('scroll'));
    });

    expect(syncs).toHaveLength(2);
  });

  // ⌘Z from the Edit menu arrives as a menu event, never as a keydown.
  test('undo hands the editor the lead', async () => {
    const { caretMoved, historyKey } = await loadWithRealScroller();

    const syncs = syncsDuring(() => {
      historyKey('undo');
      caretMoved();
    });

    expect(syncs).toEqual([expect.objectContaining({ line: 7, onlyIfOffscreen: true })]);
  });

  // The reported bug: CM6 fires `scroll` for layout too. Opening the editor
  // grows its panel from collapsed; the reflow moves scrollTop, and the editor
  // pushed that position into the preview the user was reading.
  test('a scroll nobody made in the editor leaves the preview where it is', async () => {
    const { scroller } = await loadWithRealScroller();

    expect(syncsDuring(() => scroller.dispatchEvent(new Event('scroll')))).toEqual([]);
  });

  test('reopening the editor hands the lead back to the preview', async () => {
    const { scroller } = await loadWithRealScroller();
    scroller.dispatchEvent(new Event('pointerdown'));

    // Same file, so the warm open keeps this CM6 instance and its scroller.
    const open = (window as unknown as { __moss_editor_open: (n: unknown) => void }).__moss_editor_open;
    open({ sourcePath: '/Users/test/blog/content/posts/hello.md', isRoot: false, takeover: null });

    expect(syncsDuring(() => scroller.dispatchEvent(new Event('scroll')))).toEqual([]);
  });

  test('a scroll arriving from the preview is not sent back', async () => {
    const { scroller } = await loadWithRealScroller();
    scroller.dispatchEvent(new Event('keydown'));

    listenCallbacks.get('scroll-sync:preview-to-editor')!({ type: 'source-line', line: 3 });

    expect(syncsDuring(() => scroller.dispatchEvent(new Event('scroll')))).toEqual([]);
  });

  // The caret is the SECOND sync trigger. Until it existed the
  // preview followed the editor only when the scrollbar moved, so clicking
  // something, jumping with a keystroke, or typing on a line already on screen
  // moved nothing — the preview could sit on a different part of the page than
  // the one being edited until a scroll happened to shake it loose.
  test('the caret moving syncs the preview, and asks it not to move if it need not', async () => {
    const { scroller, caretMoved } = await loadWithRealScroller();

    const syncs = syncsDuring(() => {
      scroller.dispatchEvent(new Event('keydown'));
      caretMoved();
    });

    expect(syncs).toEqual([
      expect.objectContaining({
        line: 7,
        // The whole reason this trigger is tolerable: the preview drops the
        // request when the target is already in view.
        onlyIfOffscreen: true,
        // Edge short-circuits describe the SCROLLER's position, which says
        // nothing about where the caret is.
        atTop: false,
        atBottom: false,
      }),
    ]);
  });

  // A double click in the preview puts the caret on the line it maps to. The
  // preview is already showing that line — it is where the user clicked.
  // ── Following means going where the preview is ─────────────────────────
  //
  // The preview reports its position only when it scrolls, so an editor that
  // opens over a page the user scrolled and left has to ask.

  describe('following means going where the preview is', () => {
    // A timer an earlier test's instance scheduled still fires into the shared
    // emit mock; let it, then start counting.
    beforeEach(async () => {
      await new Promise((r) => setTimeout(r, 200));
      mockEmit.mockClear();
    });

    const settle = () => new Promise((r) => setTimeout(r, 200)); // past the 150ms debounce
    const asked = () => mockEmit.mock.calls.filter(([name]) => name === 'scroll-sync:request-preview-position');
    const HELLO = '/Users/test/blog/content/posts/hello.md';

    test('once a document loads, the editor asks where the preview is', async () => {
      await loadWithRealScroller();
      await settle();
      expect(asked()).toEqual([['scroll-sync:request-preview-position', { sourceFile: HELLO }]]);
    });

    test('reopening on the same document asks again', async () => {
      await loadWithRealScroller();
      await settle();
      mockEmit.mockClear();

      (window as unknown as { __moss_editor_open: (n: unknown) => void })
        .__moss_editor_open({ sourcePath: HELLO, isRoot: false, takeover: null });
      await settle();

      expect(asked()).toHaveLength(1);
    });

    // The panel's grow animation resizes the editor on every frame; the answer
    // to a question asked mid-animation lands on text that is about to re-wrap.
    test('a resize while following asks again once it settles', async () => {
      await loadWithRealScroller();
      await settle();
      mockEmit.mockClear();

      for (let i = 0; i < 5; i++) window.dispatchEvent(new Event('resize'));
      await settle();

      expect(asked()).toHaveLength(1);
    });

    test('an editor the user is working in does not ask', async () => {
      const { scroller } = await loadWithRealScroller();
      scroller.dispatchEvent(new Event('pointerdown'));
      await settle();
      expect(asked()).toEqual([]);
    });

    // The answer arrives on the same event as a preview scroll, but nothing
    // moved over there — so it loses to whatever happened here in the meantime.
    const answer = (sourceFile: string) =>
      listenCallbacks.get('scroll-sync:preview-to-editor')!({ type: 'source-line', line: 3, answering: sourceFile });

    test('the answer moves an editor that is still following', async () => {
      const { dispatch } = await loadWithRealScroller();
      answer(HELLO);
      expect(dispatch).toHaveBeenCalledTimes(1);
    });

    test('an answer that arrives after the user started working here is dropped', async () => {
      const { scroller, dispatch } = await loadWithRealScroller();
      scroller.dispatchEvent(new Event('wheel'));
      answer(HELLO);
      expect(dispatch).not.toHaveBeenCalled();
      // ...and the editor keeps the lead the user just took.
      expect(syncsDuring(() => scroller.dispatchEvent(new Event('scroll')))).toHaveLength(1);
    });

    test('an answer about the document the editor has since left is dropped', async () => {
      const { dispatch } = await loadWithRealScroller();
      answer('/Users/test/blog/content/posts/other.md');
      expect(dispatch).not.toHaveBeenCalled();
    });

    test('leaving the editor hands the lead to the preview', async () => {
      const { scroller } = await loadWithRealScroller();
      scroller.dispatchEvent(new Event('keydown'));

      window.dispatchEvent(new Event('blur'));

      expect(syncsDuring(() => scroller.dispatchEvent(new Event('scroll')))).toEqual([]);
    });
  });

  // A double click in the preview lands in the preview's webview; the jump it
  // asks for must bring the keyboard with it.
  test('a double-click jump from the preview takes the keyboard for the editor', async () => {
    await loadWithRealScroller();
    mockFocusThisWebview.mockClear();
    listenCallbacks.get('editor:goto-line')!({ line: 7 });
    expect(mockFocusThisWebview).toHaveBeenCalledTimes(1);
  });

  test('a caret the preview placed does not move the preview', async () => {
    const { scroller, caretMoved } = await loadWithRealScroller();
    scroller.dispatchEvent(new Event('keydown'));

    const syncs = syncsDuring(() => {
      listenCallbacks.get('editor:goto-line')!({ line: 7 });
      caretMoved();
    });

    expect(syncs).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// file-changed listener: preserve cursor and focus via in-place update
// ---------------------------------------------------------------------------

describe('editor-main file-changed listener', () => {
  // Absolute paths used for: sourcePath (editor:load-source wire), parseFrontmatter assertions.
  const FILE = '/Users/test/blog/content/posts/hello.md';
  const OTHER = '/Users/test/blog/content/posts/other.md';
  // Relative paths used for: modified_paths / moved_output_paths in file-changed
  // payloads. EntryRegistry stores relative keys,
  // so affectsCurrentFile's registry.getByPath() lookup requires the relative form.
  const FILE_REL = 'content/posts/hello.md';
  const OTHER_REL = 'content/posts/other.md';

  async function loadInitial(
    cmEditorMock: () => object,
    chipBarMock: () => object,
  ): Promise<void> {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello\n\nbody' },
    });
    MockCmEditor.mockImplementation(cmEditorMock);
    MockChipBar.mockImplementation(() => ({ setPageFacts: vi.fn(), ...chipBarMock() }));

    await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
      expect(listenCallbacks.has('moss-event')).toBe(true);
    });

    // Prime currentFilePath by loading the file
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: FILE, isDir: false });
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalled());
  }

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    // Provide a project root so loadFile relativizes the absolute FILE path
    // before calling registry.upsert(), silencing guardRelative warnings.
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('updates CM6 and form IN-PLACE when current file changes externally (no destroy)', async () => {
    let cmDestroyed = 0;
    let setContentCalledWith: string | null = null;

    await loadInitial(
      () => ({
        destroy: vi.fn(() => { cmDestroyed++; }),
        getContent: vi.fn().mockReturnValue('# Hello\n\nbody'),
        getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
        setContent: vi.fn((body: string) => { setContentCalledWith = body; }),
      }),
      () => ({
        render: vi.fn(),
        destroy: vi.fn(),
        getValues: vi.fn().mockReturnValue({}),
        // setValues is no longer called directly on ChipBar — frontmatter
        // reconciliation now flows through FrontmatterStore.onFileChanged,
        // which notifies the ChipBar subscriber. The mock ChipBar captures
        // only what is actually called on it.
        setValues: vi.fn(),
      }),
    );

    cmDestroyed = 0; // reset — one destroy may have happened during initial load

    // Arrange: fresh parse returns new content
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Fresh' }, body: '# Fresh\n\nupdated body' },
    });

    // Act: fire FileChanged via the unified moss-event channel for THIS file.
    // modified_paths uses the registry-relative form so registry.getByPath()
    // correctly identifies the active file (affectsCurrentFile check).
    const cb = listenCallbacks.get('moss-event')!;
    await cb({ kind: 'FileChanged', payload: { modified_paths: [FILE_REL] } });

    // Assert: CM6 was updated in place (no destroy, setContent called with new body).
    // Frontmatter reconciliation is verified by the fact that parseFrontmatter
    // was called — the store's onFileChanged path handles the chip-bar update.
    expect(cmDestroyed).toBe(0);
    expect(setContentCalledWith).toBe('# Fresh\n\nupdated body');
    // Verify parseFrontmatter was called (data was processed, not just body):
    // handler calls parseFrontmatter(toAbsolute(currentFilePath(), projectRoot)) = FILE.
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(FILE);
  });

  test('ignores file-changed events for OTHER files', async () => {
    let setContentCalled = false;
    let setValuesCalled = false;

    await loadInitial(
      () => ({
        destroy: vi.fn(),
        getContent: vi.fn().mockReturnValue(''),
        getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
        setContent: vi.fn(() => { setContentCalled = true; }),
      }),
      () => ({
        render: vi.fn(),
        destroy: vi.fn(),
        getValues: vi.fn().mockReturnValue({}),
        setValues: vi.fn(() => { setValuesCalled = true; }),
      }),
    );

    mockCommands.parseFrontmatter.mockClear();

    const cb = listenCallbacks.get('moss-event')!;
    // OTHER_REL: registry-relative form so getByPath() lookup resolves correctly.
    await cb({ kind: 'FileChanged', payload: { modified_paths: [OTHER_REL] } });

    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
    expect(setContentCalled).toBe(false);
    expect(setValuesCalled).toBe(false);
  });

  test('source rename updates identity without treating output paths as a source edit', async () => {
    let setContentCalled = false;
    await loadInitial(
      () => ({
        destroy: vi.fn(),
        getContent: vi.fn().mockReturnValue(''),
        getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
        setContent: vi.fn(() => { setContentCalled = true; }),
      }),
      () => ({
        render: vi.fn(),
        destroy: vi.fn(),
        getValues: vi.fn().mockReturnValue({}),
        setValues: vi.fn(),
      }),
    );

    const cb = listenCallbacks.get('moss-event')!;
    // source_renames is consumed by EntryRegistry to preserve identity. It is
    // not a body reload signal; modified_paths is the source-domain reload
    // signal and is covered by the tests above.
    await cb({ kind: 'FileChanged', payload: {
      source_renames: [[FILE_REL, 'content/posts/renamed.md']],
      moved_output_paths: [['blog/old-post/index.html', 'blog/new-post/index.html']],
    } });

    expect(setContentCalled).toBe(false);
  });

  test('bails on null/undefined payload without throwing', async () => {
    await loadInitial(
      () => ({
        destroy: vi.fn(),
        getContent: vi.fn().mockReturnValue(''),
        getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
        setContent: vi.fn(),
      }),
      () => ({
        render: vi.fn(),
        destroy: vi.fn(),
        getValues: vi.fn().mockReturnValue({}),
        setValues: vi.fn(),
      }),
    );

    mockCommands.parseFrontmatter.mockClear();

    // The new single handler routes through onMossEvent('FileChanged'), which
    // filters by kind and passes the typed payload to dispatchEditorFileChanged.
    // Send an empty FileChanged event (no modified_paths, no source_creates) —
    // should not throw or trigger any reload.
    const cb = listenCallbacks.get('moss-event')!;
    await cb({ kind: 'FileChanged', payload: {} });
    await cb({ kind: 'FileChanged', payload: { modified_paths: [], source_creates: [] } });

    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// FileChangeEvent source-domain routing to EntryRegistry
//
// Routing runs UNCONDITIONALLY (before the currentFilePath guard) so the
// registry stays accurate even when the currently-open file is not directly
// affected.
//
// The registry is a real EntryRegistry instance (no Tauri deps). We capture
// it from MockFileTree.mock.calls[0][0] — editor-main passes `registry` as
// the first arg to the FileTree constructor.
// ---------------------------------------------------------------------------

describe('editor-main FileChangeEvent registry routing', () => {
  // source_creates /
  // source_renames / source_deletes carry project-root-relative wire
  // paths (as path_to_relative_key in watch.rs always has). FILE_ABS is the
  // absolute form used only when simulating editor:load-source (which
  // delivers absolute paths from the preview iframe).
  const FILE = 'content/posts/current.md';
  const NEW_FILE = 'content/posts/new.md';
  const FILE_ABS = '/Users/test/blog/content/posts/current.md';
  const NEW_FILE_ABS = '/Users/test/blog/content/posts/new.md';

  /** Initialize the editor with a project path so FileTree + registry are created. */
  async function initWithProject(): Promise<void> {
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('moss-event')).toBe(true);
    });
    // FileTree is constructed once the project path resolves.
    await vi.waitFor(() => {
      expect(MockFileTree.mock.calls.length).toBeGreaterThan(0);
    });
  }

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMWithTree();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    // Discard any pending debounce timers (e.g. flashDownloadTargets' 400ms
    // setTimeout) so they can't fire into the next test's DOM after
    // vi.resetModules() creates a fresh module instance.
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test('source_creates: new path is registered in the registry', async () => {
    await initWithProject();
    // Capture the real registry from the FileTree constructor call.
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];
    expect(registryArg).toBeDefined();

    const cb = listenCallbacks.get('moss-event')!;
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: [NEW_FILE],
          source_deletes: null,
          source_renames: null,
          modified_paths: null,
        },
      });

    expect(registryArg.getByPath(NEW_FILE)).not.toBeNull();
  });

  test('source_deletes: path is retired from the registry', async () => {
    await initWithProject();
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];

    // Pre-populate via source_creates
    const cb = listenCallbacks.get('moss-event')!;
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: [NEW_FILE],
          source_deletes: null,
          source_renames: null,
          modified_paths: null,
        },
      });
    expect(registryArg.getByPath(NEW_FILE)).not.toBeNull();

    // Now delete it
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: null,
          source_deletes: [NEW_FILE],
          source_renames: null,
          modified_paths: null,
        },
      });

    expect(registryArg.getByPath(NEW_FILE)).toBeNull();
  });

  test('source_renames: EntryId is preserved across rename', async () => {
    await initWithProject();
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];

    const cb = listenCallbacks.get('moss-event')!;
    // Create the file first
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: [FILE],
          source_deletes: null,
          source_renames: null,
          modified_paths: null,
        },
      });
    const idBefore = registryArg.getByPath(FILE);
    expect(idBefore).not.toBeNull();

    // Rename it
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: null,
          source_deletes: null,
          source_renames: [[FILE, NEW_FILE]],
          modified_paths: null,
        },
      });

    // Old path gone, new path has the same EntryId
    expect(registryArg.getByPath(FILE)).toBeNull();
    expect(registryArg.getByPath(NEW_FILE)).toBe(idBefore);
  });

  test('routing runs unconditionally even without a currently-open file', async () => {
    // No file is loaded (no editor:load-source call) — registry routing must
    // still fire so the registry stays consistent with the filesystem.
    await initWithProject();
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];

    const cb = listenCallbacks.get('moss-event')!;
    await cb({
        kind: 'FileChanged',
        payload: {
          changed_output_files: null,
          deleted_paths: null,
          moved_output_paths: null,
          source_creates: [NEW_FILE],
          source_deletes: null,
          source_renames: null,
          modified_paths: null,
        },
      });

    expect(registryArg.getByPath(NEW_FILE)).not.toBeNull();
  });

  test('source_renames: editor follows external rename (currentFilePath + currentEntryId lockstep)', async () => {
    // This test covers the load-bearing external-rename-echo path:
    // when the currently-open file is renamed externally, BOTH
    // currentFilePath and currentEntryId must update in lockstep, so the
    // editor stays in sync and future file-changed events route correctly.

    // Set up mocks with necessary ChipBar.setValues
    MockChipBar.mockImplementation(() => ({
      setPageFacts: vi.fn(),
      render: vi.fn(),
      destroy: vi.fn(),
      getValues: vi.fn().mockReturnValue({}),
      setValues: vi.fn(),
    }));
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
      setContent: vi.fn(),
    }));

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await initWithProject();
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];

    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    const fileChangedCb = listenCallbacks.get('moss-event')!;
    /** Wrap a FileChangeEvent payload for the unified moss-event channel. */
    const fire = (fce: Record<string, unknown>) =>
      fileChangedCb({ kind: 'FileChanged', payload: fce });

    // Create the file in the registry via source_creates.
    await fire({
      changed_output_files: null,
      deleted_paths: null,
      moved_output_paths: null,
      source_creates: [FILE],
      source_deletes: null,
      source_renames: null,
      modified_paths: null,
    });
    const idBefore = registryArg.getByPath(FILE);
    expect(idBefore).not.toBeNull();

    // Load FILE via editor:load-source so currentFilePath = FILE.
    // editor:load-source delivers absolute paths (from preview iframe);
    // loadFile normalizes to relative at the entry.
    await loadSourceCb({ sourcePath: FILE_ABS, isDir: false });
    mockCommands.parseFrontmatter.mockClear();

    // Pre-register NEW_FILE in the registry (simulating what a
    // refreshTree would have done before the rename echo arrives).
    await fire({
      changed_output_files: null,
      deleted_paths: null,
      moved_output_paths: null,
      source_creates: [NEW_FILE],
      source_deletes: null,
      source_renames: null,
      modified_paths: null,
    });

    // External rename echo arrives with source_renames containing
    // [FILE, NEW_FILE]. The editor's file-changed handler must update
    // currentFilePath to NEW_FILE and look up its EntryId.
    // After this, the old path must be gone and the new path must resolve
    // to the same EntryId (Entry IDs are stable across renames).
    await fire({
      changed_output_files: null,
      deleted_paths: null,
      moved_output_paths: null,
      source_creates: null,
      source_deletes: null,
      source_renames: [[FILE, NEW_FILE]],
      modified_paths: null,
    });

    // Assert 1: Registry state is correct (identity preserved).
    expect(registryArg.getByPath(FILE)).toBeNull();
    expect(registryArg.getByPath(NEW_FILE)).toBe(idBefore);

    // Assert 2: Editor's currentFilePath was updated by checking that
    // file-changed for the OLD path is now ignored (currentFilePath is no
    // longer OLD) and file-changed for the NEW path is now acted upon.
    // This is the gold standard test: it verifies that the editor actually
    // followed the rename at the app level.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'old' },
    });

    // First, dispatch file-changed for OLD. It should be ignored.
    await fire({ modified_paths: [FILE] });
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();

    // Now dispatch file-changed for NEW. It MUST be acted upon, proving
    // that currentFilePath is now NEW.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'new' },
    });
    // modified_paths on the wire is source-relative. The file-changed handler
    // matches against the registry via getByPath and then calls
    // parseFrontmatter with toAbsolute(currentFilePath).
    await fire({ modified_paths: [NEW_FILE] });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(NEW_FILE_ABS);

    // Assert 3: No drift warning was logged (assertEditorIdentityConsistent
    // was not triggered with a mismatch). This is a behavioral check: if
    // lockstep update failed, the warning would appear in console. We don't
    // have direct access to the warning in tests, but the successful routing
    // above proves the state is consistent.
  });
});

// ---------------------------------------------------------------------------
// Tree rename → editor coordination
//
// The file-tree dispatches editor:file-renaming BEFORE renameEntry (so the
// watcher's file-changed echo is silenced) and editor:file-renamed AFTER it
// succeeds (so currentFilePath updates without triggering a preview-driven
// reload that lands the editor on the home file).
// ---------------------------------------------------------------------------

describe('editor-main tree rename listeners', () => {
  // Registry paths are project-root-relative:
  //  - editor:file-renamed.detail carries project-root-relative paths
  //    (FileTree.dispatchRenameCompleted forwards entry.path, which is relative).
  //  - editor:load-source.detail.sourcePath stays absolute on the wire;
  //    loadFile normalizes at the entry.
  //  - file-changed wire payloads (modified_paths / source_renames etc.)
  //    are relative in the registry-routing block and absolute when they
  //    represent output-domain paths under .moss/build/site/. This test
  //    fixture uses the OLD/NEW symbols for the registry-domain (relative)
  //    form because that's what FileTree dispatches and what the registry
  //    consumes.
  const OLD = 'content/posts/old.md';
  const NEW = 'content/posts/new.md';
  const OLD_ABS = '/Users/test/blog/content/posts/old.md';
  const NEW_ABS = '/Users/test/blog/content/posts/new.md';

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMWithTree();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('editor:file-renamed updates currentFilePath without dispatching loadFile/navigate-preview', async () => {
    // Provide a ChipBar mock with setValues so file-changed callbacks
    // can apply frontmatter without throwing.
    MockChipBar.mockImplementation(() => ({
      setPageFacts: vi.fn(),
      render: vi.fn(),
      destroy: vi.fn(),
      getValues: vi.fn().mockReturnValue({}),
      setValues: vi.fn(),
    }));
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
      setContent: vi.fn(),
    }));

    // Prime currentFilePath with a loadFile.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Old' }, body: '# Old' },
    });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/posts/old/' });

    await bootEditorMain();

    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));

    // Load OLD so currentFilePath in the module is OLD (relative form
    // after loadFile's normalization).
    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: OLD_ABS, isDir: false });

    mockCommands.parseFrontmatter.mockClear();
    mockEmit.mockClear();

    // Dispatch editor:file-renamed (what file-tree fires on successful rename).
    // The event detail carries the registry
    // domain (project-root-relative).
    document.dispatchEvent(new CustomEvent('editor:file-renamed', {
      detail: { oldPath: OLD, newPath: NEW },
    }));

    // Allow any microtasks to drain.
    await new Promise(r => setTimeout(r, 10));

    // Must NOT reload the file (no parseFrontmatter) and MUST NOT emit the
    // '/' fallback — that's the home-jump path.
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
    // PreviewFollower may emit navigate-preview if the test's
    // resolveUrlForFile mock returns a URL. What we still must NOT do is
    // emit the '/' fallback that creates the home-jump loop.
    expect(mockEmit).not.toHaveBeenCalledWith('editor:navigate-preview', { urlPath: '/' });

    // The follow-up file-changed for NEW (which the watcher will fire) must
    // verify currentFilePath was updated. file-changed only acts when its
    // changedPaths include currentFilePath, so the cleanest probe is to
    // dispatch file-changed for OLD: now that currentFilePath is NEW, OLD
    // should be ignored.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'stale' },
    });
    const cb = listenCallbacks.get('moss-event')!;
    await cb({ kind: 'FileChanged', payload: { modified_paths: [OLD] } });
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();

    // And file-changed for NEW MUST act (currentFilePath now matches NEW).
    // The handler wraps currentFilePath via toAbsolute() before calling
    // parseFrontmatter.
    await cb({ kind: 'FileChanged', payload: { modified_paths: [NEW] } });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(NEW_ABS);
  });

  test('editor:file-renaming silences the watcher rename echo via source_renames probe', async () => {
    // pendingRenameSet stores project-root-relative SOURCE paths like
    // "content/posts/old.md" (set by editor:file-renaming via
    // registerPendingRename, which uses entry.path from the registry).
    //
    // The production watcher emits file-changed with BOTH source_renames
    // (source-domain, consumed by EntryRegistry) and moved_output_paths
    // (output-domain, used by the preview to follow renamed URLs). The
    // pending-rename guard must probe source_renames — NOT moved_output_paths —
    // to match the source-domain keys stored by registerPendingRename.
    //
    // NOTE: moved_output_paths also fail the affectsCurrentFile check (output
    // paths are not in the source-domain registry), so the echo would not
    // trigger a spurious reload even without the guard; probing source_renames
    // makes the guard fire at the right layer regardless of future changes to
    // affectsCurrentFile.

    // Prime currentFilePath with OLD.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));

    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: OLD_ABS, isDir: false });

    // Dispatch editor:file-renaming (what file-tree fires BEFORE renameEntry).
    document.dispatchEvent(new CustomEvent('editor:file-renaming', {
      detail: { oldPath: OLD, newPath: NEW },
    }));

    mockCommands.parseFrontmatter.mockClear();
    const cb = listenCallbacks.get('moss-event')!;

    // Simulate the production rename echo: the watcher emits source_renames
    // (source-domain) and moved_output_paths (output-domain) together.
    // The pending-rename guard must probe source_renames and fire (OLD is in
    // pendingRenameSet), suppressing the echo without reaching the
    // affectsCurrentFile check.
    const outputOld = 'content/posts/old/index.html';
    const outputNew = 'content/posts/new/index.html';
    await cb({
        kind: 'FileChanged',
        payload: {
          // Source-domain rename pairs — matches pendingRenameSet domain.
          // The guard probes this field. OLD is in pendingRenameSet.
          source_renames: [[OLD, NEW]],
          // Output-domain rename pairs — does NOT match pendingRenameSet keys.
          // Probing the other field instead would make the guard a no-op.
          moved_output_paths: [[outputOld, outputNew]],
          modified_paths: null,
          source_creates: null,
          source_deletes: null,
          changed_output_files: null,
          deleted_paths: null,
        },
      });

    // No parseFrontmatter call — the rename echo is correctly suppressed.
    // The source_renames probe fired (OLD in pendingRenameSet → early return).
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// pendingRenameSet cleared on project-folder switch
//
// Stale entries from project A must not suppress file-changed events in
// project B when a file in B happens to share the same relative path.
// build-config-changed is broadcast to all webviews (app.emit) on every
// folder switch; editor-main must clear pendingRenameSet on receipt.
// ---------------------------------------------------------------------------

describe('editor-main pendingRenameSet cleared on folder switch', () => {
  // Scenario: in project A, the user renames old.md → new.md. Both paths
  // land in pendingRenameSet. Before the 1500ms eviction fires, the user
  // switches to project B. Project B also has a file at the same relative
  // path (old.md). The stale pendingRenameSet entry from project A must NOT
  // suppress the file-changed event for old.md in project B.
  const OLD = 'content/posts/old.md';
  const NEW = 'content/posts/new.md';
  const OLD_ABS = '/Users/test/blog/content/posts/old.md';

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMWithTree();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('build-config-changed clears pendingRenameSet so file-changed is no longer suppressed', async () => {
    // Init with project A, load old.md as the current file.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));
    await vi.waitFor(() => expect(listenCallbacks.has('build-config-changed')).toBe(true));

    const loadSourceCb = listenCallbacks.get('editor:load-source')!;
    await loadSourceCb({ sourcePath: OLD_ABS, isDir: false });

    // Register both OLD and NEW in pendingRenameSet via editor:file-renaming
    // (simulates the rename flow in project A).
    document.dispatchEvent(new CustomEvent('editor:file-renaming', {
      detail: { oldPath: OLD, newPath: NEW },
    }));

    // Confirm the pendingRenameSet is active — file-changed for OLD is
    // suppressed even though it IS the current file.
    mockCommands.parseFrontmatter.mockClear();
    const fileChangedCb = listenCallbacks.get('moss-event')!;
    await fileChangedCb({ kind: 'FileChanged', payload: { modified_paths: [OLD] } });
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled(); // suppressed by pendingRenameSet

    // Simulate the user switching to project B (build-config-changed).
    const buildConfigChangedCb = listenCallbacks.get('build-config-changed')!;
    await buildConfigChangedCb({ folder_path: '/Users/test/other-project', target_file: null });

    // After the folder switch, pendingRenameSet must be cleared.
    // File-changed for OLD (now interpreted as a file in project B) must NOT
    // be suppressed — the stale rename entry from project A is gone.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'project B content' },
    });
    mockCommands.parseFrontmatter.mockClear();
    await fileChangedCb({ kind: 'FileChanged', payload: { modified_paths: [OLD] } });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalled(); // no longer suppressed
  });
});

// ---------------------------------------------------------------------------
// Another writer (an agent, another editor, a sync client) changes the open
// file. The editor compares every disk text with its base by content hash,
// merges the other writer's edits into unsaved text when they touch other
// lines, and every save names the base it was built on, so the backend writes
// nothing over a text the buffer has not seen. The end-to-end proof, against
// the real backend, requires a browser test with an external file writer.
// ---------------------------------------------------------------------------

describe('editor-main when another writer changes the open file', () => {
  const FILE = '/Users/test/blog/content/posts/hello.md';
  const FILE_REL = 'content/posts/hello.md';
  const FM = { title: 'Hello' };
  const BASE = 'one\ntwo\nthree\n';
  const OTHER = '/Users/test/blog/content/posts/other.md';
  let editors: { text: string }[];
  let setContent: ReturnType<typeof vi.fn>;
  /** The text the open editor shows. */
  const shown = () => editors.at(-1)!.text;

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
  });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  let ownEdits: ChangeSet[];
  async function boot(): Promise<void> {
    editors = [];
    ownEdits = [];
    setContent = vi.fn((t: string) => { editors.at(-1)!.text = t; });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: FM, body: BASE } });
    // One text per editor, restored from a parked document as CM6 does.
    MockCmEditor.mockImplementation((options: { content: string; restore?: { doc: string }; onChange: () => void }) => {
      const inst = { text: options.restore?.doc ?? options.content };
      editors.push(inst);
      return {
        destroy: vi.fn(),
        // An edit moss makes for the person: it changes the text as typing does.
        applyOwnEdit: (changes: ChangeSet) => {
          ownEdits.push(changes);
          inst.text = changes.apply(Text.of(inst.text.split('\n'))).toString();
          options.onChange();
        },
        getContent: () => inst.text,
        toJSON: () => ({ doc: inst.text }),
        getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
        setContent,
      };
    });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: FILE, isDir: false });
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalled());
  }

  /** The person types: the buffer changes and CM6 reports it, as a keystroke does. */
  const type = (next: string) => {
    editors.at(-1)!.text = next;
    (MockCmEditor.mock.calls.at(-1)![0] as { onChange: () => void }).onChange();
  };
  const disk = (body: string, frontmatter: Record<string, unknown> = FM) => ({
    status: 'ok' as const, data: { frontmatter, body, hash: textHash(frontmatter, body), has_frontmatter: true },
  });
  const outsideWrite = async (body: string, frontmatter: Record<string, unknown> = FM) => {
    mockCommands.parseFrontmatter.mockResolvedValue(disk(body, frontmatter));
    await listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: { modified_paths: [FILE_REL] } });
  };
  const flush = () => (window as any).__moss_debug.flushPendingSave() as Promise<boolean>;
  const saves = () => mockCommands.saveEditorContent.mock.calls as [string, Record<string, unknown>, string, string, boolean][];

  test("the watcher's echo of moss's own save changes nothing", async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    expect(await flush()).toBe(true);
    expect(saves().at(-1)![3]).toBe(textHash(FM, BASE));
    await outsideWrite('one typed\ntwo\nthree\n');
    expect(setContent).not.toHaveBeenCalled();
    expect(await flush()).toBe(true);
    expect(saves()).toHaveLength(1);
  });

  test('a buffer with no unsaved edits takes the outside write', async () => {
    await boot();
    await outsideWrite('external write\n');
    expect(shown()).toBe('external write\n');
    expect(saves()).toHaveLength(0);
  });

  test("unsaved typing and an outside write to another line both survive, saved on the disk's hash", async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    await outsideWrite('one\ntwo\nthree by an agent\n');
    expect(shown()).toBe('one typed\ntwo\nthree by an agent\n');
    expect(await flush()).toBe(true);
    expect(saves().at(-1)!.slice(2)).toEqual(['one typed\ntwo\nthree by an agent\n', textHash(FM, 'one\ntwo\nthree by an agent\n'), false]);
  });

  test("an outside write to the line being typed: the person's line is saved over it, keeping the other first", async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one by an agent\ntwo\nthree by an agent\n');
    await outsideWrite(agent.data.body);
    // The agent's other line joins; the line both changed stays the person's.
    expect(shown()).toBe('one typed\ntwo\nthree by an agent\n');
    mockCommands.saveEditorContent.mockImplementation(async (...a: [string, unknown, string]) => ({ ...(await savedWithHash(...a)), data: { ...(await savedWithHash(...a)).data, displaced: 'v1' } }));
    expect(await flush()).toBe(true);
    // Named on the disk's hash, so a third write is still refused, and asked to keep what it replaces.
    expect(saves().at(-1)!.slice(2)).toEqual(['one typed\ntwo\nthree by an agent\n', agent.data.hash, true]);
    await vi.waitFor(() => expect(notices()).toContain('was kept in Versions'));
  });

  test("Undo puts the other writer's line back as the person's own edit, saved, keeping what they typed since", async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one by an agent\ntwo\nthree\n');
    await outsideWrite(agent.data.body);
    mockCommands.saveEditorContent.mockImplementationOnce(async (...a: [string, unknown, string]) => ({ status: 'ok', data: { ...(await savedWithHash(...a)).data, displaced: 'v1' } }))
      .mockImplementation(savedWithHash);
    expect(await flush()).toBe(true);
    type('one typed\ntwo\nthree\nfour, typed since\n');
    expect(await flush()).toBe(true);
    const undo = [...document.querySelectorAll('#moss-toast-container button')].find((b) => b.textContent?.includes('Undo')) as HTMLButtonElement;
    expect(undo).toBeTruthy();
    undo.click();
    expect(ownEdits).toHaveLength(1);
    expect(shown()).toBe('one by an agent\ntwo\nthree\nfour, typed since\n');
    expect(await flush()).toBe(true);
    // An ordinary save: on the base the last save left, keeping nothing.
    expect(saves().at(-1)!.slice(2)).toEqual(['one by an agent\ntwo\nthree\nfour, typed since\n', textHash(FM, 'one typed\ntwo\nthree\nfour, typed since\n'), false]);
    await vi.waitFor(() => expect(notices()).not.toContain('was kept in Versions'));
  });

  test('a save that comes back conflict merges the disk text and saves again on its hash', async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one\ntwo\nthree by an agent\n');
    mockCommands.saveEditorContent
      .mockResolvedValueOnce({ status: 'ok', data: { status: 'conflict', disk: agent.data } })
      .mockImplementation(savedWithHash);
    expect(await flush()).toBe(true);
    expect(saves().map((c) => c.slice(2))).toEqual([
      ['one typed\ntwo\nthree\n', textHash(FM, BASE), false],
      ['one typed\ntwo\nthree by an agent\n', agent.data.hash, false],
    ]);
  });

  test("a chip edit and an outside change to another key both survive", async () => {
    await boot();
    const store = (window as any).__moss_debug.getFrontmatterStore();
    store.setField('title', 'Local edit');
    await outsideWrite(BASE, { title: 'Hello', uid: '1a2b3c4d' });
    expect(store.getValues()).toEqual({ title: 'Local edit', uid: '1a2b3c4d' });
    expect(await flush()).toBe(true);
    expect(saves().at(-1)!.slice(1)).toEqual([{ title: 'Local edit', uid: '1a2b3c4d' }, BASE, textHash({ title: 'Hello', uid: '1a2b3c4d' }, BASE), false]);
  });

  const openFile = (filePath: string) => listenCallbacks.get('editor:load-file')!({ filePath });
  const notices = () => document.getElementById('moss-toast-container')?.textContent ?? '';

  test('text a save could not keep over an outside write survives a switch to another file and back, still keeping it', async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one by an agent\ntwo\nthree\n');
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'ok', data: { status: 'not-kept' } });
    await outsideWrite(agent.data.body);
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => (p === OTHER ? disk('other\n') : agent));
    await openFile(OTHER);
    await vi.waitFor(() => expect(shown()).toBe('other\n'));
    await openFile(FILE);
    await vi.waitFor(() => expect(shown()).toBe('one typed\ntwo\nthree\n'));
    expect(await flush()).toBe(false);
    // Every save of this page, before and after the switch, names the agent's bytes and asks to keep them.
    const ofFile = saves().filter((c) => c[0] === FILE);
    expect(ofFile.length).toBeGreaterThan(0);
    expect(ofFile.every((c) => c[3] === agent.data.hash && c[4] === true)).toBe(true);
  });

  test('closing or quitting does not report saved while a switched-away page still holds unsaved text', async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one by an agent\ntwo\nthree\n');
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => (p === OTHER ? disk('other\n') : agent));
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'ok', data: { status: 'conflict', disk: agent.data } });
    await openFile(OTHER);
    await vi.waitFor(() => expect(shown()).toBe('other\n'));
    mockEmit.mockClear();
    await saveRequest!('quit');
    expect(mockEmit).toHaveBeenCalledWith('editor:flush-save-done', { token: 'quit', ok: false });
  });

  test('a save that cannot keep the other writer\'s text writes nothing and shows one notice, gone once a save lands', async () => {
    await boot();
    type('one typed\ntwo\nthree\n');
    const agent = disk('one by an agent\ntwo\nthree\n');
    await outsideWrite(agent.data.body);
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'ok', data: { status: 'not-kept' } });
    expect(await flush()).toBe(false);
    await vi.waitFor(() => expect(notices()).toContain('changed outside moss'));
    expect(shown()).toBe('one typed\ntwo\nthree\n');
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    type('one typed again\ntwo\nthree\n');
    expect(await flush()).toBe(true);
    await vi.waitFor(() => expect(notices()).not.toContain('changed outside moss'));
  });
});

// ---------------------------------------------------------------------------
// Window debug hook: window.__moss_debug.registry exposure
// ---------------------------------------------------------------------------

describe('editor external reload lifetime', () => {
  const root = '/Users/test/blog';
  const first = 'content/posts/first.md';
  const second = 'content/posts/second.md';
  const parsed = (body: string) => ({ status: 'ok', data: { frontmatter: { title: body }, body } });
  const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
  const changed = () => listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: { modified_paths: [first] } });
  const open = (path: string) => listenCallbacks.get('editor:load-file')!({ filePath: `${root}/${path}` });
  const currentEditor = () => MockCmEditor.mock.results.at(-1)!.value;
  const debugEditor = () => (window as any).__moss_debug;

  beforeEach(async () => {
    vi.resetModules(); setupEditorDOM(); resetMocks();
    const heading = document.createElement('div'); heading.id = 'editor-heading'; document.body.appendChild(heading);
    mockCommands.computeHeadingState.mockResolvedValue({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: root });
    mockCommands.parseFrontmatter.mockResolvedValue(parsed('initial'));
    mockCommands.getFileInfo.mockResolvedValue({ status: 'ok', data: { modified: '2026-05-22T10:00:00Z' } });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-file')).toBe(true));
    await open(first);
  });
  afterEach(() => { vi.restoreAllMocks(); });

  test('the outgoing editor remains paired with its store while the incoming role probe waits', async () => {
    const outgoing = currentEditor();
    const outgoingStore = debugEditor().getFrontmatterStore();
    let finishRole!: (v: any) => void;
    mockCommands.describeSourceRole.mockImplementationOnce(() => new Promise((resolve) => { finishRole = resolve; }));
    const opening = open(second);
    await settle();
    expect(finishRole).toBeDefined();
    mockCommands.parseFrontmatter.mockResolvedValue(parsed('incoming external'));
    await listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: { modified_paths: [second] } });
    expect(outgoing.setContent).not.toHaveBeenCalled();
    expect(debugEditor().getFrontmatterStore()).toBe(outgoingStore);
    expect(outgoingStore.getValues().title).toBe('initial');
    finishRole({ status: 'ok', data: { slot: null } });
    await opening;
  });

  test.each(['switch', 'reopen', 'newer-read'])('watcher reload drops stale body, chips and heading after %s', async (replacement) => {
    const outgoing = currentEditor();
    const oldStore = debugEditor().getFrontmatterStore();
    const oldValues = oldStore.getValues();
    let finish!: (v: any) => void;
    mockCommands.parseFrontmatter.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const pending = changed();
    if (replacement === 'newer-read') {
      mockCommands.parseFrontmatter.mockResolvedValue(parsed('newer'));
      await changed();
    } else {
      await open(second);
      if (replacement === 'reopen') await open(first);
    }
    const editor = currentEditor();
    const calls = editor.setContent.mock.calls.length;
    mockCommands.computeHeadingState.mockClear();
    finish(parsed('stale'));
    await pending;
    expect(editor.setContent.mock.calls).toHaveLength(calls);
    expect(debugEditor().getFrontmatterStore().getValues().title).not.toBe('stale');
    expect(mockCommands.computeHeadingState).not.toHaveBeenCalled();
    if (replacement !== 'newer-read') {
      expect(outgoing.setContent).not.toHaveBeenCalled();
      expect(oldStore.getValues()).toEqual(oldValues);
    }
  });

  test.each(['body', 'frontmatter'])('an intervening %s edit invalidates a parse even after its save ack', async (kind) => {
    let finish!: (v: any) => void;
    mockCommands.parseFrontmatter.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const pending = changed();
    if (kind === 'body') MockCmEditor.mock.calls.at(-1)![0].onChange();
    else debugEditor().getFrontmatterStore().setField('title', 'my edit');
    await debugEditor().flushPendingSave();
    finish(parsed('stale'));
    await pending;
    expect(currentEditor().setContent).not.toHaveBeenCalled();
    expect(debugEditor().getFrontmatterStore().getValues().title).not.toBe('stale');
  });

  test("a chip edit made during the read keeps the person's value of a key the disk also changed, and takes the disk's body", async () => {
    let finish!: (v: any) => void;
    mockCommands.parseFrontmatter.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const pending = changed();
    await settle();
    debugEditor().getFrontmatterStore().setField('title', 'my edit');
    finish(parsed('external'));
    await pending;
    expect(currentEditor().setContent).toHaveBeenCalledWith('external');
    expect(debugEditor().getFrontmatterStore().getValues().title).toBe('my edit');
  });

  test('an external heading compute cannot overwrite a later frontmatter edit', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue(parsed('external'));
    // The store subscriber's compute starts first; the explicit external reload
    // projection follows it. Hold both so the user can make a newer title edit.
    const oldComputes: Array<(v: any) => void> = [];
    mockCommands.computeHeadingState.mockImplementation(() => new Promise((resolve) => { oldComputes.push(resolve); }));
    await changed();
    mockCommands.computeHeadingState.mockResolvedValue({ status: 'ok', data: { visible: true, text: 'my title', source: 'Title' } });
    debugEditor().getFrontmatterStore().setField('title', 'my title');
    await settle();
    for (const resolve of oldComputes) resolve({ status: 'ok', data: { visible: false, text: 'stale', source: 'Filename' } });
    await settle();
    expect(document.getElementById('editor-heading')?.dataset.empty).toBe('false');
    expect(document.getElementById('editor-heading')?.dataset.headingSource).toBe('title');
  });

  test('post-rename reload uses the stable entry at its new path and one reconcile owner', async () => {
    const id = debugEditor().registry.activeEntryId;
    document.dispatchEvent(new CustomEvent('editor:file-renamed', { detail: { oldPath: first, newPath: second } }));
    expect(debugEditor().registry.activeEntryId).toBe(id);
    mockCommands.parseFrontmatter.mockClear();
    mockCommands.parseFrontmatter.mockResolvedValue(parsed('renamed source'));
    const hooks = MockFileTree.mock.calls[0][2] as { reloadActiveFileFromDisk(): Promise<void> };
    await hooks.reloadActiveFileFromDisk();
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledOnce();
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/${second}`);
    expect(currentEditor().setContent).toHaveBeenCalledWith('renamed source');
    expect(debugEditor().getFrontmatterStore().getValues().title).toBe('renamed source');
  });

  test('a reload waiting on a save cannot put the outgoing document into a newly opened editor', async () => {
    MockCmEditor.mock.calls.at(-1)![0].onChange();
    let finishSave!: (v: any) => void;
    mockCommands.saveEditorContent.mockImplementationOnce(() => new Promise((resolve) => { finishSave = resolve; }));
    const flushing = debugEditor().flushPendingSave();
    await settle();
    mockCommands.parseFrontmatter.mockImplementation(async (p: string) => parsed(p.endsWith('second.md') ? 'second' : 'stale'));
    const pending = changed();
    const opening = open(second);
    await settle();
    finishSave(await savedWithHash('', { title: 'initial' }, 'initial'));
    await Promise.all([flushing, pending, opening]);
    await settle();
    const editor = currentEditor();
    expect(editor.getContent()).toBe('second');
    expect(editor.setContent).not.toHaveBeenCalled();
    expect(debugEditor().getFrontmatterStore().getValues().title).not.toBe('stale');
  });
});

describe('editor-main window.__moss_debug.registry exposure', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOMWithTree();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    // Discard any pending debounce timers (e.g. flashDownloadTargets' 400ms
    // setTimeout) so they can't fire into the next test's DOM.
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test('window.__moss_debug.registry is exposed after init for inspection', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '' },
    });

    await bootEditorMain();

    // Wait for init to complete and project to load
    await vi.waitFor(() => {
      expect(MockFileTree.mock.calls.length).toBeGreaterThan(0);
    });

    // The registry is passed as the first arg to FileTree constructor
    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];
    expect(registryArg).toBeDefined();

    // Verify window.__moss_debug.registry is the same instance
    const debugRegistry = (window as unknown as { __moss_debug?: { registry: unknown } }).__moss_debug?.registry;
    expect(debugRegistry).toBeDefined();
    expect(debugRegistry).toBe(registryArg);
  });

  test('load → external rename → consistency flow end-to-end via window debug hook', async () => {
    // source_creates / source_renames carry project-root-relative paths on the
    // wire. Use relative forms here
    // to avoid guardRelative warnings from registry.upsert().
    const FILE = 'content/posts/hello.md';
    const NEW_FILE = 'content/posts/hello-renamed.md';

    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    });

    await bootEditorMain();

    await vi.waitFor(() => {
      expect(listenCallbacks.has('moss-event')).toBe(true);
      expect(MockFileTree.mock.calls.length).toBeGreaterThan(0);
    });

    const registryArg: import('../entry-registry').EntryRegistry = MockFileTree.mock.calls[0][0];
    const fileChangedCb = listenCallbacks.get('moss-event')!;
    expect(fileChangedCb).toBeDefined();

    // Create the file via source_creates
    await fileChangedCb({
        kind: 'FileChanged',
        payload: {
          source_creates: [FILE],
          source_deletes: null,
          source_renames: null,
          modified_paths: null,
          changed_output_files: null,
          moved_output_paths: null,
          deleted_paths: null,
        },
      });

    const idBefore = registryArg.getByPath(FILE);
    expect(idBefore).not.toBeNull();

    // Rename the file via source_renames
    await fileChangedCb({
        kind: 'FileChanged',
        payload: {
          source_creates: null,
          source_deletes: null,
          source_renames: [[FILE, NEW_FILE]],
          modified_paths: null,
          changed_output_files: null,
          moved_output_paths: null,
          deleted_paths: null,
        },
      });

    // Verify consistency through the window debug hook
    const debugRegistry = (window as unknown as { __moss_debug?: { registry: unknown } }).__moss_debug?.registry;
    expect(debugRegistry).toBe(registryArg);

    // Assert the registry is consistent: old path gone, new path has same ID
    expect(registryArg.getByPath(FILE)).toBeNull();
    expect(registryArg.getByPath(NEW_FILE)).toBe(idBefore);

    // The debug hook provides direct access for dev inspection
    const debugInstance = debugRegistry as import('../entry-registry').EntryRegistry;
    expect(debugInstance.getByPath(NEW_FILE)).toBe(idBefore);
  });
});

// ---------------------------------------------------------------------------
// Single writer: saveActiveDocument reads live path after in-place rename
// ---------------------------------------------------------------------------

describe('editor-main single writer saveActiveDocument', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('single writer targets the live path after an in-place rename', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Hello' }, body: '# Hello' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/old.md', isDir: false });

    document.dispatchEvent(new CustomEvent('editor:file-renamed', { detail: { oldPath: 'old.md', newPath: 'new.md' } }));

    (window as any).__moss_debug.getFrontmatterStore().removeField('title');
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenCalled());

    const calls = mockCommands.saveEditorContent.mock.calls;
    const lastPathArg = calls[calls.length - 1][0];
    expect(lastPathArg).toBe('/proj/new.md');
  });

  test('rename then remove a frontmatter field writes only the new path', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Hello' }, body: '# Hello' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/old.md', isDir: false });

    document.dispatchEvent(new CustomEvent('editor:file-renamed', { detail: { oldPath: 'old.md', newPath: 'new.md' } }));
    (window as any).__moss_debug.getFrontmatterStore().removeField('title');
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenCalled());

    // Assert the invariant (the write targets the live path; nothing
    // targets the captured pre-rename path) — NOT totality over the shared log,
    // which a surviving chain from a dead module graph can append to at any
    // time (the CI-only flake this replaced).
    const paths = mockCommands.saveEditorContent.mock.calls.map((c: any[]) => c[0]);
    expect(paths).toContain('/proj/new.md');
    expect(paths).not.toContain('/proj/old.md');
  });

  test('saveActiveDocument coalesces concurrent saves into initial + one trailing', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'x' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });

    let resolveFirst!: () => void;
    let calls = 0;
    mockCommands.saveEditorContent.mockImplementation((path: string, fm: unknown, body: string) => {
      calls++;
      if (calls === 1) {
        return new Promise((res) => { resolveFirst = () => res(savedWithHash(path, fm, body)); });
      }
      return savedWithHash(path, fm, body);
    });

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/p.md', isDir: false });

    // Reset after load so any incidental saves during load don't pollute the count.
    calls = 0;
    mockCommands.saveEditorContent.mockClear();

    const save = (window as any).__moss_debug.saveActiveDocument as () => Promise<void>;
    const p1 = save();   // starts, awaits the deferred (in flight)
    const p2 = save();   // hits the in-flight guard, sets saveQueued, returns same inner promise
    await Promise.resolve();
    expect(calls).toBe(1);   // still only the first IPC while in flight

    resolveFirst();           // first completes → loop sees saveQueued → trailing run
    await Promise.all([p1, p2]);
    expect(calls).toBe(2);   // initial + exactly one trailing coalesced save
  });

  // The title projection must remain consistent across surfaces: the
  // dual-mode Title-source heading commit writes the title: field via the store
  // (heading-routing setTitleField → chipBar.setFieldValue → store.setField).
  // Because the store is path-less, that must flow through the single writer to
  // the LIVE path. This locks the cross-branch invariant — it fails if a future
  // change reintroduces a captured-path title write (the bug both branches fix).
  test('a title write after rename targets the new path via the single writer', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Old' }, body: '# Old' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/old.md', isDir: false });

    document.dispatchEvent(new CustomEvent('editor:file-renamed', { detail: { oldPath: 'old.md', newPath: 'new.md' } }));
    (window as any).__moss_debug.getFrontmatterStore().setField('title', 'New Title', { immediate: true });
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenCalled());

    const calls = mockCommands.saveEditorContent.mock.calls;
    const last = calls[calls.length - 1];
    expect(last[0]).toBe('/proj/new.md');                 // never /proj/old.md
    expect(last[1]).toMatchObject({ title: 'New Title' }); // and carries the new title
  });

  // Each save
  // logs a path hash (never the raw path), a byte count, and whether it was
  // an autosave — see save-log assertions below.
  test('an autosave logs a path hash (not the raw path) and the byte count', async () => {
    // MockCmEditor.getContent() defaults to '' regardless of the parsed body
    // (see the module-level mock factory above) — bytes=0 here is a fixture
    // artifact, not a claim about real save sizes; the byte-count FIELD is
    // what's asserted (via the regex below), not a specific value.
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'hello' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/secret-name.md', isDir: false });

    vi.mocked(mockedLogInfo).mockClear();
    const save = (window as any).__moss_debug.saveActiveDocument as (reason?: 'autosave' | 'forced') => Promise<void>;
    await save();
    // hashPathForLog resolves on a microtask queued from inside saveActiveDocument.
    await vi.waitFor(() => {
      expect(
        vi.mocked(mockedLogInfo).mock.calls.some((c) => String(c[0]).startsWith('[editor] save ')),
      ).toBe(true);
    });

    const saveLog = vi.mocked(mockedLogInfo).mock.calls.map((c) => String(c[0])).find((m) => m.startsWith('[editor] save '))!;
    expect(saveLog).not.toContain('secret-name');
    expect(saveLog).toMatch(/^\[editor\] save path_hash=[0-9a-f]{8} bytes=\d+ autosave=true$/);
  });

  test('a forced flush logs autosave=false', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Old' }, body: '# Old' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/p.md', isDir: false });

    vi.mocked(mockedLogInfo).mockClear();
    const save = (window as any).__moss_debug.saveActiveDocument as (reason?: 'autosave' | 'forced') => Promise<void>;
    await save('forced');
    await vi.waitFor(() => {
      expect(
        vi.mocked(mockedLogInfo).mock.calls.some((c) => String(c[0]).startsWith('[editor] save ')),
      ).toBe(true);
    });

    const saveLog = vi.mocked(mockedLogInfo).mock.calls.map((c) => String(c[0])).find((m) => m.startsWith('[editor] save '))!;
    expect(saveLog).toContain('autosave=false');
  });
});

// ---------------------------------------------------------------------------
// Editor import-refresh on FileChanged (an import landing files
// while the editor shows the empty-folder onboarding surface). The handler is
// registered via onMossEvent('FileChanged', ...), which subscribes to the
// MOSS_EVENT_CHANNEL ('moss-event') under the hood — so the captured listener
// callback lives in listenCallbacks.get('moss-event'), and a FileChanged event
// is dispatched as { payload: { kind: 'FileChanged', payload: <FileChanged> } }.
// ---------------------------------------------------------------------------
describe('editor-main import-refresh on FileChanged', () => {
  beforeEach(async () => {
    // Fire beforeunload to null out the previous test's init() module-level
    // state (fileTree, previewFollower, fileEvents). The URL-params section of
    // any still-pending init() checks `else if (fileTree && projectRoot)` —
    // with fileTree=null the branch is skipped, so applyEditorSurface() is
    // never called from that stale init and cannot stomp the current test's DOM.
    window.dispatchEvent(new Event('beforeunload'));
    // Give the old init's remaining microtasks a few ticks to settle (they
    // now all hit early-returns because fileTree === null).
    for (let i = 0; i < 5; i++) await Promise.resolve();
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Fire a FileChanged MOSS event through the captured 'moss-event' listener. */
  async function dispatchFileChanged(sourceCreates: string[]): Promise<void> {
    const cb = listenCallbacks.get('moss-event');
    expect(cb).toBeDefined();
    // The download flash is debounced (~400ms). Run the handler under fake timers
    // so the debounce setTimeout is captured, then advance SYNCHRONOUSLY to flush
    // it — `advanceTimersByTime` does not pump the microtask queue, so the boot's
    // pending surface promise can't settle and overwrite these assertions (an
    // extra real await would). Mirrors the original synchronous flash timing.
    vi.useFakeTimers();
    try {
      await cb!({ kind: 'FileChanged', payload: { source_creates: sourceCreates } });
      if (sourceCreates.length > 0) vi.advanceTimersByTime(400);
    } finally {
      vi.useRealTimers();
    }
  }

  test('files arriving on an empty folder (no file open, no home) refresh the tree, flash, and re-decide off root-empty to the root-home takeover', async () => {
    // getFirstFile() is called in two places:
    //   1. init() URL-params tail (line 1116): `isFolderEmpty: fileTree.getFirstFile() == null`
    //      — this is the FIRST call; must be null so that init() sets 'generated' (no URL
    //      params → sourcePath=null → generated node via decideEditorSurface).
    //   2. onboardingRefresh in the FileChanged handler: `wasEmpty = fileTree?.getFirstFile() == null`
    //      — must also be null so wasEmpty=true and resolveAndApplyRootSurface() is called.
    // After both null reads, all subsequent reads (resolveAndApplyRootSurface check) return
    // 'article-one.md' to simulate a freshly populated folder.
    mockGetFirstFile.mockReturnValueOnce(null).mockReturnValueOnce(null).mockReturnValue('article-one.md');
    // Source-truth resolve: no home file on disk → source_path null, so the
    // surface stays the create-home CTA (not an opened content page).
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, is_dir: true, is_article: false, syndicated: [], takeover: { kind: 'root-home', display: 'blog', files: [{ dir: '/Users/test/blog', name: 'blog.md', frontmatter: { home: true } }] } } });

    await bootEditorMain();
    // The FileChanged handler subscribes to 'moss-event' via onMossEvent during init.
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    // Wait for init() to fully complete. init()'s tail (URL-params branch) runs
    // asynchronously after listener registration; it calls applyEditorSurface() which
    // sets data-editor-surface to 'generated' (no URL params → sourcePath=null → generated
    // node). We MUST let init() finish before firing dispatchFileChanged so the
    // resolveAndApplyRootSurface() call inside onboardingRefresh sets the takeover LAST
    // and is not later overwritten by init()'s URL-params tail.
    const cm = document.getElementById('moss-editor-cm')!;
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).not.toBeNull());

    await dispatchFileChanged(['article-one.md', 'article-two.md']);

    // Tree INDEX refreshed (so it populates when the user opens a file). The
    // contextual tree is intentionally NOT force-expanded here — see the
    // handler note in editor-main.ts (enterDirMode had no visible effect).
    expect(mockRefreshAndRender).toHaveBeenCalledTimes(1);

    // Surface re-decided off root-empty: with a populated folder + isRoot but no
    // home file, the editor leaves the four cards for the root-home takeover.
    expect(cm.getAttribute('data-editor-surface')).toBe('takeover');

    // Green arrival flash: no visible tree row/breadcrumb target (mock returns
    // null) → falls back to the whole nav-surface.
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });

  test('files arriving with a home file present OPEN the home (loadFile), not the create-CTA', async () => {
    // getFirstFile() reads, in deterministic order (init completes before the
    // dispatch — see the surface waitFor below):
    //   1. init()'s URL-params tail (`isFolderEmpty` read) → null
    //   2. onboardingRefresh's `wasEmpty` read → null (still empty pre-refresh)
    //   3+. post-refresh reads → 'index.md' (folder now populated)
    mockGetFirstFile.mockReturnValueOnce(null).mockReturnValueOnce(null).mockReturnValue('index.md');
    // Home file exists on disk → resolvePageSource('/') returns its absolute path.
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: '/Users/test/blog/index.md', is_dir: true, is_article: false, syndicated: [], takeover: null } });
    // loadFile parses the home file's frontmatter/body.
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'home body' } });

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    // Let init() fully complete (its tail sets the boot surface) before firing
    // the FileChanged, so the getFirstFile read order above is deterministic —
    // same pattern as the empty-folder sibling test above.
    const cmBoot = document.getElementById('moss-editor-cm')!;
    await vi.waitFor(() => expect(cmBoot.getAttribute('data-editor-surface')).not.toBeNull());

    await dispatchFileChanged(['index.md']);

    // Source-truth resolve opened the home page through the FULL CM6 mount path,
    // NOT the create-home CTA. (CmEditor construction is the marker for loadFile
    // landing on a real content page.)
    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalled());
    // parseFrontmatter was called for the home file (relativized).
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith('/Users/test/blog/index.md');
    // Flash still fired (nav-surface fallback).
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });

  test('an output-only change refreshes directory truth and preserves existing empty cards', async () => {
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'ok', data: { source_path: null, takeover: { ...folderHomeTakeover('/Users/test/blog', 'blog', true), folder_empty: true } } });
    mockGetFirstFile.mockReturnValue(null);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    // Wait for init() to fully complete. init()'s tail (URL-params branch) runs
    // asynchronously after listener registration; it sets data-editor-surface to
    // 'generated' (no URL params → sourcePath=null, isRoot=false → generated node).
    // We must let init() finish BEFORE firing editor:load-source so the test's
    // surface assertion isn't later overwritten by init()'s tail.
    const cm = document.getElementById('moss-editor-cm')!;
    // vi.waitFor retries while the callback throws — use expect() so it keeps
    // polling until init()'s URL-params tail sets data-editor-surface.
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).not.toBeNull());

    // Establish the real empty-folder onboarding surface (four cards) via the
    // boot load-source path: no source + isRoot + empty folder → root-empty.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isDir: true, isRoot: true });
    expect(cm.getAttribute('data-editor-surface')).toBe('root-empty');

    await dispatchFileChanged([]);

    expect(mockRefreshAndRender).toHaveBeenCalled();
    expect(cm.getAttribute('data-editor-surface')).toBe('root-empty'); // still the four cards
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(false);
  });

  test('a file already open suppresses the refresh entirely (currentFilePath guard)', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'body' } });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    // Open a file — currentFilePath() is now non-null.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/open.md', isDir: false });
    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalled());

    await dispatchFileChanged(['article-one.md']);

    // Guard short-circuits before touching the tree or the surface.
    expect(mockRefreshAndRender).not.toHaveBeenCalled();
  });

  test('files arriving on an already populated folder refresh its canonical surface and flash', async () => {
    // Folder was already non-empty before the event — getFirstFile is non-null
    // on both reads, so the empty→populated re-decide branch never runs.
    mockGetFirstFile.mockReturnValue('existing.md');
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchFileChanged(['article-three.md']);

    // The tree still refreshes (new rows must render) ...
    expect(mockRefreshAndRender).toHaveBeenCalledTimes(1);
    // Folder reconciliation always uses disk truth; generated page counts do not decide emptiness.
    expect(mockCommands.resolvePageSource).toHaveBeenCalled();
    // FIX 4: the download flash DOES fire on every arrival (user-approved rule),
    // falling back to the nav-surface when no visible row/breadcrumb targets it.
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });

  test('FIX 4: a new file under a visible folder row flashes that ROW (coalesced once), not the nav-surface', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    // Two arrivals share the same parent folder "works" whose row is visible →
    // deepestVisibleContainerFor resolves both to the SAME element, so it flashes
    // exactly once (coalesced).
    const worksRow = document.createElement('div');
    worksRow.dataset.path = 'works';
    document.getElementById('nav-surface')!.appendChild(worksRow);
    mockDeepestVisibleContainerFor.mockImplementation((dir: string) =>
      dir === 'works' ? worksRow : null,
    );

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchFileChanged(['works/a.md', 'works/b.md']);

    // Resolver asked for the parent of each arrival.
    expect(mockDeepestVisibleContainerFor).toHaveBeenCalledWith('works');
    // The shared target row got the flash class ...
    expect(worksRow.classList.contains('moss-just-committed')).toBe(true);
    // ... and the nav-surface did NOT (a concrete container absorbed the flash).
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(false);
  });

  test('FIX 4: arrivals with NO visible container fall back to a single nav-surface flash', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    mockDeepestVisibleContainerFor.mockReturnValue(null); // nothing expanded/visible

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchFileChanged(['deep/nested/x.md', 'other/y.md']);

    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// The preview owns rebuild progress, so the editor mounts no breadcrumb hairline.
// The ambient breadcrumb hairline had no production producer (only the
// dev_simulate_panel_task dev command); the one progress surface lives in the
// preview, not a second one in the editor. init() must NOT create the hairline
// DOM under #nav-surface.
// ---------------------------------------------------------------------------

describe('editor-main does not mount the breadcrumb hairline', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('no breadcrumb-hairline track / fill / hover DOM exists after init', async () => {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });

    // The ambient hairline used to be mounted under #nav-surface during init().
    // The removed breadcrumb hairline must create no DOM nodes.
    expect(document.getElementById('moss-breadcrumb-hairline')).toBeNull();
    expect(document.getElementById('moss-breadcrumb-hairline-track')).toBeNull();
    expect(document.getElementById('moss-breadcrumb-hairline-hover')).toBeNull();
    expect(
      document.querySelector('.moss-breadcrumb-hairline, .moss-breadcrumb-hairline-track'),
    ).toBeNull();
  });
});

// ── File-tree lock via editor:seal ─────────────────────────────
//
// The SSOT seal wire is `editor:seal`. editor-main.ts listens and calls
// fileTree.setSealed(boolean) — no direct coupling between file-tree.ts and
// deploy-handler. Tests assert the wiring only (DOM lock is tested in
// file-tree.test.ts).
describe('editor-main file-tree lock via editor:seal', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({
      status: 'ok',
      data: '/Users/test/blog',
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('editor:seal {sealed:true} calls fileTree.setSealed(true)', async () => {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:seal')).toBe(true);
    });

    await listenCallbacks.get('editor:seal')!({ sealed: true });

    expect(mockSetSealed).toHaveBeenCalledWith(true);
  });

  test('editor:seal {sealed:false} calls fileTree.setSealed(false)', async () => {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:seal')).toBe(true);
    });

    await listenCallbacks.get('editor:seal')!({ sealed: false });

    expect(mockSetSealed).toHaveBeenCalledWith(false);
  });

});

// ---------------------------------------------------------------------------
// RawFileCreated → green flash (source-agnostic)
//
// RawFileCreated events arrive on the MOSS_EVENT_CHANNEL ('moss-event') as
// { kind: 'RawFileCreated', payload: { paths: string[] } }.
// editor-main.ts registers via fileEvents.onRawFileCreated and routes to
// flashDownloadTargets — same accumulator/dedup logic as source_creates.
// ---------------------------------------------------------------------------
describe('editor-main RawFileCreated → green flash', () => {
  beforeEach(async () => {
    window.dispatchEvent(new Event('beforeunload'));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Fire a RawFileCreated MOSS event through the captured 'moss-event' listener. */
  async function dispatchRawFileCreated(paths: string[]): Promise<void> {
    const cb = listenCallbacks.get('moss-event');
    expect(cb).toBeDefined();
    vi.useFakeTimers();
    try {
      await cb!({ kind: 'RawFileCreated', payload: { paths } });
      if (paths.length > 0) vi.advanceTimersByTime(400);
    } finally {
      vi.useRealTimers();
    }
  }

  test('onRawFileCreated: flashDownloadTargets is called with payload paths when fileTree is set', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchRawFileCreated(['posts/hello.md']);

    // null ancestor (mock returns null) → nav-surface flash
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });

  test('onRawFileCreated with fileTree == null does NOT call flashDownloadTargets', async () => {
    // The `if (fileTree)` guard in the onRawFileCreated handler means that when
    // fileTree is null (e.g. DOM is missing #nav-grid and init never set it),
    // a RawFileCreated event must NOT attempt to flash any nav row.
    // Force fileTree to remain null by stripping the DOM nodes that init()
    // requires to construct the tree — without #nav-grid the `if (navGrid && ...)`
    // gate never runs and fileTree stays null.
    document.body.innerHTML = `
      <div id="moss-save-indicator"></div>
      <div id="moss-editor-cm"></div>
    `;
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    // Fire a RawFileCreated event — fileTree is null so the handler must no-op.
    const cb = listenCallbacks.get('moss-event')!;
    vi.useFakeTimers();
    try {
      await cb({ kind: 'RawFileCreated', payload: { paths: ['posts/new.md'] } });
      vi.advanceTimersByTime(400);
    } finally {
      vi.useRealTimers();
    }

    // Confirm MockFileTree was NOT constructed (no #nav-grid → no tree init).
    // A crash inside the handler would also surface here, proving the guard is present.
    expect(MockFileTree).not.toHaveBeenCalled();
  });

  test('400ms accumulator: two batches <400ms apart coalesce into one flush', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    const navSurface = document.getElementById('nav-surface')!;
    // Track how many times the class is added to measure flush count.
    let flashCount = 0;
    const origAdd = navSurface.classList.add.bind(navSurface.classList);
    const addSpy = vi.spyOn(navSurface.classList, 'add').mockImplementation((...args: string[]) => {
      if (args.includes('moss-just-committed')) flashCount++;
      return origAdd(...args);
    });

    const cb = listenCallbacks.get('moss-event')!;
    vi.useFakeTimers();
    try {
      // Batch 1
      await cb({ kind: 'RawFileCreated', payload: { paths: ['a.md'] } });
      // Must not have flushed yet (debounce hasn't fired)
      expect(flashCount).toBe(0);
      // Advance < 400ms so the first timer hasn't fired yet
      vi.advanceTimersByTime(100);
      // Batch 2 arrives (resets/extends the debounce)
      await cb({ kind: 'RawFileCreated', payload: { paths: ['b.md'] } });
      // Still no flush — debounce reset
      expect(flashCount).toBe(0);
      // Flush: advance past 400ms from the second batch
      vi.advanceTimersByTime(400);
    } finally {
      addSpy.mockRestore();
      vi.useRealTimers();
    }
    // Both arrivals coalesced into EXACTLY ONE flush — flash fired exactly once
    expect(flashCount).toBe(1);
    expect(navSurface.classList.contains('moss-just-committed')).toBe(true);
  });

  test('flushDownloadFlash: N paths in one folder deduplicate to one flash of that row', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    const worksRow = document.createElement('div');
    worksRow.dataset.path = 'works';
    document.getElementById('nav-surface')!.appendChild(worksRow);
    mockDeepestVisibleContainerFor.mockImplementation((dir: string) =>
      dir === 'works' ? worksRow : null,
    );

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchRawFileCreated(['works/a.md', 'works/b.md', 'works/c.md']);

    // All 3 paths resolve to the same 'works' row → flashed exactly once
    expect(worksRow.classList.contains('moss-just-committed')).toBe(true);
    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(false);
  });

  test('null ancestor → one flashNavSurface', async () => {
    mockGetFirstFile.mockReturnValue('existing.md');
    mockDeepestVisibleContainerFor.mockReturnValue(null);

    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));

    await dispatchRawFileCreated(['deep/nested/x.md', 'other/y.md']);

    expect(document.getElementById('nav-surface')!.classList.contains('moss-just-committed')).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// vi.resetModules() leaves the previous editor-main module graph
// alive (its listeners, timers, and async save chains all survive), and every
// graph shares the ONE hoisted mockCommands object: mocked-module factories
// run once per file, NOT once per graph (verified empirically — resetModules
// returned the same bindings instance). So a dead graph's surviving save can
// land in the shared saveEditorContent.mock.calls log DURING a later test,
// and no mock-layer guard can attribute calls to graphs. The defenses are
// (1) the file-wide afterEach drain and (2) invariant-form assertions. This
// describe pins (2): the assertion the flake killed must tolerate a foreign
// call while still catching the actual regression it exists for.
// ---------------------------------------------------------------------------

describe('shared command log asserted by invariant, not totality', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('a foreign call in the shared log does not fail the rename invariant', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Hello' }, body: '# Hello' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/old.md', isDir: false });

    document.dispatchEvent(new CustomEvent('editor:file-renamed', { detail: { oldPath: 'old.md', newPath: 'new.md' } }));
    (window as any).__moss_debug.getFrontmatterStore().removeField('title');
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenCalled());

    // Simulate the CI bleed: a write from a dead module graph lands in the
    // shared log inside this test's window.
    mockCommands.saveEditorContent('/proj/GHOST-from-stale-graph.md', {}, '');

    const paths = mockCommands.saveEditorContent.mock.calls.map((c: any[]) => c[0]);
    // The totality form the flaky test used is exactly what the bleed breaks:
    expect(paths.every((p: string) => p === '/proj/new.md')).toBe(false);
    // The invariant that matters survives the foreign call…
    expect(paths).toContain('/proj/new.md');
    expect(paths).not.toContain('/proj/old.md');
    // …and still catches the real regression: a captured-path write would put
    // '/proj/old.md' in the log and fail the not.toContain above.
  });
});

// ---------------------------------------------------------------------------
// A click that LIES is worse than a dead click. Clicking a tree row paints the
// selection in the click's own frame; if the read behind it then fails, the
// editor keeps the PREVIOUS document.
// A failed load was a
// console.error and nothing else, so the tree claimed one file while another
// was mounted and the next keystroke edited a file the user believed they had
// navigated away from. Both halves of the fix are asserted here: the failure is
// VISIBLE, and selection never disagrees with editor content.
// ---------------------------------------------------------------------------

describe('a failed file open is visible and never leaves the tree lying', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
    // resetMocks() does not cover describeSourceRole, and this block's
    // restoreAllMocks strips the hoisted default — leaving it resolving
    // `undefined`, which both loadFile and PreviewFollower survive but log
    // about. Re-arm it so the stderr in this block is only ever about a real
    // failure under test.
    mockCommands.describeSourceRole.mockResolvedValue({
      status: 'ok',
      data: { slot: null, language: null },
    });
  });

  afterEach(async () => {
    // Let every open's TAIL (heading state, preview follow) finish BEFORE the
    // stubs are stripped. restoreAllMocks turns an in-flight command call into
    // `undefined`, and the chain then rejects into a LATER test as an unhandled
    // rejection — the shared-log bleed described at the top of this file, in its
    // async-chain form.
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));
    vi.restoreAllMocks();
  });

  /** Live toast text — the only user-visible channel a failed open has. */
  function toastText(): string {
    return document.getElementById('moss-toast-container')?.textContent ?? '';
  }

  /** The FileTree instance the live module graph constructed. */
  function tree(): any {
    return MockFileTree.mock.results.at(-1)!.value;
  }

  async function initEditor(): Promise<void> {
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
  }

  test('the user is told, in words, that the file did not open', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'No such file' });
    await initEditor();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'posts/gone.md' },
    }));

    await vi.waitFor(() => expect(toastText()).toContain('gone.md'));
    // The underlying reason travels with it — a bare "couldn't open" leaves the
    // user with no next move.
    expect(toastText()).toContain('No such file');
  });

  test('the tree reverts to the document actually mounted, by EntryId', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'first' },
    });
    await initEditor();

    // Open a file successfully — this is the document that must stay both
    // MOUNTED and SELECTED after the failed open below.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/open.md', isDir: false });
    const registry = (window as any).__moss_debug.registry;
    const openId = registry.activeEntryId;
    expect(registry.getById(openId).path).toBe('open.md');

    const editorsBuilt = MockCmEditor.mock.calls.length;
    tree().highlight.mockClear();

    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'unreadable' });
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'broken.md' },
    }));

    await vi.waitFor(() => expect(tree().highlight).toHaveBeenCalled());
    // The tree is put back onto the mounted document — not left on broken.md.
    expect(tree().highlight).toHaveBeenLastCalledWith('open.md');
    // …and identity is unchanged: same EntryId, no phantom active entry.
    expect(registry.activeEntryId).toBe(openId);
    // The editor was never swapped, which is the whole reason the revert exists.
    expect(MockCmEditor.mock.calls.length).toBe(editorsBuilt);
  });

  test('with nothing mounted, the selection is cleared rather than left phantom', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'unreadable' });
    await initEditor();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'broken.md' },
    }));

    await vi.waitFor(() => expect(tree().clearSelection).toHaveBeenCalled());
    expect(tree().highlight).not.toHaveBeenCalledWith('broken.md');
  });

  test('the failure wedges nothing — clicking the same file again retries it', async () => {
    // The failure path releases whatever it claimed: a second click on the same
    // row must reach the command again, not be swallowed. (Opening a file is
    // deliberately guardless in loadFile — a second open SUPERSEDES an in-flight
    // one — so this pins that the failure handling did not quietly add one.)
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'locked' });
    await initEditor();

    const attempts = (): number =>
      mockCommands.parseFrontmatter.mock.calls
        .filter((c: any[]) => String(c[0]).endsWith('flaky.md')).length;

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'flaky.md' },
    }));
    await vi.waitFor(() => expect(attempts()).toBeGreaterThanOrEqual(1));
    await vi.waitFor(() => expect(toastText()).toContain('flaky.md'));
    const afterFirst = attempts();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'flaky.md' },
    }));
    await vi.waitFor(() => expect(attempts()).toBeGreaterThan(afterFirst));
  });

  // The preview follows the EDITOR. If the editor did not move, neither may the
  // preview — otherwise the user is shown one document beside another with
  // nothing saying which is real. loadFile returns 'opened' | 'failed' |
  // 'superseded'; the file-selected handler navigates on 'opened' only.

  /** navigate-preview emits, newest last. */
  function navigations(): string[] {
    return mockEmit.mock.calls
      .filter((c: any[]) => c[0] === 'editor:navigate-preview')
      .map((c: any[]) => c[1].urlPath);
  }

  test('a file that fails to open does not move the preview', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: 'first' },
    });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/open/' });
    await initEditor();

    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/open.md', isDir: false });
    await vi.waitFor(() => expect(navigations()).toContain('/open/'));
    mockEmit.mockClear();

    // broken.md HAS a resolvable URL — the preview would happily go there. The
    // only reason it must not is that the editor never opened the file.
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'unreadable' });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/broken/' });

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'broken.md' },
    }));

    // Wait past the failure — the toast is the last thing the failed open does.
    await vi.waitFor(() => expect(toastText()).toContain('broken.md'));
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    expect(navigations()).toEqual([]);
    // Not even asked: the preview never starts resolving a file the editor
    // could not open.
    expect(mockCommands.resolveUrlForFile).not.toHaveBeenCalledWith(
      expect.stringContaining('broken.md'),
    );
  });

  test('a brand-new file that fails to open does not move the preview either', async () => {
    // isNewFile takes a second, build-INDEPENDENT route to the preview
    // (resolveExpectedUrlForFile, emitted before any build lands). It is gated
    // on the same outcome — one failed open, zero preview moves.
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'unreadable' });
    mockCommands.resolveExpectedUrlForFile.mockResolvedValue({ status: 'ok', data: '/ghost/' });
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/ghost/' });
    await initEditor();
    mockEmit.mockClear();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'ghost.md', isNewFile: true },
    }));

    await vi.waitFor(() => expect(toastText()).toContain('ghost.md'));
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    expect(navigations()).toEqual([]);
  });

  test('a SUPERSEDED open moves neither the preview nor the tree', async () => {
    // Superseded is not failure: clicking B while A is still loading must land
    // on B, and A's late failure owns nothing at all. So A must not drag the
    // preview back to itself, and must not re-assert selection over B either —
    // both would fight the load that replaced it.
    // ONE shared deferred for slow.md (not one per call): every editor-main
    // instance still listening on `document` from an earlier test in this file
    // sees the same dispatch, and a per-call deferred would leave all but the
    // last of them hanging past the end of the test.
    let releaseSlow!: (v: any) => void;
    const slowRead = new Promise((resolve) => { releaseSlow = resolve; });
    mockCommands.parseFrontmatter.mockImplementation((abs: string) =>
      abs.endsWith('slow.md')
        ? slowRead
        : Promise.resolve({ status: 'ok', data: { frontmatter: {}, body: 'good' } }),
    );
    mockCommands.resolveUrlForFile.mockImplementation((abs: string) =>
      Promise.resolve({ status: 'ok', data: abs.endsWith('slow.md') ? '/slow/' : '/good/' }),
    );
    await initEditor();
    mockEmit.mockClear();
    tree().highlight.mockClear();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'slow.md' },
    }));
    await vi.waitFor(() =>
      expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(expect.stringContaining('slow.md')),
    );

    // B supersedes A while A is still awaiting its read. Routed through
    // editor:load-source (the preview-nav channel) rather than a second
    // file-selected: that channel is delivered to THIS editor-main instance
    // only, so the supersede under test is unambiguous.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/good.md', isDir: false });
    await vi.waitFor(() => expect(navigations()).toContain('/good/'));

    // A's read now fails, long after B took over.
    releaseSlow({ status: 'error', error: 'unreadable' });
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    // The preview stayed with the editor, on B. A superseded load navigates
    // nowhere — it does not drag the preview back to the file it never opened.
    expect(navigations()).not.toContain('/slow/');
    expect(mockCommands.resolveUrlForFile).not.toHaveBeenCalledWith(
      expect.stringContaining('slow.md'),
    );
    // …and it does not re-assert selection over B's either.
    expect(tree().highlight).toHaveBeenLastCalledWith('good.md', expect.anything());
  });

  // Two guards are pinned here at once, because they are
  // the same race seen from two owners:
  //  - applyHeadingState's own stale-result guard (converted from a
  //    captured-path/EntryId comparison to docGeneration capture()/isStale())
  //    must not paint the outgoing document's heading over the incoming one's.
  //  - loadFile's `loadGeneration` guard must cover its LAST await (the
  //    heading compute), not just the ones
  //    before it — a superseded load that falls through this one still
  //    returns 'opened', so the editor:file-selected handler (which gates its
  //    own preview-navigate call on that outcome) drags the preview back to
  //    the file it never actually finished opening.
  test('a slow heading compute for a superseded document neither paints its text nor drags the preview back to it', async () => {
    document.body.insertAdjacentHTML(
      'beforeend',
      '<div id="editor-heading" contenteditable="plaintext-only" data-empty="true"></div>',
    );
    mockCommands.parseFrontmatter.mockImplementation((abs: string) =>
      Promise.resolve({
        status: 'ok',
        data: { frontmatter: {}, body: abs.endsWith('heading-slow.md') ? 'slow body' : 'good body' },
      }),
    );
    mockCommands.resolveUrlForFile.mockImplementation((abs: string) =>
      Promise.resolve({ status: 'ok', data: abs.endsWith('heading-slow.md') ? '/slow/' : '/good/' }),
    );
    let releaseSlowHeading!: (v: unknown) => void;
    const slowHeading = new Promise((resolve) => { releaseSlowHeading = resolve; });
    mockCommands.computeHeadingState.mockImplementation((abs: string) =>
      abs.endsWith('heading-slow.md')
        ? slowHeading
        : Promise.resolve({ status: 'ok', data: { visible: true, text: 'GOOD', source: 'Title' } }),
    );
    await initEditor();
    mockEmit.mockClear();

    // A's own dispatch is fire-and-forget: its load never resolves until
    // releaseSlowHeading() below fires, so awaiting it here would deadlock
    // this test on itself.
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'heading-slow.md' },
    }));
    await vi.waitFor(() =>
      expect(mockCommands.computeHeadingState).toHaveBeenCalledWith(
        expect.stringContaining('heading-slow.md'), expect.anything(), expect.anything(),
      ),
    );

    // B supersedes A while A's heading compute (not its read — that already
    // finished) is still in flight.
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'heading-good.md' },
    }));
    await vi.waitFor(() =>
      expect(document.getElementById('editor-heading')!.textContent).toBe('GOOD'),
    );
    await vi.waitFor(() => expect(navigations()).toContain('/good/'));

    // A's heading compute finally comes back, long after B took over.
    releaseSlowHeading({ status: 'ok', data: { visible: true, text: 'SLOW', source: 'Title' } });
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    expect(document.getElementById('editor-heading')!.textContent).toBe('GOOD');
    // The bug this pins: loadFile's last await (the heading compute) was not
    // covered by loadGeneration, so A fell through to `return 'opened'` and
    // the file-selected handler's outcome-gated `previewFollower.navigate`
    // call fired for A — dragging the preview BACK to a file the editor
    // never actually finished opening. (A's own PreviewFollower-subscription
    // navigate to '/slow/', fired the moment loadFile calls registry.setActive
    // — well before the stuck heading compute — is legitimate and expected;
    // what must not happen is a SECOND, later one after B has already won.)
    expect(navigations().at(-1)).toBe('/good/');
  });

  // A still-arriving load never calls registry.setActive (see loadFile's
  // still_arriving branch), so currentEntryId() stays on the OUTGOING
  // document even though applyEditorSurface({kind:'pending'}) tears its
  // file-mode down (teardownFileMode → resetDocumentHistory → a docGeneration
  // bump). An identity guard (requestedEntryId vs currentEntryId) would see no
  // change and paint the outgoing document's stale heading back over the
  // pending pane; docGeneration sees the bump and drops it.
  test('a still-arriving supersession tears the heading down, and a late stale compute does not repaint it', async () => {
    document.body.insertAdjacentHTML(
      'beforeend',
      '<div id="editor-heading" contenteditable="plaintext-only" data-empty="true"></div>',
    );
    mockCommands.parseFrontmatter.mockImplementation((abs: string) =>
      Promise.resolve(
        abs.endsWith('still-b.md')
          ? { status: 'ok', data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true } }
          : { status: 'ok', data: { frontmatter: { title: 'A' }, body: 'a body' } },
      ),
    );
    let releaseStaleEdit!: (v: unknown) => void;
    const staleEdit = new Promise((resolve) => { releaseStaleEdit = resolve; });
    let headingCalls = 0;
    mockCommands.computeHeadingState.mockImplementation((abs: string) => {
      if (!abs.endsWith('still-a.md')) {
        return Promise.resolve({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } });
      }
      headingCalls += 1;
      // First call: A's own initial open — resolves fast. Second call: the
      // frontmatter edit below — held open until released, after B supersedes.
      return headingCalls === 1
        ? Promise.resolve({ status: 'ok', data: { visible: true, text: 'A-TITLE', source: 'Title' } })
        : staleEdit;
    });
    await initEditor();

    // Both loads go through 'editor:load-source' rather than
    // 'editor:file-selected': the latter is a raw document event that every
    // editor-main instance from an earlier test in this file keeps listening
    // on (nothing ever fires 'beforeunload' in jsdom to trip scaleAbort), and
    // this test's headingCalls counter — order-dependent, unlike the
    // path-only matching elsewhere in this file — is corrupted by a stale
    // instance's own concurrent call to the same shared mock.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/still-a.md', isDir: false });
    await vi.waitFor(() =>
      expect(document.getElementById('editor-heading')!.textContent).toBe('A-TITLE'),
    );

    // Edit A's frontmatter — this is the SECOND computeHeadingState call,
    // the one that hangs until released below.
    const store = (window as any).__moss_debug.getFrontmatterStore();
    store.setField('title', 'A-EDITED');
    await vi.waitFor(() => expect(headingCalls).toBe(2));

    // B supersedes A while that edit's heading compute is still in flight.
    // still_arriving never reaches registry.setActive, so currentEntryId()
    // does not change — only docGeneration does, via the teardown.
    const cm = document.getElementById('moss-editor-cm')!;
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/still-b.md', isDir: false });
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('pending'));
    expect(document.getElementById('editor-heading')!.dataset.empty).toBe('true');

    // The stale edit's heading compute finally comes back.
    releaseStaleEdit({ status: 'ok', data: { visible: true, text: 'A-STALE', source: 'Title' } });
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    // Must still be torn down — not repainted with A's stale text.
    expect(document.getElementById('editor-heading')!.dataset.empty).toBe('true');
    expect(cm.getAttribute('data-editor-surface')).toBe('pending');
  });

  test('a home file that fails to open does not move the preview on the root resolve either', async () => {
    // The SECOND route from a load straight into a preview navigation: the
    // root resolve (resolveAndApplyRootSurface → loadContentPageDeduped →
    // previewFollower.navigate). Reached here through the onboarding refresh —
    // files land in a folder that had none, so moss resolves the site home and
    // opens it. If that home is unreadable (iCloud-evicted mid-arrival is the
    // realistic case), the editor stays on a non-content surface, so the
    // preview must stay put too.
    mockGetFirstFile.mockReturnValueOnce(null).mockReturnValueOnce(null).mockReturnValue('index.md');
    mockCommands.resolvePageSource.mockResolvedValue({
      status: 'ok',
      data: { source_path: '/Users/test/blog/index.md', is_dir: true, is_article: false, syndicated: [], takeover: null },
    });
    // The home page HAS a resolvable URL — as with the file-selected leg above,
    // the only reason the preview must not go there is the failed open.
    mockCommands.resolveUrlForFile.mockResolvedValue({ status: 'ok', data: '/home/' });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'error', error: 'unreadable' });

    await initEditor();
    await vi.waitFor(() => expect(listenCallbacks.has('moss-event')).toBe(true));
    // Let init's tail settle so the getFirstFile read order above is
    // deterministic (same pattern as the import-refresh tests).
    const cm = document.getElementById('moss-editor-cm')!;
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).not.toBeNull());
    mockEmit.mockClear();

    await listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: { source_creates: ['index.md'] } });

    // The failed open is visible, exactly as on the other leg.
    await vi.waitFor(() => expect(toastText()).toContain('index.md'));
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    // No CM6 document mounted, so nothing for the preview to follow.
    expect(MockCmEditor).not.toHaveBeenCalled();
    expect(navigations()).toEqual([]);
    expect(mockCommands.resolveUrlForFile).not.toHaveBeenCalledWith(
      expect.stringContaining('index.md'),
    );
  });

  // ── A file that is still downloading is not a missing page ──────
  //
  // The reported session: Edit on the home page of a Google Drive vault
  // mid-materialization gave a toast reading "Resource deadlock avoided (os
  // error 11)" and a pane reading "No page to edit — This page is generated by
  // moss. There's nothing here to edit.", over a home note that plainly exists.
  // Two different lies from one cause: the read failed, and a failed open used
  // to leave the PREVIOUS surface (the pre-warm's generated pane) mounted where
  // it reads as a verdict about the file just asked for.

  test('a still-arriving file gets its own pane — never the generated one, never an errno', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true },
    });
    await initEditor();

    // Start on the surface the bug reported: the generated pane.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isRoot: false, takeover: null });
    const cm = document.getElementById('moss-editor-cm')!;
    expect(cm.getAttribute('data-editor-surface')).toBe('generated');

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'My Site.md' },
    }));

    await vi.waitFor(() =>
      expect(cm.getAttribute('data-editor-surface')).toBe('pending'),
    );
    // The previous pane's verdict is gone, not merely covered.
    expect(cm.textContent).not.toContain('generated by moss');
    // And nothing anywhere is speaking errno at the user.
    const shown = `${cm.textContent} ${toastText()}`.toLowerCase();
    expect(shown).not.toContain('os error');
    expect(shown).not.toContain('deadlock');
    // No document was mounted over it either — an empty CM6 buffer on a file
    // with content is the same lie in a different shape.
    expect(MockCmEditor).not.toHaveBeenCalled();
  });

  test('the announcement names the file the pane is waiting for', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true },
    });
    await initEditor();
    mockEmit.mockClear();

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'My Site.md' },
    }));

    // "Something is pending" is not enough for the shell to act on. The pane the
    // user is looking at was opened by PATH, and the shell's only other handle —
    // the live preview URL — names whatever page the preview happens to show,
    // which after a tree click is a different document. Healing that one instead
    // is a retry pointed at the wrong file.
    await vi.waitFor(() =>
      expect(mockEmit).toHaveBeenCalledWith(
        'editor:source-pending',
        expect.objectContaining({ sourcePath: expect.stringContaining('My Site.md') }),
      ),
    );
  });

  test('a pending pane already on screen re-announces, or the retry that matters never runs', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true },
    });
    await initEditor();
    const cm = document.getElementById('moss-editor-cm')!;

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'My Site.md' },
    }));
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('pending'));

    mockEmit.mockClear();

    // The shell's retry re-offered the file and the bytes are STILL not here, so
    // the editor lands on the pending pane a second time. The pane is already
    // mounted, so nothing re-mounts — and if the announcement rides on the mount,
    // nothing re-arms either. The pane then sits there through every later
    // arrival, including the one that finally delivers the file: the original
    // report, reproduced by the fix for it.
    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'My Site.md' },
    }));

    // `expect.anything()` would be satisfied by a `pending: false` emit, and
    // "the second pane announces itself SETTLED" is the neighbouring bug.
    await vi.waitFor(() =>
      expect(mockEmit).toHaveBeenCalledWith(
        'editor:source-pending',
        expect.objectContaining({ pending: true, sourcePath: expect.stringContaining('My Site.md') }),
      ),
    );
  });

  test('a real document mounting over the pending pane settles it too', async () => {
    // The exit applyEditorSurface never sees. Without it the shell stays armed
    // for a pane that content replaced, and the next arrival re-offers the old
    // file — over whatever the user is now editing.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true },
    });
    await initEditor();
    const cm = document.getElementById('moss-editor-cm')!;

    // Both opens go through `editor:load-source` rather than a document
    // CustomEvent. `editor:file-selected` is a DOCUMENT listener, and each
    // test's `vi.resetModules()` + re-import leaves the previous instance's
    // listener attached — so a dispatch runs every past module's loadFile, and
    // the ones whose mocks afterEach already stripped construct a ChipBar of
    // `{}` that the next real mount then calls `.destroy()` on. This is the
    // shared-log bleed at the top of this file, in its listener form; the listen
    // callback is per-instance and awaitable, which sidesteps it entirely.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/My Site.md', isRoot: false, takeover: null });
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('pending'));

    mockEmit.mockClear();
    MockCmEditor.mockClear();
    // The bytes landed: the same open now returns a real document.
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '# Home', has_frontmatter: false },
    });

    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/Users/test/blog/My Site.md', isRoot: false, takeover: null });

    // The settle has to ride a document actually opening, not a teardown on the
    // way to a failure — so the mount is asserted, not just the announcement.
    await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalled());
    expect(mockEmit).toHaveBeenCalledWith(
      'editor:source-pending',
      expect.objectContaining({ pending: false }),
    );
  });

  test('a resolve that FAILS under a pending pane is not evidence the file is absent', async () => {
    // The retry's own rule — "a resolve that threw is not evidence about the
    // file" — has to hold on this side too. `resolveNodeForEdit` falls back to
    // the cached node when a resolve fails, and a node arriving over
    // editor:load-source carries no `sourcePending`; undefined reads as
    // settled, so one failed resolve turns the pending root pane into "this
    // folder has no home page yet" AND announces it settled, disarming the
    // retry for good. That is the same symptom reached through the fix
    // for it: a Create-home CTA over a home page that is still downloading.
    mockCommands.resolvePageSource.mockResolvedValue({
      status: 'ok',
      data: { source_path: null, takeover: null, source_pending: true },
    });
    await initEditor();
    const cm = document.getElementById('moss-editor-cm')!;

    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isRoot: true, takeover: null });
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('pending'));

    mockEmit.mockClear();
    mockCommands.resolvePageSource.mockResolvedValue({ status: 'error', error: 'backend busy' });

    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isRoot: true, takeover: null });
    for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));

    expect(cm.getAttribute('data-editor-surface')).toBe('pending');
    expect(cm.textContent).not.toContain('home');
    expect(mockEmit).not.toHaveBeenCalledWith(
      'editor:source-pending',
      expect.objectContaining({ pending: false }),
    );
  });

  test('a surface that is not the pending one tells the shell to stop watching', async () => {
    mockCommands.parseFrontmatter.mockResolvedValue({
      status: 'ok',
      data: { frontmatter: {}, body: '', has_frontmatter: false, still_arriving: true },
    });
    await initEditor();
    const cm = document.getElementById('moss-editor-cm')!;

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      detail: { filePath: 'My Site.md' },
    }));
    await vi.waitFor(() => expect(cm.getAttribute('data-editor-surface')).toBe('pending'));
    mockEmit.mockClear();

    // The user navigated the preview onto a genuinely generated page. The arm is
    // now stale, and a stale arm re-offers the OLD file on the next arrival —
    // yanking the user out of whatever they moved to. Only the editor knows the
    // pane is gone, so only the editor can say so.
    await listenCallbacks.get('editor:load-source')!({ sourcePath: null, isRoot: false, takeover: null });

    // One channel, both directions: a separate "stop watching" event could
    // overtake an arm emitted just before it.
    await vi.waitFor(() =>
      expect(mockEmit).toHaveBeenCalledWith(
        'editor:source-pending',
        expect.objectContaining({ pending: false }),
      ),
    );
  });
});

describe('editor-main boot — own-titlebar', () => {
  // init() calls applyOwnTitlebarClass() (utils/fullscreen-class.ts)
  // synchronously, before its first await — so it has already run by the
  // time the dynamic import below resolves. Fails red if that call is ever
  // deleted from editor-main.ts's init(): the body never gets the class
  // under a mocked-mac UA.
  //
  // No "leaves it off elsewhere" case: jsdom already defaults to a non-mac
  // platform, so that case passes whether or not the call site exists — it
  // proves nothing about the wiring. The setter's own off-mac behavior is
  // covered by fullscreen-class.test.ts's applyOwnTitlebarClass suite.
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    delete (navigator as Navigator & { userAgentData?: unknown }).userAgentData;
  });

  /** jsdom reports a Linux platform, so mac has to be simulated (kbd-label.test.ts). */
  function withPlatform(value: string): void {
    Object.defineProperty(navigator, 'platform', { value, configurable: true });
  }

  async function initEditor(): Promise<void> {
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
  }

  test('adds body.own-titlebar under a mocked-mac UA', async () => {
    withPlatform('MacIntel');
    await initEditor();
    expect(document.body.classList.contains('own-titlebar')).toBe(true);
  });
});

// ---------------------------------------------------------------------------
describe('mountEmailComposer — editor:flush-save handshake', () => {
  const originalLocation = window.location;

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
    mockCommands.emailListSubscribers.mockResolvedValue({ status: 'ok', data: [] });
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', { value: originalLocation, writable: true });
    vi.restoreAllMocks();
  });

  test('a host without a composer ignores ?compose= and keeps the editor chrome', async () => {
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?compose=${encodeURIComponent('posts/hello.md')}&deployed_url=x` },
      writable: true,
      configurable: true,
    });
    await fillHost();
    delete (await import("../../host")).host.composer;
    await import('../editor-main');
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    expect(document.getElementById('nav-surface')!.style.display).not.toBe('none');
    expect(mockCommands.emailListSubscribers).not.toHaveBeenCalled();
  });

  test('an unsaved composer draft is written before the flush-save ack', async () => {
    Object.defineProperty(window, 'location', {
      value: { ...originalLocation, search: `?compose=${encodeURIComponent('posts/hello.md')}&deployed_url=x` },
      writable: true,
      configurable: true,
    });
    await fillHost();
    await bootEditorMain();
    await vi.waitFor(() => expect(document.querySelector('[data-action="send-now"]')).not.toBeNull());

    // One bar: the composer's own titlebar replaces the breadcrumb surface.
    expect(document.getElementById('nav-surface')!.style.display).toBe('none');
    mockComposer.flush.mockClear();
    mockEmit.mockClear();

    await saveRequest!('t1');

    expect(mockComposer.flush).toHaveBeenCalledTimes(1);
    const writeOrder = mockComposer.flush.mock.invocationCallOrder[0];
    const ackCall = mockEmit.mock.calls.findIndex((c) => c[0] === 'editor:flush-save-done');
    expect(ackCall).toBeGreaterThanOrEqual(0);
    expect(mockEmit.mock.invocationCallOrder[ackCall]).toBeGreaterThan(writeOrder);
  });
});

// Publish-preflight evidence is a view of one completed build, not document
// state. These lifecycle checks deliberately drive the editor entry point: the
// individual hashing/range helpers have their own unit tests, while the risk
// here is stale evidence surviving an edit, a click, or a root transition.
describe('editor-main publish-preflight evidence lifecycle', () => {
  const root = '/Users/test/blog';
  const sourceRevision = '230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5';

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: root });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'body' } });
    mockCommands.describeSourceRole.mockResolvedValue({ status: 'ok', data: { slot: null, language: null } });
    mockCommands.readSourceText.mockResolvedValue({ status: 'ok', data: 'body' });
  });

  afterEach(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve();
    vi.restoreAllMocks();
  });

  function projection(folder = root) {
    return {
      build_generation: 1,
      missing_references: [{
        source_path: 'page.md',
        source_revision: sourceRevision,
        source_span: { start_byte: 0, end_byte: 4, line: 1 },
        reference: `${folder}/missing.png`,
      }],
    };
  }

  function tree(): any {
    return MockFileTree.mock.results.at(-1)!.value;
  }

  function store(): any {
    return MockPublishPreflightStore.mock.results.at(-1)!.value;
  }

  async function openPage(view: any | (() => any)): Promise<any> {
    MockCmEditor.mockImplementation((options: any) => ({
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue('body'),
      setContent: vi.fn(),
      getView: vi.fn().mockImplementation(() => typeof view === 'function' ? view() : view),
    }));
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: `${root}/page.md`, isDir: false });
    return MockCmEditor.mock.calls.at(-1)![0];
  }

  async function seedHints(view: any): Promise<void> {
    store().emit(root, projection());
    await vi.waitFor(() => expect(view.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      effects: expect.objectContaining({ value: [expect.objectContaining({ reference: `${root}/missing.png` })] }),
    })));
  }

  test('says which file it loaded once loadFile completes', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    await openPage(view);

    await vi.waitFor(() => expect(mockEmit).toHaveBeenCalledWith('editor:file-ready', { path: 'page.md' }));
  });

  test('clears existing hints synchronously from CM onChange, before the autosave debounce', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    const options = await openPage(view);
    await seedHints(view);
    view.dispatch.mockClear();

    options.onChange();

    expect(view.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      effects: expect.objectContaining({ value: [] }),
    }));
    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();
  });

  test('a reload from disk clears the evidence before it replaces the text', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    await openPage(view);
    await seedHints(view);
    view.dispatch.mockClear();
    const editor = MockCmEditor.mock.results.at(-1)!.value;
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: 'changed elsewhere' } });
    mockCommands.getFileInfo.mockResolvedValue({ status: 'ok', data: { modified: '2026-10-13T10:00:00Z' } });

    await listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload: { modified_paths: ['page.md'] } });
    await vi.waitFor(() => expect(editor.setContent).toHaveBeenCalledWith('changed elsewhere'));

    const cleared = view.dispatch.mock.calls.findIndex(([tr]: any[]) => Array.isArray(tr.effects?.value) && tr.effects.value.length === 0);
    expect(cleared).toBeGreaterThanOrEqual(0);
    expect(view.dispatch.mock.invocationCallOrder[cleared]).toBeLessThan(editor.setContent.mock.invocationCallOrder[0]);
  });

  test('ignores a preflight update while CM is only partially mounted', async () => {
    const partialView = { scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };

    await openPage(partialView);
    store().emit(root, projection());
    await Promise.resolve();

    expect(mockCommands.readSourceText).toHaveBeenCalledWith('page.md');
  });

  test('clears painted hints when the completed projection becomes clean', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    await openPage(view);
    await seedHints(view);
    view.dispatch.mockClear();

    store().emit(root, null);

    expect(view.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      effects: expect.objectContaining({ value: [] }),
    }));
  });

  test('paints retained evidence after a partial CM view becomes live and the store refreshes', async () => {
    const partialView = { scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    const liveView = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    let view: any = partialView;

    await openPage(() => view);
    store().emit(root, projection());
    for (let i = 0; i < 6; i++) await Promise.resolve();
    view = liveView;
    store().emit(root, projection());

    await vi.waitFor(() => expect(liveView.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      effects: expect.objectContaining({ value: [expect.objectContaining({ reference: `${root}/missing.png` })] }),
    })));
  });

  test('clears existing hints synchronously when frontmatter changes', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    await openPage(view);
    await seedHints(view);
    view.dispatch.mockClear();

    (window as any).__moss_debug.getFrontmatterStore().setField('draft', true);

    expect(view.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      effects: expect.objectContaining({ value: [] }),
    }));
  });

  test('rejects an open-preflight request while dirty without selecting or reading disk', async () => {
    const view = { dispatch: vi.fn(), scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() };
    const options = await openPage(view);
    options.onChange();
    view.dispatch.mockClear();
    mockCommands.readSourceText.mockClear();

    listenCallbacks.get('editor:open-publish-preflight')!({
      path: 'page.md', sourceRevision, sourceSpan: { start_byte: 0, end_byte: 4, line: 1 },
    });

    await vi.waitFor(() => expect(document.getElementById('moss-toast-container')?.textContent).toContain('Build again'));
    expect(mockCommands.readSourceText).not.toHaveBeenCalled();
    expect(view.dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ selection: expect.anything() }));
  });

  test('clears tree evidence before a delayed B mount and rejects a late A projection for the same relative file', async () => {
    let releaseMount!: () => void;
    const delayedMount = new Promise<void>((resolve) => { releaseMount = resolve; });
    MockFileTree.mockImplementation(() => ({
      mount: vi.fn().mockReturnValue(delayedMount),
      unmount: vi.fn(), setupDragHandle: vi.fn(), highlight: vi.fn(), revealNewFile: vi.fn(),
      getFirstFile: mockGetFirstFile, refreshAndRender: mockRefreshAndRender,
      deepestVisibleContainerFor: mockDeepestVisibleContainerFor, flashRow: mockFlashRow,
      parkAtMinHeight: vi.fn(), collapseForReopen: vi.fn(), clearSelection: vi.fn(),
      isEditing: vi.fn().mockReturnValue(false), setSealed: mockSetSealed,
      setPublishPreflight: vi.fn(),
    }));
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/folders/B' });

    await bootEditorMain();
    await vi.waitFor(() => expect(tree().mount).toHaveBeenCalledWith(expect.anything(), '/folders/B', expect.anything(), true));

    expect(tree().setPublishPreflight).toHaveBeenCalledWith(null);
    expect(mockPrimePublishPreflight).not.toHaveBeenCalled();

    releaseMount();
    await vi.waitFor(() => expect(mockPrimePublishPreflight).toHaveBeenCalledWith(store(), '/folders/B'));
    tree().setPublishPreflight.mockClear();

    store().emit('/folders/A', projection('/folders/A'));

    expect(tree().setPublishPreflight).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Changes made outside moss: a save never re-creates a file that left, and
// unsaved text outlives a delete.
// ---------------------------------------------------------------------------

describe('editor-main when the open file leaves outside moss', () => {
  const moss = () => (window as any).__moss_debug;
  const fileChanged = (payload: Record<string, unknown>) =>
    listenCallbacks.get('moss-event')!({ kind: 'FileChanged', payload });
  const toastTexts = () => [...document.querySelectorAll('.moss-toast-message')].map((n) => n.textContent ?? '');

  beforeEach(async () => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: { title: 'Hello' }, body: '# Hello' } });
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: '/proj' });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-source')).toBe(true));
    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/note.md', isDir: false });
  });
  afterEach(() => { vi.restoreAllMocks(); });

  test('a save that finds the file gone writes nothing more there, then saves where the rename took it', async () => {
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'ok', data: { status: 'gone' } });
    moss().getFrontmatterStore().removeField('title');
    await moss().saveActiveDocument('forced');
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1);

    await moss().saveActiveDocument('forced');
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1);

    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    await fileChanged({ source_renames: [['note.md', 'field-notes.md']] });
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenLastCalledWith('/proj/field-notes.md', expect.anything(), expect.anything(), expect.any(String), false), { timeout: 3000 });
  });

  test('deleted with unsaved text: the document stays, and Save it back creates the file then saves into it', async () => {
    // Typing in the body: dirty, with the autosave still waiting out its delay.
    MockCmEditor.mock.calls.at(-1)![0].onChange();
    await fileChanged({ source_deletes: ['note.md'] });

    await vi.waitFor(() => expect(toastTexts()).toContain('"note.md" was deleted or moved outside moss. Your unsaved text is still here.'));
    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();

    mockCommands.createFiles.mockResolvedValue({ status: 'ok', data: ['/proj/note.md'] });
    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    const saveBack = [...document.querySelectorAll('.moss-toast-action')].find((b) => b.textContent?.includes('Save it back')) as HTMLElement;
    saveBack.click();
    await vi.waitFor(() => expect(mockCommands.saveEditorContent).toHaveBeenCalledWith('/proj/note.md', expect.anything(), expect.anything(), expect.any(String), false));
    expect(mockCommands.createFiles).toHaveBeenCalledWith([{ dir: '/proj/', name: 'note.md', frontmatter: {} }]);
    expect(mockCommands.createFiles.mock.invocationCallOrder[0]).toBeLessThan(mockCommands.saveEditorContent.mock.invocationCallOrder[0]);
  });

  test('moving to another file while a save waits on a gone file keeps the text and offers it back', async () => {
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'ok', data: { status: 'gone' } });
    moss().getFrontmatterStore().removeField('title');
    await moss().saveActiveDocument('forced');

    await listenCallbacks.get('editor:load-source')!({ sourcePath: '/proj/other.md', isDir: false });
    await vi.waitFor(() => expect(toastTexts()).toContain('"note.md" was deleted or moved outside moss. Your unsaved text is still here.'));
  });

  test('deleted with nothing unsaved: the editor lets it go and says why', async () => {
    await fileChanged({ source_deletes: ['note.md'] });
    await vi.waitFor(() => expect(toastTexts()).toContain('"note.md" was deleted or moved outside moss.'));
    expect(mockCommands.createFiles).not.toHaveBeenCalled();
  });
});


// ---------------------------------------------------------------------------
// Opening a file with a special role asks first (editor:file-selected is the
// one door for a user-chosen open)
// ---------------------------------------------------------------------------

describe('opening a special file from the tree', () => {
  const root = '/Users/test/blog';
  const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
  const select = (filePath: string) => document.dispatchEvent(new CustomEvent('editor:file-selected', { detail: { filePath } }));
  const dialog = () => document.querySelector<HTMLElement>('.moss-modal--special-file.visible');
  const press = (action: string) => dialog()!.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`)!.click();

  beforeEach(async () => {
    vi.resetModules(); setupEditorDOM(); resetMocks();
    localStorage.clear();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: root });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: '' } });
    mockCommands.computeHeadingState.mockResolvedValue({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-file')).toBe(true));
    (window as any).__moss_debug.registry.reconcileSnapshot([
      { path: 'AGENTS.md', isDir: false, fileId: null, special: 'agent-instructions' },
      { path: 'about.md', isDir: false, fileId: null },
    ]);
    mockCommands.parseFrontmatter.mockClear();
  });
  afterEach(() => { document.body.innerHTML = ''; localStorage.clear(); vi.restoreAllMocks(); });

  test('asks before opening, and Open then opens it', async () => {
    select('AGENTS.md');
    await settle();
    expect(dialog()).not.toBeNull();
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
    press('open');
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/AGENTS.md`));
  });

  test('Cancel opens nothing', async () => {
    select('AGENTS.md');
    await settle();
    press('cancel');
    await settle();
    expect(mockCommands.parseFrontmatter).not.toHaveBeenCalled();
  });

  test('Open with the box ticked opens it, and the next open of that kind skips the dialog', async () => {
    select('AGENTS.md');
    await settle();
    dialog()!.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();
    press('open');
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(1));

    select('AGENTS.md');
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledTimes(2));
    expect(dialog()).toBeNull();
  });

  test('a file with no kind opens directly', async () => {
    select('about.md');
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/about.md`));
    expect(dialog()).toBeNull();
  });

  test('a new file opens directly, whatever it is called', async () => {
    document.dispatchEvent(new CustomEvent('editor:file-selected', { detail: { filePath: 'AGENTS.md', isNewFile: true } }));
    await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/AGENTS.md`));
    expect(dialog()).toBeNull();
  });

  test('a file restored on launch or followed from the preview opens without the dialog', async () => {
    await listenCallbacks.get('editor:load-file')!({ filePath: `${root}/AGENTS.md` });
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`${root}/AGENTS.md`);
    expect(dialog()).toBeNull();
  });
});

describe('the chip bar learns where the build placed the open file', () => {
  const root = '/Users/test/blog';
  beforeEach(async () => {
    vi.resetModules(); setupEditorDOM(); resetMocks();
    mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: root });
    mockCommands.parseFrontmatter.mockResolvedValue({ status: 'ok', data: { frontmatter: {}, body: '' } });
    mockCommands.computeHeadingState.mockResolvedValue({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } });
    await bootEditorMain();
    await vi.waitFor(() => expect(listenCallbacks.has('editor:load-file')).toBe(true));
    (window as any).__moss_debug.registry.reconcileSnapshot([
      { path: 'works', isDir: true, fileId: null },
      { path: 'works/poem.md', isDir: false, fileId: null },
    ]);
  });
  afterEach(() => { document.body.innerHTML = ''; vi.restoreAllMocks(); });

  type Bar = { setPageFacts: ReturnType<typeof vi.fn> };
  const lastBar = () => MockChipBar.mock.results.at(-1)!.value as Bar;
  const positionFor = async (rel: string) => {
    await listenCallbacks.get('editor:load-file')!({ filePath: `${root}/${rel}` });
    await vi.waitFor(() => expect(lastBar().setPageFacts).toHaveBeenCalled());
    return lastBar().setPageFacts.mock.lastCall![0].position;
  };
  const buildComplete = () => {
    for (const cb of listenCallbacksArrays.get('moss-event') ?? []) cb({ kind: 'BuildComplete', payload: {} });
  };

  const facts = (position: string | null, auto: Record<string, unknown> = {}) =>
    ({ status: 'ok', data: { position, auto } });

  test('where the open page sits is what the build reports, not what the path suggests', async () => {
    mockCommands.pageFacts.mockResolvedValue(facts('home'));
    expect(await positionFor('works/poem.md')).toBe('home');
  });

  test("hands the chip bar the build's facts on open and again after each build", async () => {
    mockCommands.pageFacts.mockResolvedValue(facts('page', { layout: 'article' }));
    await positionFor('works/poem.md');
    expect(mockCommands.pageFacts).toHaveBeenCalledWith(`${root}/works/poem.md`);
    expect(lastBar().setPageFacts).toHaveBeenLastCalledWith({ position: 'page', auto: { layout: 'article' } });

    mockCommands.pageFacts.mockResolvedValue(facts('folder', { layout: 'page' }));
    buildComplete();
    await vi.waitFor(() => expect(lastBar().setPageFacts).toHaveBeenLastCalledWith({ position: 'folder', auto: { layout: 'page' } }));
  });

  test('a failed read offers every field', async () => {
    mockCommands.pageFacts.mockRejectedValue(new Error('no arm'));
    expect(await positionFor('works/poem.md')).toBeNull();
    expect(lastBar().setPageFacts).toHaveBeenLastCalledWith({ position: null, auto: {} });
  });
});
