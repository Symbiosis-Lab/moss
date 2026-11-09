/**
 * The inline rename and create sessions: the one <input> the tree mounts into
 * a row, the rename commit with its optimistic prediction and rollback, the
 * create commit with its inline retry, and the drain of the work the gate
 * parked while either was open. This module drives the EditGate
 * (tree/edit-gate.ts), which owns the session token, the watchdog and the
 * parked-work lattice; what is owned here is what the user sees — the input,
 * the widened segment, the inline error — and the IPC each session ends in.
 *
 * The commands
 * arrive through the host, which the controller fills. The anchor policy around a create —
 * where the breadcrumb points before the input mounts, and what opens once
 * the entry lands — stays with the controller: it builds the session and
 * passes the reveal in as the continuation of `startCreate`.
 */

import type { Documents } from "../../host";
import { t } from "../../i18n/index";
import { showToast } from "../../shared/ui/toast/toast-manager";
import {
  toAbsolute, toRelative, normalizePath, PROJECT_ROOT_REL,
  siblingPath, childPath, parentDirOf, baseName, fileDisplayParts,
} from '../path-domain';
import type { EntryRegistry, EntryId, EntryRecord } from '../entry-registry';
import { remapActivePathForRename, reanchorAfterMove, withAnchor, type TreeView } from './model';
import type { EditGate, EditSession, CreateSession } from './edit-gate';
import { createOneFile, type NewFileInput, type TreeRefresh, type CommandResult, type ListedNode } from './refresh';
import {
  showRenameSummaryToast, performRenameUndo,
  type RenameToastSummary, type RenameUndoHost, type UndoResultShape,
} from './rename-toast';

/** The commands the two sessions end in, injected by the controller. */
export interface EditSessionCommands {
  /** The toast's Undo action — reverses the applied edits `undo_id` names. */
  undoRename(undoId: string): Promise<CommandResult<UndoResultShape>>;
  createFiles(files: NewFileInput[]): Promise<CommandResult<string[]>>;
  createFolder(parentDirAbs: string, name: string): Promise<CommandResult<string>>;
  instantiateTemplate(id: string, targetDirAbs: string, name: string): Promise<CommandResult<string>>;
}

/** The controller interface used by the sessions; it exposes only their coordination needs. */
export interface EditSessionHost {
  projectRoot(): string;
  readonly commands: EditSessionCommands;
  /** The tree model: identity across a rename, the temp row of a create. */
  registry(): EntryRegistry;
  /** The refresh cycle: re-list after a commit, `forget` a temp row, drain a parked refresh, and the bare listing a failed rename reconciles against. */
  readonly refresh: Pick<TreeRefresh, 'refreshTree' | 'refreshAndRender' | 'forget' | 'list'>;
  /** The session token, its watchdog and the parked-work lattice. */
  readonly gate: EditGate;
  update(fn: (view: TreeView) => TreeView): void;
  render(): void;
  /** The element rendering entry `id`, or null when it is not on screen. */
  rowFor(id: EntryId): HTMLElement | null;
  /** The tree's root element while mounted: the rename events bubble from it and the live input is found under it. */
  container(): HTMLElement | null;
  /** Would renaming this folder also rename a file inside it? Declines the optimistic prediction (see commitRename). */
  carriesSelfNamedHome(path: string): boolean;
  /** The open documents: the pending save a rename flushes first, the rename itself, and the open file following its path. */
  readonly documents: Documents;
  /** Freeze/unfreeze the open buffer for a commit's duration — see `EditorCommitHooks` (controller.ts). */
  setEditorCommitLock(locked: boolean): void;
  /** Re-parse the active file from disk once a commit (or its undo) completes — see `EditorCommitHooks` (controller.ts). */
  reloadActiveFileFromDisk(): Promise<void>;
}

/**
 * Replace a row's `.seg-name` with the tree's one inline <input>, focused,
 * with `initial` selected — all of it, or `select`'s range. Both sessions
 * mount exactly this element, so its presence is the DOM-side truth the gate
 * backs a session with (`liveEditInput`).
 *
 * Enter and Escape are consumed here — stopPropagation, not just
 * preventDefault: the tree's document-level key handler maps a bare Enter on
 * the focused row to "rename", and the rename's confirm path ends the session
 * before the key finishes bubbling, so without this the Enter that ENDS a
 * rename immediately starts another one on the row it just renamed. Blur
 * cancels. What commit and cancel then do is the caller's, including whether
 * a failed commit lets Enter try again: the rename latches once, the create
 * un-latches on failure. Listeners sit on the input, never on the row
 * The input is created and destroyed with the session.
 */
