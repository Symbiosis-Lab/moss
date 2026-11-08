/**
 * The one owner of "how much of this folder is still in the cloud".
 *
 * `icloud-sync` is a single app-wide event carrying a single fact, so this
 * store is the one place that derives state from it: **one listener, one
 * state, and the surfaces are projections.** Each surface still decides what
 * it *draws* — that is presentation and belongs to it — but no surface
 * re-derives the state, and no surface tells another what the state is.
 *
 * # The division of labour, stated once
 *
 * - The **gate** (`gateUp`) is the build's verdict: it went up on
 *   `home_waiting` (a build finished with no servable home page) and comes down
 *   on `home_ready`.
 * - **`blocking`** is the count of render-relevant outstanding files. Whether
 *   the full-window screen is up (`withScreen`'s `screenUp`) is decided by one
 *   ladder, `resolveUnshowableReason` in `preview-state.ts`, run once by
 *   `PreviewManager` against every surface's facts (folder health, the error
 *   verdict, and this store's). This store stays a store of facts, not
 *   conclusions.
 * - **Progress** (`downloaded`/`total`/`provider`) is the supervisor's, and
 *   both surfaces read the same copy.
 * - **`stalled`** is a STATE, re-asserted by Rust on every 2s tick — a
 *   one-shot event would be overwritten by the next tick and never come back.
 *   The EDGE is recovered here, once, and handed to every subscriber, so a
 *   surface can no longer disagree with another about whether a tick is news.
 *
 * # This module owns no I/O
 *
 * It does not subscribe to host events. The store is a fold over payloads:
 * pure state, testable without a host. One caller (`PreviewManager`)
 * registers the single
 * `icloud-sync` listener and feeds [`ingest`]; every other surface
 * subscribes — so "one listener" is structural rather than a convention
 * someone must remember.
 */

/**
 * The phases Rust emits. Each producer owns its own phases exclusively:
 * `home_*` come from the build (`build::pipeline`) or a navigation check of
 * that exact selected seal; the other phases come from the supervisor
 * (`system::cloud_supervisor`). See `emit_cloud_sync`.
 */
export type CloudPhase =
  | "home_waiting"
  | "home_ready"
  | "materializing"
  | "stalled"
  | "unavailable";

/** The `icloud-sync` payload, exactly as Rust sends it. */
export interface CloudSyncPayload {
  folder?: string;
  phase: string;
  provider?: string;
  provider_label?: string;
  total: number;
  remaining: number;
  /**
   * The subset of `remaining` the render actually waits on. Null or absent from
   * the build's own ticks, and from any Rust that predates the field — the
   * fallback is `remaining`, which is what every producer meant before it.
   */
  blocking?: number | null;
  downloaded: number;
  /** Full concrete failure count; names below are only a bounded sample. */
  unavailable_count?: number;
  unavailable?: string[];
}

export interface CloudProvider {
  id: string | undefined;
  label: string | undefined;
}

export interface CloudSyncState {
  /** The last phase accepted for the active folder; null before the first. */
  phase: CloudPhase | null;
  /**
   * Sticky: a tick that carries no provider does not erase the one we know.
   * Both surfaces name the provider in their copy ("…from Google Drive"), and
   * a blank name mid-episode reads as a different, broken product.
   */
  provider: CloudProvider | undefined;
  downloaded: number;
  total: number;
  remaining: number;
  /**
   * How many of `remaining` the site is actually waiting for: the subset the
   * supervisor classified as render-relevant.
   *
   * `remaining` answers "what is moss still downloading", and it keeps that
   * meaning — the panel's counter is honest about moss fetching something. This
   * answers the different question the full-window screen turns on. The sweep asks the
   * provider for all of `.moss/`'s materialized set, so a bookkeeping file nothing renders
   * from must not raise a dismissal-less screen over a site that is already served.
   *
   * A pending structural file stays counted here even after a long wait.
   */
  blocking: number;
  /**
   * The build's home-page gate. True from `home_waiting` until `home_ready`
   * (or until [`lowerGate`], for the one surface that can end the episode
   * without Rust). While true the full-window waiting screen owns the window
   * and the progress panel renders nothing — see the module doc; this is the
   * fact, not a boolean pushed to the panel.
   *
   * It is STICKY on purpose: while it is up, the generic build-complete signal
   * is ignored, so a build that finished without a home page cannot reveal the
   * 404 underneath. There is no user-facing dismissal, because the only one would show that 404
   * and no
   * timeout, because a deadline is a guess about a download that may still
   * succeed. The backend emits exactly one of `home_ready`/`home_waiting` per
   * build, from whether it actually produced a home page
   * A later navigation can also re-evaluate the selected seal and emit either
   * gate phase without rebuilding it.
   */
  gateUp: boolean;
  /**
   * Has the preview got nothing of its own on screen?
   *
   * Pushed by `PreviewManager`, because it is the only surface that knows: it
   * is true exactly while `PreviewStatus` is `'loading'` — the initial build,
   * before the server has a page to serve. Once a page is up the status stays
   * `'ready'` for the rest of the session (`preview-state.ts`: rebuilds are
   * silent), so this cannot flicker on a watcher rebuild.
   *
   * It is pushed rather than derived for the same reason `gateUp` is not
   * pushed: the fact has ONE owner, and this one belongs to the preview.
   * `PreviewManager` is the one place that folds this, `gateUp`, `blocking`
   * and `stalled` into the ladder that decides what — if anything — owns the
   * window; see `resolveUnshowableReason` in `preview-state.ts`.
   */
  previewBlank: boolean;
  /** The supervisor's 90s-of-silence verdict. A state, not an event. */
  stalled: boolean;
  /** Null when an older event reported errors without an exact count. */
  unavailable_count: number | null;
  /**
   * A bounded sample of names with concrete read errors. The full count is
   * `unavailable_count`; each supervisor snapshot replaces both values.
   */
  unavailable: string[];
}

