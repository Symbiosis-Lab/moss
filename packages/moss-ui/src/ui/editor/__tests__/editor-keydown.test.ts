/**
 * The editor webview's own document-level keyboard shortcuts. Keydown never
 * crosses webviews, so every shortcut a writer expects while typing has to be
 * bound HERE as well as in the shell.
 *
 * Font scale (Cmd+= / Cmd+- / Cmd+0): 11–18px presets, default 13px, applied
 * as the unitless --moss-editor-font-scale ratio on the document root (see
 * shared/editor-font-size.ts) — NOT body.style.fontSize. Persists to
 * localStorage under 'moss-editor-font-size' and broadcasts
 * 'moss:editor-font-size' so the Settings ▸ Editor stepper follows live.
 *
 * Settings (Cmd+,): relayed to the shell, which owns the modal.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';

// As editor.html does: the desktop's host record, then the editor. After a
// module reset both are fresh, so they load together.
async function bootEditorMain() {
  // A fresh host per boot, as the editor webview loads it; the bus records emits.
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit, (name, handler) => listenCallbacks.set(name, handler as Function));
  return import('../editor-main');
}

// ---------------------------------------------------------------------------
// Hoisted mocks — vi.mock factories are hoisted above imports, so we must
// use vi.hoisted() for any variables referenced inside them.
// ---------------------------------------------------------------------------
const { mockEmit, mockCommands, listenCallbacks, MockChipBar, MockCmEditor, MockFileTree, history, clusterHooks, MockDocumentHistory, MockDocStateCluster } = vi.hoisted(() => {
  // The undo/redo tests assert on WHAT HAPPENED, not only on whether the key
  // was claimed. preventDefault alone is satisfied by an adapter that claims
  // the key and does nothing — and by one that runs redo for undo, the two
  // directions swapped at the call site. These spies are the observable that
  // tells them apart.
  const history = {
    undo: vi.fn().mockReturnValue(true),
    redo: vi.fn().mockReturnValue(true),
    canUndo: vi.fn().mockReturnValue(true),
    canRedo: vi.fn().mockReturnValue(true),
    reset: vi.fn(),
    recordField: vi.fn(),
    observeCm6: vi.fn(),
    depths: vi.fn().mockReturnValue({ undo: 0, redo: 0 }),
  };
  const MockDocumentHistory = vi.fn().mockImplementation(() => history);
  // The cluster's buttons are the OTHER entry point into the same router, and
  // capturing its hooks is the only way to press them from here: the real
  // cluster mounts on the first edit of the session.
  const clusterHooks: { current: Record<string, (...a: never[]) => unknown> | null } = {
    current: null,
  };
  const MockDocStateCluster = vi.fn().mockImplementation((hooks: Record<string, () => unknown>) => {
    clusterHooks.current = hooks;
    return { mount: vi.fn(), sync: vi.fn(), destroy: vi.fn(), get mounted() { return false; } };
  });
  const listenCallbacks = new Map<string, Function>();
  const mockEmit = vi.fn().mockResolvedValue(undefined);
  const mockCommands = {
    getSchema: vi.fn().mockResolvedValue({ status: 'ok', data: { fields: [] } }),
    getProjectPath: vi.fn().mockResolvedValue({ status: 'error', error: 'no project' }),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    getFileInfo: vi.fn().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn(),
  };
  const MockChipBar = vi.fn().mockImplementation(() => ({
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
    hasOpenEditOverlay: vi.fn().mockReturnValue(false),
    closeEditOverlay: vi.fn(),
  }));
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    // `hasFocus` is what excludes the CM6 body from the native route. The mock
    // used to omit it entirely, so the exclusion was constant-false and could
    // be deleted from the source with the suite still green.
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, hasFocus: false }),
  }));
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null),
    collapseForReopen: vi.fn(),
  }));
  return { mockEmit, mockCommands, listenCallbacks, MockChipBar, MockCmEditor, MockFileTree, history, clusterHooks, MockDocumentHistory, MockDocStateCluster };
});

vi.mock('../../engine', () => ({ engine: mockCommands }));
vi.mock('../chip-bar', () => ({ ChipBar: MockChipBar }));
vi.mock('../cm-editor', () => ({ CmEditor: MockCmEditor }));
vi.mock('../tree', () => ({ FileTree: MockFileTree }));
vi.mock('../file-viewer', () => ({ showFileViewer: vi.fn(), destroyFileViewer: vi.fn() }));
vi.mock('../document-history', async (orig) => ({
  ...(await orig<typeof import('../document-history')>()),
  DocumentHistory: MockDocumentHistory,
}));
vi.mock('../doc-state-cluster', () => ({ DocStateCluster: MockDocStateCluster }));

// ---------------------------------------------------------------------------
// DOM helper
// ---------------------------------------------------------------------------

function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div class="moss-editor-scroll">
      <div class="editor-heading" id="editor-heading" contenteditable="plaintext-only" spellcheck="false" data-empty="true"></div>
      <div id="chip-bar"></div>
      <div id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    </div>
    <div id="moss-save-indicator"></div>
  `;
}

// ---------------------------------------------------------------------------
// editorBootstrap delegate — routes through the existing getSchema /
// getProjectPath mocks so per-test overrides keep working unchanged.
// Struct fields are snake_case (specta does not camelCase struct fields);
// `initial_file: null` makes loadFile fall back to the mocked parseFrontmatter.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Helper to dispatch Cmd+key
// ---------------------------------------------------------------------------

function pressCmd(key: string): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key,
    metaKey: true,
    bubbles: true,
    cancelable: true,
  });
  document.dispatchEvent(event);
  return event;
}

/** The exact ratio string applyEditorFontScale writes for a preset px. */
function ratio(px: number): string {
  return String(Math.round((px / 13) * 10000) / 10000);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('editor webview keyboard shortcuts', () => {
  const scaleOf = () =>
    document.documentElement.style.getPropertyValue('--moss-editor-font-scale');

  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    document.documentElement.style.removeProperty('--moss-editor-font-scale');
    localStorage.clear();
    setupEditorDOM();

    // Reset mocks to default implementations
    listenCallbacks.clear();
    mockEmit.mockReset().mockResolvedValue(undefined);
    mockCommands.getSchema.mockReset().mockResolvedValue({ status: 'ok', data: { fields: [] } });
    mockCommands.getProjectPath.mockReset().mockResolvedValue({ status: 'error', error: 'no project' });
    mockCommands.editorBootstrap.mockReset();
    applyEditorBootstrapDelegate();
    mockCommands.parseFrontmatter.mockReset();
    mockCommands.getFileInfo.mockReset().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } });
    mockCommands.saveEditorContent.mockReset();
    mockCommands.resolveUrlForFile.mockReset();
    MockChipBar.mockImplementation(() => ({
      render: vi.fn(), destroy: vi.fn(), getValues: vi.fn().mockReturnValue({}), setPageFacts: vi.fn(),
    }));
    MockCmEditor.mockImplementation(() => ({
      destroy: vi.fn(), getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
    }));
    MockFileTree.mockImplementation(() => ({
      mount: vi.fn().mockResolvedValue(undefined), unmount: vi.fn(),
      setupDragHandle: vi.fn(), highlight: vi.fn(),
      getFirstFile: vi.fn().mockReturnValue(null),
      collapseForReopen: vi.fn(),
    }));
    // `vi.restoreAllMocks()` in afterEach strips implementations, so these are
    // re-established here exactly as the mocks above are.
    for (const fn of Object.values(history)) fn.mockClear();
    history.undo.mockReturnValue(true);
    history.redo.mockReturnValue(true);
    history.canUndo.mockReturnValue(true);
    history.canRedo.mockReturnValue(true);
    history.depths.mockReturnValue({ undo: 0, redo: 0 });
    MockDocumentHistory.mockReset().mockImplementation(() => history);
    clusterHooks.current = null;
    MockDocStateCluster.mockReset().mockImplementation((hooks: Record<string, () => unknown>) => {
      clusterHooks.current = hooks;
      return { mount: vi.fn(), sync: vi.fn(), destroy: vi.fn(), mounted: false };
    });
  });

  afterEach(() => {
    // Fire beforeunload to clean up module-level event listeners (AbortController)
    window.dispatchEvent(new Event('beforeunload'));
    vi.restoreAllMocks();
  });

  async function boot(): Promise<void> {
    await bootEditorMain();
    await vi.waitFor(() => {
      expect(listenCallbacks.has('editor:load-source')).toBe(true);
    });
  }

  test('Cmd+= steps 13px → 14px: persists and applies the scale ratio', async () => {
    await boot();

    pressCmd('=');
    expect(localStorage.getItem('moss-editor-font-size')).toBe('14');
    expect(scaleOf()).toBe(ratio(14));
  });

  test('Cmd+- steps 13px → 12px', async () => {
    await boot();

    pressCmd('-');
    expect(localStorage.getItem('moss-editor-font-size')).toBe('12');
    expect(scaleOf()).toBe(ratio(12));
  });

  test('Cmd+0 resets: clears the key and removes the inline property', async () => {
    localStorage.setItem('moss-editor-font-size', '16');

    await boot();

    pressCmd('0');
    expect(localStorage.getItem('moss-editor-font-size')).toBeNull();
    expect(scaleOf()).toBe('');
  });

  test('clamps at minimum (11px)', async () => {
    localStorage.setItem('moss-editor-font-size', '11');

    await boot();

    pressCmd('-');
    expect(localStorage.getItem('moss-editor-font-size')).toBe('11');
  });

  test('clamps at maximum (18px)', async () => {
    localStorage.setItem('moss-editor-font-size', '18');

    await boot();

    pressCmd('=');
    expect(localStorage.getItem('moss-editor-font-size')).toBe('18');
  });

  test('restores from localStorage on init (old body-px values stay valid)', async () => {
    localStorage.setItem('moss-editor-font-size', '15');

    await boot();

    expect(scaleOf()).toBe(ratio(15));
  });

  test('broadcasts every change so the settings stepper follows', async () => {
    await boot();

    pressCmd('=');
    expect(mockEmit).toHaveBeenCalledWith('moss:editor-font-size', { px: 14 });
  });

  test('follows a change broadcast by the shell stepper without re-emitting', async () => {
    await boot();

    const cb = listenCallbacks.get('moss:editor-font-size');
    expect(cb).toBeDefined();
    cb!({ px: 16 });

    expect(localStorage.getItem('moss-editor-font-size')).toBe('16');
    expect(scaleOf()).toBe(ratio(16));
    expect(mockEmit).not.toHaveBeenCalledWith('moss:editor-font-size', expect.anything());
  });

  test('preventDefault called on Cmd+= to prevent browser zoom', async () => {
    await boot();

    const event = new KeyboardEvent('keydown', {
      key: '=',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    const preventSpy = vi.spyOn(event, 'preventDefault');
    document.dispatchEvent(event);
    expect(preventSpy).toHaveBeenCalled();
  });

  // ⌘, from the editor. Without this binding the key did nothing at all in
  // full-width mode: the shell webview is hidden there, so its own ⌘, never
  // fires. The relay is the same event the ⋯ "Settings…" row emits — the
  // shell owns the modal.
  test('Cmd+, relays to the shell instead of dying in the editor webview', async () => {
    await boot();
    mockEmit.mockClear();

    const event = new KeyboardEvent('keydown', {
      key: ',', metaKey: true, bubbles: true, cancelable: true,
    });
    const preventSpy = vi.spyOn(event, 'preventDefault');
    document.dispatchEvent(event);

    expect(mockEmit).toHaveBeenCalledWith('editor:open-settings', {});
    expect(preventSpy).toHaveBeenCalled();
  });

  // ── Undo / Redo ─────────────────────────────────────────────────────────
  // The CM6 keymap in cm-editor.ts only sees keys whose event path runs through
  // its contentDOM, and moss's Edit menu is macOS-only — so on Linux and
  // Windows Ctrl+Z with focus anywhere but the body reached NOTHING. It was
  // invisible to the suite because the one test of the decision
  // (history-routing.test.ts) calls the pure function directly, and nothing
  // exercised the adapter that has to reach it. jsdom reports a Linux platform,
  // which is exactly the platform that was broken.
  const pressKey = (init: KeyboardEventInit) => {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    const spy = vi.spyOn(event, 'preventDefault');
    document.dispatchEvent(event);
    return spy;
  };

  test('Ctrl+Z is answered with focus on nothing at all — and runs UNDO', async () => {
    await boot();
    expect(pressKey({ key: 'z', ctrlKey: true })).toHaveBeenCalled();
    expect(history.undo).toHaveBeenCalledTimes(1);
    expect(history.redo).not.toHaveBeenCalled();
  });

  test('Ctrl+Shift+Z and Ctrl+Y both reach redo — and neither reaches undo', async () => {
    await boot();
    expect(pressKey({ key: 'Z', ctrlKey: true, shiftKey: true })).toHaveBeenCalled();
    expect(pressKey({ key: 'y', ctrlKey: true })).toHaveBeenCalled();
    expect(history.redo).toHaveBeenCalledTimes(2);
    expect(history.undo).not.toHaveBeenCalled();
  });

  test('Ctrl+Y is a redo here, but Cmd+Y is not one on macOS', async () => {
    // Ctrl+Y is the Windows/Linux redo convention. ⌘Y is nobody's redo — the
    // Edit menu, which is the platform's own statement of what the shortcuts
    // are, does not list it — so claiming it would swallow a key some other
    // surface may want, with no tooltip anywhere admitting we took it.
    await boot();
    expect(pressKey({ key: 'y', ctrlKey: true })).toHaveBeenCalled();
    expect(history.redo).toHaveBeenCalledTimes(1);

    const original = Object.getOwnPropertyDescriptor(navigator, 'platform');
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
    try {
      expect(pressKey({ key: 'y', metaKey: true })).not.toHaveBeenCalled();
      expect(history.redo).toHaveBeenCalledTimes(1);
      // …while ⌘Z still is undo there, so the platform switch is real and the
      // assertion above is not passing because nothing matched at all.
      expect(pressKey({ key: 'z', metaKey: true })).toHaveBeenCalled();
      expect(history.undo).toHaveBeenCalledTimes(1);
    } finally {
      // `platform` lives on the prototype in jsdom, so there is no own
      // descriptor to put back — restoring only when one existed leaves the
      // override in place and every LATER test in this file runs as macOS.
      if (original) Object.defineProperty(navigator, 'platform', original);
      else delete (navigator as unknown as Record<string, unknown>).platform;
    }
  });

  test('a non-Latin layout reaches undo too', async () => {
    // ⌃Z on a Russian layout reports `e.key === 'я'`. CM6's keymap has a
    // keyCode fallback for exactly this, so without a matching one here the
    // shortcut worked inside the editor body and nowhere else in the pane.
    await boot();
    expect(pressKey({ key: 'я', ctrlKey: true, keyCode: 90 })).toHaveBeenCalled();
    expect(history.undo).toHaveBeenCalledTimes(1);
  });

  test('mid-IME-composition the key is left to the IME', async () => {
    await boot();
    expect(pressKey({ key: 'z', ctrlKey: true, isComposing: true })).not.toHaveBeenCalled();
    expect(history.undo).not.toHaveBeenCalled();
  });

  // The CM6-body exclusion is NOT tested here: this fixture boots with
  // `initial_file: null`, so no CmEditor is ever constructed and `hasFocus` is
  // unreachable. It is a table case in history-routing.test.ts instead, against
  // `computeHistoryInputs` — which is why that mapping was extracted from this
  // file's blind spot in the first place.

  test('the buttons route exactly as the key does, and serve the fall-through themselves', async () => {
    // A button click has no event to leave un-prevented, so the `native` route
    // has to be performed explicitly. A button that walked the log
    // unconditionally would disagree with the key carrying its own tooltip.
    await boot();
    const hooks = clusterHooks.current!;
    expect(hooks).toBeTruthy();

    hooks.undo();
    expect(history.undo).toHaveBeenCalledTimes(1);
    hooks.redo();
    expect(history.redo).toHaveBeenCalledTimes(1);

    const exec = vi.fn().mockReturnValue(true);
    (document as unknown as { execCommand: unknown }).execCommand = exec;
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    try {
      hooks.undo();
      // Still one: the focused field owns this, and the button says so by
      // running the browser's undo rather than the document log's.
      expect(history.undo).toHaveBeenCalledTimes(1);
      expect(exec).toHaveBeenCalledWith('undo');
    } finally {
      input.remove();
    }
  });

  test('a bare z is left alone', async () => {
    await boot();
    expect(pressKey({ key: 'z' })).not.toHaveBeenCalled();
  });

  test('a focused text field keeps its own history', async () => {
    await boot();
    // Route 'native': the field's edits were typed, so the browser kept them.
    // Preventing the event here would strand the user with no undo at all in
    // every chip input.
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    try {
      expect(pressKey({ key: 'z', ctrlKey: true })).not.toHaveBeenCalled();
      expect(history.undo).not.toHaveBeenCalled();
    } finally {
      input.remove();
    }
  });

  test('a handler that already claimed the key is not answered twice', async () => {
    await boot();
    // CM6's keymap preventDefaults when it handles the key, and the event still
    // bubbles to document. Without the defaultPrevented guard the body's ⌘Z
    // would walk the history twice and undo two steps for one press.
    const event = new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true, cancelable: true });
    event.preventDefault();
    const spy = vi.spyOn(event, 'preventDefault');
    document.dispatchEvent(event);
    expect(spy).not.toHaveBeenCalled();
    expect(history.undo).not.toHaveBeenCalled();
  });
});