function mountInlineInput(
  textSpan: HTMLElement,
  initial: string,
  opts: { select?: [start: number, end: number]; commit(): void; cancel(): void },
): HTMLInputElement {
  const input = document.createElement('input');
  input.className = 'moss-tree-rename-input';
  input.value = initial;
  input.type = 'text';

  textSpan.replaceWith(input);
  input.focus();
  if (opts.select) input.setSelectionRange(opts.select[0], opts.select[1]);
  else input.select();

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); opts.commit(); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); opts.cancel(); }
  });
  input.addEventListener('blur', () => opts.cancel());
  return input;
}

/** Put `message` in a `.moss-tree-error` line directly under `seg`. The
 *  lifetime is the caller's: the rename removes it after 3s, the create
 *  keeps it until the next keystroke. */
function inlineError(seg: HTMLElement, message: string): HTMLElement {
  const errorEl = document.createElement('div');
  errorEl.className = 'moss-tree-error';
  errorEl.textContent = message;
  seg.parentElement?.insertBefore(errorEl, seg.nextSibling);
  return errorEl;
}

export class TreeEditSession {
  constructor(private readonly host: EditSessionHost) {}

  /** The live inline rename/create <input>, if one is mounted. startRename
   *  and startCreate both mount exactly this class, so its presence is the
   *  DOM-side truth an editing flag must be backed by. */
  liveEditInput(): HTMLInputElement | null {
    return this.host.container()?.querySelector<HTMLInputElement>('input.moss-tree-rename-input') ?? null;
  }

  /**
   * Run work that was deferred while an inline edit was open. Called from
   * the end of startRename's `finish` (via try/finally), from the create
   * flow's `finish` and `cancelCreate`, and by the controller when the gate
   * reaps a session or a listing lands.
   */
  flushPendingWork(): void {
    switch (this.host.gate.takeDeferred()) {
      case 'refresh': void this.host.refresh.refreshAndRender(); break;
      case 'render': this.host.render(); break;
      case 'none': break;
    }
  }

  // ── Rename lifecycle events ─────────────────────────────────────────
  //
  /**
   * The canonical "rename done; post-state is consistent" broadcast, AFTER
   * refreshTree. Distinct from `editor:file-renamed` (the narrow registry-
   * update signal editor-main consumes synchronously): this is what
   * downstream consumers (the breadcrumb flash, future search-index updates)
   * subscribe to.
   */
  broadcastRenameComplete(oldPath: string, newPath: string): void {
    this.host.container()?.dispatchEvent(
      new CustomEvent('editor:rename-complete', {
        bubbles: true,
        detail: { oldPath, newPath },
      }),
    );
  }

  /**
   * Show an in-place rename immediately: re-key local state, move the
   * EntryId, re-anchor the breadcrumb, render. Called BEFORE the rename IPC
   * (see commitRename) and, with the arguments swapped, to undo the
   * prediction when that IPC fails.
   *
   * Ordering: `dispatchRenameCompleted` must run before any refreshTree so
   * reconcileSnapshot sees newPath already bound to the active EntryId (the
   * rename-keeps-identity invariant). The unconditional
   * `registry.rename` after it is the non-active case; it is a no-op when the
   * dispatch's editor-main handler has already done the move.
   */
  private applyRenameLocally(oldPath: string, newPath: string, isActive: boolean): void {
    // Dispatch BEFORE re-keying the tree indexes. editor-main's
    // `editor:file-renamed` handler documents that it may not read tree state
    // (it would see the pre-rename layout), and that rule has to hold for
    // EVERY rename or it is worse than no rule: a cross-parent move still
    // dispatches first, so an in-place rename must too, or the next author
    // gets opposite answers from the same handler.
    if (isActive) this.host.documents.retarget(oldPath, newPath, 'done');
    this.host.registry().rename(oldPath, newPath);
    this.host.update(v => withAnchor(v, remapActivePathForRename(v.anchor, oldPath, newPath)));
    this.host.render();
  }

  /**
   * Reconcile a failed rename with what is actually on disk.
   *
   * A failed rename is NOT proof that the filesystem refused. `rename_entry_
   * with_refs` renames the file on its FIRST line and then spends the rest of
   * its time canonicalizing paths and rewriting every link to it — and any of
   * that can fail with the file already sitting at its new name. Undoing the
   * prediction in that case would re-key the tree onto a path that no longer
   * exists, and the next refresh prunes overrides to what exists, so an
   * expanded folder would collapse — the exact bug the rename remap was
   * written to fix, restored in the error path.
   *
   * So ask disk which name is really there, and only reverse when the rename
   * genuinely did not happen. The extra tree read is paid on the error path
   * only. A failed refresh keeps the last-good index, which is exactly the
   * state this reconciliation just produced.
   */
  private async rollbackRename(oldPath: string, newPath: string, isActive: boolean): Promise<void> {
    if (!(await this.existsOnDisk(newPath))) {
      this.applyRenameLocally(newPath, oldPath, isActive);
    }
    await this.host.refresh.refreshTree();
    this.host.render();
  }

