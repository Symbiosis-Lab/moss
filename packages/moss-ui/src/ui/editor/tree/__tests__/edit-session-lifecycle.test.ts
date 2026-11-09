/**
 * Editing-state lifecycle hardening: a stuck edit session must be
 * structurally impossible.
 *
 * Why this matters: the gate (tree/edit-gate.ts) closes render() AND
 * refreshAndRender() while a session is open. One leaked session
 * therefore freezes the ENTIRE tree permanently — dead chevrons, invisible
 * watcher updates, frozen breadcrumb — and the guard at the top of
 * startCreate makes every subsequent create a silent no-op.
 *
 * Three layers under test (see tree/edit-session.ts, driven through the
 * mounted controller because every recovery ends in a repaint it owns):
 *   1. Leak fixes at the mount branches — startCreate's no-row path (the
 *      strip torn down or the temp seg gone before the deferred mount)
 *      cancels instead of bare-returning with the session held; startRename
 *      locates .seg-name BEFORE opening one.
 *   2. Watchdog (EDIT_WATCHDOG_MS) — armed by the gate for the whole session,
 *      reaps iff no live inline input exists when it fires. Its arm/disarm/
 *      reap arithmetic is proved in tree/__tests__/edit-gate.test.ts; what is
 *      here is that the tree recovers when it fires.
 *   3. Phantom self-heal (EDIT_PHANTOM_GRACE_MS) — admit() clears a session
 *      that has had no inline input in the DOM past the grace window (the
 *      timer-free backstop for when the watchdog timer itself is throttled).
 *      The grace window is the RACE GUARD: a fresh create legitimately has no
 *      input between begin() and its deferred setTimeout mount, and must NOT
 *      be reaped in that gap.
 *
 * The gate's own lattice and grace arithmetic are proved in
 * tree/__tests__/edit-gate.test.ts; this suite is about the tree wiring.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { installPointerEventPolyfill, installCSSEscapePolyfill, priv, startRenameByKey, dir, file } from './test-helpers';

installPointerEventPolyfill();
installCSSEscapePolyfill();

const { mockListTree, mockGetProjectPath, mockCreateFile, mockCreateFolder } = vi.hoisted(() => ({
  mockListTree: vi.fn(),
  mockGetProjectPath: vi.fn(),
  mockCreateFile: vi.fn(),
  mockCreateFolder: vi.fn(),
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
    resolveUrlForFile: vi.fn().mockResolvedValue({ status: 'ok', data: '/' }),
  },
}));


import { FileTree } from '../controller';
import { EDIT_WATCHDOG_MS, EDIT_PHANTOM_GRACE_MS } from '../edit-gate';
import type { CreateSession, DeferredWork, EditSession } from '../edit-gate';
import { EntryRegistry, type EntryRecord } from '../../entry-registry';

const ROOT = '/Users/me/Sites/blog';

/** Private seam for the gate's internals — the two facts a test needs that
 *  the public getters do not expose: when the session started (so a session
 *  can be aged past the grace window without advancing the throttled
 *  watchdog) and what work is parked. */
interface GateSeam {
  readonly creating: CreateSession | null;
  readonly renamingPath: string | null;
  readonly isEditing: boolean;
  open: { startedAt: number } | null;
  deferred: DeferredWork;
  begin(session: EditSession): void;
}

/** Private seam for the lifecycle members this suite exercises. */
interface LifecycleSeam {
  /** The renderer (tree/render.ts) owns the strip; the leak tests pull it out from under a session. */
  render: { navBcStrip: HTMLElement | null; render(): void };
  gate: GateSeam;
  view: { anchor: string[] };
  startCreate(folderPath: string, type: 'file' | 'folder'): Promise<void>;
  /** The sessions themselves (tree/edit-session.ts); `startRename` is reached here. */
  editSession: { startRename(segEl: HTMLElement, entry: EntryRecord): void };
  toggleFolderOpen(path: string): void;
}/** Root with two files plus `works/` and `notes/` folders — the folders give
 *  the "chevron works again after healing" assertions something to toggle. */
