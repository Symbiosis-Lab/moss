/**
 * The vault operations the context menus and the keyboard reach for: delete
 * (with the reference check), duplicate, move, reveal, copy, URL import.
 * Each one asks the engine (or, for reveal and URL import, the host),
 * surfaces its error, and applies the result locally through the controller —
 * `forget` for a deleted subtree, `update` for the breadcrumb — so the tree
 * reflects the change before the file watcher confirms it.
 */

import { host as uiHost, type Documents } from "../../host";
import { mossAlert } from "../../shared/ui/moss-alert";
import { t } from "../../i18n/index";
import { showToast, showAck, showOrUpdateToast, removeToastById } from "../../shared/ui/toast/toast-manager";
import { runAction } from "../../shared/ui/run-action";
import { toAbsolute, toRelative, parentDirOf, baseName, childPath, shortenKnownPaths } from '../path-domain';
import type { EntryRegistry, EntryRecord } from '../entry-registry';
import { DeleteConfirmModal, type DeleteQuestion } from '../delete-confirm-modal';
import type { FileReferenceHit } from '@symbiosis-lab/moss-ui';
import { reanchorAfterMove, type TreeView } from './model';
import type { TreeRefresh, CommandResult } from './refresh';
import type { TreeEditSession } from './edit-session';
import {
  showRenameSummaryToast, mapMovedPath, performRenameUndo,
  type RenameToastSummary, type RenameUndoHost, type UndoResultShape,
} from './rename-toast';

/** The engine commands these operations call. */
export interface OpsCommands {
  duplicateEntry(abs: string): Promise<CommandResult<string>>;
  deleteEntries(absPaths: string[]): Promise<CommandResult<unknown>>;
  cleanReferencesAndDelete(absPaths: string[]): Promise<CommandResult<unknown>>;
  scanReferencesForDelete(absPaths: string[]): Promise<CommandResult<FileReferenceHit[]>>;
  /** The toast's Undo action — reverses the applied edits `undo_id` names. */
  undoRename(undoId: string): Promise<CommandResult<UndoResultShape>>;
}

/** The controller interface used by the operations; it exposes only their coordination needs. */
export interface OpsHost {
  projectRoot(): string;
  readonly commands: OpsCommands;
  /** The tree model: the record at a path, the open file (`activeEntryId`). */
  registry(): EntryRegistry;
  /** A folder's children in display order, or null when the index has no entry for it. */
  children(folderPath: string): EntryRecord[] | null;
  /** May a refresh-shaped purge run now, or is an inline edit open? */
  readonly gate: { admit(work: 'refresh'): boolean };
  /** The refresh cycle: `forget` unlinks a deleted subtree, `refreshTree` re-lists after a move or duplicate. */
  readonly refresh: Pick<TreeRefresh, 'forget' | 'refreshTree'>;
  /** The rename-complete broadcast a move of the ACTIVE file ends with (moveEntriesTo). */
  readonly editSession: Pick<TreeEditSession, 'broadcastRenameComplete'>;
  render(): void;
  update(fn: (view: TreeView) => TreeView): void;
  /** Point the breadcrumb and the editor surface at `folder` (its home, or the create-page CTA) and select it. */
  showFolder(folder: string): void;
  /** The open documents: the pending save a move flushes first, the move itself, and the open file following its path. */
  readonly documents: Documents;
  /** Freeze/unfreeze the open buffer for a commit's duration — see `EditorCommitHooks` (controller.ts). */
  setEditorCommitLock(locked: boolean): void;
  /** Re-parse the active file from disk once a commit (or its undo) completes — see `EditorCommitHooks` (controller.ts). */
  reloadActiveFileFromDisk(): Promise<void>;
}

/** How long a trash command runs before the status toast appears. */
const SLOW_TRASH_TOAST_MS = 2000;
/** Distinct toast id per trash run, so overlapping deletes can't clear each other's. */
let slowTrashToastSeq = 0;

export class TreeOps {
  constructor(private readonly host: OpsHost) {}

