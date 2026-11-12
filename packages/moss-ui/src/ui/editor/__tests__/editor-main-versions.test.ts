/**
 * Versions is a mode of the editor body, entered from the tree's context
 * menu in THIS webview.
 * Module state is per-webview, so the mode cannot be routed through the main
 * window's panel manager. What this file pins is the composition editor-main
 * owns: the tree's `editor:open-versions` request mounts the surface into
 * #moss-editor-cm with no navigation command and no cross-webview event, and
 * every exit — "‹ Editor", a restore, or another tree entry being opened —
 * puts a document back through the ordinary in-webview load path.
 *
 * Entering hides the nav surface (tree, create button) via
 * `hideChrome`/`showChrome` on `VersionsModeHost`, and every exit restores it,
 * since leaving this mode never reloads the page. The breadcrumb stays up
 * The SURFACE builds
 * a title bar (versions-surface.ts, real here) and hands it to
 * `FileTree.setTitleBar` through `host.setTitleBar`/`ctx.setTitleBar`. This
 * file's FileTree is mocked wholesale (`vi.mock('../tree', …)`), so it never
 * builds the real `.nav-bc-strip__mode` DOM; what it pins is the wiring —
 * `setTitleBar` called with a real element on entry, with `null` on exit, and
 * "‹ Editor" modeled by dispatching a click on the `mkBackCrumb` element
 * inside that content. The real mount/crossfade behavior against the genuine
 * `FileTree` is tree/__tests__/controller.test.ts's "FileTree.setTitleBar"
 * suite.
 *
 * Its own file because it needs the real versions-surface with the data
 * commands mocked on the engine, which editor-main.test.ts's shared engine
 * does not carry.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { editorBand } from '../editor-band';

const {
  mockEmit,
  mockCommands,
  MockChipBar,
  MockCmEditor,
  MockFileTree,
  mockOnWebviewDragDrop,
} = vi.hoisted(() => {
  const mockEmit = vi.fn().mockResolvedValue(undefined);
  const mockCommands = {
    getSchema: vi.fn().mockResolvedValue({ status: 'ok', data: { fields: [] } }),
    getProjectPath: vi.fn().mockResolvedValue({ status: 'ok', data: '/Users/test/blog' }),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { frontmatter: { title: 'Hello' }, body: '# Hello' },
    }),
    computeHeadingState: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { visible: false, text: '', source: 'Filename' },
    }),
    getFileInfo: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { size: '0', modified: null, width: null, height: null },
    }),
    describeSourceRole: vi.fn().mockResolvedValue({ status: 'ok', data: { slot: null, language: null } }),
    pageFacts: vi.fn().mockResolvedValue({ status: 'ok', data: { position: null, auto: {} } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    resolveExpectedUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    copyFilesToProject: vi.fn(),
    closeActionPanel: vi.fn(),
    listVersions: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        rows: [{
          id: 'v1',
          published_at: new Date(2026, 8, 13, 14, 2).toISOString(),
          target: 'moss',
          trigger: 'publish',
          label: null,
          changed: 0,
          added: 0,
          removed: 0,
        }],
        live_id: 'v1',
        has_multiple_targets: false,
        store_dir: '/tmp/store',
        changed_since: [],
        unchanged: [],
      },
    }),
    readVersion: vi.fn().mockResolvedValue({ status: 'ok', data: { kept: true, text: '# Older' } }),
    restoreVersion: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { restored: ['content/posts/hello.md'], failed: [], trashed: [], undo_id: 'undo-1' },
    }),
  };
  const mockOnWebviewDragDrop = vi.fn().mockResolvedValue(vi.fn());
  const MockChipBar = vi.fn().mockImplementation(() => ({
    setPageFacts: vi.fn(),
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
  }));
  const MockCmEditor = vi.fn().mockImplementation(() => {
    // The real CmEditor creates `.editor-band` as a side effect of wiring the
    // insert bar for a markdown file (cm-insert-bar.ts's constructor calls
    // editorBand()) — reproduced here since CmEditor itself is mocked out
    // wholesale in this file, and "‹ Editor" reopening the source through the
    // ordinary load path still needs the band to exist.
    editorBand();
    return {
      destroy: vi.fn(),
      getContent: vi.fn().mockReturnValue(''),
      getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() }, focus: vi.fn() }),
    };
  });
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    clearSelection: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue('content/posts/hello.md'),
    collapse: vi.fn(),
    collapseForReopen: vi.fn(),
    isEditing: vi.fn().mockReturnValue(false),
    setSealed: vi.fn(),
    // Round six replaces the breadcrumb wholesale with a title bar the
    // surface builds
    // through this method; this mock never builds the real
    // `.nav-bc-strip__mode` DOM (the tree module is mocked wholesale here),
    // so the tests below can only assert the WIRING — that editor-main calls
    // it with a real element on entry and with null on exit. The real
    // mount/crossfade behavior is covered against the genuine FileTree in
    // tree/__tests__/controller.test.ts's "FileTree.setTitleBar" suite.
    setTitleBar: vi.fn(),
  }));
  return { mockEmit, mockCommands, MockChipBar, MockCmEditor, MockFileTree, mockOnWebviewDragDrop };
});

vi.mock('../../engine', () => ({ engine: mockCommands }));
vi.mock('../chip-bar', () => ({ ChipBar: MockChipBar }));
vi.mock('../cm-editor', () => ({ CmEditor: MockCmEditor }));
vi.mock('../file-viewer');
// FileTree is mocked; mkBackCrumb/mkTitleBarRow stay REAL (versions-surface.ts
// imports them from this same module, tree/index.ts's public surface) — the
// real DOM those two build is what the "‹ Editor"/restore tests below dispatch
// clicks on.
vi.mock('../tree', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../tree')>()),
  FileTree: MockFileTree,
}));

mockCommands.editorBootstrap.mockImplementation(async () => ({
  status: 'ok',
  data: { schema: { fields: [] }, project_path: '/Users/test/blog', initial_file: null, root_name: 'blog' },
}));

const FILE = 'content/posts/hello.md';
const cm = () => document.getElementById('moss-editor-cm')!;

/** Mirrors editor.html's nav-surface anatomy closely enough for the
 *  hide/show assertions below: `#nav-grid` (where the — mocked — FileTree
 *  would build `.nav-bc-strip`/`.nav-body`) plus its `#nav-right`/
 *  `#nav-caption` siblings, the two ids `hideEditorChromeForSurface` now
 *  targets directly (it no longer hides
 *  `#nav-surface` as one unit, which this harness's bare markup would have
 *  hidden even without these siblings existing). Round five (item 3):
 *  `#nav-right`/`#nav-caption` toggle `.is-chrome-faded` (opacity, editor.css)
 *  rather than `display: none` — real `.nav-body` is never built here
 *  (FileTree is mocked below), so this harness cannot exercise that half —
 *  see tree/__tests__/controller.test.ts for coverage against the genuine
 *  tree. */
