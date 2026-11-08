/**
 * Tests for the startCreate flow.
 *
 * Root cause: startCreate() installed the rename <input> inside
 * requestAnimationFrame(), which is throttled/suspended in an idle Tauri child
 * WKWebView. The fix replaces rAF with setTimeout so the callback fires
 * regardless of paint state.
 *
 * These tests assert:
 *   - startCreate does NOT install the input before timers flush (timer deferred)
 *   - startCreate DOES install the input after vi.runAllTimersAsync()
 *   - The input carries value 'untitled.md' and selection covering the stem
 *   - Pressing Enter calls commands.createFiles and dispatches editor:file-selected
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import type { TreeNode } from '@symbiosis-lab/moss-ui';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, childPaths, dir, file } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath, mockCreateFile, mockCreateFolder, mockInstantiateTemplate } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockCreateFile: vi.fn(),
  mockCreateFolder: vi.fn(),
  mockInstantiateTemplate: vi.fn(),
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
    createFiles: mockCreateFile,
    createFolder: mockCreateFolder,
    instantiateTemplate: mockInstantiateTemplate,
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/blog';

// Reach the private startCreate(folderPath, type) seam the context menu uses.
type CreateSeam = {
  startCreate(folderPath: string, type: 'file' | 'folder', opts?: { templateId?: string }): Promise<void>;
};
const startCreate = (ft: FileTree, folderPath: string, type: 'file' | 'folder', opts?: { templateId?: string }) =>
  priv<CreateSeam>(ft).startCreate(folderPath, type, opts);

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'blog' });
}

describe('startCreate: setTimeout, not rAF', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  test('no rename input exists in DOM before timers flush', () => {
    void startCreate(fileTree, '', 'file');
    // The input is installed inside setTimeout — it must NOT be present yet.
    const input = container.querySelector('input.moss-tree-rename-input');
    expect(input).toBeNull();
  });

  test('rename input appears after flushing all timers', async () => {
    void startCreate(fileTree, '', 'file');
    // Flush the setTimeout callback
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
  });

  test('input carries value "untitled.md"', async () => {
    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input?.value).toBe('untitled.md');
  });

  test('selection covers the stem (index 0..7 for "untitled" in "untitled.md")', async () => {
    // jsdom does not implement focus/selection side effects fully, but
    // setSelectionRange IS tracked on the value.
    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    // selectionStart = 0, selectionEnd = 'untitled'.length = 8
    // (the stem before the '.' in 'untitled.md')
    expect(input!.selectionStart).toBe(0);
    expect(input!.selectionEnd).toBe('untitled'.length);
  });

  test('pressing Enter calls commands.createFiles and dispatches editor:file-selected', async () => {
    const newPath = `${ROOT}/hello.md`;
    mockCreateFile.mockResolvedValueOnce({ status: 'ok', data: [newPath] });
    // After creating the file the tree refreshes — return an updated tree
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md', 'hello.md']) });

    const fileSelectedHandler = vi.fn();
    container.addEventListener('editor:file-selected', fileSelectedHandler);

    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();

    // Type a name and confirm
    input!.value = 'hello.md';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // Flush any async work (IPC + re-render)
    await vi.runAllTimersAsync();
    // Resolve all pending promises
    await Promise.resolve();
    await Promise.resolve();

    // toAbsolute('', ROOT) produces ROOT + '/' for the root folder — trailing slash
    // is expected and harmless (Rust fs normalises it). We verify the filename arg.
    expect(mockCreateFile).toHaveBeenCalledWith([{ dir: expect.stringContaining(ROOT), name: 'hello.md', frontmatter: {} }]);
    expect(fileSelectedHandler).toHaveBeenCalled();
    const ev = fileSelectedHandler.mock.calls[0][0] as CustomEvent;
    expect(ev.detail.filePath).toBe('hello.md');

    // After a successful create the edit session must be fully torn down:
    // createPending cleared (isEditing() false) and the breadcrumb must NOT
    // still end with the temp 'untitled.md' leaf (guards the orphan-leaf bug).
    expect(fileTree.isEditing()).toBe(false);
    const activePath = priv(fileTree).view.anchor;
    expect(activePath[activePath.length - 1]).not.toBe('untitled.md');
    expect(activePath.some(p => p.endsWith('untitled.md'))).toBe(false);
  });

  test('Escape cancels: input removed, createPending cleared, no orphan untitled breadcrumb', async () => {
    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();

    // Cancel via Escape
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();

    // Input gone, edit session cleared
    expect(container.querySelector('input.moss-tree-rename-input')).toBeNull();
    expect(fileTree.isEditing()).toBe(false);
    expect(mockCreateFile).not.toHaveBeenCalled();

    // No stale 'untitled.md' breadcrumb leaf left over (guards removeTempEntry's
    // activePath pop). The breadcrumb should not have an aria-current 'untitled' seg.
    const activePath = priv(fileTree).view.anchor;
    expect(activePath.some(p => p.endsWith('untitled.md'))).toBe(false);
    const orphan = container.querySelector('.nav-bc-strip .seg[data-full-name="untitled.md"]');
    expect(orphan).toBeNull();
  });

  test('a folder template commits through instantiateTemplate and opens the seeded home file', async () => {
    mockInstantiateTemplate.mockResolvedValueOnce({ status: 'ok', data: `${ROOT}/Guides/Guides.md` });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });

    const fileSelectedHandler = vi.fn();
    container.addEventListener('editor:file-selected', fileSelectedHandler);

    void startCreate(fileTree, '', 'folder', { templateId: 'tpl-folder' });
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = 'Guides';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockCreateFolder).not.toHaveBeenCalled();
    expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-folder', expect.stringContaining(ROOT), 'Guides');
    // The backend returns the seeded home file; a folder create alone would
    // only select the folder, but a template opens what it seeded.
    const ev = fileSelectedHandler.mock.calls[0][0] as CustomEvent;
    expect(ev.detail).toEqual({ filePath: 'Guides/Guides.md', isNewFile: true });
    expect(fileTree.isEditing()).toBe(false);
  });

  test('startCreate mounts the input into a seg carrying the ft-renaming class (BUG 3 class-parity with startRename)', async () => {
    // startRename widens its target seg by adding `ft-renaming`; startCreate must
    // do the same via the shared beginSegEdit() helper, or the create input stays
    // pinned to the narrow measured width. Assert class-parity here; the actual
    // pixel width is only observable under a real layout engine (see Playwright note).
    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    const seg = input!.closest('.seg');
    expect(seg).not.toBeNull();
    expect(seg!.classList.contains('ft-renaming')).toBe(true);
  });

  test('"already exists" keeps the input open with a visible localized error, and Enter retries', async () => {
    mockCreateFile.mockResolvedValueOnce({ status: 'error', error: `'${ROOT}/index.md' already exists` });

    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = 'index.md';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    // The failure is VISIBLE: input still mounted, localized collision error
    // shown inline, session still open (the old code console.error'd and the
    // finally block silently removed the typed name).
    expect(document.querySelector('input.moss-tree-rename-input')).not.toBeNull();
    const errorEl = document.querySelector('.moss-tree-error');
    expect(errorEl).not.toBeNull();
    expect(errorEl!.textContent).toContain('already exists');
    expect(errorEl!.textContent).toContain('index.md');
    expect(fileTree.isEditing()).toBe(true);

    // Typing clears the error…
    input!.value = 'hello';
    input!.dispatchEvent(new Event('input', { bubbles: true }));
    expect(document.querySelector('.moss-tree-error')).toBeNull();

    // …and Enter retries: this time the create succeeds and tears down.
    mockCreateFile.mockResolvedValueOnce({ status: 'ok', data: [`${ROOT}/hello.md`] });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md', 'hello.md']) });
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();
    expect(mockCreateFile).toHaveBeenCalledTimes(2);
    expect(fileTree.isEditing()).toBe(false);
    expect(document.querySelector('.moss-tree-error')).toBeNull();
  });

  test('A non-collision create failure is shown inline too, and Escape still cancels', async () => {
    mockCreateFile.mockResolvedValueOnce({ status: 'error', error: 'disk full' });

    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    input!.value = 'hello';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    const errorEl = document.querySelector('.moss-tree-error');
    expect(errorEl).not.toBeNull();
    expect(errorEl!.textContent).toContain('disk full');
    expect(fileTree.isEditing()).toBe(true);

    // Escape after a failure cancels the whole session and removes the error.
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.runAllTimersAsync();
    expect(fileTree.isEditing()).toBe(false);
    expect(document.querySelector('.moss-tree-error')).toBeNull();
    expect(document.querySelector('input.moss-tree-rename-input')).toBeNull();
  });

  test('root-level create builds a domain-legal temp path (no leading slash)', async () => {
    // Concatenating `'' + '/' + 'untitled.md'` minted
    // '/untitled.md' at the root — outside the relative-path domain
    // (guardRelative throws on exactly this shape; toAbsolute treats it as
    // already absolute). Same fix class as rename's siblingPath — see the
    // comment above startRename's finish().
    void startCreate(fileTree, '', 'file');

    const activePath = priv(fileTree).view.anchor;
    const leaf = activePath[activePath.length - 1];
    expect(leaf).toBe('untitled.md');
    expect(activePath.some(p => p.startsWith('/'))).toBe(false);

    // The temp dirIndex entry carries the same domain-legal path.
    const rootEntries = childPaths(fileTree, '') ?? [];
    expect(rootEntries).toContain('untitled.md');
    expect(rootEntries).not.toContain('/untitled.md');

    // The deferred mount finds the temp seg by that path, and Escape tears
    // the session down cleanly (removeTempEntry pops the same path it pushed).
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.runAllTimersAsync();
    expect(fileTree.isEditing()).toBe(false);
    const after = childPaths(fileTree, '') ?? [];
    expect(after.some(p => p.endsWith('untitled.md'))).toBe(false);
  });

  test('a create beside a real untitled.md takes the next free placeholder and leaves the real file alone', async () => {
    // The temp row is a registry entry for the session, and the registry is
    // keyed by path: sharing a real file's path would hand the session that
    // file's id, and cancelling would retire it. So the placeholder steps to
    // the first free name instead, Finder-style.
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'untitled.md']) });
    fileTree.unmount();
    container.remove();

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);

    const realId = registry.getByPath('untitled.md');
    expect(realId).not.toBeNull();

    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    expect(input!.value).toBe('untitled 2.md');
    expect(registry.getByPath('untitled 2.md')).not.toBeNull();
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.runAllTimersAsync();

    // The real file keeps its identity; the placeholder is gone everywhere.
    expect(registry.getByPath('untitled.md')).toBe(realId);
    expect(registry.getByPath('untitled 2.md')).toBeNull();
    expect(childPaths(fileTree, '')).toEqual(['index.md', 'untitled.md']);
  });

  test('collision: a real untitled.md already in the folder is NOT stolen by the rename input', async () => {
    // Tree where the folder already has a real untitled.md
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'untitled.md']) });
    fileTree.unmount();
    container.remove();

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);

    void startCreate(fileTree, '', 'file');
    await vi.runAllTimersAsync();

    // The rename input must attach to the TEMP entry's breadcrumb seg (the
    // activePath leaf — the free placeholder, not the existing file's path),
    // not the existing file's body row. Exactly one input exists, and it sits
    // inside .nav-bc-strip (where the temp leaf renders), not the .nav-body
    // (where the real file renders).
    const inputs = container.querySelectorAll('input.moss-tree-rename-input');
    expect(inputs.length).toBe(1);
    const input = inputs[0] as HTMLInputElement;
    expect(input.value).toBe('untitled 2.md');
    expect(input.closest('.nav-bc-strip')).not.toBeNull();
    expect(input.closest('.nav-body')).toBeNull();
  });
});

/**
 * BUG 2 — Creating a New Folder must not blank/break the tree.
 *
 * Regression: startCreate.finish() gated BOTH the activePath update and the
 * post-create navigation behind `if (type === 'file')`. For a folder create,
 * neither ran, so removeTempEntry() popped the temp leaf back off activePath,
 * collapsing it to [''] (root-only) → spineBody rendered ZERO body rows → the
 * tree went blank and unexpandable.
 *
 * Correct behavior (mirrors applyDeleteLocally's no-home-file branch): after
 * refreshTree, unconditionally set activePath = buildActivePath(createdPath) so
 * the NEW folder is the active leaf, dispatch `editor:folder-navigate` with the
 * ABSOLUTE folderPath + folderName so the editor/preview show the empty folder
 * page, and selectFolder(createdPath) with NO toggle.
 */
