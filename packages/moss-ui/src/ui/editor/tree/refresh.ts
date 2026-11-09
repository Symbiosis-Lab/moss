/**
 * The refresh cycle: re-list the vault, rebuild `dirIndex` from the listing,
 * merge or drop the `.moss/` subtree, and hear file and directory events. This is the
 * single writer of `dirIndex` — `flattenTree` on a listing, `adopt` for the
 * one row that is added by hand (a create session's temp row), `forget` for
 * every removal (delete, the `.moss` toggle, a discarded temp row) — so the
 * index is never mid-clear when a gesture reads it and an older listing
 * can never land after a newer one.
 *
 * A field on
 * the controller (the retry state outlives no mount, but `forget`/`adopt`
 * are reached from the create flow and the delete family, which the
 * controller wires before mount); the controller's AbortSignal is the one
 * teardown — its abort runs `dispose()`, which drops the event subscriptions,
 * disarms the retry and empties `dirIndex`.
 * `listTree` arrives through the host, which the controller fills with the
 * engine.
 */

import { warn as logWarn } from "../../log";
import type { Unlisten } from "../../host";
import { onMossEvent } from "../../utils/moss-events";
import type { EntryRegistry, EntryId, EntryRecord, SnapshotEntry } from '../entry-registry';
import type { SpecialFileKind } from '../session/special-file';
import type { FileEventsCoordinator } from '../editor-file-events';
import { toRelative, isAbsolutePath, normalizePath, PROJECT_ROOT_REL, parentDirOf } from '../path-domain';
import { prune, forgetSubtree, type TreeView } from './model';
import { createGenerationToken, type GenerationToken } from "../../shared/utils/generation-token";

export type CommandResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: string };

/** What `create_files` takes: an absolute directory, a filename, and the frontmatter the page starts with. */
export type NewFileInput = { dir: string; name: string; frontmatter: Record<string, unknown> };

/** One empty page through the batch command the takeover recipes use: the create-flow, the tree's inline create and the render-gate harness all unwrap it here. */
export function createOneFile(
  commands: { createFiles(files: NewFileInput[]): Promise<CommandResult<string[]>> },
  absDir: string,
  name: string,
): Promise<CommandResult<string>> {
  return commands
    .createFiles([{ dir: absDir, name, frontmatter: {} }])
    .then((r) => (r.status === 'ok' ? { status: 'ok' as const, data: r.data[0] } : r));
}

/** The listing the backend returns, structurally (bindings' `TreeNode`, minus the fields the tree does not read). */
export interface ListedNode {
  path: string;
  is_dir: boolean;
  file_id: string | null;
  is_home: boolean;
  children: ListedNode[] | null;
  /** `TreeNode.hidden` — how many of this directory's own immediate children
   *  moss's curated listing filtered out. `0` for files. */
  hidden: number;
  /** `TreeNode.special`; absent from a listing that predates the field. */
  special?: SpecialFileKind | null;
}

/** The one command the refresh cycle calls, injected by the controller. */
export interface RefreshCommands {
  listTree(showInternal: boolean): Promise<CommandResult<ListedNode>>;
}

/** Bounded wait for a `list()` IPC round
 *  trip. Well above a healthy localhost listTree call; well below a user
 *  concluding the tree is frozen. On timeout the previous index is kept
 *  rather than left mid-clear — see refreshTree(). */
const REFRESH_TREE_TIMEOUT_MS = 5000;

/** Delay before the single bounded retry after a failed/timed-out
 *  refreshTree(). Not immediate — a transient IPC hiccup (e.g. the backend
 *  mid-restart) is likely to still be down a tick later. */
const REFRESH_TREE_RETRY_MS = 2000;

/** WARN when a refresh can't replace the
 *  index, so a stale index is never silent. */
function logTreeWarn(msg: string): void {
  try {
    void logWarn(`[tree] ${msg}`).catch(() => { /* logging is best-effort */ });
  } catch { /* no IPC bridge (tests) — logging is a no-op */ }
}

