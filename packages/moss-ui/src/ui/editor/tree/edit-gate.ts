// The file tree's coordination state: every reason a piece of tree work
// cannot happen right now, in one place, with the booleans as derivations.
//
// THE MODEL. Two facts, neither derived from the other:
//
//   open           — the inline rename/create the USER has on screen, or null.
//                    Backed by a real <input>, so it can leak; the gate arms
//                    its own watchdog and carries the pull-side phantom check.
//   renameCommit   — a rename IPC the TREE has in flight. Set by commitRename,
//                    which the inline gesture, a drag-drop move and the CM6
//                    heading rename all reach, so it is NOT a phase of `open`.
//
// They answer different questions and the gate keeps them apart:
//   `admit(work)`      — may this render/refresh run? Closed by `open` only.
//                        A rename IPC deliberately does NOT close it; painting
//                        the predicted name while the vault-wide link rewrite
//                        runs is the whole point of the optimistic path.
//   `canStartGesture`  — may a NEW inline edit begin? Closed by both.
//
// Everything else is derived. `isEditing`, `renamingPath` and `creating` are
// getters. The pending-work pair is one value on the order
// none < render < refresh, because a refresh always renders — so "which parked
// job wins" is a property of the lattice rather than four hand-written
// assignments. Admitting work at or above the parked rank clears it; below it
// does not.
//
// NOT here: index-write ordering. Two overlapping `listTree` round trips are
// ordered by the generation token in `FileTree.refreshTree`, because a full
// re-listing is last-writer-wins — the newest listing is the truth and an
// older one is worthless, so discarding it beats making a caller wait for a
// listing it did not ask for.

import type { EntryId } from '../entry-registry';

/** An inline rename of an existing entry. `path` is registry-domain. */
export interface RenameSession {
  readonly kind: 'rename';
  readonly path: string;
}

/** An inline create. `name` is the temporary placeholder the row shows;
 *  `tempId` is the registry id the tree minted for that row, so its teardown
 *  can tell its own row from a real entry that later lands on the same path. */
export interface CreateSession {
  readonly kind: 'create';
  readonly folderPath: string;
  readonly type: 'file' | 'folder';
  readonly name: string;
  readonly tempId: EntryId;
  /** Set when the commit instantiates a saved template instead of creating blank. */
  readonly templateId?: string;
}

/** What the user has open inline in the tree. Exactly one at a time: the tree
 *  has one inline <input>, and both gestures mount the same element. */
export type EditSession = RenameSession | CreateSession;

/** Work parked while the gate was closed. Ordered — a refresh subsumes a
 *  render, so merging two parked jobs is `max`, not a special case. */
export type DeferredWork = 'none' | 'render' | 'refresh';

const RANK: Record<DeferredWork, number> = { none: 0, render: 1, refresh: 2 };

/**
 * Watchdog (ms) for a session's inline input — the PUSH half of leak
 * detection. Both gestures mount their <input> from a deferred `setTimeout`
 * that an idle action-panel WKWebView can starve, and an input can also be
 * torn out of the DOM without ever firing blur/Enter/Escape. Either way the
 * session would be held with nothing for the user to dismiss, and because the
 * gate closes render AND refresh, one held session freezes the ENTIRE tree —
 * dead chevrons, invisible watcher updates, frozen breadcrumb. Timer-queue
 * ordering guarantees a 0 ms mount callback runs before this one, so the
 * watchdog can never outrace a mount that was merely slow to schedule.
 */
export const EDIT_WATCHDOG_MS = 1500;

/**
 * Grace window (ms) for the PULL half — `admit()`'s check that the session
 * still has an input. This is the backstop for when the watchdog timer is
 * itself throttled, so it must be event-driven and it must be looser than the
 * watchdog: comfortably past the mount gap, so a legitimate in-flight create
 * is never reaped by a watcher-triggered render landing inside it.
 */
export const EDIT_PHANTOM_GRACE_MS = 2000;

export interface EditGateDeps {
  /** True while the inline rename/create <input> is mounted. The DOM-side
   *  truth a session must be backed by. */
  hasLiveInput(): boolean;
  /** Monotonic clock. `performance.now()` in production; injectable so the
   *  windows are testable without waiting. */
  now(): number;
  /** Undo the tree-side bookkeeping a leaked session left behind (a create's
   *  temp dirIndex entry and its activePath tail). Bookkeeping only — the
   *  caller is already on a render path. */
  discard(session: EditSession): void;
  /** The watchdog fired: the session is gone and nothing else is about to
   *  repaint, so this must both discard and drive the tree's own recovery
   *  (render + flush). Runs after the session has been closed. */
  reap(session: EditSession): void;
  /** Gated diagnostic — the tree's `treeTrace`. */
  trace(message: string): void;
  /** Injectable for tests; `setTimeout`/`clearTimeout` in production. */
  setTimer?(fn: () => void, ms: number): unknown;
  clearTimer?(handle: unknown): void;
}

interface OpenEdit {
  readonly session: EditSession;
  readonly startedAt: number;
  watchdog: unknown | null;
}

export class EditGate {
  private open: OpenEdit | null = null;
  private deferred: DeferredWork = 'none';
  /** A `commitRename` IPC is in flight. Two renames dispatched together are
   *  two independent async commands with nothing ordering them, and if the
   *  second lands first it renames a path the first has not created yet and is
   *  silently discarded. */
  private renameCommit = false;