/**
 * What changed on this tick. The edges are computed once, here, because two
 * surfaces answering "is this news?" separately is what let a re-asserted
 * state re-open a panel the user had closed.
 */
export interface CloudSyncChange {
  state: CloudSyncState;
  /** The tick where `stalled` first became true. */
  stalledEdge: boolean;
  /**
   * The supervisor tick where observed arrivals increased.
   *
   * The only edge that can turn a "still arriving" answer into a different one,
   * which is what the editor's pending pane is waiting for. It is
   * an edge and not a level for the reason every other edge here is: a
   * subscriber keeping its own copy of `downloaded` to spot the change would be
   * a second answer to "is this news?", and that is the shape this store
   * exists to remove.
   *
   * A stall, a refusal, and a `previewBlank` push all report false — nothing
   * arrived on any of them.
   */
  filesArrived: boolean;
}

const INITIAL: CloudSyncState = {
  phase: null,
  provider: undefined,
  downloaded: 0,
  total: 0,
  remaining: 0,
  blocking: 0,
  gateUp: false,
  previewBlank: false,
  stalled: false,
  unavailable: [],
  unavailable_count: 0,
};

type Subscriber = (change: CloudSyncChange) => void;

class CloudSyncStore {
  private state: CloudSyncState = { ...INITIAL };
  private subscribers = new Set<Subscriber>();
  private folderPath: string | null = null;
  /** Changes on every accepted supervisor snapshot or folder switch. */
  private snapshotRevision = 0;
  /** Has the supervisor spoken yet? See the denominator note in `ingest`. */
  private supervisorReported = false;

  /**
   * Scope the stream to one folder. The event is app-wide and the previous
   * folder's supervisor only observes cancellation between ticks, so one
   * already in flight lands after a switch — and would raise a waiting screen
   * over a folder that is entirely local. Filtering happens HERE, once, rather
   * than in each surface against its own copy of the path.
   *
   * Switching folders resets the state: the new folder has its own gate, and
   * inheriting the old one's counts would show the user someone else's
   * progress.
   */
  setFolder(path: string | null): void {
    if (this.folderPath === path) return;
    this.folderPath = path;
    this.snapshotRevision++;
    // `previewBlank` survives the switch: it describes the WINDOW, not the
    // folder, and the preview does not repaint just because the store was
    // retargeted. Zeroing it here would drop the screen for the new folder
    // until the next progress tick re-asserted it.
    this.state = { ...INITIAL, previewBlank: this.state.previewBlank };
    this.supervisorReported = false;
  }

  /**
   * Tell the store whether the preview has anything of its own on screen.
   *
   * `PreviewManager` is the only caller — see [`CloudSyncState.previewBlank`].
   * A no-op when nothing changes, so the `'loading'` branch of `renderStatus`
   * can call it on every progress tick without generating traffic.
   */
  setPreviewBlank(blank: boolean): void {
    if (this.state.previewBlank === blank) return;
    this.state = { ...this.state, previewBlank: blank };
    this.notify({
      state: this.state,
      stalledEdge: false,
      filesArrived: false,
    });
  }

  getState(): CloudSyncState {
    return this.state;
  }

  getUnavailableSnapshot(): { folder: string | null; revision: number; count: number | null } {
    return {
      folder: this.folderPath,
      revision: this.snapshotRevision,
      count: this.state.unavailable_count,
    };
  }

  subscribe(fn: Subscriber): () => void {
    this.subscribers.add(fn);
    return () => this.subscribers.delete(fn);
  }