  async runUrlImport(url: string, folderAbsPath: string): Promise<void> {
    const importSite = uiHost.importSite;
    if (!importSite) return;
    const result = await importSite(url, folderAbsPath);
    if (result.status === 'error') {
      showToast({ what: result.error, severity: 'Blocking' });
      return;
    }
    if (!result.data.success) {
      showToast({ what: result.data.error ?? t('onboard.flow.url_invalid'), severity: 'Blocking' });
      return;
    }
    const total = result.data.total_pages;
    if (total > 0) {
      showAck(t('onboard.flow.url_imported_pages', { total: String(total) }));
    }
  }

  // ── Reveal ─────────────────────────────────────────────

  async revealEntry(path: string): Promise<void> {
    if (!uiHost.reveal) return;
    // The registry stores relative paths; the host opens absolute ones.
    const result = await uiHost.reveal(toAbsolute(path, this.host.projectRoot()));
    if (result.status !== 'ok') {
      console.error('[file-tree] reveal failed:', result.error);
    }
  }

  async duplicateEntry(path: string): Promise<void> {
    const result = await this.host.commands.duplicateEntry(toAbsolute(path, this.host.projectRoot()));
    if (result.status !== 'ok') {
      console.error('[file-tree] duplicate failed:', result.error);
      void mossAlert(t('tree.duplicate_failed').replace('{error}', String(result.error)));
      return;
    }
    await this.host.refresh.refreshTree();
    this.host.render();
  }

  async copyToClipboard(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch (err) {
      console.error('[file-tree] copy to clipboard failed:', err);
      void mossAlert(t('tree.copy_clipboard_failed'));
    }
  }

  /**
   * Trash `paths` (relative), one or many: the context menus and Cmd+Backspace
   * all arrive here. The modal opens in THIS call's frame with the question,
   * and the project-wide reference scan resolves into it — it used to sit in
   * front of a native prompt, and then behind the answer, as seconds of dead
   * air.md.
   *
   * Runs under a keyed `runAction`, so a repeat Delete on the same selection
   * is refused for exactly the span from the click to the decision settling.
   */
  async deleteEntries(paths: string[]): Promise<void> {
    if (paths.length === 0) return;
    const absPaths = paths.map(p => toAbsolute(p, this.host.projectRoot()));
    await runAction({
      key: `tree.delete:${absPaths.join('\n')}`,
      run: () => this.decideAndTrash(paths, absPaths),
      // The one failure surface: a rejected command, an error-shaped result
      // (`trash` throws it) and a thrown callback all land here. The Rust
      // error echoes back whichever absolute path failed; shorten it to a
      // basename since the full path is noise in a toast.
      failMessage: error => t('tree.delete_failed', { error: shortenKnownPaths(error, absPaths) }),
    });
  }

  private async decideAndTrash(paths: string[], absPaths: string[]): Promise<void> {
    const { host } = this;
    const scan = this.scanReferences(absPaths);
    // An EMPTY folder has never prompted: trash on a clean scan,
    // and only hits open the modal — as the disclosure from its first paint,
    // since the scan it is handed has already resolved.
    if (paths.length === 1 && this.folderChildCount(paths[0]) === 0 && (await scan).length === 0) {
      await this.trash(paths, abs => host.commands.deleteEntries(abs));
      return;
    }
    await new DeleteConfirmModal({
      question: this.deleteQuestion(paths),
      hits: scan,
      onTrash: () => this.trash(paths, abs => host.commands.deleteEntries(abs)),
      onCleanAndTrash: () => this.trash(paths, abs => host.commands.cleanReferencesAndDelete(abs)),
    }).run();
  }

  /** How many entries a folder holds (an unlisted folder counts as 0); null for a file. */
  private folderChildCount(path: string): number | null {
    if (this.host.registry().recordAt(path)?.isDir !== true) return null;
    return this.host.children(path)?.length ?? 0;
  }