  /**
   * Is `relPath` (registry domain) present in the tree the backend reports
   * right now? Deliberately reads fresh rather than consulting `dirIndex` —
   * the caller is asking precisely because local state may be a prediction
   * that disk did not confirm.
   *
   * A read that FAILS answers `true`, not `false`. The only caller uses this
   * to decide whether to undo a prediction, and undoing one wrongly destroys
   * the user's expansion state; leaving one standing wrongly shows a stale
   * name that the next refresh corrects. "I could not find out" must fall on
   * the recoverable side of that, so `list()`'s null — a timeout or a failed
   * tree read — means "assume it landed" rather than a confident no.
   */
  private async existsOnDisk(relPath: string): Promise<boolean> {
    const projectRoot = this.host.projectRoot();
    if (!projectRoot) return true;
    const tree = await this.host.refresh.list();
    if (tree === null) return true;
    // `node.path` is whatever the filesystem spelled; `relPath` may be a name
    // the user typed. Compare in the one form (normalizePath) and via the same
    // ingest helper every other reader of a TreeNode uses — a raw `===` here
    // answered "no" for an accented name and collapsed the folder, which is
    // the exact bug this reconciliation exists to avoid.
    const wanted = normalizePath(relPath);
    const found = (node: ListedNode): boolean =>
      normalizePath(toRelative(node.path, projectRoot)) === wanted
      || (node.children ?? []).some(found);
    return found(tree);
  }

  /**
   * Surface a failed rename beside the row it happened on. By the time this
   * runs the tree has been re-rendered (the prediction was rolled back), so
   * the segment element the input lived in is detached — re-find the live row
   * by id, and fall back to a toast if the row is gone entirely.
   */
  private showRenameError(id: EntryId, err: unknown): void {
    const message = err instanceof Error ? err.message : t('tree.rename_failed');
    const seg = this.host.rowFor(id);
    if (!seg?.parentElement) {
      showToast({ what: message, severity: 'Blocking' });
      return;
    }
    const errorEl = inlineError(seg, message);
    setTimeout(() => errorEl.remove(), 3000);
  }