/** Reject with a timeout Error if `promise` hasn't settled within `ms`.
 *  P3's bounded wait on the `list()` IPC round trip — see
 *  REFRESH_TREE_TIMEOUT_MS. Does not cancel `promise`; a late resolution
 *  after the timeout fires is simply never awaited. */
function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timed out after ${ms}ms`)), ms);
    promise.then(
      (v) => { clearTimeout(timer); resolve(v); },
      (e) => { clearTimeout(timer); reject(e); },
    );
  });
}

/**
 * What `refreshTree` / `toggleInternal` own.
 * - `showInternal`: list `.moss/` contents (curated allowlist). Set from
 *   `mount()`, written afterwards only by `toggleInternal()`.
 * - `indexWriteEpoch`: generation-token (see shared/utils/generation-token.ts)
 *   for a write to `dirIndex` that straddles an await — `refreshTree`'s full
 *   replacement and `toggleInternal`'s merge. Captured before the await,
 *   checked after: a writer whose token has gone stale has been superseded
 *   and must drop its payload (the merge re-reads instead). Why the gate
 *   carries no `reloading` flag: serializing the two would make a caller
 *   wait for a listing it did not ask for, and `withTimeout` abandons a call
 *   without cancelling it.
 * - `retryTimer` / `retryExhausted`: P3's single bounded retry after a failed
 *   or timed-out refreshTree(); `retryExhausted` bounds it to one attempt per
 *   failure streak. Both clear on unmount and on the next successful refresh.
 */
export type RefreshState = {
  showInternal: boolean;
  indexWriteEpoch: GenerationToken;
  retryTimer: ReturnType<typeof setTimeout> | null;
  retryExhausted: boolean;
};

/** What the refresh cycle needs from the controller, and nothing more. */
export interface RefreshHost {
  projectRoot(): string;
  readonly commands: RefreshCommands;
  /** The tree model: every fact about an entry. Reconciled here, read everywhere. */
  registry(): EntryRegistry;
  /** Folder id → child ids in display order — only the shape. Written here and nowhere else. */
  readonly dirIndex: Map<EntryId, EntryId[]>;
  update(fn: (view: TreeView) => TreeView): void;
  /** Drop selected ids that no longer exist and notify if the selection changed. */
  reconcileSelection(validIds: Set<EntryId>): void;
  /** May a refresh run now, or is an inline edit open? */
  readonly gate: { admit(work: 'refresh'): boolean };
  render(): void;
  /** Re-home the breadcrumb and the active file to the root if either was inside `.moss/`. */
  leaveInternal(): void;
  /** Run whatever the gate parked while an edit or a listing was in flight. */
  flushPendingWork(): void;
  /** Re-read the folder derivations (tree/derivations.ts) with the listing, so the hint rows repaint with the files. */
  refreshDerivations(): Promise<void>;
}

export class TreeRefresh {
  private readonly state: RefreshState = {
    showInternal: false,
    indexWriteEpoch: createGenerationToken(),
    retryTimer: null,
    retryExhausted: false,
  };

  // Teardown for the filesystem and structural-settlement subscriptions.
  private readonly unlistenFns: Unlisten[] = [];

  constructor(private readonly host: RefreshHost) {}

  /** Whether `.moss/` is listed. `mount()` sets the initial value; afterwards only `toggleInternal()` writes it. */
  get showInternal(): boolean {
    return this.state.showInternal;
  }

  set showInternal(value: boolean) {
    this.state.showInternal = value;
  }

  /** Unmount, via the mount signal's abort: drop the event subscriptions,
   *  disarm the retry, and empty the index this module alone writes. */
  private dispose(): void {
    this.state.indexWriteEpoch.bump();
    for (const unlisten of this.unlistenFns.splice(0)) unlisten();
    this.clearRefreshRetry();
    this.host.dirIndex.clear();
  }

  /**
   * Re-scan the tree and repaint — THE refresh path.
   *
   * Reached by the watcher (`startWatching`), by os-drag-in after an import,
   * by CreateFlow, and by editor-main.
   * own two-line copy of this that asked the gate while this one did not, so
   * an import landing mid-rename ran `refreshTree` — which clears dirIndex —
   * underneath the input the user was typing into. The two copies had
   * also already drifted on whether a FAILED refresh still repaints.
   *
   * Parks behind an open inline edit (the in-flight IPC and the destructive
   * render that follows would tear the input down) rather than dropping the
   * work; `flushPendingWork` runs it once the edit concludes.
   */
  public async refreshAndRender(): Promise<void> {
    const { host } = this;
    if (!host.gate.admit('refresh')) return;
    if (!await this.refreshTree()) return;
    host.render();
  }

  /** Arm the single bounded retry after a failed/timed-out refreshTree().
   *  Bounded to exactly one attempt per failure streak — state.retryExhausted
   *  is reset only by a subsequent SUCCESSFUL refresh (clearRefreshRetry), so
   *  a persistently-down backend gets one retry, not an unbounded loop.
   *
   *  This is also the ONLY recovery for a refresh the gate had already
   *  considered satisfied: `admit` clears the parked job when it lets work
   *  through, so a refresh that then fails is not re-parked. See admit's doc. */
  private scheduleRefreshRetry(): void {
    // `exhausted` alone answers "already tried this streak?" — it is set the
    // instant the timer is armed and cleared only together with it, so a
    // second test on the timer would be unreachable.
    if (this.state.retryExhausted) return;
    this.state.retryExhausted = true;
    this.state.retryTimer = setTimeout(() => {
      this.state.retryTimer = null;
      void this.refreshAndRender();
    }, REFRESH_TREE_RETRY_MS);
  }

  /** Disarm the retry timer and reset its bound — called on a successful
   *  refresh and on unmount(). */
  private clearRefreshRetry(): void {
    if (this.state.retryTimer !== null) {
      clearTimeout(this.state.retryTimer);
      this.state.retryTimer = null;
    }
    this.state.retryExhausted = false;
  }

  /**
   * One bounded `listTree` round trip at the tree's current `.moss`
   * visibility: the tree the backend reports right now, or `null` when the
   * call failed, rejected or did not resolve within REFRESH_TREE_TIMEOUT_MS —
   * logged with the cause either way. Writes nothing: `refreshTree` replaces
   * the index with the result, and the edit session's exists probe reads it
   * without touching the index at all. What to do about a `null` is the
   * caller's (a retry here, fail-open there).
   */
  async list(): Promise<ListedNode | null> {
    const { host } = this;
    let result: CommandResult<ListedNode>;
    try {
      result = await withTimeout(
        host.commands.listTree(this.state.showInternal),
        REFRESH_TREE_TIMEOUT_MS,
      );
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      const cause = e instanceof Error && message === `timed out after ${REFRESH_TREE_TIMEOUT_MS}ms`
        ? `did not resolve within ${REFRESH_TREE_TIMEOUT_MS}ms`
        : `rejected: ${message}`;
      logTreeWarn(`listTree ${cause} — keeping last-good index`);
      return null;
    }
    if (result.status !== 'ok') {
      logTreeWarn(`listTree failed (${result.error}) — keeping last-good index`);
      return null;
    }
    return result.data;
  }

  /**
   * Re-fetch the full tree from the backend, replacing `dirIndex`.
   *
   * The index a gesture reads must never
   * be mid-clear. So the indexes are cleared and repopulated ONLY once the
   * new data has actually arrived — on a failed or non-resolving (bounded
   * REFRESH_TREE_TIMEOUT_MS) `listTree` call, the previous index is left
   * completely untouched, a WARN is logged with the cause, and a single
   * bounded retry is armed. Called by `mount()`, `toggleInternal()`, and the
   * 5 event-handler refresh sites (file-changed, rename, drop, …).
   *
   * The watcher burst, rename and drop paths all call this,
   * so two round trips can be in flight at once — and `withTimeout` abandons
   * a call without cancelling it. Each takes an `indexWriteEpoch` before its
   * await and drops its payload if a later writer has since started, so an
   * OLDER listing can never be the last one to write dirIndex. The
   * Epoch rule; previously nothing ordered the two and the tree showed stale
   * contents until something else refreshed it.
   *
   * @returns `true` on success, `false` on failure/timeout/supersession
   *          (caller may treat `false` as "nothing changed" — the previous
   *          index still stands).
   */
  async refreshTree(): Promise<boolean> {
    const { host } = this;
    if (!host.projectRoot()) return false;
    const epoch = this.state.indexWriteEpoch.bump();
    const tree = await this.list();
    // Unmount (or a newer refresh) can win while the listing is in flight.
    // Drop its result before it can write, render, or arm a retry.
    if (this.state.indexWriteEpoch.isStale(epoch)) return false;
    if (tree === null) {
      this.scheduleRefreshRetry();
      return false;
    }
    // Only now — the new data is in hand — replace the index. A folder
    // deleted since the last refresh needs its stale dirIndex entry gone,
    // which is why this clears before flattenTree's walk, but doing so
    // here (not at the top of the function) is exactly what keeps the index
    // intact through a failure/timeout above.
    host.dirIndex.clear();
    this.flattenTree(tree);
    this.clearRefreshRetry();
    // Prune overrides for folders no longer in the tree, or a deleted folder's
    // 'open' would linger and pre-expand a later folder at the same path.
    // flattenTree has just reconciled the registry, so "exists" is "has a
    // record" — in either Unicode spelling, since the registry compares in one.
    host.update(v => prune(v, (id) => host.registry().getById(id)?.isDir === true));
    // Reconcile selection: drop ids that no longer exist on disk
    // For N=1 selection the activeEntryId facade already handled this via
    // its setter clear path; for N>1 the stale ids would silently linger
    // and break subsequent shift+click range computation. Reconcile drops
    // them and falls back anchor/focus to a surviving member if either
    // disappeared (per FileSelection.reconcile contract).
    host.reconcileSelection(new Set(host.registry().allIds()));
    await host.refreshDerivations();
    return !this.state.indexWriteEpoch.isStale(epoch);
  }

  /**
   * Toggle `.moss/` visibility in the tree.
   *
   * Surgically adds or removes `.moss/` without disturbing the rest of the
   * tree: existing `overrides` and the active file survive the toggle. The
   * panel height follows the rendered open-forest row count (render applies
   * it), so it shortens/lengthens naturally as `.moss/` appears or vanishes.
   * Called by the internal-files toggle.
   */
  async toggleInternal(showInternal: boolean): Promise<void> {
    const { host } = this;
    if (!host.projectRoot()) return;
    if (this.state.showInternal === showInternal) return;
    this.state.showInternal = showInternal;

    // Registry-domain paths: PROJECT_ROOT_REL ('') is the project root;
    // the .moss/ subtree is keyed simply as '.moss'. The TreeNode payload
    // from Rust still carries absolute paths; we convert at the seam below.
    const mossPath = '.moss';

    // The repaint and the flush run on EVERY exit, including the failed
    // listTree — anything the watcher raised while it was in flight is parked
    // in the gate, and a failure path that skipped the drain would leave it
    // sitting there until some unrelated later event happened to run it.
    try {
    if (showInternal) {
      // Fetch the full tree (cheap, backend already caches) and merge only the
      // `.moss` subtree + root child list into our indexes. Other keys are
      // already in dirIndex and are identical across the toggle.
      // Same generation model as refreshTree: this merge is a read-modify-
      // write of dirIndex, so a refresh that landed while its listTree
      // was in flight has already replaced the indexes it was going to merge
      // into. Fall back to a full re-listing, which honours the `showInternal`
      // set above and reaches the same tree.
      const epoch = this.state.indexWriteEpoch.bump();
      const result = await host.commands.listTree(true);
      if (result.status !== 'ok') {
        console.error('Failed to reload tree:', result.error);
        return;
      }
      if (!this.state.indexWriteEpoch.isStale(epoch)) {
        this.mergeInternalSubtree(result.data, PROJECT_ROOT_REL, mossPath);
      } else if (!await this.refreshTree()) {
        return;
      }
    } else {
      // Remove `.moss` and every key beneath it from our indexes, and drop
      // `.moss` from the root's child list.
      this.removeInternalSubtree(mossPath);
      // If the user was viewing a `.moss/` file (whether still in activePath
      // or not — e.g. they navigated away first), re-home to root.
      host.leaveInternal();
    }

    } finally {
      // Preserve activePath/overrides/currentFilePath (when still valid).
      // Re-render (which re-fits the height — sibling counts changed).
      host.render();
      host.flushPendingWork();
    }
  }

  /** Merge `.moss/` entries from a freshly-fetched tree into our indexes.
   *
   * `root` is the registry-domain key for the project root (PROJECT_ROOT_REL).
   * `mossPath` is the registry-domain key for `.moss/` (the relative string).
   * The TreeNode payload carries ABSOLUTE paths; we convert to relative when
   * looking up registry-domain children below. */
  private mergeInternalSubtree(tree: ListedNode, root: string, mossPath: string): void {
    const { host } = this;
    // tree.children[].path is absolute; convert each to the registry-domain
    // form when searching for the .moss/ child.
    const mossNode = (tree.children ?? []).find(
      c => toRelative(c.path, host.projectRoot()) === mossPath,
    );
    if (!mossNode) return;

    // Populate dirIndex for the `.moss` subtree. mainReconcile=false
    // scopes the registry reconcile to the .moss/ pathPrefix only — the
    // main flattenTree call (which uses excludePrefixes: ['.moss']) already
    // protects these entries during its own reconcile pass.
    this.flattenTree(mossNode, false);
    const mossId = host.registry().getByPath(mossPath);
    if (mossId !== null) this.adopt(root, mossId);
  }

  /** Append `id` to `folderPath`'s child list, once. The two writers of a
   *  child list that add — the `.moss` merge and the create-session temp
   *  row — both come here, so "only the shape" holds for writes too. */
  adopt(folderPath: string, id: EntryId): void {
    const { host } = this;
    const folderId = host.registry().getByPath(folderPath);
    if (folderId === null) return;
    const siblings = host.dirIndex.get(folderId) ?? [];
    if (!siblings.includes(id)) host.dirIndex.set(folderId, [...siblings, id]);
  }

  /**
   * Take `record` and everything beneath it out of the tree: unlink it from
   * its parent's child list, drop the subtree from dirIndex and overrides,
   * retire it in the registry (which clears active if it was hit). The one
   * owner for every removal — delete, the `.moss` toggle, a discarded temp row.
   */
  forget(record: EntryRecord): void {
    const { host } = this;
    const parentId = host.registry().getByPath(parentDirOf(record.path));
    const siblings = parentId === null ? undefined : host.dirIndex.get(parentId);
    if (parentId !== null && siblings) host.dirIndex.set(parentId, siblings.filter(c => c !== record.id));
    const gone = new Set<EntryId>();
    const collect = (node: EntryId): void => {
      gone.add(node);
      for (const child of host.dirIndex.get(node) ?? []) collect(child);
      host.dirIndex.delete(node);
    };
    collect(record.id);
    host.update(v => forgetSubtree(v, gone));
    host.registry().delete(record.path);
  }

  /** Drop `.moss` and every descendant from our indexes and the registry. */
  private removeInternalSubtree(mossPath: string): void {
    const r = this.host.registry();
    const id = r.getByPath(mossPath);
    const moss = id === null ? null : r.getById(id);
    if (moss !== null) this.forget(moss);
  }

  /**
   * Ingest a TreeNode subtree: reconcile the registry against it, then
   * rebuild dirIndex from the ids the reconcile settled on. The root node
   * itself is an entry too, so the root has an EntryId like every folder.
   *
   * `mainReconcile` controls how the reconcile call is scoped:
   *   true (default): main project tree. pathPrefix = PROJECT_ROOT_REL
   *                   (the empty string), excludePrefixes = ['.moss'] so
   *                   the .moss/ subtree (loaded separately via
   *                   mergeInternalSubtree) survives.
   *   false: internal subtree merge. pathPrefix = '.moss', no excludes.
   *
   * TreeNode.path arrives ABSOLUTE from Rust's list_directory_inner
   * (crates/moss-build/src/editor/filesystem.rs). We convert to project-root-
   * relative at this ingest boundary — matches Zed's `Entry.path:
   * Arc<RelPath>` (worktree.rs:3573), VS Code's `Uri.path` vs `Uri.fsPath`,
   * and Obsidian's `TFile.path` convention: relative-everywhere internal
   * model, explicit toAbsolute() at the FS boundary.
   */
  private flattenTree(node: ListedNode, mainReconcile: boolean = true): void {
    const { host } = this;
    const rel = (n: ListedNode) => toRelative(n.path, host.projectRoot());
    // file_id (null on Windows or unknown fs) bypasses the registry's
    // byFileId index — graceful no-op when identity reuse can't apply.
    const entry = (n: ListedNode): SnapshotEntry => ({ path: rel(n), isDir: n.is_dir, fileId: n.file_id, isHome: n.is_home, hidden: n.hidden, special: n.special ?? null });
    const entries: SnapshotEntry[] = [entry(node)];
    const lists: [string, string[]][] = [];
    const walk = (n: ListedNode): void => {
      if (!n.children) return;
      lists.push([rel(n), n.children.map(rel)]);
      for (const child of n.children) {
        entries.push(entry(child));
        walk(child);
      }
    };
    walk(node);
    host.registry().reconcileSnapshot(entries, mainReconcile
      ? { pathPrefix: PROJECT_ROOT_REL, excludePrefixes: ['.moss'] }
      : { pathPrefix: '.moss' });
    // Every listed path was just reconciled, so each has an id.
    const idOf = (path: string): EntryId => {
      const id = host.registry().getByPath(path);
      if (id === null) throw new Error(`[file-tree] no registry id after reconcile: ${path}`);
      return id;
    };
    for (const [dir, childPaths] of lists) host.dirIndex.set(idOf(dir), childPaths.map(idOf));
  }

  /** Subscribe to filesystem changes and deferred directory completion. */
  async startWatching(signal: AbortSignal, fileEvents: FileEventsCoordinator | undefined): Promise<void> {
    const { host } = this;
    signal.addEventListener('abort', () => this.dispose());
    if (!host.projectRoot() || signal.aborted) return;
    const refreshWhileMounted = () => {
      if (!signal.aborted) return this.refreshAndRender();
    };
    try {
      const structureSubscription = onMossEvent('SourceStructureSettled', (msg) => {
        if (signal.aborted || !this.state.retryExhausted) return;
        const root = host.projectRoot();
        const { folder_path: folderPath, path } = msg.payload;
        if (!this.sameProjectFolder(folderPath, root) || !this.isWithinProject(path, root)) return;
        void this.refreshAndRender();
      });
      const fileChangedSubscription = fileEvents
        ? fileEvents.onFileChanged(refreshWhileMounted)
        : onMossEvent('FileChanged', refreshWhileMounted);
      const subscriptions = await Promise.allSettled([structureSubscription, fileChangedSubscription]);
      const unlisteners: Unlisten[] = [];
      let setupError: unknown;
      let failed = false;
      for (const subscription of subscriptions) {
        if (subscription.status === 'fulfilled') unlisteners.push(subscription.value);
        else {
          setupError = subscription.reason;
          failed = true;
        }
      }
      if (signal.aborted) {
        for (const unlisten of unlisteners) unlisten();
        return;
      }
      this.unlistenFns.push(...unlisteners);
      if (failed) throw setupError;
    } catch (err) {
      console.error('Failed to setup tree event listeners:', err);
    }
  }

  /** Accept the owning project directory or any descendant path. */
  private isWithinProject(path: string, root: string): boolean {
    if (!isAbsolutePath(path)) return false;
    const comparablePath = normalizePath(path);
    const comparableRoot = normalizePath(root);
    return comparablePath === comparableRoot || toRelative(comparablePath, comparableRoot) !== comparablePath;
  }

  /** Compare path identity while preserving the wire spelling for I/O. */
  private sameProjectFolder(folderPath: string, root: string): boolean {
    if (!isAbsolutePath(folderPath) || !isAbsolutePath(root)) return false;
    const comparableFolder = normalizePath(folderPath);
    const comparableRoot = normalizePath(root);
    return toRelative(comparableFolder, comparableRoot) === ''
      && toRelative(comparableRoot, comparableFolder) === '';
  }
}
