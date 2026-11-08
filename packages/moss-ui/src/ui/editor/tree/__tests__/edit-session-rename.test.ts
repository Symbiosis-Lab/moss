/**
 * Integration tests for the in-app rename (tree/edit-session.ts), driven
 * through the mounted controller: what the registry, the active id and the
 * breadcrumb anchor look like after a rename — the invariants the
 * bug exposed — and, from the gesture itself, that the input comes down and
 * the row moves before the rename command returns.
 *
 * Three describe blocks:
 *   1. In-place rename (renameThroughTree seam → commitRename)
 *   2. Cross-directory rename through the same seam — the drag move is NOT
 *      this path (performDrop calls moveEntries and shares only the two
 *      lifecycle dispatches), so a cross-parent commitRename is covered here
 *   3. Canonical editor:rename-complete event dispatch
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, enterDirMode, priv, setView, startRenameByKey, overrideAt, childPaths, dir, file } from './test-helpers';
import { setFolder, type TreeView } from '../model';
import type { TreeRefresh } from '../refresh';

installPointerEventPolyfill();

const { mockListTree, mockGetProjectPath, mockRenameEntry } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockRenameEntry: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntry: mockRenameEntry,
    // Bundle H: commitRename now calls renameEntryWithRefs (drop-in).
    renameEntryWithRefs: mockRenameEntry,
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
}

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/Mira Vale';

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'Mira Vale' });
}

/** The `RenameRefsSummary` shape a held rename settles with on success —
 *  zero edits, so `settle({ status: 'ok', data: renameOkSummary })` never
 *  triggers the link-update toast unless a test overrides it. */
const renameOkSummary = { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' };

describe('in-app rename: registry & breadcrumb post-state (inline path)', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    // Simulate editor-main.ts:404 — the editor:file-renamed handler that
    // calls registry.rename. file-tree dispatches the event; editor-main
    // is the consumer. We attach the contract here so commitRename's
    // post-dispatch state assumptions actually hold in the test.
    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);

    // After mount, the registry should have all root-level files
    expect(registry.getByPath('index.md')).not.toBeNull();
    expect(registry.getByPath('about.md')).not.toBeNull();
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('renaming the active file leaves exactly one EntryId pointing at newPath', async () => {
    const indexId = registry.getByPath('index.md')!;
    fileTree.highlight(`${ROOT}/index.md`);

    // What listTree returns AFTER the rename — driven by refreshTree's call.
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['Mira Vale.md', 'about.md']) });

    // Drive the rename via the public test seam.
    await fileTree.renameThroughTree(`${ROOT}/index.md`, `${ROOT}/Mira Vale.md`);

    // Invariant 1: the original EntryId now points at the new path
    expect(registry.getById(indexId)?.path).toBe('Mira Vale.md');

    // Invariant 2: byPath has the new key, not the old
    expect(registry.getByPath('Mira Vale.md')).toBe(indexId);
    expect(registry.getByPath('index.md')).toBeNull();

    // Invariant 3: file-tree's activeEntryId resolves to a live record (NOT
    // dangling). This is the invariant that's broken before the fix.
    const active = fileTree.getActiveEntryId();
    expect(active).not.toBeNull();
    expect(registry.getById(active!)?.path).toBe('Mira Vale.md');

    // Invariant 4: activeEntryId is the SAME id as before — identity
    // preserved across rename (Zed's `ProjectEntryId` invariant; see
    // https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs)
    expect(active).toBe(indexId);
  });

  test('renaming a non-active file does not change the active file identity', async () => {
    const indexId = registry.getByPath('index.md')!;
    fileTree.highlight(`${ROOT}/index.md`);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about-renamed.md']) });
    await fileTree.renameThroughTree(`${ROOT}/about.md`, `${ROOT}/about-renamed.md`);

    // The active file's identity is unchanged
    expect(fileTree.getActiveEntryId()).toBe(indexId);
    expect(registry.getById(indexId)?.path).toBe('index.md');
  });
});

