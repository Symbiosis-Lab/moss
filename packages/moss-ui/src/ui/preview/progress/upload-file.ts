/**
 * Display policy for "the file being uploaded".
 *
 * There is no such thing as *the* current file: `deploy.rs` uploads twenty at
 * once. The backend picks the **largest in flight** — the one actually costing
 * the time — and emits it on `DeployProgress.current_file` at the same 4 Hz as
 * everything else on that stream.
 *
 * 4 Hz is the sampling ceiling, not the display rate. At the tail of a deploy,
 * when only small files are left, even the largest-in-flight can change every
 * tick, and text that changes four times a second is unreadable — and flashing
 * text is an accessibility problem, not merely an ugly one. So the winner keeps
 * the slot for [`MIN_HOLD_MS`] regardless of what arrives behind it.
 *
 * Pure: `now` is passed in, never read. The crossfade is the renderer's job.
 */

/**
 * How long a filename keeps the slot once it wins it.
 *
 * 700 ms is ~3 ticks of the 4 Hz stream — long enough to read a path, short
 * enough that the line still tracks a deploy in real time. Below ~500 ms the
 * eye reads motion instead of words; much above 1 s and the name lags what the
 * bytes are doing.
 */
export const MIN_HOLD_MS = 700;

/** A path split for display: the directory recedes, the basename does not. */
export interface FileParts {
  /** Trailing slash included, `""` when the file is at the site root. */
  dir: string;
  base: string;
}

/**
 * Split at the LAST separator. The directory is de-emphasised rather than
 * dropped: `harvest-timelapse.mp4` alone is ambiguous across a site with
 * per-post asset folders, and the ellipsis belongs at the tail (where the
 * basename is) rather than eating the part that disambiguates.
 */
export function splitFilePath(path: string): FileParts {
  const cut = path.lastIndexOf("/");
  return cut < 0
    ? { dir: "", base: path }
    : { dir: path.slice(0, cut + 1), base: path.slice(cut + 1) };
}

/**
 * The minimum-hold gate. One instance per upload pass; `reset()` between
 * passes so a new deploy cannot inherit the previous one's last filename.
 */
export class CurrentFileHold {
  private held: string | null = null;
  private heldSince = 0;

  /**
   * Offer the newest pick and get back what should be DISPLAYED.
   *
   * `null` in means the backend has nothing in flight to name (a commit-only
   * deploy, the gap between files). That clears the slot immediately rather
   * than holding a name for a file that has finished — a stale filename claims
   * work is happening that isn't, which is the one direction of wrongness the
   * author cannot check.
   */
  offer(pick: string | null | undefined, now: number): string | null {
    if (pick == null || pick === "") {
      this.held = null;
      return null;
    }
    if (this.held === null || now - this.heldSince >= MIN_HOLD_MS) {
      this.held = pick;
      this.heldSince = now;
    }
    return this.held;
  }

  /** Current display value without advancing the clock. */
  current(): string | null {
    return this.held;
  }

  reset(): void {
    this.held = null;
    this.heldSince = 0;
  }
}
