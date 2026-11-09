/**
 * The rename/move link-update toast: one message summarizing what a rename
 * or batch move did to project links, with an Undo action — the decided
 * design: no confirmation modal before the
 * rename/move, one toast after, offering Undo; no toast at all when nothing
 * was edited (same as today).
 *
 * Built from the backend's `RenameRefsSummary` (rename_entry_with_refs /
 * move_entries) — the raw applied edits never reach this module, only the
 * counts and names the message needs, plus the opaque `undo_id` the
 * caller's own `undoRename` command call takes. This module owns the
 * message text and the toast's own lifecycle; it knows nothing about the
 * file tree — the caller's `onUndo` does whatever a rename does to the UI
 * (tree refresh, active-EntryId re-keying, breadcrumb/preview follow).
 */
import type { Documents } from "../../host";
import { t } from "../../i18n/index";
import { showAck } from "../../shared/ui/toast/toast-manager";
import { runAction } from "../../shared/ui/run-action";

/**
 * The part of the generated `RenameRefsSummary` (`rename_entry_with_refs` /
 * `move_entries`) the toast reads; the controller calls the commands, through
 * `OpsCommands`/`EditSessionCommands`, and passes the summary in.
 */
export interface RenameToastMoveEntry {
  old_path: string;
  new_path: string;
  is_dir: boolean;
}

export interface RenameToastSummary {
  moved: RenameToastMoveEntry[];
  edited_link_count: number;
  edited_pages: string[];
  skipped_pages: string[];
  undo_id: string;
}

/**
 * How long the toast stays up. The app's default Ack duration (3000ms,
 * `toast-manager.ts`) is tuned for a pure confirmation nobody needs to act
 * on; this toast carries an action a person has to read the sentence and
 * then reach for, so it gets the same ballpark the versions-restore Undo
 * toast's own persistent treatment implies — long enough to read two short
 * sentences and click, short enough not to linger once read.
 */
export const RENAME_TOAST_DURATION_MS = 8000;

/** What the toast is about — the parts {@link buildRenameToastMessage} can't
 *  derive from the backend summary alone (display names, batch shape). */
export type RenameToastSubject =
  | { kind: 'rename'; oldName: string; newName: string }
  | { kind: 'move'; itemName: string }
  | { kind: 'move-batch'; itemCount: number };

/**
 * The toast message for `summary`, or `null` when nothing was edited — the
 * caller's cue to skip the toast entirely (no link edited ⇒ no toast, same
 * as today).
 *
 * English has no plural engine (`i18n/index.ts`): a `_one_page`/`_many_pages`
 * key pair covers the page-count branch the task's own template variants are
 * shaped around, and the one further inflection this text needs — "link" vs
 * "links" — is computed here and interpolated as `{linkWord}` rather than
 * doubling the key count for a distinction the zh templates don't have at
 * all (Chinese nouns don't inflect, so their templates never reference it).
 */
export function buildRenameToastMessage(
  subject: RenameToastSubject,
  summary: Pick<RenameToastSummary, 'edited_link_count' | 'edited_pages' | 'skipped_pages'>,
): string | null {
  if (summary.edited_link_count <= 0) return null;

  const n = summary.edited_link_count;
  const linkWord = n === 1 ? 'link' : 'links';
  const pages = summary.edited_pages;

  let main: string;
  if (subject.kind === 'rename') {
    const base = { old: subject.oldName, new: subject.newName, n, linkWord };
    main = pages.length === 1
      ? t('tree.rename_toast_one_page', { ...base, page: pages[0] })
      : t('tree.rename_toast_many_pages', { ...base, m: pages.length });
  } else if (subject.kind === 'move-batch') {
    main = t('tree.move_batch_toast', { k: subject.itemCount, n, linkWord, m: pages.length });
  } else {
    const base = { name: subject.itemName, n, linkWord };
    main = pages.length === 1
      ? t('tree.move_toast_one_page', { ...base, page: pages[0] })
      : t('tree.move_toast_many_pages', { ...base, m: pages.length });
  }

  if (summary.skipped_pages.length === 0) return main;
  const p = summary.skipped_pages.length;
  const pageWord = p === 1 ? 'page' : 'pages';
  const skipped = t('tree.rename_toast_skipped', { p, pageWord, names: summary.skipped_pages.join(', ') });
  return `${main} ${skipped}`;
}

/**
 * Show the toast for a completed rename/move, with an Undo action. No-ops
 * (shows nothing) when {@link buildRenameToastMessage} returns `null`.
 *
 * `onUndo` is the caller's own UI-refresh sequence; a failure it throws (or
 * an error-shaped Result it surfaces — same contract every other
 * runAction-driven trigger uses, see `versions-mode.ts`'s restore Undo)
 * becomes the standard failure toast, not a silent no-op.
 */
export function showRenameSummaryToast(
  subject: RenameToastSubject,
  summary: RenameToastSummary,
  onUndo: () => Promise<void>,
): void {
  const message = buildRenameToastMessage(subject, summary);
  if (message === null) return;
  showAck(message, {
    duration: RENAME_TOAST_DURATION_MS,
    actions: [{
      label: t('versions.undo'),
      onClick: () => void runAction({ key: 'rename-undo', run: onUndo }),
    }],
  });
}