describe('in-app rename: breadcrumb anchor stays frozen', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md', 'notes.md']) });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);

    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('renaming a NON-anchor file while the anchor is diverged does NOT move the breadcrumb', async () => {
    // Diverge the anchor from the open file (frozen-anchor model): anchor stays
    // index.md while the OPEN file becomes about.md.
    fileTree.highlight(`${ROOT}/index.md`); // closed → anchor = index.md
    enterDirMode(fileTree);                 // open
    fileTree.highlight(`${ROOT}/about.md`);  // open → frozen: anchor stays index.md
    const anchorBefore = [...priv(fileTree).view.anchor];
    expect(anchorBefore[anchorBefore.length - 1]).toBe('index.md');

    // Rename a THIRD, unrelated file (neither the anchor nor the open file).
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md', 'notes-renamed.md']) });
    await fileTree.renameThroughTree(`${ROOT}/notes.md`, `${ROOT}/notes-renamed.md`);

    // The breadcrumb must NOT be yanked onto the open file (about.md) — it stays
    // frozen at the anchor. (Old code rebuilt activePath from currentFilePath.)
    expect(priv(fileTree).view.anchor).toEqual(anchorBefore);
  });

  test('renaming the ANCHOR file DOES update the breadcrumb to its new name', async () => {
    fileTree.highlight(`${ROOT}/index.md`);
    enterDirMode(fileTree);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index-renamed.md', 'about.md', 'notes.md']) });
    await fileTree.renameThroughTree(`${ROOT}/index.md`, `${ROOT}/index-renamed.md`);

    // The anchor WAS the renamed file → breadcrumb follows the new path (guards
    // against over-correcting to "never re-anchor").
    const anchor = priv(fileTree).view.anchor;
    expect(anchor[anchor.length - 1]).toBe('index-renamed.md');
  });
});

describe('in-app rename: registry post-state (cross-directory rename)', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: dir(ROOT, [
          file(`${ROOT}/index.md`),
          file(`${ROOT}/about.md`),
          dir(`${ROOT}/archive`, []),
        ], { name: 'Mira Vale' }),
    });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    // Simulate editor-main.ts:404 — the editor:file-renamed handler that
    // calls registry.rename. file-tree dispatches the event; editor-main
    // is the consumer. We attach the contract here so commitRename's
    // post-dispatch state assumptions actually hold in the test.
    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('cross-directory move of the active file preserves its EntryId at the new path', async () => {
    const indexId = registry.getByPath('index.md')!;
    fileTree.highlight(`${ROOT}/index.md`);

    // After the rename, the file is under archive/
    mockListTree.mockResolvedValue({
      status: 'ok',
      data: dir(ROOT, [
          file(`${ROOT}/about.md`),
          dir(`${ROOT}/archive`, [
              file(`${ROOT}/archive/index.md`),
            ]),
        ], { name: 'Mira Vale' }),
    });

    // Drive via the public seam: commitRename re-keys across parents exactly
    // as it does in place, and this is the one cross-parent caller.
    await fileTree.renameThroughTree(`${ROOT}/index.md`, `${ROOT}/archive/index.md`);

    expect(registry.getById(indexId)?.path).toBe('archive/index.md');
    expect(fileTree.getActiveEntryId()).toBe(indexId);
  });
});