  /**
   * The one rename flow — reached by `startRename`'s Enter and by
   * `renameThroughTree` (the CM6 heading rename, the tests). Both paths are
   * project-root-relative here. The order is load-bearing:
   *
   *   1. `dispatchRenameStarting` (active file only) — editor-main registers
   *      the pending pair so the watcher's file-changed echo is silenced.
   *   2. Predict: an in-place rename is applied and painted before the IPC.
   *   3. `renameEntryWithRefs`, serialized through the gate; a failure rolls
   *      the prediction back against what disk really holds.
   *   4. `dispatchRenameCompleted` BEFORE `refreshTree`: editor-main's
   *      synchronous handler moves the active EntryId to newPath, so the
   *      refresh's reconcileSnapshot finds it bound there and keeps it. The
   *      other order retired the id and minted a fresh one — identity lost.
   *   5. Re-anchor only if the anchor was the renamed path (frozen anchor),
   *      then broadcast `editor:rename-complete` for the active file. Preview
   *      navigation does not hang off that event — PreviewFollower follows
   *      the registry's active-path signal, which fired in step 4.
   *
   * The identity check compares EntryIds and falls back to the path string
   * for tests that mount without populating the registry.
   *
   * @throws on refusal or IPC failure — the caller owns the UX
   */
  private async commitRename(oldPath: string, newPath: string): Promise<void> {
    const { host } = this;
    // Locked BEFORE the flush below, not after — a keystroke landing while
    // flushPendingSave's own IPC is still in flight would otherwise arm a
    // fresh autosave debounce that nothing then cancels, and it can fire
    // mid-rewrite (or after the reload, over the reloaded content) and
    // clobber this rename's own rewrite with whatever the buffer held before
    // the flush. The lock therefore wraps the
    // WHOLE commit rather than just the backend round trip, so every throw
    // path below — a name collision, a failed flush, a rejected IPC call —
    // still reaches the `finally` and unlocks; see EditorCommitHooks
    // (controller.ts).
    host.setEditorCommitLock(true);
    let result: CommandResult<RenameToastSummary>;
    try {
      // The planner reads the CURRENT bytes on disk (plan_moves/
      // apply_planned_moves); an unflushed edit in the open buffer is
      // invisible to it, and a debounced autosave landing after the rename
      // would overwrite its own file's just-rewritten links with the stale
      // pre-flush content. Same ordering `versions-mode.ts`'s `open()` uses.
      await host.documents.flush();
      const registry = host.registry();
      const oldEntryId = registry.getByPath(oldPath);
      // Precondition surface: oldPath should always be in the registry at
      // this point (it was at least minted by flattenTree's reconcileSnapshot
      // during the most recent refreshTree). If we somehow reach commitRename
      // with an unknown oldPath, the subsequent registry.rename will no-op
      // and reconcileSnapshot will retire the active EntryId without remap.
      // Warn so the invariant violation surfaces in dev/test.
      if (oldEntryId === null) {
        console.warn(`[FileTree.commitRename] oldPath '${oldPath}' is not in the registry; identity preservation may fail`);
      }
      // The active id and its path, read from the registry — the canonical
      // owner the controller's own `activeEntryId` / `currentFilePath` derive from.
      const activeEntryId = registry.activeEntryId;
      const currentFilePath = activeEntryId === null ? null : (registry.getById(activeEntryId)?.path ?? null);
      const isActive = (oldEntryId !== null && activeEntryId !== null)
        ? oldEntryId === activeEntryId
        : currentFilePath === oldPath;

      // An in-place rename (same parent) is fully describable without asking
      // the filesystem anything, so refuse a name the tree already holds BEFORE
      // touching disk — an instant, accurate error beats a rename that appears
      // and is yanked back a moment later. Cross-parent moves are not checked
      // here: the destination list may not be loaded.
      //
      // Compared in COMPARISON form, and this is the site where it matters most:
      // `newPath` carries the spelling the user TYPED (precomposed) while the
      // sibling carries the spelling the filesystem LISTED (decomposed on
      // macOS). A raw `===` misses the collision, and what is behind this check
      // is a bare `std::fs::rename` with no destination check of its own — so a
      // miss is not a worse error message, it is the other file gone.
      const sameParent = parentDirOf(oldPath) === parentDirOf(newPath);
      if (sameParent && newPath !== oldPath) {
        const taken = registry.getByPath(newPath);
        if (taken !== null && taken !== registry.getByPath(oldPath)) {
          throw new Error(
            t('tree.rename_name_taken').replace('{name}', baseName(newPath)),
          );
        }
      }

      if (isActive) host.documents.retarget(oldPath, newPath, 'starting');

      // PREDICT. `rename_entry_with_refs` renames the file in its first line and
      // then spends the rest of its time walking every .md in the vault
      // rewriting links to it. None of that is information this UI needs, so an
      // in-place rename is applied to local state and rendered NOW; the IPC
      // below still runs, and refreshTree afterwards reconciles against disk.
      // A cross-parent move is NOT predicted — the row's position in the
      // destination list is the backend's sort to decide, and guessing it wrong
      // would make the row jump. Nor is a folder that carries a self-named home
      // file, whose child the backend renames too (see carriesSelfNamedHome).
      const predict = sameParent && !host.carriesSelfNamedHome(oldPath);
      if (predict) this.applyRenameLocally(oldPath, newPath, isActive);

      // Tauri command boundary: registry stores relative, Rust opens via std::fs.
      // Serialized through the gate, which is also what both gesture entry
      // points ask before opening a new inline edit. The gate stays OPEN across
      // this await on purpose: painting the predicted name while the vault-wide
      // link rewrite runs is the wait this path exists to remove.
      try {
        result = await host.gate.duringRenameCommit(() => host.documents.rename(
          toAbsolute(oldPath, host.projectRoot()),
          toAbsolute(newPath, host.projectRoot()),
        ));
      } catch (err) {
        // ROLL BACK. A prediction with no rollback is a lie the user cannot see.
        if (predict) await this.rollbackRename(oldPath, newPath, isActive);
        throw err;
      }
      if (result.status !== 'ok') {
        if (predict) await this.rollbackRename(oldPath, newPath, isActive);
        throw new Error(result.error);
      }

      // Dispatch BEFORE refreshTree (step 4 above); the predicted path has
      // already done both in applyRenameLocally.
      if (!predict) {
        if (isActive) host.documents.retarget(oldPath, newPath, 'done');
        registry.rename(oldPath, newPath);
      }

      await host.refresh.refreshTree();

      // Keep the breadcrumb VALID without moving it:
      // rebuild only if the anchor leaf IS the renamed path (or under a renamed
      // folder); otherwise leave it frozen. Rebuilding from currentFilePath here
      // would yank the breadcrumb onto the OPEN file, which — under the frozen
      // model — can differ from the anchor.
      host.update(v => withAnchor(v, remapActivePathForRename(v.anchor, oldPath, newPath)));

      // The backend may have rewritten the open file's OWN content (a
      // rebased relative link) or, if it's some OTHER open page, its
      // incoming links — the rename-complete lifecycle above is a path
      // change only, never a reload.
      await host.reloadActiveFileFromDisk();

      if (isActive) this.broadcastRenameComplete(oldPath, newPath);
    } finally {
      host.setEditorCommitLock(false);
    }

    const summary = result.data;
    showRenameSummaryToast(
      { kind: 'rename', oldName: baseName(oldPath), newName: baseName(newPath) },
      summary,
      () => performRenameUndo(this.undoHostAdapter(), summary),
    );
  }