/**
 * Map `path` through a [`RenameRefsSummary`]'s `moved` list the same way the
 * backend's own `map_path` does: an exact hit wins over a directory-prefix
 * hit, so a moved folder's own (more specific) file-level entry is never
 * shadowed by its folder's coarser prefix. Used to find where the ACTIVE
 * file actually landed — including when it lives inside a moved folder
 * rather than being its own entry in `moved` — since the naive
 * `childPath(targetDir, baseName)` prediction callers make before the
 * command runs can't know about a collision suffix.
 */
export function mapMovedPath(path: string, moved: readonly RenameToastMoveEntry[]): string {
  for (const m of moved) {
    if (m.old_path === path) return m.new_path;
  }
  for (const m of moved) {
    if (m.is_dir && path.startsWith(`${m.old_path}/`)) {
      return m.new_path + path.slice(m.old_path.length);
    }
  }
  return path;
}

/** Structural echo of the real `UndoResult` (`undo_applied` in moss-build). */
export interface UndoResultShape {
  restored_files: string[];
  skipped: { file: string; reason: string }[];
}

/** The toast's own "N page(s) left as-is" note, standalone — for when an
 *  UNDO itself skips stale pages (the applied edit's recorded text no
 *  longer matches what's on disk). Reuses the forward toast's own key. */
function buildSkippedPagesNote(skipped: readonly { file: string }[]): string {
  const p = skipped.length;
  const pageWord = p === 1 ? 'page' : 'pages';
  return t('tree.rename_toast_skipped', { p, pageWord, names: skipped.map((s) => s.file).join(', ') });
}

/** The minimal registry surface `performRenameUndo` needs, written
 *  structurally so this module doesn't have to import `EntryRegistry`. */
export interface RenameUndoRegistry {
  pathOf(id: unknown): string | null;
  readonly activeEntryId: unknown;
  rename(oldPath: string, newPath: string): void;
}

/** What `performRenameUndo` needs from its caller — `edit-session.ts` and
 *  `ops.ts` each build one of these from their own (differently-shaped)
 *  host, which is why this stays a plain parameter object rather than
 *  something either host implements directly. */
export interface RenameUndoHost {
  registry(): RenameUndoRegistry;
  /** The open documents: the pending save, and the open file following its path. */
  readonly documents: Pick<Documents, 'flush' | 'retarget'>;
  broadcastRenameComplete(oldPath: string, newPath: string): void;
  readonly refresh: { refreshTree(): Promise<unknown> };
  /** Reanchor the breadcrumb after these (already-reversed) moves — the
   *  caller wraps its own `host.update(v => reanchorAfterMove(v, moved))`;
   *  this module doesn't import `TreeView`/`model.ts`. */
  reanchor(moved: readonly RenameToastMoveEntry[]): void;
  setEditorCommitLock(locked: boolean): void;
  reloadActiveFileFromDisk(): Promise<void>;
  undoRename(undoId: string): Promise<{ status: 'ok'; data: UndoResultShape } | { status: 'error'; error: string }>;
}

/**
 * The toast's Undo action, shared by a single rename (edit-session.ts) and
 * a batch move (ops.ts) alike — both reduce to "reverse `summary.moved`".
 *
 * Flushes any pending save first (same reasoning as the forward commit:
 * `undo_applied`'s server-side staleness check compares against disk, not
 * the open buffer), locks the editor for the round trip (see
 * `EditorCommitHooks` in controller.ts), mirrors the active-file lifecycle
 * in reverse — dispatch/refresh/reanchor/broadcast, only when the active
 * file was among the reversed entries, directly or inside a reversed
 * folder — reloads the active buffer from disk, and, if the backend left
 * any page as-is because it changed since the rename, shows a toast naming
 * them (a fully-successful undo shows nothing further — the tree visibly
 * changing back is confirmation enough).
 */
export async function performRenameUndo(host: RenameUndoHost, summary: RenameToastSummary): Promise<void> {
  // Locked BEFORE the flush below, not after — same reasoning as
  // commitRename/moveEntriesTo: a keystroke landing while the flush's own
  // IPC is in flight would otherwise arm a fresh autosave debounce nothing
  // then cancels, and it can fire mid-undo and clobber the restored content.
  // Wrapping the flush itself in the try means a failed
  // flush still unlocks.
  host.setEditorCommitLock(true);
  try {
    await host.documents.flush();
    const registry = host.registry();
    const activePathNow = registry.pathOf(registry.activeEntryId);
    const reversed: RenameToastMoveEntry[] = summary.moved.map((m) => ({
      old_path: m.new_path,
      new_path: m.old_path,
      is_dir: m.is_dir,
    }));
    const activeMove = (() => {
      if (activePathNow === null) return null;
      const to = mapMovedPath(activePathNow, reversed);
      return to === activePathNow ? null : { from: activePathNow, to };
    })();

    if (activeMove !== null) host.documents.retarget(activeMove.from, activeMove.to, 'starting');
    const undoResult = await host.undoRename(summary.undo_id);
    if (undoResult.status !== 'ok') throw new Error(undoResult.error);

    if (activeMove !== null) host.documents.retarget(activeMove.from, activeMove.to, 'done');
    for (const m of reversed) registry.rename(m.old_path, m.new_path);

    await host.refresh.refreshTree();
    host.reanchor(reversed);
    await host.reloadActiveFileFromDisk();

    if (activeMove !== null) host.broadcastRenameComplete(activeMove.from, activeMove.to);

    if (undoResult.data.skipped.length > 0) {
      showAck(buildSkippedPagesNote(undoResult.data.skipped), { duration: RENAME_TOAST_DURATION_MS });
    }
  } finally {
    host.setEditorCommitLock(false);
  }
}
