// Renames moss started, and has not yet seen the watcher say so.
//
// WHY. The watcher fires for BOTH sides of a rename — and the editor's own
// write-then-rename produces a stale event for the old path on top of that.
// Acting on either would reload the buffer from a path that is mid-move, so
// the dispatcher swallows them and lets the rename's own follow-up
// `editor:file-selected` do the reload.
//
//
// The record has a definitive terminator. A `source_renames` pair that
// matches one we registered IS the echo we were waiting for, so it retires
// the record and the very next event about that path is treated as real. The
// timeout is kept only as a bounded
// backstop for the case where the echo is coalesced away entirely and never
// arrives at all.

/** A rename moss issued, in project-root-relative source-domain paths — the
 *  same domain as `FileChangeEvent.modified_paths` and `source_renames`.
 *  `moved_output_paths` is output-domain and never matches. */
export interface PendingRename {
  readonly from: string;
  readonly to: string;
}

/** Backstop (ms) for a rename whose watcher echo never arrives — coalesced
 *  away by the debouncer, or emitted in a shape that carries no pair. Long
 *  enough to cover the round trip (renameEntry IPC + the tree's listTree +
 *  watcher debounce), which on large projects exceeds 500 ms; short enough
 *  that a rename whose echo is genuinely lost stops suppressing real edits.
 *  It is no longer the normal exit — the matching echo is. */
export const RENAME_ECHO_BACKSTOP_MS = 1500;

export class PendingRenames {
  private live: PendingRename[] = [];
  private timers = new Map<PendingRename, ReturnType<typeof setTimeout>>();

  constructor(private readonly backstopMs: number = RENAME_ECHO_BACKSTOP_MS) {}

  /** Announce a rename that is about to go out. */
  register(from: string, to: string): void {
    const record: PendingRename = { from, to };
    this.live.push(record);
    this.timers.set(record, setTimeout(() => this.retire(record), this.backstopMs));
  }

  /**
   * Should this event be swallowed as the echo of one of our own renames?
   *
   * A `source_renames` pair matching a registered record is the definitive
   * echo: the record retires and later events about the same path are real.
   * ALL matching records retire, not the first — the debouncer coalesces, and
   * `source_renames` is an array of pairs precisely because one event can
   * carry several renames. Retiring one per event would leave the others
   * suppressing genuine edits for the full backstop, which is the failure
   * this class was rewritten to remove.
   *
   * A one-sided touch (either path appearing in `modified`, or on one side of
   * some other rename pair) is suppressed but retires nothing — the definitive
   * pair may still be in flight behind it.
   *
   * Retiring on the pair is safe against the OTHER event this class swallows,
   * the stale one the editor's own write-then-rename leaves on the old path,
   * because that write HAPPENS BEFORE the rename: an ordered watcher reports
   * it first (suppressed as a one-sided touch) or coalesces it into the same
   * payload as the pair (suppressed by the pair match, which is checked
   * first). There is no order in which it arrives after its own rename.
   */
  swallows(modified: readonly string[], renames: readonly (readonly [string, string])[]): boolean {
    const matched = this.live.filter(
      (r) => renames.some(([from, to]) => from === r.from && to === r.to),
    );
    for (const record of matched) this.retire(record);
    if (matched.length > 0) return true;
    const touches = (p: string) => this.live.some((r) => r.from === p || r.to === p);
    if (modified.some(touches)) return true;
    return renames.some(([from, to]) => touches(from) || touches(to));
  }

  /** Drop everything — the project folder changed, so these paths belong to a
   *  vault that is no longer open. Left standing, a record would suppress a
   *  real event in the NEW project for a file that happens to share the same
   *  relative path (e.g. "index.md" in both). */
  clear(): void {
    for (const timer of this.timers.values()) clearTimeout(timer);
    this.timers.clear();
    this.live = [];
  }

  private retire(record: PendingRename): void {
    const timer = this.timers.get(record);
    if (timer !== undefined) clearTimeout(timer);
    this.timers.delete(record);
    this.live = this.live.filter((r) => r !== record);
  }
}
