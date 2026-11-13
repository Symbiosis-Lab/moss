/**
 * Tests for the `editor:flush-save` handshake in editor-main.ts — a
 * correctness fix for the unified-shutdown flush.
 *
 * The handler replies `editor:flush-save-done` with `{ token, ok }` where `ok`
 * reflects the write the flush *actually forced or awaited*, NOT a post-hoc
 * `saveState` sample:
 *
 *   - in-flight save that FAILS after the flush is invoked → ok:false (the
 *     flush awaits the settling write rather than returning a stale ok:true);
 *   - a no-op flush over a stale prior 'error' (nothing pending, nothing in
 *     flight) → ok:true (does not trap the user behind an un-closable window);
 *   - a dirty buffer that saves OK → ok:true;
 *   - a dirty buffer whose save fails → ok:false.
 *
 * Harness mirrors editor-external-update.test.ts: hoisted mocks + a listen
 * callback pump, boot editor-main, drive the wired onChange to dirty the
 * editor, then invoke the 'editor:flush-save' listener directly and read the
 * payload of the captured 'editor:flush-save-done' emit.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { savedWithHash } from './disk-hash';

// As editor.html does: the desktop's host record, then the editor. After a
// module reset both are fresh, so they load together.
// The flush request the editor registered with the host (the host's onSaveRequest).
let saveRequest: ((token: string) => Promise<void>) | undefined;

async function bootEditorMain() {
  // A fresh host per boot, as the editor webview loads it; the bus records emits.
  saveRequest = undefined;
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit, (name, handler) => listenCallbacks.set(name, handler as Function));
  host.onSaveRequest = (handler) => {
    saveRequest = async (token: string) => {
      const ok = await handler();
      host.bus.emit('editor:flush-save-done', { token, ok });
    };
    return Promise.resolve(() => {});
  };
  return import('../editor-main');
}

// ---------------------------------------------------------------------------
// Hoisted mocks (same shape as editor-external-update.test.ts).
// ---------------------------------------------------------------------------
const { mockEmit, mockCommands, listenCallbacks, emittedEvents, MockChipBar, MockCmEditor, MockFileTree } = vi.hoisted(() => {
  const listenCallbacks = new Map<string, Function>();
  const emittedEvents: Array<{ event: string; payload: unknown }> = [];
  const mockEmit = vi.fn().mockImplementation((event: string, payload: unknown) => {
    emittedEvents.push({ event, payload });
    return Promise.resolve(undefined);
  });
  const mockCommands = {
    getSchema: vi.fn(),
    pageFacts: vi.fn().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } }),
    getProjectPath: vi.fn(),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    computeHeadingState: vi.fn().mockResolvedValue({ status: 'ok', data: { visible: false, text: '', source: 'Filename' } }),
    getFileInfo: vi.fn().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn(),
    copyFilesToProject: vi.fn(),
  };
  const MockChipBar = vi.fn().mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    setValues: vi.fn(),
    setFieldValue: vi.fn(),
    revealField: vi.fn(),
    hasPendingWrite: vi.fn().mockReturnValue(false),
    cancelPendingWrite: vi.fn(),
  }));
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    setContent: vi.fn(),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
    setReadOnly: vi.fn(),
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
  }));
  return { mockEmit, mockCommands, listenCallbacks, emittedEvents, MockChipBar, MockCmEditor, MockFileTree };
});


vi.mock('../../engine', async () => ({ engine: (await import('./disk-hash')).withContentHash(mockCommands) }));

vi.mock('../chip-bar', () => ({
  ChipBar: MockChipBar,
}));

vi.mock('../cm-editor', async () => ({
  CmEditor: (await import('./disk-hash')).acceptingExternalChanges(MockCmEditor),
}));

vi.mock('../file-viewer');

vi.mock('../tree', () => ({
  FileTree: MockFileTree,
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
      },
    };
  });
}
applyEditorBootstrapDelegate();

function resetMocks(): void {
  listenCallbacks.clear();
  emittedEvents.length = 0;
  mockEmit.mockReset().mockImplementation((event: string, payload: unknown) => {
    emittedEvents.push({ event, payload });
    return Promise.resolve(undefined);
  });
  mockCommands.getSchema.mockReset().mockResolvedValue({ status: 'ok', data: { fields: [] } });
  mockCommands.getProjectPath.mockReset();
  mockCommands.editorBootstrap.mockReset();
  applyEditorBootstrapDelegate();
  mockCommands.parseFrontmatter.mockReset();
  mockCommands.getFileInfo.mockReset().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } });
  mockCommands.saveEditorContent.mockReset().mockImplementation(savedWithHash);
  mockCommands.resolveUrlForFile.mockReset().mockResolvedValue({ status: 'error', error: 'Preview server not started (test environment)' });
  mockCommands.copyFilesToProject.mockReset();
  mockCommands.pageFacts.mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } });
  MockChipBar.mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    setValues: vi.fn(),
    setFieldValue: vi.fn(),
    revealField: vi.fn(),
    hasPendingWrite: vi.fn().mockReturnValue(false),
    cancelPendingWrite: vi.fn(),
  }));
  MockCmEditor.mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    setContent: vi.fn(),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
    setReadOnly: vi.fn(),
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
}

function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div id="nav-surface"><div id="nav-grid"><div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div></div></div>
    <div id="divider"></div>
    <div id="chip-bar"></div>
    <div id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    <div id="moss-save-indicator"></div>
  `;
}

const PROJECT = '/Users/test/blog';
const FILE = '/Users/test/blog/posts/hello.md';

/**
 * Boot editor-main + load the current file. Returns the wired onChange (drives
 * the editor 'dirty') and a getContent spy.
 */