  /**
   * Fold one Rust payload into the state and notify. The Tauri listener calls
   * this; tests call it directly.
   *
   * An unknown phase is dropped rather than guessed at: a future Rust build
   * teaching this event a new phase must not silently move the gate.
   */
  ingest(payload: CloudSyncPayload): void {
    // Events from Rust builds that predate the field carry no `folder`; accept
    // those rather than going silent.
    if (payload.folder && this.folderPath && payload.folder !== this.folderPath) {
      return;
    }
    if (!isKnownPhase(payload.phase)) return;

    const prev = this.state;
    const phase = payload.phase;

    const fromBuild = phase === "home_waiting" || phase === "home_ready";
    const gateUp =
      phase === "home_waiting" ? true : phase === "home_ready" ? false : prev.gateUp;
    const stalled = fromBuild ? prev.stalled : phase === "stalled";
    // Build ticks own the gate. Only the supervisor can replace its advisory,
    // including with an empty snapshot after recovery.

    // Both producers carry a `downloaded`/`total` pair and they are NOT the
    // same denominator — deliberately. The build counts what its scan saw
    // evicted; the supervisor counts what its own sweep found, which is
    // strictly more, because a pre-Sonoma placeholder replaces the real
    // filename with a hidden `.name.icloud` sibling the scan's walk never
    // returns. They cannot be reconciled without making the supervisor blind
    // to exactly the case it exists for.
    //
    // So the build's numbers are a SEED, used until the supervisor's first
    // tick; every tick after that wins. Otherwise "3 of 5" and "3 of 12"
    // alternate on screen every couple of seconds. A gate raise carrying
    // nothing to report likewise keeps what we have — the screen it puts up
    // has nowhere else to get a count.
    if (!fromBuild) this.supervisorReported = true;
    const keepCounts =
      fromBuild && (this.supervisorReported || payload.total === 0);

    this.state = {
      phase,
      provider: fromBuild && this.supervisorReported
        ? prev.provider
        : payload.provider || payload.provider_label
          ? { id: payload.provider, label: payload.provider_label }
          : prev.provider,
      downloaded: keepCounts ? prev.downloaded : payload.downloaded,
      total: keepCounts ? prev.total : payload.total,
      remaining: keepCounts ? prev.remaining : payload.remaining,
      blocking: keepCounts ? prev.blocking : (payload.blocking ?? payload.remaining),
      gateUp,
      previewBlank: prev.previewBlank,
      stalled,
      unavailable: fromBuild ? prev.unavailable : (payload.unavailable ?? []),
      unavailable_count: fromBuild ? prev.unavailable_count :
        (payload.unavailable_count ?? (phase === "unavailable" ? null : 0)),
    };
    if (!fromBuild) this.snapshotRevision++;

    this.notify({
      state: this.state,
      stalledEdge: stalled && !prev.stalled,
      filesArrived: !fromBuild && this.state.downloaded > prev.downloaded,
    });
  }

  /**
   * Give the window back, without a `home_ready` from Rust.
   *
   * There is a second way the waiting episode ends: the preview takes the
   * window for something more urgent — a build error, whose message the user
   * must see instead of a download count. Rust never learns this happened, so
   * no `home_ready` is coming.
   *
   * This has to be a store operation rather than the manager quietly dropping
   * its own copy, because the fact has a SECOND reader. The panel stays silent
   * while the screen is up, so a manager-local dismissal would leave the store
   * saying "screen up" forever and the panel mute for the rest of the episode
   * — it would never report the download that is still running. That is the
   * same two-owners-one-fact shape this store exists to remove; the fix is that
   * whoever ends the episode says so here, once, and both surfaces hear it.
   *
   * BOTH contributors drop, not just the gate. The caller's claim is "I am
   * showing something else now", and leaving `previewBlank` set would let the
   * next supervisor tick repaint the waiting screen over the error message two
   * seconds later — the exact fight the error branch of `renderStatus` was
   * written to settle.
   */
  lowerGate(): void {
    const prev = this.state;
    // The guard is on the CONTRIBUTORS, not on a derived "is the screen up"
    // conclusion. `previewBlank` with nothing yet downloading raises nothing
    // today but arms the ladder for the next tick. Returning early because
    // "nothing was showing" left the arming half set, which is how an error
    // message got painted over two seconds after the caller claimed the
    // window.
    if (!prev.gateUp && !prev.previewBlank) return;
    this.state = { ...prev, gateUp: false, previewBlank: false };
    this.notify({
      state: this.state,
      stalledEdge: false,
      // The episode ended because another surface claimed the window, not
      // because anything downloaded.
      filesArrived: false,
    });
  }

  private notify(change: CloudSyncChange): void {
    for (const fn of [...this.subscribers]) {
      try {
        fn(change);
      } catch (err) {
        // One surface throwing must not stop the others from being told —
        // a panel that fails to render must not also strand the waiting screen.
        console.warn("[cloud-sync] subscriber failed", err);
      }
    }
  }

  /** Test seam: forget everything, including subscribers. */
  reset(): void {
    this.state = { ...INITIAL };
    this.subscribers.clear();
    this.folderPath = null;
    this.snapshotRevision = 0;
    this.supervisorReported = false;
  }
}

function isKnownPhase(phase: string): phase is CloudPhase {
  return (
    phase === "home_waiting" ||
    phase === "home_ready" ||
    phase === "materializing" ||
    phase === "stalled" ||
    phase === "unavailable"
  );
}

/** The single instance. There is one cloud, one folder, one answer. */
export const cloudSync = new CloudSyncStore();
