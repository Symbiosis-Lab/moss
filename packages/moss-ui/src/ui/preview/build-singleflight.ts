/**
 * One build per folder at a time.
 *
 * Opening a folder asks the backend to build it TWICE, by design and from two
 * places that cannot see each other: the `config-ready` event listener, and
 * `PreviewManager.initialize()`'s `waitForConfig()` poll, which exists for the
 * window-reuse case where the event already came and went. Whichever arrives
 * second would start a whole second pipeline.
 *
 * Nothing downstream caught it. `build_folder` → `run_pipeline` is re-entrant
 * by construction, so N calls are N concurrent builds rendering the same site
 * into the same `staging` directory, serialized only by the stage-write lock,
 * so the preview keeps showing the previous build's output while they queue.
 *
 * So: same key while a run is in flight → join it. Different key → let it
 * through; that is a folder switch, and the backend supersedes the old build
 * through `FolderSession`. The cell clears when the run settles, so an ordinary
 * later rebuild of the same folder is untouched — swallowing those would be a
 * worse bug than the one this fixes.
 *
 * Deliberately not a general utility: one caller, one key type, no eviction
 * policy, no timeout. Rust's `build::cache::Singleflight` is the same idea on
 * the other side of the IPC boundary, and the two are unrelated in code on
 * purpose — they dedupe different things at different layers.
 */
// Logging flows through the UI log (`../log`) — delegates
// to @tauri-apps/plugin-log in the desktop shell, falls back to console
// elsewhere — never `@tauri-apps/plugin-log` directly.
import { debug } from "../log";

export class BuildSingleflight {
  private inFlight: { key: string; done: Promise<void> } | null = null;

  /**
   * Run `start()` unless a run for `key` is already in flight, in which case
   * return that run's promise instead. Rejections propagate to every joiner.
   */
  run(key: string, start: () => Promise<void>): Promise<void> {
    const current = this.inFlight;
    if (current && current.key === key) {
      debug(`[BuildSingleflight] build already in flight for this folder — joining it`);
      return current.done;
    }

    const done = start();
    this.inFlight = { key, done };
    return done.finally(() => {
      // Clear only our own entry: a folder switch may have replaced the cell
      // while this run was going, and clearing that would un-guard the new one.
      if (this.inFlight?.done === done) this.inFlight = null;
    });
  }
}