async function boot(view: object = {}): Promise<{ onChange: (gestureEnd?: boolean) => void }> {
  const initialBody = '# Hello\n\nbody';

  mockCommands.getProjectPath.mockResolvedValue({ status: 'ok', data: PROJECT });
  mockCommands.parseFrontmatter.mockResolvedValue({
    status: 'ok',
    data: { frontmatter: { title: 'Hello' }, body: initialBody },
  });
  mockCommands.getFileInfo.mockResolvedValue({
    status: 'ok',
    data: { size: '0', modified: '2026-05-22T10:00:00Z', width: null, height: null },
  });
  // Default: saves succeed. Individual tests override before flushing.
  mockCommands.saveEditorContent.mockImplementation(savedWithHash);

  MockCmEditor.mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(initialBody),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, ...view }),
    setReadOnly: vi.fn(),
    setContent: vi.fn(),
  }));

  await bootEditorMain();

  await vi.waitFor(() => {
    expect(listenCallbacks.has('editor:load-source')).toBe(true);
    expect(saveRequest).toBeDefined();
  });

  const loadSourceCb = listenCallbacks.get('editor:load-source')!;
  await loadSourceCb({ sourcePath: FILE, isDir: false });
  await vi.waitFor(() => expect(mockCommands.parseFrontmatter).toHaveBeenCalled());

  const cmCtor = MockCmEditor.mock.calls[MockCmEditor.mock.calls.length - 1][0] as { onChange?: (gestureEnd?: boolean) => void };
  expect(cmCtor.onChange).toBeDefined();
  return { onChange: cmCtor.onChange! };
}

/** Invoke the editor:flush-save listener and return the acked `ok`. */
async function invokeFlush(token = 'tok-1'): Promise<boolean | undefined> {
  await saveRequest!(token);
  const done = [...emittedEvents].reverse().find((e) => e.event === 'editor:flush-save-done');
  expect(done).toBeDefined();
  expect((done!.payload as { token: string }).token).toBe(token);
  return (done!.payload as { ok?: boolean }).ok;
}