describe('startCreate: BUG 2 — folder create selects the new folder, no blank tree', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;  const rootTree = (children: TreeNode[]) => dir(ROOT, children, { name: 'blog' });

  async function mountWith(children: TreeNode[]) {
    mockListTree.mockResolvedValue({ status: 'ok', data: rootTree(children) });
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
  }

  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  test('ROOT folder create: activePath becomes the new folder (length >= 2), folder-navigate fires with absolute path, no dangling temp leaf', async () => {
    await mountWith([file(`${ROOT}/index.md`)]);
    fileTree.highlight(`${ROOT}/index.md`);

    const createdAbs = `${ROOT}/newfolder`;
    mockCreateFolder.mockResolvedValueOnce({ status: 'ok', data: createdAbs });
    // After creating the folder the tree refreshes — return a tree that now
    // contains the new (empty) folder so refreshTree/selectFolder see it.
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: rootTree([file(`${ROOT}/index.md`), dir(createdAbs, [])]),
    });

    const folderNav = vi.fn();
    container.addEventListener('editor:folder-navigate', folderNav);

    await startCreate(fileTree, '', 'folder');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = 'newfolder';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    // (a) createFolder called with the folder name; edit session torn down.
    expect(mockCreateFolder).toHaveBeenCalledWith(expect.stringContaining(ROOT), 'newfolder');
    expect(fileTree.isEditing()).toBe(false);

    // (b) activePath is the NEW folder as the active leaf — length >= 2 (root + folder),
    //     NOT collapsed to [''] (the blank-tree symptom).
    const activePath = priv(fileTree).view.anchor;
    expect(activePath).toEqual(['', 'newfolder']);
    expect(activePath.length).toBeGreaterThanOrEqual(2);

    // (c) editor:folder-navigate fired with ABSOLUTE folderPath + folderName.
    expect(folderNav).toHaveBeenCalled();
    const ev = folderNav.mock.calls[0][0] as CustomEvent;
    expect(ev.detail.folderPath).toBe(createdAbs);
    expect(ev.detail.folderName).toBe('newfolder');
    // …and says the folder is brand new, so the shell covers the 404 the
    // preview is about to land on rather than showing it. The delete-fallback
    // shares this event and leaves the flag off.
    expect(ev.detail.justCreated).toBe(true);

    // (d) No dangling temp 'untitled' leaf left on activePath.
    expect(activePath.some(p => p === '/untitled')).toBe(false);
    expect(activePath[activePath.length - 1]).toBe('newfolder');
  });

  test('SUBFOLDER folder create: activePath is the nested new folder, folder-navigate fires with absolute path', async () => {
    const worksAbs = `${ROOT}/works`;
    await mountWith([dir(worksAbs, [file(`${worksAbs}/post.md`)])]);

    const createdAbs = `${worksAbs}/sub`;
    mockCreateFolder.mockResolvedValueOnce({ status: 'ok', data: createdAbs });
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: rootTree([
        dir(worksAbs, [
          file(`${worksAbs}/post.md`),
          dir(createdAbs, []),
        ]),
      ]),
    });

    const folderNav = vi.fn();
    container.addEventListener('editor:folder-navigate', folderNav);

    await startCreate(fileTree, 'works', 'folder');
    await vi.runAllTimersAsync();

    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = 'sub';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockCreateFolder).toHaveBeenCalledWith(expect.stringContaining('works'), 'sub');
    expect(fileTree.isEditing()).toBe(false);

    const activePath = priv(fileTree).view.anchor;
    expect(activePath).toEqual(['', 'works', 'works/sub']);

    expect(folderNav).toHaveBeenCalled();
    const ev = folderNav.mock.calls[0][0] as CustomEvent;
    expect(ev.detail.folderPath).toBe(createdAbs);
    expect(ev.detail.folderName).toBe('sub');

    // Temp leaf ('works/untitled') must not dangle.
    expect(activePath.some(p => p === 'works/untitled')).toBe(false);
  });

  // A row the tree draws is an entry the registry knows, on disk or not — the
  // temp row is a folder for every id-keyed lookup (isDirectory, createAnchor)
  // for exactly as long as its session is open.
  test('a folder create session registers the temp row, and cancelling retires it', async () => {
    await mountWith([dir(`${ROOT}/works`, [])]);
    void startCreate(fileTree, 'works', 'folder');
    await vi.runAllTimersAsync();

    const tempId = registry.getByPath('works/untitled');
    expect(tempId).not.toBeNull();
    expect(registry.getById(tempId!)?.isDir).toBe(true);

    const input = container.querySelector<HTMLInputElement>('input');
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.runAllTimersAsync();
    expect(registry.getByPath('works/untitled')).toBeNull();
  });

  test('a folder create that keeps the default name survives its own teardown', async () => {
    // The temp row and the real folder share a path. The teardown that
    // retires the temp row must leave disk's row — in the registry AND in
    // dirIndex — alone.
    await mountWith([dir(`${ROOT}/works`, [])]);
    mockCreateFolder.mockResolvedValueOnce({ status: 'ok', data: `${ROOT}/works/untitled` });
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: rootTree([dir(`${ROOT}/works`, [dir(`${ROOT}/works/untitled`, [])])]),
    });

    void startCreate(fileTree, 'works', 'folder');
    await vi.runAllTimersAsync();
    const input = container.querySelector<HTMLInputElement>('input');
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    expect(fileTree.isEditing()).toBe(false);
    const id = registry.getByPath('works/untitled');
    expect(id).not.toBeNull();
    expect(registry.getById(id!)?.isDir).toBe(true);
    expect(childPaths(fileTree, 'works')).toContain('works/untitled');
  });
});