function makeTree() {
  return dir(ROOT, [
      file(`${ROOT}/index.md`),
      file(`${ROOT}/about.md`),
      dir(`${ROOT}/works`, [file(`${ROOT}/works/post.md`)]),
      dir(`${ROOT}/notes`, [file(`${ROOT}/notes/memo.md`)]),
    ], { name: 'blog' });
}

describe('editing-state lifecycle: no stuck edit session', () => {
  let registry: EntryRegistry;
  let fileTree: FileTree;
  let container: HTMLElement;
  let seam: LifecycleSeam;
  let gate: GateSeam;

  const editInput = () =>
    container.querySelector<HTMLInputElement>('input.moss-tree-rename-input');

  /** Build a real phantom: run a create through its actual path (temp entry
   *  in dirIndex, activePath leaf pushed, watchdog armed, input mounted),
   *  then tear the input out of the DOM without any blur/Enter/Escape and
   *  age the session past the grace window WITHOUT advancing fake timers —
   *  the watchdog stays pending, modeling a throttled webview where only
   *  the self-heal path can run. All entries route through the context-menu
   *  startCreate seam (the public createInCurrentDir wrapper was dead code —
   *  removed). */
  async function forcePhantomCreate(folderPath: string = ''): Promise<void> {
    await seam.startCreate(folderPath, 'file');
    await vi.advanceTimersByTimeAsync(0); // deferred mount runs
    const input = editInput();
    expect(input).not.toBeNull();
    input!.remove(); // torn out of the DOM — no blur fires in jsdom (spec: unfocusing steps skip removed nodes)
    expect(fileTree.isEditing()).toBe(true); // flag leaked
    // Age the session by rewinding the stamp instead of advancing timers, so
    // the (throttled) watchdog cannot be the thing that heals.
    ageSession();
  }

  /** Push the live session's start stamp past the grace window, so the
   *  (throttled) watchdog cannot be the thing that heals. */
  function ageSession(): void {
    (gate.open as { startedAt: number }).startedAt =
      performance.now() - (EDIT_PHANTOM_GRACE_MS + 1000);
  }

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mockGetProjectPath.mockResolvedValue({ status: 'ok', data: ROOT });
    mockListTree.mockResolvedValue({ status: 'ok', data: makeTree() });

    registry = new EntryRegistry();
    fileTree = new FileTree(registry);
    container = document.createElement('div');
    document.body.appendChild(container);

    await fileTree.mount(container, ROOT, false);
    fileTree.highlight(`${ROOT}/index.md`);
    seam = priv<LifecycleSeam>(fileTree);
    gate = priv<GateSeam>(seam.gate);
  });

  afterEach(() => {
    fileTree.unmount();
    container.remove();
    vi.useRealTimers();
  });

  // ── Layer 1: mount-branch leaks ─────────────────────────────────────

  test('LEAK 1: strip gone at mount time → the session is cancelled, next create works', async () => {
    const strip = seam.render.navBcStrip;
    expect(strip).not.toBeNull();
    seam.render.navBcStrip = null; // simulate teardown between flag-set and the deferred mount

    void seam.startCreate('', 'file');
    expect(fileTree.isEditing()).toBe(true); // held across the mount gap, as designed
    await vi.runAllTimersAsync();

    // The old code bare-returned here, leaking createPending forever.
    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.creating).toBeNull();
    // No orphan temp leaf on the breadcrumb path.
    expect(seam.view.anchor.some(p => p.endsWith('untitled.md'))).toBe(false);

    // And the tree is NOT silently dead: a subsequent create mounts its input.
    seam.render.navBcStrip = strip;
    void seam.startCreate('', 'file');
    await vi.runAllTimersAsync();
    expect(editInput()).not.toBeNull();
  });

  test('temp seg missing at mount time → cancel branch clears the flag, next create works', async () => {
    void seam.startCreate('', 'file');
    // Remove the temp seg before the deferred mount runs (targetSeg-null branch).
    seam.render.navBcStrip!.innerHTML = '';
    await vi.runAllTimersAsync();

    expect(fileTree.isEditing()).toBe(false);
    expect(seam.view.anchor.some(p => p.endsWith('untitled.md'))).toBe(false);

    void seam.startCreate('', 'file');
    await vi.runAllTimersAsync();
    expect(editInput()).not.toBeNull();
  });

  test('startRename on a seg with no .seg-name does NOT leak renamingPath; a real rename still works', () => {
    const bareSeg = document.createElement('div'); // no .seg-name child
    seam.editSession.startRename(bareSeg, registry.getById(registry.getByPath('index.md')!)!);
    // The old code set renamingPath BEFORE looking for .seg-name — stuck forever.
    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.renamingPath).toBeNull();

    // A real rename (dblclick on the breadcrumb leaf) mounts its input
    // SYNCHRONOUSLY — this is why startRename needs no mount watchdog.
    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    expect(bcLeaf).not.toBeNull();
    startRenameByKey(bcLeaf!);
    expect(editInput()).not.toBeNull();
    expect(fileTree.isEditing()).toBe(true);
  });

  // ── Layer 2: watchdog ───────────────────────────────────────────────

  test('WATCHDOG: input torn out of the DOM with no blur/Enter/Escape → session reaped at the window', async () => {
    void seam.startCreate('', 'file');
    await vi.advanceTimersByTimeAsync(0);
    const input = editInput();
    expect(input).not.toBeNull();

    input!.remove(); // no event fires — the leak the watchdog exists for
    expect(fileTree.isEditing()).toBe(true);

    await vi.advanceTimersByTimeAsync(EDIT_WATCHDOG_MS);
    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.creating).toBeNull();
    expect(seam.view.anchor.some(p => p.endsWith('untitled.md'))).toBe(false);

    // Tree alive again: a new create mounts a fresh input.
    void seam.startCreate('', 'file');
    await vi.advanceTimersByTimeAsync(0);
    expect(editInput()).not.toBeNull();
  });

  test('WATCHDOG grace side: a live input the user is typing into is NOT reaped; Enter still commits', async () => {
    const newPath = `${ROOT}/hello.md`;
    mockCreateFile.mockResolvedValueOnce({ status: 'ok', data: [newPath] });

    void seam.startCreate('', 'file');
    await vi.advanceTimersByTimeAsync(0);
    const input = editInput();
    expect(input).not.toBeNull();

    // Let the watchdog fire while the input is live — it must be a no-op.
    await vi.advanceTimersByTimeAsync(EDIT_WATCHDOG_MS + 100);
    expect(editInput()).not.toBeNull();
    expect(fileTree.isEditing()).toBe(true);

    // Typing + Enter commits normally, well after the watchdog window.
    input!.value = 'hello.md';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockCreateFile).toHaveBeenCalledWith([{ dir: expect.stringContaining(ROOT), name: 'hello.md', frontmatter: {} }]);
    expect(fileTree.isEditing()).toBe(false);
  });

  // ── Layer 3: phantom self-heal (timer-free backstop) ────────────────

  test('SELF-HEAL: a stale phantom create → render() clears it and the tree renders again', async () => {
    // Create inside `works/` (context-menu path) so the healed activePath
    // (['', 'works']) renders body rows — a ROOT create's cancel collapses
    // activePath to [''] (zero body rows by the spineBody ancestor model,
    // pre-existing cancel behavior) which would leave nothing to assert
    // liveness against.
    await forcePhantomCreate('works');

    // A render (e.g. locale relabel, watcher echo) self-heals instead of parking.
    seam.render.render();

    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.creating).toBeNull();
    expect(seam.gate.deferred).toBe('none'); // render RAN, was not parked
    // Temp leaf gone from breadcrumb state and DOM.
    expect(seam.view.anchor.some(p => p.endsWith('untitled.md'))).toBe(false);
    expect(container.querySelector('.nav-bc-strip .seg[data-full-name="untitled.md"]')).toBeNull();

    // The healed render actually painted rows (not the frozen/blank tree).
    const rowByPath = (p: string) =>
      Array.from(container.querySelectorAll<HTMLElement>('.seg')).find(el => el.dataset.path === p);
    expect(rowByPath('notes')).toBeTruthy();

    // BLAST-RADIUS check: the chevron/toggle path is alive again — opening
    // `notes/` must render its child row. A stuck flag would park this render
    // (renderPending) and the child would never appear.
    expect(rowByPath('notes/memo.md')).toBeFalsy();
    seam.toggleFolderOpen('notes');
    expect(rowByPath('notes/memo.md')).toBeTruthy();
    expect(seam.gate.deferred).toBe('none');
  });

  test('SELF-HEAL: a phantom create no longer parks watcher refreshes', async () => {
    await forcePhantomCreate();
    const listTreeCallsBefore = mockListTree.mock.calls.length;

    // The watcher's own callback — no private wrapper stands between them.
    await fileTree.refreshAndRender();

    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.deferred).toBe('none'); // refresh RAN, was not parked
    expect(mockListTree.mock.calls.length).toBeGreaterThan(listTreeCallsBefore);
  });

  test('SELF-HEAL: a stale phantom rename → render() clears it (rename side of the backstop)', () => {
    // Model a rename input torn out of the DOM without blur: session held, no
    // input anywhere, stamp aged past the grace window.
    gate.begin({ kind: 'rename', path: 'index.md' });
    ageSession();

    seam.render.render();

    expect(fileTree.isEditing()).toBe(false);
    expect(seam.gate.renamingPath).toBeNull();
    expect(seam.gate.deferred).toBe('none');
  });

  test('RACE GUARD: a fresh create in its mount gap is parked, not reaped — and still commits', async () => {
    const newPath = `${ROOT}/fresh.md`;
    mockCreateFile.mockResolvedValueOnce({ status: 'ok', data: [newPath] });

    void seam.startCreate('', 'file');
    // The dangerous moment: flag set, deferred mount not yet run, NO input in
    // the DOM. A naive "no input → phantom" check would cancel this create.
    expect(editInput()).toBeNull();
    expect(fileTree.isEditing()).toBe(true);

    seam.render.render(); // e.g. a watcher-triggered render landing in the gap

    // Parked (grace window), NOT healed.
    expect(fileTree.isEditing()).toBe(true);
    expect(seam.gate.deferred).toBe('render');

    // The mount then proceeds normally and the create commits end-to-end.
    await vi.advanceTimersByTimeAsync(0);
    const input = editInput();
    expect(input).not.toBeNull();
    input!.value = 'fresh.md';
    input!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.runAllTimersAsync();
    await Promise.resolve();
    await Promise.resolve();

    expect(mockCreateFile).toHaveBeenCalledWith([{ dir: expect.stringContaining(ROOT), name: 'fresh.md', frontmatter: {} }]);
    expect(fileTree.isEditing()).toBe(false);
  });

  test('RACE GUARD: a fresh RENAME with its live input is parked, not reaped', () => {
    const bcLeaf = container.querySelector<HTMLElement>('.nav-bc-strip .seg.bc-current');
    startRenameByKey(bcLeaf!);
    expect(editInput()).not.toBeNull();

    seam.render.render();

    // Live input → never phantom regardless of age; render parks as before.
    expect(fileTree.isEditing()).toBe(true);
    expect(seam.gate.deferred).toBe('render');
    expect(editInput()).not.toBeNull();
  });

  test('STALE SESSION: a starved mount timer firing after heal + a NEW create does not double-mount', async () => {
    // Session 1: start a create but do NOT advance timers — its 0ms mount
    // timer stays PENDING (modeling a starved/throttled webview). With no
    // input mounted, aging the stamp past grace makes it a phantom directly;
    // forcePhantomCreate would defeat this test by running the mount timer.
    void seam.startCreate('', 'file');
    expect(editInput()).toBeNull(); // deferred mount has not run
    ageSession();

    seam.render.render(); // heals session 1 — its mount timer is STILL pending
    expect(fileTree.isEditing()).toBe(false);

    // Session 2 starts, then ALL timers run: session 2's mount AND session
    // 1's stale mount. Both sessions share the same tempPath ('untitled.md'
    // in the same folder), so without the session-identity guard the
    // stale timer would mount a SECOND input into session 2's seg.
    void seam.startCreate('', 'file');
    await vi.runAllTimersAsync();
    const inputs = container.querySelectorAll('input.moss-tree-rename-input');
    expect(inputs.length).toBe(1);
    expect(fileTree.isEditing()).toBe(true);
  });
});
