/**
 * Session-scoped file-identity registry for the moss action-panel webview.
 *
 * Each markdown source file (and each folder rendered in the file tree)
 * gets an opaque `EntryId` that survives rename. Two indices: `byId`
 * (id → record) and `byPath` (path → id). Lookup is "id first, path
 * fallback" per Zed's `BufferStore::local_worktree_entry_changed`.
 *
 * Lifetime: in-memory only. Resets on app reload (same as Zed/Helix —
 * persistence would require an IntelliJ-style on-disk VFS, which moss
 * doesn't need).
 *
 * **Path domain: project-root-relative.** All paths stored in this
 * registry (in `byPath` keys, `EntryRecord.path` values, and
 * `SnapshotEntry.path` arguments) are project-root-relative,
 * forward-slash-delimited. The project root itself is the empty string
 * `PROJECT_ROOT_REL` (exported from `path-domain.ts`).
 *
 * This convention matches the cross-app convergence:
 *   - Zed: `Entry.path: Arc<RelPath>` (https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs)
 *   - VS Code: `Uri.path` (URI hierarchical part) vs `Uri.fsPath` (OS-resolved)
 *   - Obsidian: `TFile.path` (vault-relative, the only addressing form)
 *
 * Callers MUST convert via `path-domain.ts` helpers at the boundaries:
 *   - **Ingest** (TreeNode.path / Tauri responses → registry): `toRelative(absPath, projectRoot)`
 *   - **Egress** (registry → Tauri commands that open files / display abs paths): `toAbsolute(record.path, projectRoot)`
 *
 * The registry enforces the relative-path contract at mutating boundaries:
 * passing an absolute path (leading `/`) throws. Callers must convert via
 * `toRelative(absPath, projectRoot)` before calling `upsert`, `rename`, or
 * `delete`.
 *
 * Three mutating operations correspond to the three source-domain wire
 * fields on `FileChangeEvent`:
 *   - `upsert(path, isDir, fileId?)` ← `FileChangeEvent.source_creates` + file-tree render
 *   - `rename(oldPath, newPath)` ← `FileChangeEvent.source_renames` + in-app rename
 *   - `delete(path)` ← `FileChangeEvent.source_deletes` + in-app delete
 *
 * `rename` and `delete` act on the SUBTREE: the registry is the file tree's
 * model, and a
 * folder moves or goes with everything under it.
 *
 * **Unicode spelling.** `byPath` is keyed in comparison form (`normalizePath`,
 * NFC) while `EntryRecord.path` keeps the bytes disk reported: a name the user
 * typed precomposed and the same name a macOS listing reports decomposed are
 * one record, and every filesystem call still derives from disk's spelling —
 * which is what makes a normalization-SENSITIVE filesystem (ext4) able to open
 * the file (see `normalizePath`'s comment for why the stored path is never
 * rewritten).
 */

import { normalizePath } from './path-domain';
import type { SpecialFileKind } from './session/special-file';

/** Branded `number` so it can't be confused with an unrelated numeric id. */
export type EntryId = number & { readonly __brand: 'EntryId' };

export interface EntryRecord {
  id: EntryId;
  /** Project-root-relative path, forward-slash-delimited
   *  (e.g. `"works/post.md"`, NOT `"/Users/me/Sites/blog/works/post.md"`),
   *  spelled the way the filesystem listed it. Convert to absolute via
   *  `toAbsolute(record.path, projectRoot)` before any Tauri command that
   *  opens the file via `std::path`. */
  path: string;
  isDir: boolean;
  /** Opaque cross-platform identity (Rust u64-as-string), `null` when not
   *  extractable. Tracked when known so retirement can clean byFileId.
   *  Mirrors `SnapshotEntry.fileId` and `DirEntry::file_id` / `TreeNode::file_id`. */
  fileId: string | null;
  /** The backend's per-folder home election (`TreeNode.is_home`, at most one
   *  true per folder). Only a listing knows it; `upsert` records `false`. */
  isHome: boolean;
  /** `TreeNode.hidden` — how many of this directory's own immediate children
   *  moss's curated tree filtered out (its own `.moss/` housekeeping,
   *  never OS/VCS noise). `0` for files and for a folder nothing was filtered
   *  from. Only a listing knows it; `upsert` carries the record's existing
   *  value forward, following the `isHome` precedent. */
  hidden: number;
  /** `TreeNode.special` — the role moss reads from the file. Only a listing
   *  knows it; `upsert` carries the record's existing value forward. */
  special: SpecialFileKind | null;
}