describe('in-app rename: canonical editor:rename-complete event', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);

    // Simulate editor-main.ts:404 — the editor:file-renamed handler that
    // calls registry.rename. file-tree dispatches the event; editor-main
    // is the consumer. We attach the contract here so commitRename's
    // post-dispatch state assumptions actually hold in the test.
    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('dispatches editor:rename-complete with old/new paths when the renamed file is active', async () => {
    fileTree.highlight(`${ROOT}/index.md`);

    const handler = vi.fn();
    container.addEventListener('editor:rename-complete', handler as EventListener);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['Mira Vale.md', 'about.md']) });
    await fileTree.renameThroughTree(`${ROOT}/index.md`, `${ROOT}/Mira Vale.md`);

    // The event detail
    // carries the registry domain (project-root-relative).
    expect(handler).toHaveBeenCalledTimes(1);
    const ev = handler.mock.calls[0][0] as CustomEvent;
    expect(ev.detail).toEqual({
      oldPath: 'index.md',
      newPath: 'Mira Vale.md',
    });
  });

  test('event detail contract matches what the preview-navigate listener consumes', () => {
    // The editor-main listener (editor-main.ts:438) reads e.detail.newPath
    // and passes it to commands.resolveUrlForFile. Documenting that
    // contract here protects against accidental detail-shape regression
    // even though file-tree.commitRename is the sole emitter.
    const fakeEvent = new CustomEvent('editor:rename-complete', {
      detail: { oldPath: '/Users/me/Sites/blog/old.md', newPath: '/Users/me/Sites/blog/new.md' },
    });
    expect(fakeEvent.detail.newPath).toBe('/Users/me/Sites/blog/new.md');
    expect(typeof fakeEvent.detail.newPath).toBe('string');
  });

  test('does NOT dispatch editor:rename-complete when the renamed file is NOT active', async () => {
    // Highlight index.md; then rename about.md. The completion event should
    // not fire — it's scoped to active-file renames because that's what
    // triggers preview navigation.
    fileTree.highlight(`${ROOT}/index.md`);

    const handler = vi.fn();
    container.addEventListener('editor:rename-complete', handler as EventListener);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about-renamed.md']) });
    await fileTree.renameThroughTree(`${ROOT}/about.md`, `${ROOT}/about-renamed.md`);

    expect(handler).not.toHaveBeenCalled();
  });
});

/**
 * The rename IPC renames the file in its
 * first line and then walks the whole vault rewriting links to it. The tree
 * must not wait for that walk. These tests hold the command open (a promise
 * that never resolves) and assert the surface has ALREADY moved.
 *
 * This is a DOM-structure question with a controllable clock, so it belongs
 * in vitest — no engine, no computed style, no layout.
 */