function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div id="nav-surface">
      <div id="nav-grid"><div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div></div>
      <div id="nav-right"></div>
      <div id="nav-caption"></div>
    </div>
    <div id="divider"></div>
    <div class="moss-editor-scroll">
      <div id="chip-bar"></div>
      <div class="editor-heading" id="editor-heading" data-empty="true"></div>
      <div class="moss-editor-cm" id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    </div>
    <div id="moss-save-indicator"></div>
    <div id="moss-toast-container"></div>
  `;
}

/** Boot the editor on FILE, the way open_editor bakes `?source=` into the URL. */
async function bootOnFile(): Promise<void> {
  window.history.replaceState(null, '', `/?source=${encodeURIComponent(FILE)}`);
  // As editor.html does: the host record, then the editor. A fresh host per boot;
  // the bus records emits.
  const { resetTestHost, recordingBus } = await import("../../__tests__/helpers/test-host");
  resetTestHost();
  const { host } = await import("../../host");
  host.bus = recordingBus(mockEmit);
  host.onFileDrop = mockOnWebviewDragDrop;
  await import('../editor-main');
  await vi.waitFor(() => expect(MockCmEditor).toHaveBeenCalledTimes(1));
}

function askForVersions(scope: 'page' | 'folder' | 'site', path: string, isRoot = false): void {
  document.dispatchEvent(new CustomEvent('editor:open-versions', { detail: { scope, path, isRoot } }));
}

/** A promise this test settles by hand, standing in for the in-flight
 *  `saveEditorContent` write two overlapping `flushPendingSave` calls race
 *  on (editor-main.ts ~2312-2343). */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
}

/** The most recently constructed mock FileTree — `MockFileTree.mock.results`
 *  accumulates across tests (nothing here clears it), so this always wants
 *  the LAST entry, from the current test's `bootOnFile()`. Typed loosely:
 *  it's a mock object, not the real `FileTree`. */
function latestFileTree(): { setTitleBar: ReturnType<typeof vi.fn> } {
  const results = MockFileTree.mock.results;
  return results[results.length - 1]!.value;
}

const drain = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe('Versions as an in-place mode of the editor body', () => {
  beforeEach(async () => {
    // Retire the previous test's module graph the way the editor webview
    // does: its document listeners (the Versions request included) are
    // bound to the abort signal beforeunload fires, so they never answer a
    // dispatch meant for this test's graph.
    window.dispatchEvent(new Event('beforeunload'));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    vi.resetModules();
    setupEditorDOM();
    MockCmEditor.mockClear();
    mockCommands.parseFrontmatter.mockClear();
    mockCommands.listVersions.mockClear();
    mockCommands.closeActionPanel.mockClear();
    mockEmit.mockClear();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('"Versions…" on a file mounts the list under the heading, in this webview, with the tree hidden', async () => {
    await bootOnFile();

    askForVersions('page', FILE);
    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface')).toBe('versions'));
    await drain();

    // The list is here, fed by the data command through the platform seam …
    expect(cm().querySelector('.moss-versions')).not.toBeNull();
    expect(mockCommands.listVersions).toHaveBeenCalledWith('page', FILE);
    // The subject name and "‹ Editor" no longer render inside the surface
    // They're the editor's own
    // breadcrumb now, which this harness's mocked FileTree never builds (see
    // tree/__tests__/controller.test.ts for that DOM). Only the scope
    // caption is still this surface's own markup to assert.
    expect(cm().querySelector('.moss-versions__caption')?.textContent).toBe('Versions');
    // … not through a webview navigation, and not through the main window.
    expect(mockCommands.closeActionPanel).not.toHaveBeenCalled();
    expect(mockEmit.mock.calls.map((c) => c[0])).not.toContain('editor:versions-exit');
    // The tree and the create button are gone; the breadcrumb strip is NOT
    // among them.
    // nav-right/nav-caption FADE
    // (round five, item 3: `.is-chrome-faded`, editor.css) rather than
    // `display: none`, so the strip's own width allocation stays untouched.
    expect(document.getElementById('nav-right')!.classList.contains('is-chrome-faded')).toBe(true);
    expect(document.getElementById('nav-caption')!.classList.contains('is-chrome-faded')).toBe(true);
    // FileTree.setTitleBar(el) is how the breadcrumb is replaced with the
    // "‹ Editor" + subject title bar (round six) — this harness's mock can
    // only confirm the WIRING (called with a real element the surface
    // built), not the mount/crossfade behavior itself.
    expect(latestFileTree().setTitleBar).toHaveBeenLastCalledWith(expect.any(HTMLElement));
  });

  test('"Folder versions…" on a subfolder passes the folder path through, never null', async () => {
    await bootOnFile();

    askForVersions('folder', 'content/posts');
    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface')).toBe('versions'));
    await drain();

    // A non-page scope keeps its path rather than always
    // asking for the whole site.
    expect(mockCommands.listVersions).toHaveBeenCalledWith('folder', 'content/posts');
    expect(cm().querySelector('.moss-versions__caption')?.textContent).toBe('Versions');
  });

  test('"Site versions…" from the root heads the list with the site name', async () => {
    await bootOnFile();

    askForVersions('site', '', true);
    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface')).toBe('versions'));
    await drain();

    expect(mockCommands.listVersions).toHaveBeenCalledWith('site', '');
    expect(cm().querySelector('.moss-versions__caption')?.textContent).toBe('Site versions');
  });

  test('"‹ Editor" (the title bar\'s back crumb) reopens the source through the ordinary load path', async () => {
    await bootOnFile();
    askForVersions('page', FILE);
    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface')).toBe('versions'));
    mockCommands.parseFrontmatter.mockClear();

    // No `.moss-versions__back` on screen one any more (round six) — the
    // exit is the title bar's own back crumb (mkBackCrumb, tree/title-bar.ts),
    // reached through `FileTree.setTitleBar` (mocked here; the real
    // mount/crossfade behavior is covered in tree/__tests__/controller.test.ts's
    // "FileTree.setTitleBar" suite). Simulate the click on the real element
    // the real versions-surface.ts built and handed to the mock.
    const content = latestFileTree().setTitleBar.mock.calls.at(-1)![0] as HTMLElement;
    content.querySelector<HTMLElement>('.seg.bc')!.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    await vi.waitFor(() => expect(cm().hasAttribute('data-editor-surface')).toBe(false));
    expect(mockCommands.parseFrontmatter).toHaveBeenCalledWith(`/Users/test/blog/${FILE}`);
    expect(cm().querySelector('.moss-versions')).toBeNull();
    expect(cm().querySelector('.editor-band')).not.toBeNull();
    // The tree comes back — leaving never reloads the page, so nothing else
    // would undo the hide.
    expect(document.getElementById('nav-right')!.classList.contains('is-chrome-faded')).toBe(false);
    expect(document.getElementById('nav-caption')!.classList.contains('is-chrome-faded')).toBe(false);
    // And the breadcrumb releases the title bar back.
    expect(latestFileTree().setTitleBar).toHaveBeenLastCalledWith(null);
  });

  test('a restore returns to the source and shows the Undo toast in this webview', async () => {
    await bootOnFile();
    askForVersions('page', FILE);
    await vi.waitFor(() => expect(cm().querySelector('[data-id="v1"]')).not.toBeNull());

    cm().querySelector<HTMLElement>('[data-id="v1"]')!.click();
    // Restore lives in the title bar now (round six), not in this surface's
    // own DOM under #moss-editor-cm — it's whatever content the detail
    // screen most recently handed to the mocked FileTree.setTitleBar.
    await vi.waitFor(() => {
      const content = latestFileTree().setTitleBar.mock.calls.at(-1)?.[0] as HTMLElement | undefined;
      expect(content?.querySelector('.moss-versions__restore')).not.toBeNull();
    });
    const content = latestFileTree().setTitleBar.mock.calls.at(-1)![0] as HTMLElement;
    content.querySelector<HTMLButtonElement>('.moss-versions__restore')!.click();

    await vi.waitFor(() => expect(cm().hasAttribute('data-editor-surface')).toBe(false));
    expect(mockCommands.restoreVersion).toHaveBeenCalledWith('v1', FILE, 'in_place');
    expect(mockCommands.parseFrontmatter).toHaveBeenLastCalledWith(`/Users/test/blog/${FILE}`);
    expect(cm().querySelector('.editor-band')).not.toBeNull();
    expect(document.getElementById('nav-right')!.classList.contains('is-chrome-faded')).toBe(false);
    expect(document.getElementById('nav-caption')!.classList.contains('is-chrome-faded')).toBe(false);
    const toast = document.querySelector('#moss-toast-container .moss-toast');
    expect(toast?.textContent).toContain('Restored hello.md');
    expect(toast?.textContent).toContain('Undo');
  });

  test('opening another tree entry leaves Versions mode through the normal surface switch', async () => {
    await bootOnFile();
    askForVersions('page', FILE);
    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface')).toBe('versions'));

    document.dispatchEvent(new CustomEvent('editor:file-selected', {
      bubbles: true,
      detail: { filePath: 'content/posts/world.md' },
    }));

    await vi.waitFor(() => expect(cm().hasAttribute('data-editor-surface')).toBe(false));
    expect(mockCommands.parseFrontmatter).toHaveBeenLastCalledWith('/Users/test/blog/content/posts/world.md');
    expect(cm().querySelector('.moss-versions')).toBeNull();
    expect(cm().querySelector('.editor-band')).not.toBeNull();
    expect(document.getElementById('nav-right')!.classList.contains('is-chrome-faded')).toBe(false);
    expect(document.getElementById('nav-caption')!.classList.contains('is-chrome-faded')).toBe(false);
  });

  // versions-mode.ts's own `openGeneration` counter: two overlapping
  // "Versions…" requests race their `flushPendingSave` awaits out of order
  // (editor-main.ts's `flushPendingSave`, ~2312-2343 — the call that FORCES
  // the write awaits the wrapped `saveActiveDocument()` promise, one extra
  // microtask hop past the call that only sees the write already in flight
  // and awaits the raw `saveInFlight` promise directly), so without a guard
  // the SECOND request's mount finishes first and the FIRST clobbers it —
  // the user ends up looking at the earlier click's scope.
  test('a second "Versions…" request started before the first one flushes wins, not the first', async () => {
    await bootOnFile();

    const { promise: savePromise, resolve: resolveSave } = deferred<{ status: 'ok'; data: null }>();
    mockCommands.saveEditorContent.mockReturnValueOnce(savePromise);

    // Dirty the document the way a keystroke does — CmEditor is mocked, so
    // its onChange callback (which calls scheduleSave()) is invoked directly.
    const onChange = MockCmEditor.mock.calls[0]![0].onChange as () => void;
    onChange();

    askForVersions('page', FILE);
    askForVersions('site', '', true);
    resolveSave({ status: 'ok', data: null });

    await vi.waitFor(() => expect(cm().getAttribute('data-editor-surface-key')).toBe('site:'));
    expect(cm().querySelector('.moss-versions__caption')?.textContent).toBe('Site versions');
  });
});