  private deleteQuestion(paths: string[]): DeleteQuestion {
    if (paths.length > 1) return { title: t('tree.trash_many', { n: paths.length }) };
    const [path] = paths;
    const title = t('tree.trash_one', { name: baseName(path) });
    const count = this.folderChildCount(path);
    if (!count) return { title }; // a file (null) or an empty folder (0): no detail line
    return {
      title,
      detail: count === 1 ? t('tree.trash_contains_one') : t('tree.trash_contains', { n: count }),
    };
  }

  /**
   * Every reference in the project that points at `absPaths`. A scan that
   * errors or throws resolves to "no hits" and warns: a bookkeeping failure
   * must not block a delete the user asked for. (This is the failure policy
   * the modal's `hits` contract relies on — it never rejects.)
   */
  private async scanReferences(absPaths: string[]): Promise<FileReferenceHit[]> {
    try {
      const result = await this.host.commands.scanReferencesForDelete(absPaths);
      if (result.status === 'ok') return result.data;
      console.warn('[file-tree] ref scan returned error, proceeding with delete:', result.error);
    } catch (err) {
      console.warn('[file-tree] ref scan failed, proceeding with delete:', err);
    }
    return [];
  }

  /**
   * Run the trash command (plain, or clean-references-then-trash) over the
   * absolute paths and apply the deletion locally. `delete_entries` is a loop
   * over the same `delete_one` the single-entry command calls, so one path
   * serves N = 1 too. An error-shaped result is thrown so it reaches the
   * `runAction` in `deleteEntries` like any other failure.
   *
   * A command outliving {@link SLOW_TRASH_TOAST_MS} raises a persistent status
   * toast, cleared when it settles — on a cloud-synced vault the OS can hold a
   * trash for seconds, and that span used to be dead air the user read as a
   * broken delete.
   */
  private async trash(
    paths: string[],
    command: (absPaths: string[]) => Promise<CommandResult<unknown>>,
  ): Promise<void> {
    const toastId = `tree-trash-slow-${++slowTrashToastSeq}`;
    const slowTimer = setTimeout(() => {
      const message = paths.length === 1
        ? t('tree.trash_slow_one', { name: baseName(paths[0]) })
        : t('tree.trash_slow_many', { n: paths.length });
      showOrUpdateToast(message, { id: toastId, persistent: true });
    }, SLOW_TRASH_TOAST_MS);
    try {
      const result = await command(paths.map(p => toAbsolute(p, this.host.projectRoot())));
      if (result.status !== 'ok') throw new Error(String(result.error));
      this.applyDeleteLocally(paths);
    } finally {
      clearTimeout(slowTimer);
      removeToastById(toastId);
    }
  }

  /**
   * Surgically remove deleted paths from all in-memory state and re-render.
   *
   * Called immediately after the trash command returns `Ok` so the tree reflects the deletion without waiting for the OS
   * file-watcher notification chain. The watcher still fires and calls
   * `refreshTree()` afterward — that is idempotent and just confirms the
   * already-correct state.
   *
   * For each top-level path, `forget(record)`: unlink from the parent's child
   * list, clear `dirIndex` and `overrides` for the subtree, retire its
   * EntryIds (clearing `activeEntryId` if it pointed at any of them). Then
   * re-renders.
   *
   * If the active file is among the deleted paths, both the editor and preview
   * fall back to the parent folder via `showFolder` after the purge.
   *
   * Guards: parks a `refresh` behind an open inline edit rather than purging
   * under it; the parked work re-reads disk when the edit concludes.
   */
  private applyDeleteLocally(paths: string[]): void {
    const { host } = this;
    if (!host.gate.admit('refresh')) return;

    // Capture fallback BEFORE purging: if the active file is being deleted,
    // record its parent folder so we can navigate there after the purge.
    const registry = host.registry();
    const activePath = registry.pathOf(registry.activeEntryId);
    let fallbackFolder: string | null = null;
    if (activePath !== null) {
      for (const path of paths) {
        if (activePath === path || activePath.startsWith(`${path}/`)) {
          fallbackFolder = parentDirOf(path);
          break;
        }
      }
    }

    for (const path of paths) {
      const record = registry.recordAt(path);
      if (record !== null) host.refresh.forget(record);
    }
    host.render();

    // The open file was deleted: a FORCED navigation to its parent (a plain
    // folder click only expands and selects) — showFolder owns all of it.
    if (fallbackFolder !== null) host.showFolder(fallbackFolder);
  }