describe('in-app rename: the tree moves before the IPC returns', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  const rowNames = () => Array.from(container.querySelectorAll<HTMLElement>('.seg[data-path]'))
    .map(el => el.dataset.path);

  /**
   * The REAL gesture: double-click the row, type, press Enter. Driving
   * `renameThroughTree` instead would skip `startRename` entirely — and
   * `startRename` is what sets `renamingPath`, the flag that parks render().
   * A test that goes through the seam therefore cannot see the very bug this
   * block exists to prevent (it shipped once exactly that way).
   */
  const renameViaInput = (path: string, typed: string): void => {
    const row = container.querySelector<HTMLElement>(`.seg[data-path="${path}"]`);
    expect(row).not.toBeNull();
    startRenameByKey(row!);
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = typed;
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  };

  /** A rename command the test holds open, then settles by hand. */
  const heldRename = () => {
    let settle!: (value: { status: string; error?: string; data?: unknown }) => void;
    mockRenameEntry.mockReturnValue(new Promise(r => { settle = r; }));
    return (value: { status: string; error?: string; data?: unknown }) => { settle(value); };
  };

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['index.md', 'about.md']) });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    container.addEventListener('editor:file-renamed', ((e: Event) => {
      const ev = e as CustomEvent<{ oldPath: string; newPath: string }>;
      registry.rename(ev.detail.oldPath, ev.detail.newPath);
    }) as EventListener);
    enterDirMode(fileTree);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('the row carries the new name while the rename command is still running', async () => {
    fileTree.highlight(`${ROOT}/about.md`);
    expect(rowNames()).toContain('about.md');
    heldRename(); // never settled

    renameViaInput('about.md', 'colophon');
    await Promise.resolve();
    await Promise.resolve();

    // The IPC has not returned and no refreshTree has run — listTree still
    // describes the OLD tree. The row moved anyway.
    expect(rowNames()).toContain('colophon.md');
    expect(rowNames()).not.toContain('about.md');
    // And the input is gone: Enter visibly confirmed.
    expect(container.querySelector('input.moss-tree-rename-input')).toBeNull();
    expect(registry.getByPath('colophon.md')).not.toBeNull();
  });

  test('a rename the filesystem refuses shows the new name, then puts the old one back', async () => {
    fileTree.highlight(`${ROOT}/about.md`);
    const aboutId = registry.getByPath('about.md')!;
    const settle = heldRename();

    renameViaInput('about.md', 'colophon');
    await Promise.resolve();
    await Promise.resolve();

    // Assert the prediction is ON SCREEN before the failure. Without this the
    // test passes with the optimistic update deleted — "put back" and "never
    // left" are indistinguishable once the promise has settled.
    expect(rowNames()).toContain('colophon.md');
    expect(registry.getByPath('colophon.md')).not.toBeNull();

    settle({ status: 'error', error: 'EACCES: permission denied' });
    await vi.waitFor(() => expect(rowNames()).toContain('about.md'));

    expect(rowNames()).not.toContain('colophon.md');
    // Identity survives the round trip: the same EntryId, back at its path.
    expect(registry.getByPath('about.md')).toBe(aboutId);
    expect(registry.getById(aboutId)?.path).toBe('about.md');
    expect(registry.getByPath('colophon.md')).toBeNull();
    // The failure is visible, not silent.
    expect(container.querySelector('.moss-tree-error')?.textContent).toMatch(/EACCES/);
  });

  test('a second rename cannot start while the first command is in flight', async () => {
    // Two renames dispatched together are two independent async commands with
    // nothing ordering them. If the second lands first it renames a path the
    // first has not created yet, fails, and is silently discarded while the
    // first succeeds. Holding renamingPath across the IPC used to make this
    // impossible for free; painting the prediction means releasing that flag
    // on Enter, so the serialization needs saying out loud.
    fileTree.highlight(`${ROOT}/about.md`);
    heldRename(); // never settles

    renameViaInput('about.md', 'colophon');
    await Promise.resolve();
    expect(rowNames()).toContain('colophon.md');
    expect(mockRenameEntry).toHaveBeenCalledTimes(1);

    // The row is right there under the cursor, already carrying its new name.
    const row = container.querySelector<HTMLElement>('.seg[data-path="colophon.md"]');
    expect(row).not.toBeNull();
    startRenameByKey(row!);

    expect(container.querySelector('input.moss-tree-rename-input')).toBeNull();
    expect(mockRenameEntry).toHaveBeenCalledTimes(1);
  });

  test('the file-renamed handler sees the tree BEFORE the re-render, on an in-place rename too', async () => {
    // editor-main's handler documents that it may not read tree state, because
    // it runs before the tree is rebuilt. That was true for a drag-move and
    // false for an in-place rename, which is worse than no rule: the next
    // author gets opposite answers from the same handler. The registry IS the
    // tree model now and the handler is what moves it, so what the tree still
    // owes the handler is the rest: rows and breadcrumb not yet re-keyed.
    fileTree.highlight(`${ROOT}/about.md`);
    let rowsAtDispatch: (string | undefined)[] = [];
    container.addEventListener('editor:file-renamed', () => {
      rowsAtDispatch = rowNames();
    });

    heldRename();
    renameViaInput('about.md', 'colophon');
    await Promise.resolve();

    // The handler ran, and at that moment the tree still described the old
    // layout — the re-key happens after the dispatch, not before it.
    expect(rowsAtDispatch).toContain('about.md');
    expect(rowsAtDispatch).not.toContain('colophon.md');
    // And by now the re-key HAS happened — the ordering is the only thing
    // under test, not whether it runs at all.
    expect(rowNames()).toContain('colophon.md');
  });

  test('finishing one rename does not disturb a rename started on another row', async () => {
    // The IPC is serialized, but the refreshTree that follows it is not: the
    // user can start renaming another row while it runs. When the first
    // rename's teardown finally fires it must touch nothing that is no longer
    // its own — neither the editing flag nor endSegEdit, which strips
    // `flex-shrink` from every breadcrumb segment and would narrow the input
    // someone is typing into. One ownership check guards both.
    fileTree.highlight(`${ROOT}/about.md`);
    let settleRefresh!: (v: unknown) => void;
    mockListTree.mockReturnValueOnce(new Promise(r => { settleRefresh = r; }));
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });

    renameViaInput('about.md', 'colophon');           // rename A
    await Promise.resolve();
    await Promise.resolve();
    expect(rowNames()).toContain('colophon.md');       // A's refreshTree is hanging

    const other = container.querySelector<HTMLElement>('.seg[data-path="index.md"]');
    startRenameByKey(other!);   // rename B
    const bInput = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(bInput).not.toBeNull();

    // A finishes.
    settleRefresh({ status: 'ok', data: makeTree(['index.md', 'colophon.md']) });
    // Let A's whole tail run — refreshTree, the rename-complete dispatch, and
    // the teardown in its finally.
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    // B is untouched: still editing, same input still mounted.
    expect(fileTree.isEditing()).toBe(true);
    expect(container.querySelector('input.moss-tree-rename-input')).toBe(bInput);

    bInput!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  });

  test('a name already taken in the folder is refused before disk is touched', async () => {
    // Not just a latency guard: rename_entry_inner calls bare std::fs::rename,
    // which REPLACES the destination without a word — there is no destination
    // check anywhere in Rust. Two pages titled the same on the same day mint
    // the same basename, and the second used to overwrite the first. The check
    // has to happen before the command, because after it the other file is
    // already gone.
    //
    // Its reach is exactly the siblings the TREE knows about: it is a dirIndex
    // lookup, so a hidden file (showInternal off) or one that appeared since
    // the last refresh is not seen. That is a real gap, and closing it belongs
    // in the backend, not here.
    await expect(
      fileTree.renameThroughTree(`${ROOT}/about.md`, `${ROOT}/index.md`),
    ).rejects.toThrow(/index\.md/);
    expect(mockRenameEntry).not.toHaveBeenCalled();
    // Both files are still there.
    expect(rowNames()).toContain('about.md');
    expect(rowNames()).toContain('index.md');
    expect(registry.getByPath('index.md')).not.toBeNull();
  });

  test('the taken-name check sees a collision across Unicode spellings', async () => {
    // The one comparison in this region where a miss is not a worse error
    // message but the other file GONE: behind it is a bare std::fs::rename
    // with no destination check, on a filesystem whose lookup ignores the
    // difference. Disk lists 'café.md' decomposed; the user types the
    // precomposed form. Same file to macOS, two strings to ===.
    const NFC = 'caf\u00e9';   // precomposed: caf + é
    const NFD = 'cafe\u0301';  // decomposed: caf + e + combining acute
    expect(NFC).not.toBe(NFD);           // guard: not a tautology
    expect(NFD.normalize('NFC')).toBe(NFC);

    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['about.md', `${NFD}.md`]) });
    await (fileTree as unknown as { refreshAndRender(): Promise<void> }).refreshAndRender();

    await expect(
      fileTree.renameThroughTree(`${ROOT}/about.md`, `${ROOT}/${NFC}.md`),
    ).rejects.toThrow(/already exists/);
    expect(mockRenameEntry).not.toHaveBeenCalled();
    expect(rowNames()).toContain(`${NFD}.md`);
  });
});