  /** Adapts this session onto {@link RenameUndoHost} for `performRenameUndo`
   *  — `this` supplies the dispatch/broadcast methods it already has;
   *  `this.host` supplies the rest. */
  private undoHostAdapter(): RenameUndoHost {
    const { host } = this;
    return {
      registry: () => host.registry(),
      documents: host.documents,
      broadcastRenameComplete: (a, b) => this.broadcastRenameComplete(a, b),
      refresh: { refreshTree: () => host.refresh.refreshTree() },
      reanchor: (moved) => host.update(v => reanchorAfterMove(v, moved)),
      setEditorCommitLock: (locked) => host.setEditorCommitLock(locked),
      reloadActiveFileFromDisk: () => host.reloadActiveFileFromDisk(),
      undoRename: (undoId) => host.commands.undoRename(undoId),
    };
  }

  /**
   * Drive a rename through `commitRename` from outside the inline gesture:
   * editor-main's CM6 heading rename (Filename mode,
   * so it gets the same refs-aware command, lifecycle and re-anchor) and the
   * tests. Absolute or relative paths; `toRelative` is idempotent on the
   * registry domain, which is what `commitRename` and its events speak.
   */
  async renameThroughTree(oldPath: string, newPath: string): Promise<void> {
    return this.commitRename(
      toRelative(oldPath, this.host.projectRoot()),
      toRelative(newPath, this.host.projectRoot()),
    );
  }

  // ── Inline-edit segment widening (shared by create + rename) ─────────
  //
  // Both startCreate and startRename mount an <input> (flex:1; min-width) into a
  // segment. That input is only comfortably editable if its host segment is
  // allowed to grow. A TREE-BODY seg widens purely via the `.ft-renaming` CSS
  // class (align-self:stretch). A BREADCRUMB seg (`.seg.bc`) cannot: the spine
  // allocator writes an INLINE `style.flex = "0 0 Wpx"` (Spine.apply)
  // that beats the class rule `.seg.bc.ft-renaming { flex:1 1 auto }`. So for a
  // breadcrumb seg we ALSO set the inline flex to '1 1 auto' (inline beats
  // inline; last-write-wins) and let every other seg in the strip give up room.
  //
  // Routing create AND rename through this one helper is deliberate: the July-3
  // widen fix (commit 1f04ee9f4) touched rename only, so create silently stayed
  // narrow. One helper => the two cannot diverge again.
  //
  // The strip is reached as the edited seg's parent: a `.bc` seg renders
  // nowhere else, and the sweep is over its siblings.

  /** Widen a segment (tree row or breadcrumb) that is about to host an edit input. */
  private beginSegEdit(segEl: HTMLElement): void {
    segEl.classList.add('ft-renaming');
    if (segEl.classList.contains('bc')) {
      // Inline flex is REQUIRED: the allocator's inline `flex:0 0 Wpx` defeats
      // the class rule. Also let the other breadcrumb segs shrink so the edited
      // seg can claim the freed room.
      segEl.style.flex = '1 1 auto';
      for (const el of Array.from(segEl.parentElement?.children ?? [])) {
        if (el !== segEl && el instanceof HTMLElement && el.classList.contains('seg')) {
          el.style.flexShrink = '1';
        }
      }
    }
  }

  /** Undo beginSegEdit(): restore the seg + siblings to allocator-driven widths. */
  private endSegEdit(segEl: HTMLElement): void {
    segEl.classList.remove('ft-renaming');
    segEl.style.removeProperty('flex');
    for (const el of Array.from(segEl.parentElement?.children ?? [])) {
      if (el instanceof HTMLElement && el.classList.contains('seg')) {
        el.style.removeProperty('flex-shrink');
      }
    }
  }

  // ── Rename ────────────────────────────────────────────────────────