export interface SnapshotEntry {
  /** Project-root-relative path. Callers (e.g. `FileTree.flattenTree`)
   *  MUST convert from absolute `TreeNode.path` via `toRelative()` before
   *  pushing to reconcileSnapshot's entries array. */
  path: string;
  isDir: boolean;
  /** Opaque cross-platform identity. `null` means not extractable —
   *  byFileId is bypassed (matches today's path-only correctness).
   *  Mirrors Rust DirEntry::file_id / TreeNode::file_id.
   */
  fileId: string | null;
  /** `TreeNode.is_home`; absent means not elected. */
  isHome?: boolean;
  /** `TreeNode.hidden`; absent means `0` (see `EntryRecord.hidden`). */
  hidden?: number;
  /** `TreeNode.special`; absent means a plain file. */
  special?: SpecialFileKind | null;
}

export interface ReconcileResult {
  minted: EntryId[];
  retired: EntryId[];
  preserved: EntryId[];
}

export interface ReconcileOptions {
  /**
   * Path prefix that defines the snapshot's scope. Entries in the
   * registry whose path is NOT under this prefix are exempt from
   * retirement.
   *
   * Trailing slash is normalized away so '/foo/' and '/foo' are
   * equivalent. The empty default ('') matches everything (full-registry
   * reconcile).
   */
  pathPrefix?: string;

  /**
   * Paths matching any of these prefixes survive retirement even if
   * they fall inside `pathPrefix`. Used when a sub-subtree is loaded
   * separately by a different call (e.g., .moss/ via mergeInternalSubtree
   * vs. the main project tree via flattenTree).
   *
   * Each excluded prefix is independently normalized like pathPrefix.
   */
  excludePrefixes?: string[];
}

export class EntryRegistry {
  private byId = new Map<EntryId, EntryRecord>();
  /** Keyed by `normalizePath(path)` — see the module comment. */
  private byPath = new Map<string, EntryId>();
  /** Inode → EntryId index for identity reuse during reconcileSnapshot.
   *  Populated alongside byPath/byId in upsert and reconcileSnapshot Pass 1.
   *  Cleaned in delete and reconcileSnapshot Pass 2.
   *  Never contains a `null` key — callers MUST guard before insertion. */
  private byFileId = new Map<string, EntryId>();
  private nextId = 1;

  /**
   * Enforces the relative-path contract at mutating boundaries. Throws if
   * the caller passes an absolute path (leading `/`) — registry paths are
   * project-root-relative; an absolute path is a programming error.
   *
   * Convert via `toRelative(path, projectRoot)` before calling any mutating
   * method. The function returns the input unchanged so call sites read
   * cleanly when the path is already relative.
   */
  private guardRelative(method: string, path: string): string {
    if (path.startsWith('/')) {
      throw new Error(
        `[EntryRegistry.${method}] absolute path '${path}' — registry expects project-root-relative. ` +
        `Convert via toRelative(path, projectRoot) at the caller.`,
      );
    }
    return path;
  }
  private _activeEntryId: EntryId | null = null;
  private activeSubscribers = new Set<(newId: EntryId | null, oldId: EntryId | null) => void>();

  /**
   * The canonical "which file is the editor showing". Single source of
   * truth for active-file identity. Read freely; write only via setActive.
   *
   * Distinct from FileSelection.focus (file-tree's visual / keyboard
   * focus), which is downstream of this via a subscriber registered in
   * FileTree.mount. They coincide on click but can diverge during
   * multi-select keyboard navigation: visual focus moves while the
   * editor's active file does not until the user presses Enter.
   *
   * Lifetime: cleared on delete(path) when the deleted entry is active.
   * Preserved across rename: id is stable, only path changes.
   */
  get activeEntryId(): EntryId | null {
    return this._activeEntryId;
  }

  /**
   * Set the canonical active EntryId. No-op when value unchanged.
   * Emits to subscribers on change.
   */
  setActive(id: EntryId | null): void {
    if (this._activeEntryId === id) return;
    const oldId = this._activeEntryId;
    const oldPath = oldId !== null ? (this.byId.get(oldId)?.path ?? null) : null;
    this._activeEntryId = id;
    const newPath = id !== null ? (this.byId.get(id)?.path ?? null) : null;
    this.notifyActiveSubscribers(id, oldId);
    this.notifyActivePathSubscribers(newPath, oldPath);
  }