/**
 * An expansion override is keyed by path, and refreshTree prunes overrides to
 * paths that still exist — so before the rename remap, renaming an expanded
 * folder collapsed it and everything under it. The one-write-path rule exists to stop exactly
 * this kind of silent loss of the user's "I opened this" intent.
 */
describe('in-app rename: an expanded folder stays expanded across its own rename', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  const folderTree = (dirName: string) => (dir(ROOT, [
    dir(`${ROOT}/${dirName}`, [
      file(`${ROOT}/${dirName}/a.md`),
      ], { name: dirName }),
    ], { name: 'Mira Vale' }));

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree('posts') });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    enterDirMode(fileTree);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  test('the override follows the folder, so its child is still on screen', async () => {
    const p = priv<{ view: TreeView; render: { render(): void } }>(fileTree);
    // The user opened this folder. That intent is what must survive.
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));
    p.render.render();
    expect(overrideAt(fileTree, 'posts')).toBe('open');

    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree('writing') });
    await fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/writing`);

    expect(overrideAt(fileTree, 'writing')).toBe('open');
    expect(overrideAt(fileTree, 'posts')).toBeUndefined();
    // The child row is reachable under the new folder path.
    const paths = Array.from(container.querySelectorAll<HTMLElement>('.seg[data-path]'))
      .map(el => el.dataset.path);
    expect(paths).toContain('writing/a.md');
  });

  test('a folder whose home file is named after it is NOT predicted', async () => {
    // The backend carries a self-named home file with its folder:
    // posts/posts.md becomes writing/writing.md. A path-keyed prediction keeps
    // each descendant's own basename, so it would put writing/posts.md on
    // screen — a path that exists nowhere — for the whole rewrite window and
    // then swap it. Better to wait than to show a name that is not there.
    const withHome = (dirName: string, homeName: string) => (dir(ROOT, [
      dir(`${ROOT}/${dirName}`, [
        file(`${ROOT}/${dirName}/${homeName}`, { name: homeName }),
        ], { name: dirName }),
      ], { name: 'Mira Vale' }));
    mockListTree.mockResolvedValue({ status: 'ok', data: withHome('posts', 'posts.md') });
    await (fileTree as unknown as { refreshAndRender(): Promise<void> }).refreshAndRender();
    const p = priv<{ view: TreeView; render: { render(): void } }>(fileTree);
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));
    p.render.render();

    let settle!: (v: { status: string; data?: unknown }) => void;
    mockRenameEntry.mockReturnValue(new Promise(r => { settle = r as typeof settle; }));

    const rename = fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/writing`);
    await Promise.resolve();
    const paths = () => Array.from(container.querySelectorAll<HTMLElement>('.seg[data-path]'))
      .map(el => el.dataset.path);
    // Nothing was predicted — and above all, the invented child path is absent.
    expect(paths()).not.toContain('writing/posts.md');
    expect(paths()).toContain('posts/posts.md');

    // Once the command returns, disk has the answer for both names.
    mockListTree.mockResolvedValue({ status: 'ok', data: withHome('writing', 'writing.md') });
    settle({ status: 'ok', data: renameOkSummary });
    await rename;
    // The expansion still follows the folder — declining to PREDICT must not
    // mean declining to re-key.
    expect(overrideAt(fileTree, 'writing')).toBe('open');
    // commitRename re-reads the tree but does not paint (the gesture's
    // flushPendingWork does, and the drag path paints for itself), so ask for
    // the frame here to see what the refresh brought back.
    p.render.render();
    expect(paths()).toContain('writing/writing.md');
    expect(paths()).not.toContain('writing/posts.md');
  });

  // 'café' has two spellings: precomposed (NFC, what a keyboard emits) and
  // decomposed (NFD, what a macOS directory listing emits). One filename, two
  // strings — so an override keyed on the typed form was pruned by the listed
  // form and the folder collapsed. The rule is that paths are STORED as disk
  // spells them and normalized only to compare, so what these tests check is
  // that the state survives, keyed the way the filesystem spells it.
  const NFC = 'caf\u00e9';   // precomposed: caf + é
  const NFD = 'cafe\u0301';  // decomposed: caf + e + combining acute

  test('the two spellings really are different strings', () => {
    // Guard for the two tests below: with a name that has no decomposed form
    // (any CJK example) they would pass whether or not the code normalizes.
    expect(NFC).not.toBe(NFD);
    expect(NFD.normalize('NFC')).toBe(NFC);
    expect(NFC.normalize('NFC')).toBe(NFC);
  });

  test('an NFD folder the user expands is still expanded after the next refresh', async () => {
    // No rename involved at all. The override is keyed the way the ROW is
    // spelled (decomposed, straight off the listing), and the prune looks it
    // up in comparison form — so the lookup side needs normalizing just as
    // much as the index side. Miss it and an ordinary refresh collapses the
    // folder the user just opened.
    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree(NFD) });
    const p = priv<{ view: TreeView; refresh: TreeRefresh }>(fileTree);
    await fileTree.refreshAndRender();

    setView(fileTree, setFolder(p.view, registry.getByPath(NFD)!, 'open'));
    expect(await p.refresh.refreshTree()).toBe(true);

    expect(overrideAt(fileTree, NFD)).toBe('open');
  });

  test('an accented name keeps its expansion, whichever Unicode form disk uses', async () => {
    const p = priv<{ view: TreeView; render: { render(): void } }>(fileTree);
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));
    p.render.render();

    // The user types the precomposed form; the listing afterwards is decomposed.
    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree(NFD) });
    await fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/${NFC}`);

    // Survives — and is keyed the way the filesystem spelled it, so every
    // path derived from it is bytes the filesystem will accept.
    expect(overrideAt(fileTree, NFD)).toBe('open');
    expect(p.view.overrides.size).toBe(2); // the root's own override, plus this one
    // The rows the refresh brought back carry the filesystem's spelling — the
    // prediction's spelling was only ever a guess at it. (commitRename
    // re-reads but does not paint; the gesture's flushPendingWork does.)
    p.render.render();
    const paths = Array.from(container.querySelectorAll<HTMLElement>('.seg[data-path]'))
      .map(el => el.dataset.path);
    expect(paths).toContain(`${NFD}/a.md`);
    expect(paths).not.toContain(`${NFC}/a.md`);
  });

  test('an accented rename that lands and then errors keeps the expansion too', async () => {
    // The error path asks disk whether the new name is there. It used to ask
    // with a raw `===` against the backend's spelling, so for an accented name
    // the answer was always "no" — the prediction was reversed, the refresh
    // pruned the override, and the folder collapsed. The ASCII case passed
    // throughout, which is why this row is here as well as that one.
    const p = priv<{ view: TreeView }>(fileTree);
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));

    // The rename LANDED (disk lists the new folder, decomposed) and the
    // command then failed on the link-rewrite half.
    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree(NFD) });
    mockRenameEntry.mockResolvedValue({ status: 'error', error: 'Cannot canonicalize root' });

    await expect(
      fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/${NFC}`),
    ).rejects.toThrow(/canonicalize/);

    expect(overrideAt(fileTree, NFD)).toBe('open');
    expect(overrideAt(fileTree, 'posts')).toBeUndefined();
  });

  test('a rename that lands on disk and THEN errors is not undone', async () => {
    // rename_entry_with_refs renames the folder on its first line and can
    // still fail afterwards, canonicalizing or rewriting links. Undoing the
    // prediction then re-keys the tree onto a path that no longer exists, and
    // the next refresh — pruning to what exists — drops the override: the
    // folder-collapse bug, restored in the error path. Disk, not the error,
    // decides. Asserting after the settled rejection is the point: an
    // assertion one tick in cannot tell "not undone" from "not undone YET".
    const p = priv<{ view: TreeView }>(fileTree);
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));

    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree('writing') });
    mockRenameEntry.mockResolvedValue({ status: 'error', error: 'Cannot canonicalize root' });

    await expect(
      fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/writing`),
    ).rejects.toThrow(/canonicalize/);

    expect(overrideAt(fileTree, 'writing')).toBe('open');
    expect(overrideAt(fileTree, 'posts')).toBeUndefined();
  });

  test('a rename that fails while the tree cannot be read is left standing', async () => {
    // "I could not find out" is not "it did not happen". Reversing on an
    // unknown destroys the expansion state for good (the refresh that follows
    // fails the same way and keeps the reversed index); leaving the prediction
    // up shows a name the next successful refresh will correct.
    const p = priv<{ view: TreeView }>(fileTree);
    setView(fileTree, setFolder(p.view, registry.getByPath('posts')!, 'open'));

    mockListTree.mockResolvedValue({ status: 'error', error: 'listTree exploded' });
    mockRenameEntry.mockResolvedValue({ status: 'error', error: 'EACCES' });

    await expect(
      fileTree.renameThroughTree(`${ROOT}/posts`, `${ROOT}/writing`),
    ).rejects.toThrow(/EACCES/);

    expect(overrideAt(fileTree, 'writing')).toBe('open');
  });

  test('the self-named-home guard compares names in one Unicode form', async () => {
    // The guard asks "is there a <folder>.md in here?", and folder path and
    // child name can reach it in different spellings. Compared raw, café/ +
    // café.md misses — and the tree predicts a child path that exists nowhere,
    // which is the entire failure the guard exists to prevent.
    const mixed = dir(ROOT, [
      dir(`${ROOT}/${NFC}`, [
        file(`${ROOT}/${NFC}/${NFD}.md`, { name: `${NFD}.md` }),
        ], { name: NFC }),
      ], { name: 'Mira Vale' });
    mockListTree.mockResolvedValue({ status: 'ok', data: mixed });
    await (fileTree as unknown as { refreshAndRender(): Promise<void> }).refreshAndRender();
    const p = priv<{ isDirectory(path: string): boolean }>(fileTree);
    expect(p.isDirectory(NFC)).toBe(true);

    let settle!: (v: { status: string; data?: unknown }) => void;
    mockRenameEntry.mockReturnValue(new Promise(r => { settle = r as typeof settle; }));
    const rename = fileTree.renameThroughTree(`${ROOT}/${NFC}`, `${ROOT}/nouveau`);
    await Promise.resolve();

    // Nothing was predicted: the folder is still under its old name, and the
    // invented child — nouveau/café.md, whichever spelling — is nowhere.
    expect(p.isDirectory('nouveau')).toBe(false);
    expect(p.isDirectory(NFC)).toBe(true);
    expect(childPaths(fileTree, 'nouveau')).toBeNull();

    mockListTree.mockResolvedValue({ status: 'ok', data: folderTree('nouveau') });
    settle({ status: 'ok', data: renameOkSummary });
    await rename;
    expect(p.isDirectory('nouveau')).toBe(true);
  });
});

/**
 * The teardown of a finished rename must touch nothing that is no longer its
 * own. `endSegEdit` is the sharp edge: it strips `flex-shrink` from EVERY
 * breadcrumb segment, so running it while a second rename's input is mounted
 * in the breadcrumb narrows the field someone is typing into. Both breadcrumb
 * renames, because that is the only shape where the damage is observable.
 */
describe('in-app rename: a finished rename does not tear down a live one', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  const nested = (dirName: string) => (dir(ROOT, [
    dir(`${ROOT}/${dirName}`, [
      file(`${ROOT}/${dirName}/a.md`),
      ], { name: dirName }),
    ], { name: 'Mira Vale' }));

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: nested('posts') });
    mockRenameEntry.mockResolvedValue({ status: 'ok', data: { moved: [], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' } });
    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/posts/a.md`);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
  });

  const bcSegs = () => Array.from(
    container.querySelectorAll<HTMLElement>('.nav-bc-strip .seg'));

  const renameSeg = (seg: HTMLElement, typed: string): HTMLInputElement => {
    startRenameByKey(seg);
    const input = container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');
    expect(input).not.toBeNull();
    input!.value = typed;
    return input!;
  };

  test('the breadcrumb rename in progress keeps its widened segments', async () => {
    // Sanity: the strip has more than one segment, which is what makes
    // endSegEdit's sibling sweep observable at all.
    expect(bcSegs().length).toBeGreaterThan(1);

    // Rename A: the folder crumb. Its refreshTree is held open, which is the
    // window in which the user can start another rename (the IPC itself is
    // serialized; the refresh that follows it is not).
    let settleRefresh!: (v: unknown) => void;
    mockListTree.mockReturnValueOnce(new Promise(r => { settleRefresh = r; }));
    const aInput = renameSeg(
      container.querySelector<HTMLElement>('.nav-bc-strip .seg[data-path="posts"]')!,
      'writing',
    );
    aInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();

    // Rename B: the leaf crumb, still being typed into.
    const bInput = renameSeg(
      container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current')!, 'b');
    const bSeg = bInput.parentElement as HTMLElement;
    const sibling = bcSegs().find(el => el !== bSeg)!;
    expect(bSeg.classList.contains('ft-renaming')).toBe(true);
    expect(sibling.style.flexShrink).toBe('1');

    // A finishes. Its teardown must not reach into B's segment or its siblings.
    settleRefresh({ status: 'ok', data: nested('writing') });
    await new Promise(r => setTimeout(r, 0));
    await new Promise(r => setTimeout(r, 0));

    expect(container.querySelector('input.moss-tree-rename-input')).toBe(bInput);
    expect(bSeg.classList.contains('ft-renaming')).toBe(true);
    expect(sibling.style.flexShrink).toBe('1');

    bInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  });
});