  startRename(segEl: HTMLElement, entry: EntryRecord): void {
    const { host } = this;
    // The project root has a record like every folder, but no name to edit.
    if (!host.gate.canStartGesture || entry.path === PROJECT_ROOT_REL) return;

    // Locate the mount point BEFORE opening the session: with the session
    // open first, a missing .seg-name would leak one (the gate closes render
    // AND watcher refreshes → whole tree frozen until the phantom heal).
    //
    // Unlike startCreate there is no mount watchdog here: the input mounts
    // SYNCHRONOUSLY below, so by the time this function returns either the
    // input exists (finish()'s try/finally owns ending the session on
    // Enter/Escape/blur) or no session was opened. The residual case — the
    // input torn out of the DOM without any of those events firing — is
    // covered by the gate's phantom self-heal.
    const textSpan = segEl.querySelector<HTMLElement>('.seg-name');
    if (!textSpan) return;
    const session: EditSession = { kind: 'rename', path: entry.path };
    host.gate.begin(session);

    // For files where the extension is hidden in the tree (.md) or shown as
    // a dimmed suffix (other extensions), the editable input scopes to the
    // stem only. The extension renders as a non-editable dimmed span next
    // to the input, Finder-style. This prevents mistakes like typing a
    // different extension and ending up with `name.txt.md`.
    const originalName = baseName(entry.path);
    const parts = !entry.isDir ? fileDisplayParts(originalName) : null;
    const stemOnly = parts !== null && (parts.extHint !== '' || originalName.toLowerCase().endsWith('.md'));
    // The full extension (with leading dot) preserved off-screen for commit.
    const preservedExt: string =
      stemOnly && parts
        ? originalName.slice(parts.stem.length)
        : '';

    const input = mountInlineInput(textSpan, stemOnly ? (parts as { stem: string }).stem : originalName, {
      commit: () => { void finish(true); },
      cancel: () => { void finish(false); },
    });

    // Stretch the segment to the full row width while renaming so the input is
    // comfortably editable. The segment normally shrink-wraps to its name (a
    // tree row via align-items:flex-start; a breadcrumb seg via the fixed spine
    // width), which left the flex:1 input cramped to the original name. Removed
    // in the finally block via endSegEdit(). See beginSegEdit() and
    // `.seg.ft-renaming` in file-tree.css.
    this.beginSegEdit(segEl);

    // If we're editing the stem-only and there was a visible ext-hint
    // sibling (non-md), it stays in the DOM as a non-editable suffix.
    // For .md (no ext-hint shown), we add a dimmed suffix during rename
    // so the user can see what they're renaming and that the .md stays.
    let renameExtHint: HTMLElement | null = null;
    if (stemOnly && originalName.toLowerCase().endsWith('.md')) {
      renameExtHint = document.createElement('span');
      renameExtHint.className = 'ext-hint';
      renameExtHint.textContent = preservedExt;
      renameExtHint.setAttribute('aria-hidden', 'true');
      input.after(renameExtHint);
    }

    // The session stays open for as long as the input is MOUNTED, so any
    // external work that calls render() (watcher events, etc.) defers
    // instead of tearing down the input mid-typing. On a confirmed rename
    // the input comes down before the IPC (see below); the finally block is
    // the backstop for every other exit.
    // Latched, as startCreate's is: taking the input out of the DOM fires
    // `blur`, in Chromium synchronously, inside the replaceWith below, and an
    // unlatched blur handler re-entered here, swapped in the old name and
    // left the outer replaceWith throwing NotFoundError before the rename
    // command ran.
    let finished = false;
    const finish = async (confirm: boolean) => {
      if (finished) return;
      finished = true;
      const stem = input.value.trim();
      // If we scoped to stem-only, reattach the original extension.
      const newName = stemOnly ? (stem ? stem + preservedExt : '') : stem;
      // siblingPath handles root-level files correctly (no leading slash) —
      // the prior `parentDir + "/" + newName` produced "/name.md" for files
      // with no parent dir, which escaped the project at the rename_entry
      // boundary. Shared with routeHeadingCommit so the two rename sites
      // cannot diverge again. See path-domain.siblingPath.
      const newPath = newName ? siblingPath(entry.path, newName) : '';

      // Put the row's text back to an ordinary span, ending the edit in the
      // DOM. `seg-name` is restored deliberately: it is what startRename
      // looks for, so a row that lost it could never be renamed twice.
      const closeInput = (label: string) => {
        const span = document.createElement('span');
        span.className = 'seg-name';
        span.textContent = label;
        input.replaceWith(span);
        renameExtHint?.remove();
      };

      try {
        if (!confirm || !newName || newName === originalName) {
          closeInput(originalName);
          return;
        }

        // END THE EDIT BEFORE THE AWAIT, not after it. render() parks itself
        // for as long as the session is open (it would otherwise wipe the
        // input the user is typing into), so holding it across commitRename
        // would park the optimistic render too — and the new name would not
        // appear until the vault-wide link rewrite finished, which is the
        // whole wait this path exists to remove. On Enter the user has
        // finished typing, so the input has no work left to do.
        closeInput(stemOnly ? stem : newName);
        this.endSegEdit(segEl);
        host.gate.endIfCurrent(session);

        try {
          // All post-rename state mutation lives in commitRename, reached
          // from here and from renameThroughTree (editor-main's heading
          // rename, the tests). The drag move is not a rename session: it
          // shares only dispatchRenameStarting/Completed. See commitRename's
          // doc-comment for ordering details.
          await this.commitRename(entry.path, newPath);
          // Trigger the render that reflects the new tree state. The render
          // itself is dispatched by `flushPendingWork()` in the finally block
          // below. We can't rely on the watcher's FileChanged echo to set
          // this flag for us: fsevents debounce can delay the echo past
          // flushPendingWork.
          host.gate.defer('render');
        } catch (err) {
          // commitRename has already rolled the prediction back and
          // re-rendered, so `segEl` is detached — the error attaches to
          // whatever row now holds the old path.
          this.showRenameError(entry.id, err);
        }
      } finally {
        // Both of these only if the edit is still OURS — object identity, so
        // a second rename of the SAME path is not mistaken for this one. The
        // confirmed path already ended it before the IPC, and another row's
        // rename may be live by now: ending it would pull it out from under
        // that input, and endSegEdit strips `flex-shrink` from EVERY
        // breadcrumb segment, so running it here would narrow the input
        // someone is typing into.
        if (host.gate.endIfCurrent(session)) this.endSegEdit(segEl);
        this.flushPendingWork();
      }
    };
  }