  private readonly setTimer: (fn: () => void, ms: number) => unknown;
  private readonly clearTimer: (handle: unknown) => void;

  constructor(private readonly deps: EditGateDeps) {
    this.setTimer = deps.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = deps.clearTimer ?? ((h) => clearTimeout(h as ReturnType<typeof setTimeout>));
  }

  // ── Derivations (never stored) ──────────────────────────────────────

  /** The gate is closed: destructive renders and watcher refreshes park. */
  get isEditing(): boolean {
    return this.open !== null;
  }

  /** May a NEW inline gesture begin? Both gesture entry points ask exactly
   *  this, so neither can drift from the other again. */
  get canStartGesture(): boolean {
    return this.open === null && !this.renameCommit;
  }

  /** The path being renamed inline, or null. */
  get renamingPath(): string | null {
    return this.open?.session.kind === 'rename' ? this.open.session.path : null;
  }

  /** The live create session, or null. */
  get creating(): CreateSession | null {
    return this.open?.session.kind === 'create' ? this.open.session : null;
  }

  // ── Session lifecycle ───────────────────────────────────────────────

  /** Open an inline edit and arm its watchdog. The caller must have a mount
   *  point in hand: a session begun with nowhere to put its <input> is exactly
   *  the leak the watchdog and the phantom heal exist to clean up after. */
  begin(session: EditSession): void {
    this.end();
    const open: OpenEdit = { session, startedAt: this.deps.now(), watchdog: null };
    this.open = open;
    open.watchdog = this.setTimer(() => {
      open.watchdog = null;
      // Only if it is still ours AND still inputless: a live input the user is
      // typing into is never reaped, however long they take.
      if (this.open !== open || this.deps.hasLiveInput()) return;
      this.deps.trace(`watchdog reaped a session with no input — ${JSON.stringify(session)}`);
      this.end();
      this.deps.reap(session);
    }, EDIT_WATCHDOG_MS);
  }

  /** End the session unconditionally, disarming its watchdog. */
  end(): void {
    if (this.open?.watchdog != null) this.clearTimer(this.open.watchdog);
    this.open = null;
  }

  /** End the session only if `session` is still the live one — object
   *  identity, so a stale deferred callback cannot end a session that a later
   *  gesture already replaced. Returns whether it ended anything. */
  endIfCurrent(session: EditSession): boolean {
    if (this.open?.session !== session) return false;
    this.end();
    return true;
  }

  /** True when `session` is still the live one. The stale-callback guard for
   *  deferred timers that captured a session token. */
  isCurrent(session: EditSession): boolean {
    return this.open?.session === session;
  }

  // ── Rename commit ───────────────────────────────────────────────────

  /** Run `work` as THE rename IPC. Serializes gestures against it without
   *  closing the gate, so the optimistic repaint of the predicted name is not
   *  parked behind the vault-wide link rewrite. */
  async duringRenameCommit<T>(work: () => Promise<T>): Promise<T> {
    this.renameCommit = true;
    try {
      return await work();
    } finally {
      this.renameCommit = false;
    }
  }

  // ── The gate ────────────────────────────────────────────────────────

  /**
   * May `work` run right now?
   *
   * `true`  — run it. Any parked job of equal or lower rank is considered
   *           satisfied and cleared — ADMITTED, not completed. The gate
   *           cannot observe whether the work then lands, so a caller whose
   *           work can fail owns its own recovery (`refreshTree` arms a
   *           bounded retry). Re-parking here instead would be inert: the
   *           gate was open, so nothing is coming along to flush it.
   * `false` — it has been parked at `max(deferred, work)`; run
   *           `takeDeferred()` when the blocker clears.
   *
   * A session that has leaked (see `isPhantom`) is torn down here and the
   * work admitted, so whichever entry point notices first unfreezes the tree.
   */
  admit(work: 'render' | 'refresh'): boolean {
    const open = this.open;
    if (open !== null) {
      if (this.isPhantom()) {
        this.deps.trace(`healing phantom edit — ${JSON.stringify(open.session)}`);
        this.end();
        this.deps.discard(open.session);
      } else {
        this.deps.trace(`${work} parked (edit open) — ${JSON.stringify(open.session)}`);
        this.defer(work);
        return false;
      }
    }
    if (RANK[work] >= RANK[this.deferred]) this.deferred = 'none';
    return true;
  }

  /** Park `work` without asking — for a caller that has already decided not
   *  to run now (e.g. an optimistic rename that wants the post-commit render
   *  rather than relying on a watcher echo that debounce may delay). */
  defer(work: 'render' | 'refresh'): void {
    if (RANK[work] > RANK[this.deferred]) this.deferred = work;
  }

  /** Drain the parked job. */
  takeDeferred(): DeferredWork {
    const parked = this.deferred;
    this.deferred = 'none';
    return parked;
  }

  /**
   * A session whose <input> has not existed in the DOM past the grace window.
   * Its flag has LEAKED and, because the gate closes render AND refresh, would
   * otherwise park every future update forever.
   */
  private isPhantom(): boolean {
    if (this.open === null) return false;
    if (this.deps.hasLiveInput()) return false;
    return this.deps.now() - this.open.startedAt > EDIT_PHANTOM_GRACE_MS;
  }

  /** Lifetime boundary (unmount). Drops everything WITHOUT discarding —
   *  unmount tears the whole tree down, so there is no bookkeeping left to
   *  undo. */
  reset(): void {
    this.end();
    this.deferred = 'none';
    this.renameCommit = false;
  }
}
