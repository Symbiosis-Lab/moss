/**
 * The read-only-lock / flush / reload ordering a rename, move and undo
 * commit each wrap the backend call in (edit-session.ts commitRename,
 * ops.ts moveEntriesTo, rename-toast.ts performRenameUndo) — the fix for
 * the open-buffer clobber: a dirty page whose links the commit rewrites
 * must not have its stale buffer autosaved back over that rewrite, and the
 * commit's own content change to an open file (a rebased link, or the
 * moved file's own outgoing link) must reach the buffer once the commit
 * settles, since the rename lifecycle event is a path change only.
 *
 * These assert the MECHANISM (call order and the lock's span across the
 * in-flight IPC call) rather than driving a real CM6 buffer end to end —
 * that would need booting editor-main.ts's own autosave/CM6 harness
 * alongside the tree, which is a second, larger integration surface.
 * setEditorCommitLock(true) runs BEFORE flushPendingSave, not after (proven
 * here) — a keystroke landing while the flush's own IPC is in flight would
 * otherwise arm a fresh autosave debounce nothing then cancels, which is
 * exactly what lets a mid-commit or post-reload autosave clobber the
 * commit's own rewrite. It then stays engaged for
 * the whole round trip (proven here via a controllable deferred command),
 * which is what leaves nothing for a mid-commit keystroke to lose; and
 * reloadActiveFileFromDisk runs before the lock lifts (proven here), which
 * is what gets a rebased link into the buffer.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, enterDirMode, priv, dir, file } from './test-helpers';

installPointerEventPolyfill();

const {
  mockListTree, mockGetProjectPath, mockRenameEntryWithRefs, mockMoveEntries, mockUndoRename,
} = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockRenameEntryWithRefs: vi.fn(),
  mockMoveEntries: vi.fn(),
  mockUndoRename: vi.fn(),
}));

vi.mock('../../../engine', () => ({
  engine: {
    listTree: mockListTree,
    getProjectPath: mockGetProjectPath,
    renameEntryWithRefs: mockRenameEntryWithRefs,
    moveEntries: mockMoveEntries,
    undoRename: mockUndoRename,
    copyFilesToProject: vi.fn(),
    deleteEntry: vi.fn(),
    revealEntry: vi.fn(),
    duplicateEntry: vi.fn(),
    createFiles: vi.fn(),
    createFolder: vi.fn(),
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));

const { mockShowAck } = vi.hoisted(() => ({ mockShowAck: vi.fn() }));
vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showAck: mockShowAck,
  showToast: vi.fn(),
}));


if (typeof globalThis.CSS === 'undefined') {
  (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
    escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
  };
}

import { FileTree } from '../controller';
import { EntryRegistry } from '../../entry-registry';

const ROOT = '/Users/me/Sites/vault';

function makeTree(files: string[]) {
  return dir(ROOT, files.map((name) => file(`${ROOT}/${name}`)), { name: 'vault' });
}

/** A deferred value: resolves only when `settle` is called, so a test can
 *  observe what happened WHILE the backend call was still in flight. */
function deferred<T>() {
  let settle!: (v: T) => void;
  const promise = new Promise<T>((resolve) => { settle = resolve; });
  return { promise, settle };
}

const okSummary = { moved: [{ old_path: 'a.md', new_path: 'b.md', is_dir: false }], edited_link_count: 0, edited_pages: [], skipped_pages: [], undo_id: 'undo-1' };