  /**
   * Subscribe to active-id changes. Returns an unsubscribe function.
   *
   * Subscribers fire in registration order (Set insertion order, spec'd
   * by ECMA-262, section 24.1). Do not rely on cross-subscriber ordering for
   * correctness.
   *
   * Pattern adopted from Zed's worktree entry_changed notification:
   * https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs
   */
  subscribeActive(
    handler: (newId: EntryId | null, oldId: EntryId | null) => void,
  ): () => void {
    this.activeSubscribers.add(handler);
    return () => { this.activeSubscribers.delete(handler); };
  }

  /**
   * Dispatch active-id change to subscribers. Each callback is wrapped
   * in try/catch so one throwing does not abort the rest of the chain.
   */
  private notifyActiveSubscribers(newId: EntryId | null, oldId: EntryId | null): void {
    for (const fn of this.activeSubscribers) {
      try {
        fn(newId, oldId);
      } catch (e) {
        console.error('[EntryRegistry] activeEntryId subscriber threw:', e);
      }
    }
  }

  private activePathSubscribers = new Set<(newPath: string | null, oldPath: string | null) => void>();

  /**
   * Subscribe to active-document-path changes. Fires whenever the path
   * associated with the active EntryId changes:
   *   - `setActive(id)`: when the new id's path differs from the old's
   *     (or either is null)
   *   - `rename(oldPath, newPath)` when the renamed entry is active
   *
   * Distinct from `subscribeActive` which fires on EntryId identity changes
   * only. Use `subscribeActivePath` when you care about "where the editor's
   * file lives now" rather than "which EntryId is active." The PreviewFollower
   * uses this signal; file-tree visual focus uses `subscribeActive`.
   *
   * Signature mirrors Obsidian's vault.on('rename', file, oldPath) callback:
   * https://docs.obsidian.md/Reference/TypeScript+API/Vault/on
   *
   * NOT wired into delete() or reconcileSnapshot retirement.
   */
  subscribeActivePath(
    handler: (newPath: string | null, oldPath: string | null) => void,
  ): () => void {
    this.activePathSubscribers.add(handler);
    return () => { this.activePathSubscribers.delete(handler); };
  }

  private notifyActivePathSubscribers(newPath: string | null, oldPath: string | null): void {
    if (newPath === oldPath) return;
    for (const fn of this.activePathSubscribers) {
      try {
        fn(newPath, oldPath);
      } catch (e) {
        console.error('[EntryRegistry] activeEntryPath subscriber threw:', e);
      }
    }
  }

  /**
   * Return the existing EntryId for `path`, or mint a new one if unknown.
   * Idempotent — calling repeatedly with the same path returns the same id.
   * Updates `isDir` on the existing record if the value differs (the tree's
   * second refresh of a path may discover it's a folder after a first
   * registration that defaulted to `false`).
   */
  upsert(path: string, isDir: boolean, fileId: string | null = null): EntryId {
    this.guardRelative('upsert', path);
    const existing = this.byPath.get(normalizePath(path));
    if (existing !== undefined) {
      const record = this.byId.get(existing);
      if (record) this.refresh(record, { path: record.path, isDir, fileId, isHome: record.isHome, hidden: record.hidden, special: record.special });
      return existing;
    }
    return this.mint({ path, isDir, fileId });
  }

  /** A brand-new record; `isHome` is a listing fact and defaults to false. */
  private mint(entry: SnapshotEntry): EntryId {
    const id = this.nextId++ as EntryId;
    const fileId = entry.fileId;
    this.byPath.set(normalizePath(entry.path), id);
    this.byId.set(id, { id, path: entry.path, isDir: entry.isDir, fileId, isHome: entry.isHome ?? false, hidden: entry.hidden ?? 0, special: entry.special ?? null });
    if (fileId !== null) this.byFileId.set(fileId, id);
    return id;
  }

  /**
   * Bring an existing record up to date with a listing of the same path:
   * disk's spelling, `isDir`, `fileId` (a null fileId keeps the known one)
   * and the home election (absent means not elected). Re-keys nothing — the
   * caller owns `byPath`.
   */
  private refresh(record: EntryRecord, entry: SnapshotEntry): void {
    const fileId = entry.fileId ?? record.fileId;
    const isHome = entry.isHome ?? false;
    const hidden = entry.hidden ?? 0;
    const special = entry.special ?? null;
    if (record.path === entry.path && record.isDir === entry.isDir && record.fileId === fileId && record.isHome === isHome && record.hidden === hidden && record.special === special) return;
    if (record.fileId !== null && record.fileId !== fileId) this.byFileId.delete(record.fileId);
    this.byId.set(record.id, { ...record, path: entry.path, isDir: entry.isDir, fileId, isHome, hidden, special });
    if (fileId !== null) this.byFileId.set(fileId, record.id);
  }