describe('editor:flush-save handshake returns real {ok} (MF-2)', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('dirty buffer that saves OK → ok:true', async () => {
    const { onChange } = await boot();
    onChange(); // saveState → 'dirty'

    mockCommands.saveEditorContent.mockImplementation(savedWithHash);
    const ok = await invokeFlush();

    expect(mockCommands.saveEditorContent).toHaveBeenCalled();
    expect(ok).toBe(true);
  });

  test('dirty buffer whose save fails → ok:false', async () => {
    const { onChange } = await boot();
    onChange();

    mockCommands.saveEditorContent.mockResolvedValue({ status: 'error', error: 'disk full' });
    const ok = await invokeFlush();

    expect(mockCommands.saveEditorContent).toHaveBeenCalled();
    expect(ok).toBe(false);
  });

  test('in-flight save that FAILS after flush is invoked → ok:false (not stale true)', async () => {
    const { onChange } = await boot();

    // The auto-save write starts (saveState → 'saving') but hangs; it will
    // settle as a failure later. While it is in flight and nothing is newly
    // dirty, flushPendingSave takes the `!needsWrite && inFlight` branch — the
    // exact path that returned early (stale ok:true) before the MF-2 fix. It
    // must AWAIT the settling write and report its real result: false.
    let settleInFlight!: (r: { status: string; error: string }) => void;
    mockCommands.saveEditorContent.mockReturnValueOnce(
      new Promise((resolve) => { settleInFlight = resolve; }),
    );

    // Install fake timers BEFORE dirtying, so scheduleSave's 2s debounce is a
    // fake timer we can advance. (Installing them after onChange — as the
    // original draft did — leaves the real timeout uncontrolled, so it never
    // fires under fake time and the save never goes in-flight.) boot() is done,
    // so its vi.waitFor polling is not affected.
    vi.useFakeTimers();
    try {
      onChange(); // saveState → 'dirty'; schedules the 2s debounced save
      await vi.advanceTimersByTimeAsync(2100); // fires it → save goes in-flight (hangs)
    } finally {
      vi.useRealTimers();
    }
    // The write is now in flight (awaiting the unresolved saveEditorContent).
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1);

    // Invoke the flush while the save is still in flight and nothing newly
    // dirty. It must await the in-flight write.
      const flushPromise = saveRequest!('inflight');
    await Promise.resolve();

    // Now the in-flight write settles as an error.
    settleInFlight({ status: 'error', error: 'write failed' });
    await flushPromise;

    const done = [...emittedEvents].reverse().find((e) => e.event === 'editor:flush-save-done');
    expect(done).toBeDefined();
    expect((done!.payload as { token: string }).token).toBe('inflight');
    expect((done!.payload as { ok?: boolean }).ok).toBe(false);
  });

  test('no-op flush over a stale prior error → ok:true (does not trap the user)', async () => {
    const { onChange } = await boot();
    onChange();

    // First write fails → saveState becomes 'error', lastWriteOk=false.
    mockCommands.saveEditorContent.mockResolvedValue({ status: 'error', error: 'transient' });
    const firstOk = await invokeFlush('first');
    expect(firstOk).toBe(false);

    // Nothing newly dirty, nothing in flight: a second flush is a pure no-op.
    // It must NOT report the stale prior 'error' — that would trap the user.
    mockCommands.saveEditorContent.mockClear();
    const secondOk = await invokeFlush('second');

    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();
    expect(secondOk).toBe(true);
  });
});

/**
 * The autosave guard independently protects the open buffer: while a
 * rename/move/undo commit
 * holds the editor read-only, `saveActiveDocument` itself refuses any
 * autosave-reason write, so a debounce that was somehow still armed when
 * the lock engaged (or fires later, still within the lock's span) can
 * never land pre-commit content over the commit's own rewrite — only the
 * commit's own explicit `flushPendingSave` (reason: 'forced') may write.
 * `setEditorCommitLock`/`flushPendingSave` are the same functions every
 * commit call site (edit-session.ts, ops.ts, rename-toast.ts) uses; this
 * exercises them directly via `__moss_debug` rather than booting a real
 * FileTree/rename round trip — that ordering is covered separately in
 * editor-commit-hooks.test.ts.
 */