describe('editor commit hooks: read-only lock, flush and reload ordering', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;
  let hooks: { flushPendingSave: ReturnType<typeof vi.fn>; setEditorCommitLock: ReturnType<typeof vi.fn>; reloadActiveFileFromDisk: ReturnType<typeof vi.fn> };
  let calls: string[];

  beforeEach(async () => {
    vi.clearAllMocks();
    calls = [];
    hooks = {
      flushPendingSave: vi.fn(async () => { calls.push('flushPendingSave'); }),
      setEditorCommitLock: vi.fn((locked: boolean) => { calls.push(`lock:${locked}`); }),
      reloadActiveFileFromDisk: vi.fn(async () => { calls.push('reloadActiveFileFromDisk'); }),
    };
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['a.md', 'b.md']) });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry, undefined, hooks);
    container = document.createElement('div');
    document.body.appendChild(container);
    await fileTree.mount(container, ROOT, false);
    enterDirMode(fileTree);
  });

  test('a rename locks, flushes, calls the command, reloads, then unlocks — in that order', async () => {
    mockRenameEntryWithRefs.mockImplementation(async () => {
      calls.push('renameEntryWithRefs');
      return { status: 'ok', data: okSummary };
    });

    await fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`);

    expect(calls).toEqual([
      'lock:true',
      'flushPendingSave',
      'renameEntryWithRefs',
      'reloadActiveFileFromDisk',
      'lock:false',
    ]);
  });

  test('the lock is still engaged while the rename command is in flight — nothing a keystroke could lose slips through', async () => {
    const held = deferred<{ status: 'ok'; data: typeof okSummary }>();
    mockRenameEntryWithRefs.mockReturnValue(held.promise);

    const rename = fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`);
    await Promise.resolve();
    await Promise.resolve();

    expect(hooks.setEditorCommitLock).toHaveBeenLastCalledWith(true);
    expect(hooks.setEditorCommitLock).not.toHaveBeenCalledWith(false);

    held.settle({ status: 'ok', data: okSummary });
    await rename;

    expect(hooks.setEditorCommitLock).toHaveBeenLastCalledWith(false);
  });

  test('a rename that fails still unlocks (no stuck read-only editor)', async () => {
    mockRenameEntryWithRefs.mockImplementation(async () => {
      calls.push('renameEntryWithRefs');
      return { status: 'error', error: 'disk full' };
    });

    await expect(fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`)).rejects.toThrow('disk full');

    expect(calls).toEqual(['lock:true', 'flushPendingSave', 'renameEntryWithRefs', 'lock:false']);
    expect(hooks.reloadActiveFileFromDisk).not.toHaveBeenCalled();
  });

  test('a batch move locks, flushes, calls the command, reloads, then unlocks — in that order', async () => {
    mockMoveEntries.mockImplementation(async () => {
      calls.push('moveEntries');
      return { status: 'ok', data: okSummary };
    });

    const ops = priv<{ ops: { moveEntriesTo(paths: string[], targetDirRel: string): Promise<{ status: string }> } }>(fileTree).ops;
    const result = await ops.moveEntriesTo([`${ROOT}/a.md`], '');
    expect(result.status).toBe('ok');

    expect(calls).toEqual([
      'lock:true',
      'flushPendingSave',
      'moveEntries',
      'reloadActiveFileFromDisk',
      'lock:false',
    ]);
  });

  test('undo locks, flushes, calls undoRename, reloads, then unlocks — in that order', async () => {
    // edited_link_count > 0 so the toast (and its Undo action) actually shows.
    const editedSummary = { ...okSummary, edited_link_count: 1, edited_pages: ['b.md'] };
    mockRenameEntryWithRefs.mockResolvedValue({ status: 'ok', data: editedSummary });
    mockUndoRename.mockImplementation(async () => {
      calls.push('undoRename');
      return { status: 'ok', data: { restored_files: ['a.md'], skipped: [] } };
    });

    await fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`);
    expect(mockShowAck).toHaveBeenCalledTimes(1);
    const [, options] = mockShowAck.mock.calls[0];
    calls.length = 0; // isolate the undo's own sequence from the forward rename's

    options.actions[0].onClick();
    await vi.waitFor(() => expect(mockUndoRename).toHaveBeenCalledTimes(1));
    // The lock cycles once more (true, then false) inside performRenameUndo,
    // after runAction's microtasks settle.
    await vi.waitFor(() => expect(calls).toContain('lock:false'));

    expect(calls).toEqual([
      'lock:true',
      'flushPendingSave',
      'undoRename',
      'reloadActiveFileFromDisk',
      'lock:false',
    ]);
  });

  test('an undo that itself skips stale pages names them in a toast', async () => {
    const editedSummary = { ...okSummary, edited_link_count: 1, edited_pages: ['b.md'] };
    mockRenameEntryWithRefs.mockResolvedValue({ status: 'ok', data: editedSummary });
    mockUndoRename.mockResolvedValue({
      status: 'ok',
      data: { restored_files: [], skipped: [{ file: 'c.md', reason: 'reference text changed since the rename' }] },
    });

    await fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`);
    const onUndoClick = mockShowAck.mock.calls[0][1].actions[0].onClick as () => void;
    mockShowAck.mockClear();

    onUndoClick();
    await vi.waitFor(() => expect(mockShowAck).toHaveBeenCalledTimes(1));
    const [message] = mockShowAck.mock.calls[0];
    expect(message).toContain('c.md');
  });
});

/**
 * To prevent writes during a commit, engage the lock BEFORE
 * `flushPendingSave`, not after — a keystroke landing while the flush's own
 * IPC is in flight arms a fresh autosave debounce that nothing then
 * cancels, and it can fire mid-commit (or after the reload) and clobber the
 * commit's own rewrite. This drives the REAL `commitRename` (not a
 * synthetic call-order tracker) with fake timers, so it is sensitive to the
 * actual source order in edit-session.ts: reverting that order (lock after
 * flush) reproduces the clobber here.
 *
 * `typeIntoOpenBuffer`'s own lock check stands in for editor-main.ts's real
 * defenses — CM6 read-only (which refuses the keystroke outright) and
 * `saveActiveDocument`'s own `editorCommitLocked` guard (covered directly,
 * against the real function, in flush-save-ack.test.ts) — without booting
 * editor-main.ts's whole module graph just to reach one debounce.
 */
describe('a keystroke during the flush cannot outlive the commit lock (fake timers)', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree(['a.md', 'b.md']) });
    registry = new EntryRegistry();
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    vi.useRealTimers();
    container.remove();
  });

  test('a rename: a debounce armed while flushPendingSave is in flight never writes', async () => {
    let locked = false;
    const writes: string[] = [];
    let content = 'pre-commit content';
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    function typeIntoOpenBuffer(text: string): void {
      content = text;
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        debounceTimer = null;
        if (!locked) writes.push(content);
      }, 800);
    }

    const flushGate = deferred<void>();
    const hooks = {
      flushPendingSave: vi.fn(async () => { await flushGate.promise; }),
      setEditorCommitLock: vi.fn((l: boolean) => { locked = l; }),
      reloadActiveFileFromDisk: vi.fn(async () => {}),
    };

    fileTree = new FileTree(registry, undefined, hooks);
    await fileTree.mount(container, ROOT, false);
    enterDirMode(fileTree);

    const heldCommand = deferred<{ status: 'ok'; data: typeof okSummary }>();
    mockRenameEntryWithRefs.mockReturnValue(heldCommand.promise);

    vi.useFakeTimers();
    try {
      const rename = fileTree.renameThroughTree(`${ROOT}/a.md`, `${ROOT}/renamed.md`);
      await vi.advanceTimersByTimeAsync(0); // let commitRename reach the (pending) flush

      // The keystroke: lands while flushPendingSave's own IPC is in flight.
      typeIntoOpenBuffer('typed during the commit');

      await vi.advanceTimersByTimeAsync(900); // the debounce tries to fire
      flushGate.settle();
      heldCommand.settle({ status: 'ok', data: okSummary });
      await rename;
    } finally {
      vi.useRealTimers();
    }

    expect(hooks.setEditorCommitLock).toHaveBeenCalledWith(true);
    expect(writes).toEqual([]); // the debounce never wrote — the lock was already held
  });
});