  /**
   * Batch move N entries to a user-chosen folder. Opens a native folder-picker
   * dialog (tauri-plugin-dialog) scoped to the project root. Uses
   * `move_entries` backend command so only ONE file-watcher refresh fires.
   *
   * If the user cancels the dialog, the operation is silently abandoned.
   * If the selected folder is outside the project root, an alert is shown —
   * this mirrors the backend's containment guard in `move_one`.
   */
  async promptMoveEntries(paths: string[]): Promise<void> {
    const { host } = this;
    if (paths.length === 0) return;

    // The host's folder picker, starting at the root; null is a cancel.
    const targetDir = await uiHost.pickFolder?.(host.projectRoot());
    if (!targetDir) return;

    // Client-side containment guard: verify the chosen folder is inside the
    // project root. The backend enforces this too (move_one), but a friendly
    // error here avoids a round-trip on the obvious mistake of picking ~/Desktop.
    // Separator-guarded: a bare prefix check would accept '/home/u/site2' for
    // root '/home/u/site' (otherwise the backend fails with a confusing error).
    if (targetDir !== host.projectRoot() && !targetDir.startsWith(host.projectRoot() + '/')) {
      void mossAlert(t('tree.must_inside').replace('{root}', host.projectRoot()));
      return;
    }

    // The picker answers absolute; the move speaks registry-relative.
    const result = await this.moveEntriesTo(paths, toRelative(targetDir, host.projectRoot()));
    if (result.status !== 'ok') {
      console.error('[file-tree] batch move failed:', result.error);
      void mossAlert(t('tree.move_failed').replace('{error}', String(result.error)));
    }
  }