  /** `record` and every record under it, deepest last. Matched on the stored
   *  bytes: a child the watcher upserted in a different spelling from its
   *  listed parent is not carried, and the next listing refreshes it. */
  private subtree(record: EntryRecord): EntryRecord[] {
    const prefix = `${record.path}/`;
    const under = [...this.byId.values()].filter(r => r !== record && r.path.startsWith(prefix));
    return [record, ...under];
  }

  /**
   * Move an existing EntryId from `oldPath` to `newPath`, and every record
   * under it along with it. Returns the id, or `null` if `oldPath` was
   * unknown (no-op). If `newPath` was already registered, its id is
   * overwritten — the surviving identity is the one coming from `oldPath`.
   * (Defensive: the filesystem rejects a rename whose target exists, so the
   * watcher should never emit such a pair, but the registry must not corrupt
   * state if it happens.)
   */
  rename(oldPath: string, newPath: string): EntryId | null {
    this.guardRelative('rename', oldPath);
    this.guardRelative('rename', newPath);
    const id = this.byPath.get(normalizePath(oldPath));
    if (id === undefined) return null;
    const record = this.byId.get(id);
    if (!record) return null;
    const destExisting = this.byPath.get(normalizePath(newPath));
    if (destExisting !== undefined && destExisting !== id) {
      // Defensive: also clean byFileId mapping for the orphaned EntryId
      // we're about to overwrite. Otherwise byFileId could keep pointing at
      // a deleted byId entry.
      const destRecord = this.byId.get(destExisting);
      this.byId.delete(destExisting);
      if (destRecord !== undefined && destRecord.fileId !== null) {
        this.byFileId.delete(destRecord.fileId);
      }
    }
    // byFileId is unchanged: rename preserves identity, so the same inode
    // still maps to the same EntryId. Each record's path is the only thing
    // that changes. The tail is cut at the RECORD's length, not `oldPath`'s:
    // the two may be the same folder in different Unicode spellings, and
    // those differ in length.
    for (const r of this.subtree(record)) {
      const moved = newPath + r.path.slice(record.path.length);
      this.byPath.delete(normalizePath(r.path));
      this.byPath.set(normalizePath(moved), r.id);
      this.byId.set(r.id, { ...r, path: moved });
      if (this._activeEntryId === r.id) this.notifyActivePathSubscribers(moved, r.path);
    }
    return id;
  }

  /**
   * Retire the EntryId associated with `path` and every record under it.
   * Clears every index so a subsequent `upsert` at the same path mints a new
   * id (semantically a different file at the same path is a different
   * identity). No-op if `path` is unknown.
   */
  delete(path: string): void {
    this.guardRelative('delete', path);
    const id = this.byPath.get(normalizePath(path));
    const record = id === undefined ? undefined : this.byId.get(id);
    if (!record) return;
    for (const r of this.subtree(record)) this.retire(r);
  }

  /** Drop one record from every index; clears and notifies active if it was. */
  private retire(record: EntryRecord): void {
    this.byPath.delete(normalizePath(record.path));
    this.byId.delete(record.id);
    // Without this, a subsequent reconcileSnapshot could reuse the EntryId
    // via byFileId for a new file with a recycled inode.
    if (record.fileId !== null) this.byFileId.delete(record.fileId);
    // A dangling active id would break getById-based path resolution downstream.
    if (this._activeEntryId === record.id) {
      this._activeEntryId = null;
      this.notifyActiveSubscribers(null, record.id);
    }
  }

  /** Returns the EntryId for `path` in either Unicode spelling, or `null` if unknown. */
  getByPath(path: string): EntryId | null {
    return this.byPath.get(normalizePath(path)) ?? null;
  }

  /** Returns the full record for `id`, or `null` if unknown. */
  getById(id: EntryId): EntryRecord | null {
    return this.byId.get(id) ?? null;
  }

  /** The record at `path`, in either Unicode spelling, or `null` if unknown. */
  recordAt(path: string): EntryRecord | null {
    const id = this.getByPath(path);
    return id === null ? null : this.getById(id);
  }

  /** The project-root-relative path of `id`, or `null` if unknown (or `id` is null). */
  pathOf(id: EntryId | null): string | null {
    return id === null ? null : (this.getById(id)?.path ?? null);
  }