  // ── Create New Item ─────────────────────────────────────────────

  /**
   * Open the create session the controller has already prepared — its temp
   * row is in the registry and the index, its anchor is the temp leaf, and
   * the tree has rendered it — and mount the name input into that row. On a
   * successful commit the temp row is retired, the tree re-listed, and
   * `onCreated` runs with the new entry's registry path: that is the
   * controller's reveal (anchor, open the file or the folder page), which
   * stays with the anchor policy.
   */
  startCreate(session: CreateSession, onCreated: (createdPath: string) => void): void {
    const { host } = this;
    const { folderPath, type, name: tempName, tempId } = session;
    // `session` is the token for the deferred callbacks below (object
    // identity): torn down — and possibly replaced by a NEW create — before a
    // starved timer fires, the stale callback must do NOTHING.
    host.gate.begin(session);

    // After render, find the temp entry's element and replace its text with an input.
    // Use setTimeout instead of requestAnimationFrame — the action-panel is a
    // Tauri child WKWebView where rAF is throttled/suspended when the webview is
    // not actively compositing (verified: rAF never fired; setTimeout did).
    // See also: Spine's timer comment and reference_raf_throttled_in_action_panel.md.
    setTimeout(() => {
      // Stale-session guard: this create was already torn down (watchdog /
      // phantom self-heal / cancel) while the timer was starved — no seg to
      // mount into, nothing to cancel (see `session` above).
      if (!host.gate.isCurrent(session)) return;

      // INVARIANT: the temp entry is pushed onto activePath as the LEAF, so
      // render() places it as the trailing breadcrumb segment (aria-current),
      // never a tree-body row. Match it by its own id — matching by
      // data-full-name ('untitled.md') would steal a pre-existing real
      // untitled.md in the same folder (its seg carries the same name).
      // Tree unmounted or strip torn down mid-defer: the row is gone, so
      // cancel — NOT a bare return, which would leak the session forever
      // (the frozen-tree bug); after a real unmount() cancelCreate is a no-op.
      const targetSeg = host.rowFor(tempId);

      if (!targetSeg) {
        this.cancelCreate(session);
        return;
      }

      const textSpan = targetSeg.querySelector<HTMLElement>('.seg-name');
      if (!textSpan) {
        this.cancelCreate(session);
        return;
      }

      // Select name without extension for files
      const dotIndex = type === 'file' ? tempName.lastIndexOf('.') : -1;
      const input = mountInlineInput(textSpan, tempName, {
        select: dotIndex > 0 ? [0, dotIndex] : undefined,
        commit: () => { void finish(true); },
        cancel: () => { void finish(false); },
      });

      // Widen the host seg so the input is comfortably editable — same helper
      // startRename uses (class-parity; BUG 3). The temp leaf is a breadcrumb
      // seg, whose inline allocator flex would otherwise pin the input to the
      // measured placeholder width. No explicit endSegEdit is needed: on
      // confirm/cancel the next render destroys this temp seg (discardTempEntry
      // drops the temp entry, then render() rebuilds the strip).
      this.beginSegEdit(targetSeg);

      let finished = false;
      // A failed create must not hit
      // console.error and return, because the finally block then removes the temp
      // row — the typed name silently vanished, and "already exists" was
      // indistinguishable from success minus the file. Now the input stays
      // open with a localized error under it, and Enter retries.
      let errorEl: HTMLElement | null = null;
      const clearCreateError = () => { errorEl?.remove(); errorEl = null; };
      const showCreateError = (raw: unknown, newName: string) => {
        const msg = raw instanceof Error ? raw.message : String(raw);
        clearCreateError();
        errorEl = inlineError(targetSeg, msg.includes('already exists')
          ? t('tree.create_exists').replace('{name}', newName)
          : t('tree.create_failed').replace('{error}', msg));
      };
      input.addEventListener('input', () => clearCreateError());

      // The session stays open across the async IPC window so external
      // calls to render() (watcher events, etc.) defer until the finally
      // below ends it — otherwise refresh+render mid-create would wipe the
      // input the user is typing into.
      const finish = async (confirm: boolean) => {
        if (finished) return;
        finished = true;

        const newName = input.value.trim();
        if (!confirm || !newName) {
          clearCreateError();
          this.cancelCreate(session);
          return;
        }

        // Tauri command boundary: registry stores relative, Rust opens via std::fs.
        // The Rust commands return the absolute path of the created entry; we
        // convert back to relative before pushing into the registry / activePath.
        const absFolderPath = toAbsolute(folderPath, host.projectRoot());
        let createdPath: string;
        try {
          // A folder template returns its seeded home file, not the folder —
          // the opened-below path is that file.
          const result = session.templateId !== undefined
            ? await host.commands.instantiateTemplate(session.templateId, absFolderPath, newName)
            : type === 'file'
              ? await createOneFile(host.commands, absFolderPath, newName)
              : await host.commands.createFolder(absFolderPath, newName);
          if (result.status !== 'ok') {
            // Visible failure, session kept: un-latch `finished` so Enter
            // retries and blur/Escape still cancel.
            finished = false;
            showCreateError(result.error, newName);
            input.focus();
            input.select();
            return;
          }
          createdPath = toRelative(result.data, host.projectRoot());
        } catch (err) {
          finished = false;
          showCreateError(err, newName);
          input.focus();
          input.select();
          return;
        }
        clearCreateError();

        try {
          // The temp row's work is done; drop it BEFORE the refresh so disk
          // mints the real row instead of inheriting the temp identity when
          // the name is unchanged. endCreate's later call then stands down.
          this.discardTempEntry(session);
          await host.refresh.refreshTree();

          // The controller's reveal: anchor on the new entry and open it.
          onCreated(createdPath);
        } catch (err) {
          // The entry WAS created — this is post-create follow-through
          // (refresh/navigation) failing, so retrying the create would only
          // hit 'already exists'. Surface it and tear down normally; the
          // watcher refresh self-heals the tree state.
          const message = err instanceof Error ? err.message : String(err);
          showToast({ what: t('tree.create_failed').replace('{error}', message), severity: 'Blocking' });
        } finally {
          // Drop the temp entry and close the session, then re-render. The
          // render runs now that the gate is open; flushPendingWork catches
          // any watcher event that was parked mid-create.
          this.endCreate(session);
          host.render();
          this.flushPendingWork();
        }
      };
    });
  }