  /**
   * The one move: `paths` (registry-relative) into `targetDirRel`, as a
   * SINGLE move_entries call plus a single refreshTree — a loop of
   * fire-and-forget moves raced the refresh. Shared by the drag-drop
   * (tree/dnd.ts performDrop) and Move to… (promptMoveEntries); the caller
   * owns the error UX and any arrival feedback, which is why the result
   * comes back rather than being surfaced here.
   *
   * Active-file tracking: moveEntries fires none of commitRename's lifecycle
   * events, so with the ACTIVE file among the moved set its EntryId would be
   * retired and the breadcrumb would keep showing the old location, and the
   * watcher echo would not be silenced. Mirror the editor-state half of
   * commitRename's order for the active file only:
   * renaming BEFORE the move, renamed (→ registry.rename remaps the EntryId)
   * AFTER success but BEFORE refreshTree, then re-anchor and broadcast
   * rename-complete. Non-active moved files reconcile in refreshTree.
   *
   * `move_entries` runs the same resolver-driven planner
   * `rename_entry_with_refs` does, so incoming [[wikilinks]]/references to a
   * moved file ARE rewritten project-wide. Its answer carries the batch's REAL destinations (collision-suffixed names
   * included), so the active-file lifecycle events and the reanchor below
   * use those, not the naive `childPath(targetDir, baseName)` guess a
   * suffix could make wrong. A move with ≥1 edited link shows the same
   * toast-with-Undo a rename does (`showRenameSummaryToast`).
   */
  async moveEntriesTo(paths: string[], targetDirRel: string): Promise<CommandResult<RenameToastSummary>> {
    const { host } = this;
    // Locked BEFORE the flush below, not after — a keystroke landing while
    // flushPendingSave's own IPC is still in flight would otherwise arm a
    // fresh autosave debounce that nothing then cancels, and it can fire
    // mid-move (or land after the reload, over the reloaded content) and
    // overwrite this move's own rewrite with whatever the buffer held before
    // the flush. Locking first makes the
    // flush itself the only save this whole commit can produce.
    host.setEditorCommitLock(true);
    let result: CommandResult<RenameToastSummary>;
    try {
      // The planner reads the CURRENT bytes on disk; an unflushed edit in the
      // open buffer is invisible to it, and a debounced autosave landing
      // after the move would overwrite its own file's just-rewritten links
      // with the stale pre-flush content. See commitRename's identical note.
      await host.documents.flush();
      const registry = host.registry();
      const activePath = registry.pathOf(registry.activeEntryId);
      const active = activePath !== null && paths.includes(activePath)
        ? { from: activePath, predictedTo: childPath(targetDirRel, baseName(activePath)) }
        : null;
      if (active !== null) host.documents.retarget(active.from, active.predictedTo, 'starting');
      // Tauri command boundary: registry stores relative, Rust opens via std::fs.
      result = await host.documents.move(
        paths.map(p => toAbsolute(p, host.projectRoot())),
        toAbsolute(targetDirRel, host.projectRoot()),
      );
      // The backend's OWN destination for the active file, if it actually
      // moved (directly, or carried inside a selected ancestor folder) —
      // `mapMovedPath` mirrors the Rust planner's own old→new resolution, so
      // this is exact even when a collision suffixed the name. `null` means
      // "no active-file lifecycle to fire": either there was no active file
      // among `paths`, or the move failed, or it turned out to be a no-op
      // (the active file's mapped destination is itself).
      const activeMove = (() => {
        if (active === null || result.status !== 'ok') return null;
        const to = mapMovedPath(active.from, result.data.moved);
        return to === active.from ? null : { from: active.from, to };
      })();
      if (activeMove !== null) {
        // BEFORE refreshTree: editor:file-renamed → registry.rename remaps the
        // active EntryId so reconcileSnapshot keeps it, not retires it.
        host.documents.retarget(activeMove.from, activeMove.to, 'done');
      }
      await host.refresh.refreshTree();
      if (result.status === 'ok') {
        // The breadcrumb stays frozen unless the anchor itself moved
        // Folded over every entry the backend actually moved,
        // at its real destination. Captured into `moved` (not read off
        // `result` inside the closure): `result` is a `let` reassigned
        // above, so TS narrowing from this `if` doesn't persist into it.
        const moved = result.data.moved;
        host.update(v => reanchorAfterMove(v, moved));
      }
      host.render();
      // The backend may have rewritten the open file's OWN content (a
      // rebased relative link) or, if it's some OTHER open page, its
      // incoming links — the rename lifecycle above is a path change only.
      await host.reloadActiveFileFromDisk();
      if (activeMove !== null) {
        host.editSession.broadcastRenameComplete(activeMove.from, activeMove.to);
      }
    } finally {
      host.setEditorCommitLock(false);
    }
    if (result.status === 'ok') {
      const summary = result.data;
      const itemCount = summary.moved.length;
      showRenameSummaryToast(
        itemCount > 1
          ? { kind: 'move-batch', itemCount }
          : { kind: 'move', itemName: baseName(summary.moved[0]?.new_path ?? '') },
        summary,
        () => performRenameUndo(undoHostAdapter(host), summary),
      );
    }
    return result;
  }
}

/** Adapts an {@link OpsHost} onto {@link RenameUndoHost} for
 *  `performRenameUndo` — `host.editSession` already carries the dispatch/
 *  broadcast methods `TreeEditSession` exposes for exactly this. */
function undoHostAdapter(host: OpsHost): RenameUndoHost {
  return {
    registry: () => host.registry(),
    documents: host.documents,
    broadcastRenameComplete: (a, b) => host.editSession.broadcastRenameComplete(a, b),
    refresh: { refreshTree: () => host.refresh.refreshTree() },
    reanchor: (moved) => host.update(v => reanchorAfterMove(v, moved)),
    setEditorCommitLock: (locked) => host.setEditorCommitLock(locked),
    reloadActiveFileFromDisk: () => host.reloadActiveFileFromDisk(),
    undoRename: (undoId) => host.commands.undoRename(undoId),
  };
}