  /**
   * All live EntryIds known to the registry. Used by `FileTree.refreshTree`
   * to build the valid-id set for `FileSelection.reconcile()` after a
   * watcher event so selected ids that no longer exist on disk get dropped.
   */
  allIds(): EntryId[] {
    return [...this.byId.keys()];
  }

  /**
   * Reconcile the registry against a fresh filesystem snapshot.
   *
   * Diff-based identity preservation:
   *   - Paths in both registry and snapshot: existing EntryId preserved
   *   - Paths only in snapshot: new EntryId minted
   *   - Paths only in registry (in pathPrefix, not in excludePrefixes):
   *     EntryId retired
   *
   * If the active EntryId is among the retired set, activeEntryId is
   * cleared and subscribers fire (null, oldId).
   *
   * Pattern: Zed Worktree::scan_dir reconciliation
   * (https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs).
   */
  reconcileSnapshot(
    entries: SnapshotEntry[],
    options: ReconcileOptions = {},
  ): ReconcileResult {
    const normalize = (p: string): string => p.replace(/\/$/, '');
    const pathPrefix = normalize(options.pathPrefix ?? '');
    const excludePrefixes = (options.excludePrefixes ?? []).map(normalize);

    const snapshotKeys = new Set(entries.map((e) => normalizePath(e.path)));

    const minted: EntryId[] = [];
    const retired: EntryId[] = [];
    const preserved: EntryId[] = [];

    // Pass 1: preserve (path match) → reuse (fileId match at different
    // path) → mint (truly new). Identity reuse via fileId is Zed-inspired
    // (reuse_entry_id at
    // https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs#L3035),
    // but structurally distinct: Zed uses a per-scan removed_entries set
    // with mtime guard; moss uses a live byFileId index without mtime
    // guard. Documented limitation: within-single-reconcile inode-recycle
    // collision is possible — see test in entry-registry.test.ts.
    for (const entry of entries) {
      const key = normalizePath(entry.path);
      const existingByPath = this.byPath.get(key);
      if (existingByPath !== undefined) {
        // Path-based preserve; the record takes the listing's facts, its
        // spelling included.
        const record = this.byId.get(existingByPath);
        if (record) this.refresh(record, entry);
        preserved.push(existingByPath);
        continue;
      }

      // No byPath match. Try byFileId for identity reuse (the rename case).
      if (entry.fileId !== null) {
        const existingByFileId = this.byFileId.get(entry.fileId);
        if (existingByFileId !== undefined) {
          const record = this.byId.get(existingByFileId);
          if (record === undefined) {
            // Defensive: byFileId points at an EntryId not in byId. Clean
            // the stale entry and fall through to mint.
            this.byFileId.delete(entry.fileId);
          } else if (snapshotKeys.has(normalizePath(record.path))) {
            // The existing entry's path is ALSO in the new snapshot —
            // hardlink-like / aliasing case. Don't move (would corrupt the
            // path-preserved entry). Fall through to mint a fresh EntryId
            // for entry.path. byFileId keeps pointing at the path-preserved
            // entry.
          } else {
            // Move the EntryId from its current path to entry.path.
            // The old path's byPath mapping is removed so Pass 2 doesn't
            // retire this EntryId.
            this.byPath.delete(normalizePath(record.path));
            this.byPath.set(key, existingByFileId);
            this.refresh(record, entry);
            // byFileId mapping stays at the same inode → same EntryId.
            preserved.push(existingByFileId);
            continue;
          }
        }
      }

      minted.push(this.mint(entry));
    }

    // Pass 2: retire entries in pathPrefix, not in excludePrefixes, and
    // not in the snapshot. Iterate a copy so we can mutate the indices safely.
    const isInPrefix = (path: string): boolean => {
      if (pathPrefix === '') return true;
      return path === pathPrefix || path.startsWith(`${pathPrefix}/`);
    };
    const isExcluded = (path: string): boolean => {
      for (const ex of excludePrefixes) {
        if (path === ex || path.startsWith(`${ex}/`)) return true;
      }
      return false;
    };
    for (const record of [...this.byId.values()]) {
      if (!snapshotKeys.has(normalizePath(record.path)) && isInPrefix(record.path) && !isExcluded(record.path)) {
        this.retire(record); // clears and notifies active if it was this one
        retired.push(record.id);
      }
    }

    return { minted, retired, preserved };
  }
}