  /**
   * Undo the bookkeeping a create session left in the tree: its temp dirIndex
   * entry, its registry entry, and the temp leaf it pushed onto `activePath`.
   * Says nothing about whether the session is over — the gate owns that.
   * One-shot: once the temp id no longer sits at the temp path, the row there
   * is disk's (a refresh ran after the create landed) and is left alone — so a
   * create that keeps its default name survives its own teardown.
   */
  discardTempEntry(session: CreateSession): void {
    const { folderPath, name, tempId } = session;
    const tempPath = childPath(folderPath, name);
    const temp = this.host.registry().getById(tempId);
    if (temp === null) return;
    this.host.refresh.forget(temp);

    // startCreate pushed tempPath onto activePath as the leaf; pop it while it
    // is still the tail so a cancelled breadcrumb keeps no stale 'untitled'
    // leaf (a successful create replaces it via file-selected → highlight).
    this.host.update(v => v.anchor[v.anchor.length - 1] === tempPath
      ? withAnchor(v, v.anchor.slice(0, -1))
      : v);
  }

  private cancelCreate(session: CreateSession): void {
    this.endCreate(session);
    this.host.render();
    this.flushPendingWork();
  }

  /** Close `session` and undo its bookkeeping — but only if it is still the
   *  live one, so a starved callback cannot tear down a create that a later
   *  gesture already replaced. */
  private endCreate(session: CreateSession): void {
    if (!this.host.gate.isCurrent(session)) return;
    this.discardTempEntry(session);
    this.host.gate.end();
  }
}