describe('saveActiveDocument refuses an autosave while the commit lock is held (round 2)', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('a debounce armed while locked never reaches saveEditorContent; the forced flush still writes', async () => {
    const { onChange } = await boot();
    const debug = (window as unknown as {
      __moss_debug: { setEditorCommitLock: (locked: boolean) => void; flushPendingSave: () => Promise<boolean> };
    }).__moss_debug;

    onChange(); // dirty, as if a rename/move/undo commit's own flush is about to run
    let settleFlush!: (r: Awaited<ReturnType<typeof savedWithHash>>) => void;
    mockCommands.saveEditorContent.mockReturnValueOnce(
      new Promise((resolve) => { settleFlush = resolve; }),
    );

    debug.setEditorCommitLock(true); // every call site locks BEFORE flushing
    const flushDone = debug.flushPendingSave();
    await Promise.resolve(); // let flushPendingSave reach its saveEditorContent await
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1); // the forced flush only

    // A keystroke somehow lands anyway (production CM6 read-only refuses it;
    // this proves the independent second guard for when it doesn't).
    vi.useFakeTimers();
    try {
      onChange(); // schedules the 800ms autosave debounce
      await vi.advanceTimersByTimeAsync(900); // the debounce tries to fire
    } finally {
      vi.useRealTimers();
    }
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1); // still just the flush

    // Settling the flush's own write lets saveActiveDocument's do-while loop
    // check `saveQueued` — WITHOUT the guard, the debounce's call above would
    // have set it (the in-flight branch), coalescing into a trailing SECOND
    // write here. With the guard, the debounce's call never reached that
    // branch at all, so nothing is queued and this stays the one write.
    settleFlush(await savedWithHash('', {}, ''));
    expect(await flushDone).toBe(true);
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1);
    debug.setEditorCommitLock(false);
  });
});

describe('the commit lock gives the body its focus back when it ends', () => {
  type Debug = { setEditorCommitLock: (locked: boolean) => void };
  const debug = () => (window as unknown as { __moss_debug: Debug }).__moss_debug;
  const focusableView = (hasFocus: boolean) => ({ hasFocus, focus: vi.fn(), dispatch: vi.fn() });

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test('a body focused when the lock starts is focused again when it ends', async () => {
    const view = focusableView(true);
    await boot(view);
    debug().setEditorCommitLock(true);
    expect(view.focus).not.toHaveBeenCalled();
    debug().setEditorCommitLock(false);
    expect(view.focus).toHaveBeenCalledTimes(1);
  });

  test('a body not focused stays unfocused', async () => {
    const view = focusableView(false);
    await boot(view);
    debug().setEditorCommitLock(true);
    debug().setEditorCommitLock(false);
    expect(view.focus).not.toHaveBeenCalled();
  });

  test('focus that moved elsewhere during the lock is left there', async () => {
    const view = focusableView(true);
    await boot(view);
    const field = document.createElement('input');
    document.body.append(field);
    debug().setEditorCommitLock(true);
    field.focus();
    debug().setEditorCommitLock(false);
    expect(view.focus).not.toHaveBeenCalled();
  });

  test("Enter in the title during the lock focuses the body when the lock ends", async () => {
    const heading = document.createElement('div');
    heading.id = 'editor-heading';
    heading.contentEditable = 'true';
    document.body.prepend(heading);
    mockCommands.computeHeadingState.mockResolvedValue({ status: 'ok', data: { visible: true, text: 'Hello', source: 'Filename' } });
    const view = focusableView(false);
    await boot(view);
    debug().setEditorCommitLock(true);
    heading.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(view.focus).not.toHaveBeenCalled();
    debug().setEditorCommitLock(false);
    expect(view.focus).toHaveBeenCalledTimes(1);
  });
});

describe("an instrument's release", () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test('is written at once; a typed edit waits out the debounce', async () => {
    const { onChange } = await boot();
    vi.useFakeTimers();
    onChange(false);
    await vi.advanceTimersByTimeAsync(50);
    expect(mockCommands.saveEditorContent).not.toHaveBeenCalled();
    onChange(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(mockCommands.saveEditorContent).toHaveBeenCalledTimes(1);
  });
});
