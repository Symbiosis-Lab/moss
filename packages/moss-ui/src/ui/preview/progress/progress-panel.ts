/**
 * ProgressPanel — single-bar pipeline progress for build and publish.
 *
 * # The primary/secondary rule
 *
 * The panel renders ONE primary bar showing the current pipeline phase —
 * a build phase (opening/scanning/materializing/compiling) when
 * publish is idle, or a publish phase (waiting/preparing/syncing/
 * uploading/committing/finalizing) when publish is engaged.
 *
 * Per event: is its progress already representable in the primary bar's own
 * current/total?
 *
 *   If yes, PRIMARY-FEED — folds into the phase state machine, no task row:
 *     - BackgroundProgress{task: markdown|html_pages} — PAGE_PHASE_BY_TASK,
 *       the only two tasks that count pages into the compiling stage
 *     - icloud-sync (cloud provider downloads) — feeds the download
 *       readout (downloaded/total/remaining) for as long as
 *       `remaining > 0`, whatever stage the build has reached
 *     - DeployWaitingForBuild / deploy-progress — advances publish stage,
 *       feeds uploading current/total
 *
 *   If no, SECONDARY-TASK — its own bg:<id> row, independent of the
 *   primary pipeline:
 *     - BackgroundProgress for every OTHER task (images, videos, assets,
 *       notebooks, sealing, …) — `bgTaskRowDecision` in progress-panel-stage.ts
 *       decides the row
 *     - PluginProgress (deploy/syndicate hooks running concurrently)
 *     - VideoConversionProgress — enriches bg:videos with filename + phase
 *     - the Domain Job (Preview-scope `PanelTaskUpdate`, DNS readiness
 *       check after publish)
 *
 * # The Build · Publish stepper
 *
 * A two-dot stepper appears below the primary bar whenever publish is
 * engaged (`currentPublishStage !== 'idle'`). Build-only flows don't
 * show it — there's no second step to anticipate.
 */
import { host } from "../../host";
import { debug, info, trace, warn } from "../../log";
import { ProgressPanelRenderer } from "./progress-panel-renderer";
import type { PreviewStage } from "../preview-state";
import {
  computeStageLabel,
  computePublishLabel,
  formatStatus,
  nextStage,
  nextPublishStage,
  groupNotices,
  noticeGroupKey,
  isAcknowledge,
  outlivesBuildStart,
  noticeTier,
  compilingLabelInput,
  nextPageProgress,
  bgTaskRowDecision,
  staleTaskAdvisoryKeys,
  type PageProgress,
  type PublishStage,
  type PublishLabelInput,
  type StageEvent,
  type StageLabel,
  type StageLabelInput,
  type StatusInput,
  type StatusResult,
  type ProgressTask,
  type AdvisoryEntry,
  type NoticeVM,
  type PanelViewModel,
} from "./progress-panel-stage";
import type {
  Action,
  Amount,
  AssetReady,
  BackgroundProgress,
  DeployProgress,
  PanelTaskWire,
  PluginProgressEvent,
  Scope,
  Severity,
  VideoConversionProgress,
} from "@symbiosis-lab/moss-ui";
import { onMossEvents, type MossEventPayload } from "../../utils/moss-events";
import {
  type NarratedTask,
  formatNarration,
  formatAwaitingCopy,
  escapeLabel,
  escapeKind,
} from "../../editor/narration-format";
import { eventBus, Events, type PushNoticeEvent, type ClearNoticeEvent } from "../../context/event-bus";
import { UploadRateEstimator, type RateReading } from "./upload-rate";
import { PublishReadout } from "./upload-readout";
import { t } from "../../i18n/index";
import { unavailableAdvisoryReason } from "../screen/cloud-wait";
import { buildOverdueAdvisoryReason } from "../screen/surface";
import { cloudSync, type CloudSyncChange } from "../cloud/cloud-sync-store";
import { CloudFileDetails } from "./cloud-file-details";

// The panel's shared type contract lives in `progress-panel-stage.ts` — the
// renderer, the status block and the formatters all need it, and importing it
// from HERE made every one of them a cycle back through the panel itself.
export type {
  ProgressTask,
  AdvisoryEntry,
  NoticeVM,
  PanelViewModel,
} from "./progress-panel-stage";

const CLOUD_UNAVAILABLE_ADV_KEY = "cloud-sync-unavailable";

/**
 * The reveal state of the panel BODY — the SINGLE source of truth for
 * visibility, owned solely by `ProgressPanel`. A boolean `_isHidden`
 * could not represent peek/pin/surfaced and produced the checkAutoClose-vs-pin
 * race (advisory-clear would auto-close a panel the user had pinned).
 *
 *   • `minimized` — no panel body. The persistent seam + advisory dot REMAIN
 *     (ProgressSeam owns them on the titlebar layer). This is the
 *     quiet resting state; `isHidden` is derived from it.
 *   • `surfaced`  — auto-shown when a consequential event needs attention:
 *     a blocking advisory, an engaged publish, or a non-ambient task. The panel
 *     pushed itself unbidden; auto-close may retract it once the work settles.
 *   • `peek`      — shown on hover. Transient; auto-closes on leave/settle.
 *   • `pinned`    — shown on click. The user asked for it; it SURVIVES
 *     mouse-leave AND advisory-clear auto-close. It closes only via ⌄ minimize.
 *
 * Seam INPUTS (hover/click) are mapped to these transitions ONLY here — the seam
 * emits inputs, the panel decides (single owner). `render()` derives the body's
 * visibility from this enum (`isHidden === state === 'minimized'`).
 */
export type RevealState = "minimized" | "peek" | "pinned" | "surfaced";

/**
 * Format a Job's frozen elapsed span for the receipt. The wire
 * carries `elapsed_ms` as a string (specta encodes u64 as string); the renderer
 * formats it — "2.1s" under a minute, "N min" at/over a minute. Empty when
 * absent or non-positive (no `· ` segment is added).
 */
function formatElapsed(ms: string | number | null): string {
  if (ms == null) return "";
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return "";
  if (n < 60_000) return `${(n / 1000).toFixed(1)}s`;
  return `${Math.round(n / 60_000)} min`;
}

/**
 * Format a Job's success quantity for the receipt — the
 * "142 files" / "3 posts" segment in "Published · 142 files · 2.1s". The wire
 * carries `amount` as `{ count, noun }` (count a string: specta encodes u64 as
 * string); the renderer composes the text from these values.
 * Empty when absent so no `· ` segment is added.
 */
// Core Job receipt verbs (Rust Verb consts) → localized; plugin verbs pass through.
const VERB_KEYS: Record<string, Parameters<typeof t>[0]> = {
  Built: "verb.built",
  Published: "verb.published",
  Uploaded: "verb.uploaded",
  Deployed: "verb.deployed",
  Connected: "verb.connected",
  Sent: "verb.sent",
  Synced: "verb.synced",
  Prepared: "verb.prepared",
  Optimizing: "verb.optimizing",
};
function localizeVerb(verb: string | undefined | null): string {
  if (!verb) return "";
  const key = VERB_KEYS[verb];
  return key ? t(key) : verb;
}

// Producer-supplied Amount nouns → localized. en keeps singular/plural ("1 file");
// Chinese is invariant (no trailing-"s" to strip).
const NOUN_KEYS: Record<string, Parameters<typeof t>[0]> = {
  pages: "noun.pages",
  files: "noun.files",
  posts: "noun.posts",
};
function formatAmount(amount: Amount | null): string {
  if (amount == null) return "";
  const n = amount.count;
  // Localize the producer's plural noun; then apply the English singular rule
  // (drop trailing "s" for count 1). Chinese nouns don't end in "s", so the
  // rule is a no-op there. Unknown nouns pass through verbatim.
  const nounKey = NOUN_KEYS[amount.noun];
  let noun = nounKey ? t(nounKey) : amount.noun;
  if (n === 1 && noun.endsWith("s")) noun = noun.slice(0, -1);
  return `${n} ${noun}`;
}

/**
 * The overdue-build advisory's key. Module-level because two
 * paths touch it: the `FolderHealthChanged` listener (set/clear on the
 * degraded flag) and `setFolder` (clear on folder switch).
 */
const BUILD_OVERDUE_ADV_KEY = "build-overdue";

/** The preview server's start failure; cleared when a later server comes up. */
const PREVIEW_SERVER_ADV_KEY = "preview-server-error";

export class ProgressPanel {
  private tasks: Map<string, ProgressTask> = new Map();
  private advisories: Map<string, AdvisoryEntry> = new Map();
  /**
   * Blocking advisory KEYS the user has explicitly closed the panel on.
   * See `dismiss()`. `shouldSurface()` skips a `Blocking` entry whose
   * key is here. `deleteAdvisory` drops the matching entry the moment a key
   * leaves `advisories`, so a problem that leaves and comes back is new.
   */
  private dismissedAdvisoryKeys: Set<string> = new Set();
  /** Explicit first-arrival tracking for `upsertTask`'s auto-show gate (see its comment). */
  private arrivedTasks: Set<string> = new Set();
  /**
   * Start time per background-job kind (images/videos/assets/notebooks/
   * sealing), keyed on the raw `BackgroundProgress.task` string — NOT
   * `arrivedTasks`, which videos skip via `skipArrival` and would therefore
   * never register a "first tick" for. Read and cleared on the terminal
   * `completed:true` tick.
   */
  private bgJobStartedAt: Map<string, number> = new Map();
  /**
   * Advisory keys a `BackgroundProgress` task has re-raised since THAT TASK's
   * own last `completed:true` tick. Reconciliation, not a blanket clear: a
   * rebuild's task set is not the whole site. Images and videos skip their
   * entire dispatch when their inputs are unchanged, and an unchanged markdown
   * page replays its cached parse without re-deriving its advisories, so a
   * task's absence this build is not evidence its advisory cleared. Only that
   * same task's next `completed:true` tick, with the item back in its reported
   * set, is.
   * Swept at that tick in the `BackgroundProgress` handler below; sticky and
   * non-`BackgroundProgress` advisories (dismissal memory, PUSH_NOTICE) never go
   * through this map.
   */
  private taskRefreshedKeys: Map<string, Set<string>> = new Map();

  /**
   * The folder whose health this panel reports — the overdue-build
   * advisory's scope. `FolderHealthChanged` is transitions-only and a
   * closing folder's sweep is cancelled before it can emit an all-clear, so
   * without a scope a degraded=true advisory from folder A would outlive a
   * switch to folder B indefinitely (B's sweep never owns the clearing
   * edge). Set beside `cloudSync.setFolder` — the same one-filter
   * discipline that store documents.
   */
  private healthFolder: string | null = null;
  private cloudFileDetails = new CloudFileDetails(() => this.renderPanel());

  /**
   * Re-scope folder health to a new folder. Drops the previous folder's
   * overdue-build advisory: the new folder starts healthy (its sweep emits
   * transitions only), and the old folder's all-clear will never arrive.
   */
  setFolder(path: string | null): void {
    if (this.healthFolder === path) return;
    this.healthFolder = path;
    this.cloudFileDetails.reset();
    this.renderer.resetCloudFilesDisclosure();
    if (this.deleteAdvisory(BUILD_OVERDUE_ADV_KEY)) this.renderPanel();
  }

  /**
   * The sole door for removing a key from `this.advisories` — every removal
   * in this class goes through here, never `this.advisories.delete()`
   * directly, so none can forget to drop the matching `dismissedAdvisoryKeys`
   * entry too (a key returning later is a NEW problem, not an old one still
   * dismissed). Returns whatever `Map.delete` returned, so a caller that
   * branches on "was it there" keeps working unchanged.
   */
  private deleteAdvisory(key: string): boolean {
    this.dismissedAdvisoryKeys.delete(key);
    return this.advisories.delete(key);
  }

  /**
   * Scoped advisory key for a publish/deploy failure. A deploy failure carries
   * the real error as the advisory DETAIL so the standard surface (severity dot
   * + collapsible notice + re-expand handle) renders — mirroring the
   * `preview-server-error` key. Cleared SCOPED (never blanket) on a re-publish
   * or a successful publish, like the preview-server-error advisory.
   */
  private readonly PUBLISH_ADV_KEY = "publish-error";
  /**
   * Ids of Preview-scope Jobs currently in `Awaiting`. An Awaiting Job
   * is the one interrupt — it satisfies `shouldSurface()`. Cleared when
   * the Job leaves Awaiting (terminal or resumed).
   */
  private awaitingJobs: Set<string> = new Set();

  // ── Narrated/Awaiting ingestion (non-preview scope) ──

  /**
   * The pill popover collects non-preview `tone==="narrated"` tasks.
   * Keyed by task id (as `narrated:<id>`). `failed` tasks persist until
   * dismissed; `succeeded`/`cancelled` tasks fade after their linger.
   */
  private narratedTasks: Map<string, NarratedTask> = new Map();

  /**
   * Fade-timer state for succeeded/cancelled narrated tasks (failed
   * MUST NOT get a timer).  Keyed by the same `narrated:<id>` key.
   */
  private narratedFadeTimers: Map<string, ReturnType<typeof setTimeout>> = new Map();

  /**
   * Linger duration for completed narrated tasks — mirrors the old Narrator
   * COMPLETION_LINGER_MS + COMPLETION_FADE_MS (1000 + 400 = 1400 ms total).
   */
  private static readonly NARRATION_FADE_MS = 1_400;

  private staleTimers: Map<string, ReturnType<typeof setTimeout>> = new Map();
  /**
   * Deferred-upsert timers for fast build hooks (see handlePluginProgress).
   * A `start` for a flashing hook schedules the row ~250ms out instead of
   * painting it immediately; the matching `done` cancels the timer so a
   * sub-250ms hook never paints at all. Keyed by the same task id the row
   * would carry, with the captured task to upsert when the timer fires.
   */
  private deferredUpserts: Map<
    string,
    { timer: ReturnType<typeof setTimeout>; task: ProgressTask }
  > = new Map();
  /**
   * The reveal state is the single source of truth for the panel
   * body's visibility, owned solely here. Default `minimized` (collapsed): the
   * resting state is a quiet hairline, and the body surfaces only on a
   * consequential EVENT (`surfaceOnInterrupt`/`show`) or a hover-peek. A routine
   * build is already quiet (the stage machine never surfaces it), so starting
   * minimized just makes the resting state consistent from t=0 instead of relying
   * on the first `checkAutoClose` to retract a briefly-shown empty panel.
   * Every visibility mutation funnels through `setRevealState()`; nothing else
   * writes this field. `_isHidden` is DERIVED from it (a read-only getter), so
   * the many `if (this._isHidden) …` read sites keep working unchanged.
   */
  private revealState: RevealState = "minimized";
  /**
   * The panel debounces raw seam hover inputs through its reveal timers. `peekOpenTimer` is the ~100ms open-delay
   * (avoids the pass-through bloom of a cursor merely crossing the seam);
   * `peekCloseTimer` is the ~200ms close-delay after a hover-leave (so a quick
   * re-entry, a click-to-pin, or an interrupt can cancel the close). Both are
   * cancelled the moment a competing input/interrupt arrives. They are the ONLY
   * timers in the reveal logic; the seam module holds none.
   */
  private peekOpenTimer: ReturnType<typeof setTimeout> | null = null;
  private peekCloseTimer: ReturnType<typeof setTimeout> | null = null;

  private renderer: ProgressPanelRenderer;
  private allCompleteCallbacks: Array<() => void> = [];
  private taskFailedCallbacks: Array<(taskId: string) => void> = [];
  /**
   * Top-severity channel. The panel is the SOLE owner of the advisory
   * store; the persistent ProgressSeam keeps no copy and learns the computed
   * `topSeverity` from here, so its dot's color reflects the highest advisory
   * tier. PreviewManager wires this to `seam.setTopSeverity(s)`. De-duped to
   * actual transitions.
   */
  private topSeverityCallbacks: Array<(severity: "broken" | "degraded" | "none") => void> = [];
  private lastTopSeverity: "broken" | "degraded" | "none" = "none";
  /** Seam VISIBILITY tracks "is there content" (a task / active stage /
   * advisory). The seam (the quiet line + the reveal hit-region) is NOT shown at
   * rest: a persistent line reads as "stuck progress" and offers a reveal handle
   * with no panel behind it. PreviewManager wires this to seam.setVisible(). */
  private contentChangeCallbacks: Array<(present: boolean) => void> = [];
  private lastContentPresent = false;
  private unlistenFns: Array<() => void> = [];

  /**
   * Whether `PreviewManager`'s ladder (`resolveUnshowableReason`) currently
   * holds the window. Pushed, not derived — this panel does not know which
   * of the eight reasons is up, only that one is. While true this panel
   * renders nothing; see `setScreenUp`.
   */
  private screenUp = false;
  private serverBaseUrl: string | null = null;
  private iframe: HTMLIFrameElement | null = null;

  /** Coarse pipeline stage driving the top-level "current stage" label. */
  private currentStage: PreviewStage = "initial";
  private stageContext: {
    found?: number;
    evicted?: number;
    provider?: string;
    downloaded?: number;
    total?: number;
    /** The page counter. One record rather than the `current`/`buildTotal`
     *  pair it replaces, because the phase is not an extra fact about the
     *  numbers — it is what makes them comparable. `nextPageProgress` is the
     *  only door in; it says which events are refused and why. */
    pages?: PageProgress;
    /**
     * Files still in the cloud, from the last `icloud-sync` tick for this
     * folder. Set alongside `downloaded`/`total`, cleared when it reaches zero.
     *
     * This is the one part of `stageContext` that is NOT scoped to a stage. A
     * download outlives the build that noticed it — the supervisor keeps
     * draining while later builds compile and go ready — so `setStage` carries
     * it across the `ready` reset and `activePrimaryLabel` reports it whatever
     * stage the build is in. Scoping it to a stage would hide a download that is still running.
     */
    remaining?: number;
  } = {};

  /**
   * Publish stage drives the same primary bar when active. While
   * `currentPublishStage !== 'idle'`, the bar shows publish text; otherwise
   * it shows build text. This is what unifies the previously-separate
   * stage tier and `deploy:push`/`deploy-waiting` tasks into one bar.
   */
  private currentPublishStage: PublishStage = "idle";
  /**
   * Everything this publish knows about itself — counts, bytes, rate, a11y
   * milestones, the in-flight filename (`upload-readout.ts`).
   */
  private readonly publish = new PublishReadout();

  /**
   * `Published · 58.0 MB · 12.4s`, minted at the moment a publish lands.
   *
   * Stored rather than derived, because the readout is reset at `complete`.
   */
  private publishReceipt: string | null = null;
  private publishReceiptTimer: ReturnType<typeof setTimeout> | null = null;

  /** Auto-remove tasks after 15s of no updates (defense-in-depth) */
  private static readonly STALE_TIMEOUT_MS = 15_000;

  /**
   * Maximum dwell time for the `Built · N pages · X.Xs` completion receipt. The receipt is a confirmation, not a resident card — after this
   * window it retracts to the pill (the panel auto-closes if nothing else is
   * keeping it open). Kept deliberately short so a "Built · …" header never
   * lingers as a floating card.
   */
  static readonly RECEIPT_DWELL_MS = 2_500;

  /**
   * Delay before a seam `onHoverEnter` opens a transient `peek`.
   * Long enough that a cursor merely PASSING THROUGH the ~14-16px seam zone on
   * its way elsewhere doesn't bloom the panel open (Rauno's pass-through guard).
   */
  static readonly PEEK_OPEN_DELAY_MS = 100;

  /**
   * Hover close-delay after a seam `onHoverLeave` before a `peek` retracts to
   * `minimized`. The delay is long enough to forgive a quick exit-and-return
   * (e.g. the cursor briefly leaving the zone), and cancelled outright by a
   * re-entry, a click-to-pin, or an interrupt-promote.
   */
  static readonly PEEK_CLOSE_DELAY_MS = 200;

  /**
   * The last completed build, or null when no build has completed since the
   * receipt last retracted. Both facts are captured together AT completion, so
   * the dwell timer can only discard data belonging to the build it armed for.
   * `elapsedMs` is the measured span in ms, held raw (NOT a pre-formatted
   * "2.1s") so the renderer formats the Duration at paint — values on data,
   * formatting at the edge (Hickey). Either field is null when that fact is
   * unknown for THIS build ("it finished, we don't know how long"): the receipt
   * still renders and merely drops the segment it cannot fill.
   */
  private lastBuild: { elapsedMs: number | null; pageCount: number | null } | null = null;
  /**
   * The page count of the build IN FLIGHT — written when the `html_pages` task
   * reports its total, consumed into `lastBuild` at that same build's
   * completion. It is deliberately neither receipt state nor `stageContext`,
   * because both are wiped on a clock that is not this build's: as receipt state
   * the dwell timer wiped it 2.5s after the PREVIOUS build, blanking the header
   * of any rebuild that started inside that window, and in `stageContext`
   * `setStage("ready")` wiped it before the receipt was even minted (the
   * backend's real order is ready THEN BuildComplete — the terminal progress
   * event closes the build fn, BuildComplete comes later from the spawned seal
   * task). Owned by the build, consumed by that build's completion.
   */
  private pendingPageCount: number | null = null;
  /**
   * The live Build parent Job's row id (`job:<id>`), captured
   * when a top-level `kind === "build"` Job is first seen. Media sub-jobs and the
   * fine-grained `VideoConversionProgress` detail nest under it. Cleared at
   * build-start and when the Build Job terminates. `null` on a content-only
   * rebuild (no parent Job is born — invariant #6).
   */
  private buildJobId: string | null = null;
  /**
   * The Build parent Job's terminal receipt text
   * (`Built · N pages · X.Xs`), composed from the parent Job's
   * verb·amount·elapsed at its `Succeeded` transition. When set it is the
   * AUTHORITATIVE completion header (a media build); `completionReceiptText()`
   * prefers it over the legacy `BuildComplete`-derived path, so the
   * two never double-render. `null` for a media-less build (no parent Job — the
   * legacy `BuildComplete` listener is the sole receipt there).
   */
  private buildReceipt: string | null = null;
  /** Pending retract timer for the completion receipt (R11). */
  private receiptTimer: ReturnType<typeof setTimeout> | null = null;

  /**
   * The last publish/deploy failure. When non-null,
   * `checkAutoClose` is suppressed (the user must see the red status), and
   * the status line shows the failure reason. Cleared by:
   *   - `advancePublishStage({kind:"complete"})` — publish succeeded
   *   - A re-publish `waiting/progress` event while stage is "failed"
   *   - `setStage("opening" | "initial")` — new build starts
   */
  private lastFailure: string | null = null;

  /**
   * Durable hover flag for the PANEL BODY (not the seam).
   * Set by `onPanelBodyHoverEnter()`; cleared by `onPanelBodyHoverLeave()`.
   * Suppresses `checkAutoClose()` while the pointer is on the glass.
   */
  private bodyHovered = false;

  /**
   * Durable focus flag for the PANEL BODY. Set by
   * `onPanelBodyFocusIn()`; cleared by `onPanelBodyFocusOut()`. Suppresses
   * `checkAutoClose()` while any child has focus.
   */
  private bodyFocused = false;

  /**
   * Current video encoding status, stashed from
   * VideoConversionProgress events for use in buildViewModel.
   * Preferred over build/publish stage labels in the status line while encoding.
   * Cleared when BackgroundProgress { task: "videos", completed: true } arrives.
   */
  private mediaStatus: {
    verb: string;
    subject: string;
    current: number;
    total: number;
  } | null = null;

  /**
   * Grace period before a fast build hook's row paints. The `enhance` hook
   * (and any other build-pipeline hook) is bracketed by start/done
   * PluginProgress events around a step that, on an incremental rebuild, is
   * near-instant — the row would upsert then remove inside one ~16ms tick,
   * a visible flash. Deferring the upsert this long lets a sub-window hook
   * cancel before it ever paints, while a genuinely-slow enhancer (e.g. a
   * comments/social network fetch) still surfaces after the grace period.
   */
  private static readonly DEFER_UPSERT_MS = 250;

  /**
   * Plugin hooks carrying publish work: user-triggered, network-bound, and
   * genuinely long. Their rows paint immediately (every other non-`process`
   * hook is deferred — see handlePluginProgress) and get the long stale window
   * below, which is why they emit no fake progress.
   */
  private static readonly PUBLISH_HOOKS: ReadonlySet<string> = new Set([
    "deploy",
    "syndicate",
  ]);

  constructor(container: HTMLElement, iframe?: HTMLIFrameElement | null) {
    this.renderer = new ProgressPanelRenderer(container);
    this.renderer.setCloudFilesCallback(() => { void this.cloudFileDetails.request(); });
    // Prevent hover flicker: a pointer on the panel glass counts as
    // inside the reveal region — see onPanelHoverEnter/onPanelHoverLeave.
    this.renderer.setHoverCallbacks(
      () => this.onPanelHoverEnter(),
      () => this.onPanelHoverLeave(),
    );
    // Wire the handle toggle. One method, branches on
    // reveal state: dismiss() when already open (pinned OR auto-surfaced) —
    // this IS the user closing the panel, see dismiss() — pin() only from a
    // transient hover-peek (keep-open). A `surfaced` panel is already open,
    // so its handle must COLLAPSE in one click — pinning it first would
    // have trapped the user needing two clicks to minimize an
    // auto-popped panel.
    this.renderer.setHandleCallbacks(() => {
      if (this.revealState === "pinned" || this.revealState === "surfaced") {
        this.dismiss();
      } else {
        this.pin();
      }
    });
    // Wire durable body hover/focus suppressors.
    this.renderer.setBodyCallbacks(
      () => this.onPanelBodyHoverEnter(),
      () => this.onPanelBodyHoverLeave(),
      () => this.onPanelBodyFocusIn(),
      () => this.onPanelBodyFocusOut(),
    );
    this.iframe = iframe ?? null;
  }

  /**
   * Set the server base URL for asset placeholder swapping.
   * Called by PreviewManager once the dev server is ready.
   */
  setServerBaseUrl(url: string): void {
    this.serverBaseUrl = url;
  }

  /** Subscribes to everything the panel reports, in one table. */
  async init(): Promise<void> {
    const subscribed = await Promise.allSettled([
      onMossEvents({
        BackgroundProgress: (e) => this.onBackgroundProgress(e.payload),
        PreviewServerFailed: (e) => this.onPreviewServerFailed(e.payload.message),
        FolderHealthChanged: (e) => this.onFolderHealthChanged(e.payload),
        VideoConversionProgress: (e) => this.onVideoConversionProgress(e.payload),
        // Plugin hook progress (process, deploy, syndicate).
        PluginProgress: (e) => this.handlePluginProgress(e.payload),
        // The unified Job wire: the panel claims Preview-scope Jobs; the
        // titlebar, inline and toast renderers claim the other scopes.
        PanelTaskUpdate: (e) => this.handleJobUpdate(e.payload),
        AssetReady: (e) => this.handleAssetReady(e.payload),
        // The one post-seal sweep of background variants that materialized
        // this build: each goes through the same swap as AssetReady, and this
        // is the signal that fires after the background .webp encode lands.
        AssetsSettled: (e) => e.payload.changed.forEach((asset) => this.handleAssetReady(asset)),
        BuildComplete: (e) => this.onBuildComplete(e.payload.total_time_ms),
        // Deploy waits for background tasks: "Waiting for build…" on the primary bar.
        DeployWaitingForBuild: () => this.advancePublishStage({ kind: "waiting" }),
      }),
      host.engine.on("scan-progress", (p: { found: number; evicted: number }) => p && this.onScanProgress(p)),
      host.engine.on("scan-complete", (p: { total_files: number; evicted_count: number; cloud_provider: string | null }) => p && this.onScanComplete(p)),
      host.engine.on("deploy-progress", (p: DeployProgress) => p && this.onDeployProgress(p)),
      cloudSync.subscribe((change) => this.onCloudSync(change)),
      eventBus.on<PushNoticeEvent>(Events.PUSH_NOTICE, (entry) => this.onPushNotice(entry)),
      eventBus.on<ClearNoticeEvent>(Events.CLEAR_NOTICE, (entry) => this.onClearNotice(entry)),
    ]);
    // Keep what did subscribe, so destroy() still releases it.
    this.unlistenFns = subscribed.flatMap((s) => (s.status === "fulfilled" ? [s.value] : []));
    for (const s of subscribed) if (s.status === "rejected") warn(`[ProgressPanel] Failed to setup a listener: ${s.reason}`);
    debug("[ProgressPanel] Event listeners initialized");
  }

  // BackgroundProgress: markdown/html_pages feed the compiling-stage bar
  // and add no row; every other task gets one (bgTaskRowDecision, below).
  // The ONLY completion signal for videos is `{task:"videos",
  // completed:true}`, always propagated to removeTask below.
  private onBackgroundProgress(p: BackgroundProgress): void {
    // Comment-sync failures are LOCAL-FIRST non-blocking issues —
    // they do NOT surface in the advisory window or dot. Comment sync
    // status + retry lives in Services > Comments (services.ts), which
    // owns its own separate BackgroundProgress listener. We bail at the
    // VERY TOP, before any stageContext write: comment-sync always emits
    // (current:1, total:1), so allowing it past this point would clobber
    // stageContext.buildTotal and corrupt the compiling-stage counter.
    if (p.task === 'comment-sync') return;
    // Apply counts on completion too. The terminal event carries
    // `current == total` (e.g. 12/12); discarding it leaves the
    // compiling-stage label stuck at the previous tick (11/12). We
    // mutate stageContext directly because the compiling-stage label
    // reads it on the next renderPanel — no need to advanceStage on
    // completion (stage is already 'compiling').
    //
    // Advisory-only events (current=0, total=0, completed=true,
    // non-empty advisories) carry no real progress and must NOT reset
    // the stage counter. The symlink-skip advisory is the canonical case:
    // it fires after html_pages completes and would zero out the page
    // counter. Guard: skip the update when total===0 (no pages to report).
    const isAdvisoryOnly =
      p.completed === true &&
      p.current === 0 &&
      p.total === 0 &&
      Array.isArray(p.advisories) &&
      p.advisories.length > 0;
    if (
      !isAdvisoryOnly &&
      typeof p.current === "number" &&
      typeof p.total === "number"
    ) {
      this.stageContext.pages = nextPageProgress(
        this.stageContext.pages,
        p.task,
        p.current,
        p.total,
      );
      // The receipt's "N pages" comes ONLY from the html_pages task: every
      // other task counts its own set, so a later one would report
      // "3 pages" for 3 videos, not the 47 pages actually built.
      if (p.task === "html_pages") {
        this.pendingPageCount = p.total;
      }
    }
    // Soft-failure advisories: merge by task+item within a build (notebooks
    // emit incrementally; video/image emit their full set once on the terminal
    // event; both dedupe by key). Reconciled — not cleared wholesale — at
    // THIS task's own completed:true tick below: every key this
    // build re-raises is remembered here so the sweep at completion knows
    // what to keep.
    // (comment-sync is dropped at the top of this handler.)
    if (Array.isArray(p.advisories)) {
      let refreshed = this.taskRefreshedKeys.get(p.task);
      for (const adv of p.advisories) {
        if (!adv || (!adv.item && !adv.what)) continue;
        const key = `${p.task}::${adv.item ?? adv.what}`;
        this.advisories.set(key, {
          task: p.task,
          item: adv.item ?? null,
          reason: adv.what,
          action: adv.action,
          severity: adv.severity,
          scope: adv.scope,
          // An act, not a condition: a rebuild neither answers it nor
          // re-derives it, so the sweep below must not wipe it
          // the moment the author saves an unrelated file.
          sticky: isAcknowledge(adv.action),
          lifetime: adv.lifetime,
        });
        if (!refreshed) {
          refreshed = new Set();
          this.taskRefreshedKeys.set(p.task, refreshed);
        }
        refreshed.add(key);
      }
      // Only a CONSEQUENTIAL advisory re-opens an auto-closed panel.
      // `shouldSurface()` is now blocking/awaiting-only, so a freshly-added
      // `ShippedDegraded` / non-blocking advisory leaves a quiet hairline
      // dot without interrupting by opening the panel. It's always
      // ingested above (it earns the dot); the surface gate is separate.
      // Showing the panel transitions `minimized → surfaced`.
      if (this.shouldSurface()) this.surfaceOnInterrupt();
    }
    if (p.completed) {
      // Sweep the current job's build-sourced, non-sticky advisories
      // that were NOT re-raised since its last completion — the keys this
      // run's `p.advisories` never touched. Scoped to THIS task alone, at
      // THIS task's own terminal tick, never at build-start: an unrelated
      // task's tick (or a build where the current job's whole dispatch was
      // cache-skipped) must not sweep advisories the current job never got a
      // chance to re-affirm.
      for (const key of staleTaskAdvisoryKeys(
        this.advisories,
        p.task,
        this.taskRefreshedKeys.get(p.task),
      )) {
        this.deleteAdvisory(key);
      }
      this.taskRefreshedKeys.delete(p.task);
      // Clear mediaStatus when video encoding finishes.
      if (p.task === "videos") {
        this.mediaStatus = null;
      }
      const startedAt = this.bgJobStartedAt.get(p.task);
      this.bgJobStartedAt.delete(p.task);
      if (startedAt !== undefined) {
        void info(`[bg-job] end kind=${p.task} duration_ms=${Date.now() - startedAt}`);
      }
      this.removeTask(`bg:${p.task}`);
      // renderPanel runs via removeTask -> checkAutoClose path; the
      // updated counts above are picked up by the next renderStage.
      // If the task wasn't present (no VideoConversionProgress was
      // upserted), removeTask is a no-op, so explicitly re-render.
      this.renderPanel();
      return;
    }
    if (!this.bgJobStartedAt.has(p.task)) {
      this.bgJobStartedAt.set(p.task, Date.now());
      // Read back by the Rust log_report::background_job_states for Send Logs.
      void info(`[bg-job] start kind=${p.task}`);
    }
    // Names what's moving the fill — decision in progress-panel-stage.ts.
    const bgRow = bgTaskRowDecision({ task: p.task, current: p.current, total: p.total, message: p.message, isAdvisoryOnly, parentId: this.buildJobId ?? undefined });
    if (bgRow) this.upsertTask(bgRow.task, { skipArrival: bgRow.skipArrival });
    this.advanceStage({ kind: "build-progress" });
  }

  // PreviewServerFailed: backend's start_preview_server returned Err.
  // Without this listener, build.rs's eventual ProgressUpdate completes
  // with `port: None` and preview-state.ts silent-warns, leaving the
  // panel stuck at the last BackgroundProgress count (e.g. "11 / 12").
  // NEW wiring (ii): surface as lastFailure (red status) + advisory (broken
  // notice) instead of the removed preview-server-error task row. This keeps
  // the failure visible without polluting the task store with a fake task.
  // Self-clears on a subsequent successful `ready` (SCOPED: only the
  // preview-server-error advisory key and lastFailure — never blanket-clear).
  private onPreviewServerFailed(msg: string): void {
    warn(`[ProgressPanel] Preview server failed: ${msg}`);
    // Drop the compiling label FIRST (same order as the old code: setStage
    // before the failure state so it isn't immediately wiped by opening/ready).
    this.setStage("ready");
    // Then set failure state (after setStage('ready') so the opening/initial
    // advisory-clear path doesn't run — this is a self-contained advisory
    // keyed to this session's server failure).
    this.lastFailure = t('preview.server_failed', { msg });
    this.advisories.set(PREVIEW_SERVER_ADV_KEY, {
      task: "preview-server-error",
      item: null,
      // Keep the " — " delimiter: groupNotices splits the reason at the
      // first " — " into head (title) + detail (the server msg). The
      // delimiter is consumed by the split, so it is never user-visible.
      reason: `${t('preview.server_failed_title')} — ${msg}`,
      severity: "Blocking",
    });
    // Surface the panel so the user sees the red status + broken notice.
    this.surfaceOnInterrupt();
    this.renderPanel();
  }

  // Folder health: `degraded` ORs two causes the backend
  // can't tell apart — the rebuild worker's overdue-build watchdog (a
  // build past its 300s deadline, still RUNNING — "timeout =
  // diagnostics + degradation, never an escape hatch") and
  // the watcher in suppressed backoff. Either way the preview
  // may lag; a quiet amber notice, cleared on the all-clear transition.
  // The `unavailable` half of this event is `PreviewManager`'s
  // (full-window screen) — two flags, two surfaces, one payload.
  private onFolderHealthChanged(health: MossEventPayload<"FolderHealthChanged">): void {
    // A stale sweep's last tick can land just after a folder switch;
    // health for a folder this panel no longer reports is not news.
    if (this.healthFolder && health.folder !== this.healthFolder) return;
    if (health.degraded) {
      this.advisories.set(BUILD_OVERDUE_ADV_KEY, {
        task: "build-overdue",
        item: null,
        // The " — " is groupNotices's head/detail delimiter, consumed by
        // the split — same discipline as the cloud notices above.
        reason: buildOverdueAdvisoryReason(),
        severity: "NeedsAction",
      });
    } else if (!this.deleteAdvisory(BUILD_OVERDUE_ADV_KEY)) {
      return; // already absent: a healthy transition we never showed
    }
    this.renderPanel();
  }

  // scan-progress: throttled (~10 Hz) updates while scan_folder_with_dedup
  // walks the project directory. Drives the "Scanning files…" stage.
  private onScanProgress(payload: { found: number; evicted: number }): void {
    if (!payload) return;
    this.stageContext.found = payload.found;
    this.stageContext.evicted = payload.evicted;
    this.advanceStage({ kind: "scan-progress" });
  }

  // scan-complete: fires once at end of walk. Decides whether the next
  // stage is "materializing" (evicted files to download) or "compiling".
  private onScanComplete(p: { total_files: number; evicted_count: number; cloud_provider: string | null }): void {
    if (!p) return;
    this.stageContext.found = p.total_files;
    this.stageContext.evicted = p.evicted_count;
    if (p.cloud_provider) {
      this.stageContext.provider = p.cloud_provider;
      this.stageContext.total = p.evicted_count;
      this.stageContext.downloaded = 0;
    }
    this.advanceStage({
      kind: "scan-complete",
      evictedCount: p.evicted_count,
    });
  }

  // VideoConversionProgress (typed): FFmpeg's callback — the SECOND
  // writer of `bg:videos` (BackgroundProgress above creates it first, quietly).
  private onVideoConversionProgress(vp: VideoConversionProgress): void {
    // Stash for status line (buildViewModel).
    this.mediaStatus = {
      verb: t("verb.optimizing"),
      subject: vp.filename,
      current: vp.current_video,
      total: vp.total_videos,
    };
    const phaseLabel =
      vp.phase === "analyzing" ? t("progress.video_analyzing") : t("progress.video_encoding");
    this.upsertTask({
      id: "bg:videos",
      label: t("progress.bg_video_converting_named", { filename: vp.filename, current: vp.current_video, total: vp.total_videos }),
      detail: phaseLabel,
      progress: Math.round(vp.overall_fraction * 100),
      source: "moss",
      ambient: true,
      parentId: this.buildJobId ?? undefined,
    });
  }

  // BuildComplete (typed): the terminal build receipt, and where a build's
  // two facts are joined — it carries the measured span (`total_time_ms`)
  // but NOT the page count, so the count `html_pages` parked in
  // `pendingPageCount` is consumed here and both are stamped onto
  // `lastBuild` together so the receipt is consistent. A toast in preview-manager.ts
  // consumes the same event independently on the one `moss-event` channel.
  private onBuildComplete(totalTimeMs: number): void {
    this.lastBuild = { elapsedMs: totalTimeMs, pageCount: this.pendingPageCount };
    this.pendingPageCount = null;
    this.showCompletionReceipt();
  }

  // icloud-sync: cloud provider download progress for evicted files.
  // Named `icloud-sync` for backwards compatibility, but covers every
  // macOS File Provider extension (iCloud, Dropbox, Google Drive,
  // OneDrive, Box, ...). `cloud-sync-store.ts` is now the single owner of
  // this event — it does the folder filtering and keeps provider/
  // downloaded/total/remaining/stalled so this panel and the waiting
  // screen read the same facts. This panel keeps only the
  // PRESENTATION decisions: what the stage bar shows, and when to
  // surface itself.
  //
  // Feeds the materializing-stage primary bar — does NOT add a separate
  // task row, because the primary bar already shows the same data.
  private onCloudSync(change: CloudSyncChange): void {
    const { state } = change;

    // Nothing left in the cloud: the episode is over, whatever phase said
    // so and whatever stage the build is in. Drop the readout — a download
    // that has finished must stop being reported — and let ordinary
    // build-progress events advance us.
    //
    // Deliberately NOT `phase === "home_ready" && remaining === 0`. Under a
    // gate that can still be up while the build runs, a finished
    // download reported through `home_waiting` would leave the readout
    // running with nothing left to report — the same dishonesty, wearing
    // the fix's clothes. `remaining` is the authority; see `cloudOutstanding`.
    // Concrete read errors are a separate supervisor fact from pending
    // downloads. Set the notice before the zero-remaining branch too.
    if (state.unavailable_count === null || state.unavailable_count > 0) {
      this.advisories.set(CLOUD_UNAVAILABLE_ADV_KEY, {
        task: "cloud-sync-unavailable",
        item: null,
        reason: unavailableAdvisoryReason(state.unavailable_count, state.unavailable),
        severity: "NeedsAction",
      });
    } else {
      this.deleteAdvisory(CLOUD_UNAVAILABLE_ADV_KEY);
    }
    // The detail request follows the same accepted snapshot. Its render
    // now sees the new advisory rather than the previous tick's wording.
    this.cloudFileDetails.onSnapshot(this.renderer.isCloudFilesOpen());

    // The waiting screen owns the window; retain the latest facts but
    // leave panel presentation to the falling edge.
    if (this.screenUp) return;

    if (state.remaining === 0) {
      this.clearCloudReadout();
      this.renderPanel();
      return;
    }

    // Otherwise the site is up but files are still arriving. That is worth
    // showing: on a large synced vault the preview goes live long before
    // every image has landed, and silence there reads as "images broken"
    // rather than "images still downloading".

    this.stageContext.provider =
      state.provider?.label || this.stageContext.provider || "cloud";
    this.stageContext.downloaded = state.downloaded;
    this.stageContext.total = state.total;
    // `remaining` is what makes the readout outlive the stage — see
    // `cloudOutstanding`. Without it, downloads would be reported only while
    // the stage is `materializing`.
    this.stageContext.remaining = state.remaining;
    // Direct mutation (vs. advanceStage) is intentional: there is no
    // matching `StageEvent` kind for icloud-sync. The condition here
    // mirrors the monotonic invariant baked into `nextStage` — only
    // promote from a strictly-earlier phase. A late icloud-sync
    // arriving after compile/ready is dropped on the floor.
    let enteredMaterializing = false;
    if (
      this.currentStage === "scanning" ||
      this.currentStage === "opening"
    ) {
      this.currentStage = "materializing";
      enteredMaterializing = true;
    }
    // Routine work remains quiet: an advisory-free,
    // publish-idle rebuild — even one that downloads evicted cloud files —
    // must NOT grow the panel. The hairline + green pill-flash own that
    // moment. Un-hide only when this rebuild is consequential (advisory
    // present OR publish engaged); otherwise leave the panel as-is.
    //
    // AND only on an EDGE — the same rule the publish path states one
    // handler below ("gated to the edge (wasIdle) so an in-flight publish's
    // 4 Hz progress ticks do NOT re-yank a panel the user has minimized").
    // This stream is a 0.5 Hz *state* re-assert, so without an edge every
    // tick re-opened a collapsed panel for as long as the download stayed
    // stuck, which is also what made the notice's detail unreadable. The
    // two edges that are genuinely news: the stall beginning, and the
    // episode beginning. Both are now recovered ONCE in the store
    // (`change.stalledEdge`) instead of re-derived here.
    if ((change.stalledEdge || enteredMaterializing) && this.shouldSurface()) {
      this.surfaceOnInterrupt();
    }
    this.renderPanel();
  }

  // deploy-progress: push_site stages. Ordered sequence:
  //   preparing -> rebuilding -> syncing -> uploading -> committing ->
  //   finalizing -> complete (or failed at any point)
  // `preparing` and `rebuilding` fire during the silent pre-deploy
  // phases (wait-for-background-tasks, optional rebuild); `finalizing`
  // covers post-commit bookkeeping (subscribers, analytics, domain
  // orchestrator). See crates/moss-build/src/deploy/progress.rs.
  //
  // Drives the primary bar via the publish stage machine — no
  // separate `deploy:push` task row.
  //
  // The payload is the TYPED `DeployProgress` binding
  // (specta-generated from `deploy::progress::DeployProgress`), not an
  // inline `listen<{…}>` literal. The `nextPublishStage` call below and its
  // monotonicity logic are UNCHANGED — only the listener's input type is
  // rewired (risk (a): the bar-rewind guard in progress-panel-stage.ts stays
  // byte-identical). `deploy-progress` remains the channel; it is the stage
  // stream, dual-emitted alongside the Deploy Job receipt.
  private onDeployProgress(p: DeployProgress): void {
    if (!p) return;

    if (p.stage === "complete") {
      this.advancePublishStage({ kind: "complete" });
      return;
    }
    if (p.stage === "failed") {
      // Thread the REAL error through: deploy.rs passes the failure as
      // `message`. Dropping it here is what orphaned deploy failures off the
      // advisory system (bare "Publish failed", no dot/seam/detail).
      this.advancePublishStage({ kind: "failed" }, p.message);
      return;
    }
    if (p.stage === "verifying") {
      // Publishing is verified live asynchronously: Complete precedes
      // Verifying on this channel, so this is a RECEIPT CORRECTION, not a
      // stage advance — swap "Published · …" for the honest rung-1
      // sentence on the normal dwell. No machine re-engagement: a resident
      // stage would pin shouldSurface() true for the whole verification
      // window. From 10s the `publish-verify` advisory is the carrier.
      this.showPublishReceipt(t('publish_verify.uploaded_checking'));
      this.renderPanel();
      return;
    }

    if (p.stage === "uploading") {
      // syncing→uploading EDGE: a fresh pass owns its own window, so the
      // sticky rate and the milestone set from a previous deploy cannot
      // leak into this one.
      if (this.currentPublishStage !== "uploading") this.publish.resetUpload();
      this.publish.noteUploading(p, performance.now());
    }
    if (p.stage === "rebuilding") {
      this.publish.noteRebuilding(p.message);
    }
    this.advancePublishStage({ kind: "progress", stage: p.stage });
  }

  // PUSH_NOTICE: non-Tauri producers (e.g. deploy-handler)
  // push a degraded notice into the advisory zone via the event bus instead
  // of a toast. Keyed by `task` — a second emit with the same task replaces
  // the first. Cleared at next build-start like other advisories.
  private onPushNotice(entry: PushNoticeEvent): void {
    this.advisories.set(entry.task, {
      task: entry.task,
      item: entry.item,
      reason: entry.reason,
      severity: entry.severity,
      scope: entry.scope,
      action: entry.action as Action | undefined,
      sticky: entry.sticky,
    });
    // Same interrupt discipline as the BackgroundProgress advisory path
    // above: a fresh Blocking row (e.g. a Publish click a pre-publish
    // gate just refused) must reach the user even from a closed panel.
    // A refused gate re-clears its own rows via CLEAR_NOTICE before
    // re-posting (`pre-publish-gates.ts`), dropping any dismissal, so a
    // repeat refusal surfaces regardless.
    if (this.shouldSurface()) this.surfaceOnInterrupt();
    this.renderPanel();
  }

  // CLEAR_NOTICE clears only the matching scope —
  // the only path that removes a sticky notice short of destroy().
  // checkAutoClose lets a settled panel retract on a cleared verdict.
  private onClearNotice({ task }: ClearNoticeEvent): void {
    // Two key spaces meet here: PUSH_NOTICE keys by `task` alone, while
    // a build advisory is keyed `task::item` and GROUPED into a row keyed
    // `task::reason-head` — the only key a dismiss button can know. Try
    // the direct key, then clear every entry that group covers.
    let hit = this.deleteAdvisory(task);
    for (const [key, entry] of this.advisories) {
      if (noticeGroupKey(entry) !== task) continue;
      this.deleteAdvisory(key);
      hit = true;
    }
    if (!hit) return;
    this.renderPanel();
    this.checkAutoClose();
  }


  upsertTask(task: ProgressTask, opts?: { skipArrival?: boolean }): void {
    // Stamp a monotonic update time for tiebreak in buildViewModel.
    this.tasks.set(task.id, { ...task, lastUpdatedAt: Date.now() });
    // Auto-show when a CONSEQUENTIAL task ARRIVES (deploy progress, dns:check,
    // a preview-server error) — those must reach the user even from a hidden,
    // settled panel, so a blanket gate here would bury a real error. A routine
    // AMBIENT sub-job (`bg:videos`, …) un-hides only when the build is already
    // consequential (`shouldSurface()`); otherwise it lives and dies at the
    // hairline without interrupting. Showing the panel is a
    // `minimized → surfaced` transition, no-op when already shown; an INTERRUPT
    // additionally cancels a pending peek close-timer and promotes peek →
    // surfaced, while a non-ambient non-blocking task (dns:check) only
    // surfaces from minimized and won't yank a peek the user is reading.
    //
    // Only arrival can open the panel: a publish pins
    // `shouldSurface()` true throughout, so plugin status messages used to
    // re-open a panel the user just minimized, as the publish and icloud
    // handlers already refuse to. Escalations are the caller's: `failed`
    // deletes the row and routes to advisories, `awaiting` has its own edge.
    //
    // `arrivedTasks` (not map presence) tracks it, so opts.skipArrival lets
    // a quieter producer create the row without spending this check.
    if (!opts?.skipArrival) {
      const isArrival = !this.arrivedTasks.has(task.id);
      this.arrivedTasks.add(task.id);
      if (isArrival) {
        if (this.shouldSurface()) {
          this.surfaceOnInterrupt();
        } else if (!task.ambient) {
          this.show();
        }
      }
    }

    // Reset stale timer — auto-remove if no further updates
    // Deploy tasks get a longer timeout (bulk uploads can take minutes)
    const existingTimer = this.staleTimers.get(task.id);
    if (existingTimer) clearTimeout(existingTimer);
    // Publish work is quiet, not dead: the Rust side emits one event per
    // transition, so the window must cover the longest *silent* step — a
    // rebuild, an upload over a slow network, a commit. `deploy:` is the Rust
    // task, `plugin:<name>:<hook>` the plugin hook running the same waits.
    const publish =
      task.id.startsWith("deploy:") ||
      (task.id.startsWith("plugin:") &&
        ProgressPanel.PUBLISH_HOOKS.has(task.id.slice(task.id.lastIndexOf(":") + 1)));
    const timeout = publish ? 600_000 : ProgressPanel.STALE_TIMEOUT_MS;
    this.staleTimers.set(
      task.id,
      setTimeout(() => this.removeTask(task.id), timeout),
    );

    this.renderPanel();
  }

  /**
   * Ingest a unified Job wire. The glass panel renders **Preview-scope**
   * Jobs as panel rows; other scopes belong to the titlebar/inline renderers.
   * Maps the Job state to the panel's task vocab:
   *   Running   -> a task row (label = title or message, progress = fraction)
   *   Awaiting  -> interrupt with a directive and the typed recovery action
   *   Succeeded -> a brief `verb · receipt` row, then auto-recede (minimize != forget)
   *   Failed    -> an error row (carrying the root-cause advisory's action, if any)
   *   Cancelled -> removed
   */
  handleJobUpdate(wire: PanelTaskWire): void {
    // Narrated/awaiting tones from ANY scope are folded
    // into the pill popover. Route them before the preview-scope guard so they
    // are never silently dropped.
    if (wire.scope !== "preview") {
      // TONE IS NOT STABLE ACROSS A TASK'S LIFE. The Matters import calls
      // `task.awaiting("Connect to Matters")` then IMMEDIATELY `task.progress(...)`
      // to drop the amber Awaiting pulse for a quiet login label — flipping the
      // SAME id OUT of the Awaiting tone. Where it lands depends on the route:
      // Ambient under the onboarding/empty-folder import (`Workspace, Process`) but
      // back to Narrated under the Settings re-sync (`Workspace, Import` rests
      // Narrated — tasks.rs Job::tone). Either way the dedicated
      // `handleAwaitingNarrationUpdate` only runs while the wire still carries the
      // awaiting tone, so on ANY non-awaiting follow-up we must drive its teardown
      // HERE — or the synthetic advisory + row + `awaitingJobs` entry it created
      // leaks forever. That leak was the root cause of the stuck "Waiting for you
      // to Connect to Matters." advisory: it both kept the amber dot AND pinned the
      // panel open (a live advisory/row suppresses `checkAutoClose`, and the
      // `awaitingJobs` entry keeps `shouldSurface()` true). Re-entering the handler
      // with a non-awaiting `state` hits its clear path (removes all three).
      if (
        wire.tone !== "awaiting" &&
        this.awaitingJobs.has(`awaiting-narration:${wire.id}`)
      ) {
        this.handleAwaitingNarrationUpdate(wire);
        // The last interrupt for this id just resolved — let a settled panel retract.
        this.checkAutoClose();
      }
      if (wire.tone === "narrated") {
        this.handleNarratedUpdate(wire);
        return;
      }
      if (wire.tone === "awaiting") {
        this.handleAwaitingNarrationUpdate(wire);
        return;
      }
      // Tone escalation: if a task we were tracking as narrated now arrives with
      // a different tone and non-preview scope, drop it from the narrated store.
      const narratedKey = `narrated:${wire.id}`;
      if (this.narratedTasks.has(narratedKey)) {
        this.dropNarratedTask(narratedKey);
      }
      return;
    }
    const id = `job:${wire.id}`;
    const state = wire.state;
    // A sub-job (media under Build) carries a parent — those are
    // routine, so they must NOT auto-show the panel (the phantom-job
    // flood). A top-level Preview Job (Domain, Deploy) has no parent and is
    // consequential-by-intent → it shows. This is the auto-show discriminator,
    // independent of `tone` (the domain Job is Ambient-toned only so the
    // titlebar/inline renderers ignore it — that must NOT suppress its panel).
    const ambient = wire.parent != null;
    const parentId = wire.parent != null ? `job:${wire.parent}` : undefined;

    // The top-level Build Job is the receipt HEADER, not a
    // task row. It anchors the nesting of every media child + the fine-grained
    // VideoConversionProgress detail, and its terminal drives the
    // `Built · N pages · X.Xs` header. Route it here so it never paints a row.
    if (wire.kind === "build" && wire.parent == null) {
      this.handleBuildJobUpdate(id, wire);
      return;
    }

    switch (state.state) {
      case "running": {
        this.awaitingJobs.delete(id);
        // The primary publish bar (driven by the kept `deploy-progress` stage
        // stream) OWNS the live progress of a publish. When it is engaged
        // (`currentPublishStage !== "idle"`), a Preview Job (the Deploy Job)
        // must NOT also render a live "Publishing" row — that double-renders the
        // same operation. While engaged, the Job contributes only its TERMINAL
        // receipt (the `succeeded` case) and its `awaiting` interrupt; its live
        // running state is suppressed (and any spawn-time row removed). When idle
        // — the standalone Domain-connect flow has no bar — the running row
        // renders as before, since the Job is then the sole live surface.
        if (this.currentPublishStage !== "idle") {
          this.removeTask(id);
          break;
        }
        this.upsertTask({
          id,
          label: wire.title || state.message || t('progress.working'),
          progress:
            state.fraction != null ? Math.round(state.fraction * 100) : null,
          source: "moss",
          ambient,
          parentId,
        });
        break;
      }
      case "awaiting": {
        // The EDGE into awaiting, like every other surface here: a repeated
        // question must not re-yank a panel minimized in between. Asking AGAIN
        // after resuming does surface — a second question is a second interruption.
        const newlyAwaiting = !this.awaitingJobs.has(id);
        this.awaitingJobs.add(id);
        this.upsertTask({
          id,
          label: state.directive,
          progress: null,
          source: "moss",
          action: state.action,
          parentId,
        });
        // The interrupt persists even when minimized — it is a one-
        // shot emit that never refreshes, so clear the stale-removal timer
        // `upsertTask` armed or it would silently vanish (~15s) before the user
        // can act. It clears only on a later state for this Job (terminal/resume).
        this.clearStaleTimer(id);
        // `upsertTask` surfaces on ARRIVAL — right for a tick, wrong here: a
        // job already running has a row, so the one update that is a QUESTION
        // would otherwise land unseen in a minimized panel. A question wins
        // regardless of tone: an ambient job that stops to ask is no longer
        // ambient.
        if (newlyAwaiting) this.surfaceOnInterrupt();
        break;
      }
      case "succeeded": {
        this.awaitingJobs.delete(id);
        // Keep routine success quiet: an AMBIENT media sub-job (a child
        // under the Build parent) that succeeded with NO advisory leaves no row —
        // the content sharpening in place is the feedback. Its advisories (if any)
        // flow through the separate BackgroundProgress.advisories path (the
        // hairline dot / banner), so silence here never swallows a real snag.
        // Any prior live row for this child (the in-progress detail) is removed.
        if (ambient && (state.advisories?.length ?? 0) === 0) {
          this.removeTask(id);
          break;
        }
        // Deploy/publish Jobs dissolve WITHOUT a receipt row dwell.
        // The persistent shelf record (showAck "Site published") is the durable
        // surface; a transient panel row would duplicate it and leave residue in
        // the pill. Clear any prior live row (e.g. from the running state) and
        // return — no upsert, no staleTimers entry.
        if (wire.kind === "deploy" || wire.kind === "publish") {
          this.clearStaleTimer(id);
          this.removeTask(id);
          break;
        }
        // Non-ambient sibling of "success makes no sound": a real Amount of
        // 0 (e.g. 0 posts) with no advisory gets no row either — a 0-count
        // WITH an advisory keeps its row (the advisory's home).
        if (wire.amount != null && wire.amount.count === 0 && (state.advisories?.length ?? 0) === 0) {
          this.clearStaleTimer(id);
          this.removeTask(id);
          break;
        }
        const verb = localizeVerb(wire.verb);
        // verb · amount · elapsed. Amount is producer-supplied
        // The Deploy Job carries `{142, "files"}` → "Published · 142
        // files · 2.1s"); the measured span is on the wire as `elapsed_ms`
        // (frozen at terminal), formatted HERE (Hickey: the renderer formats,
        // the data carries a span/quantity). A Job with no amount (Domain
        // "Connected") simply omits that segment.
        const amount = formatAmount(wire.amount);
        const elapsed = formatElapsed(wire.elapsed_ms);
        const receipt =
          [verb, amount, state.receipt ?? undefined, elapsed]
            .filter((p) => p && p.length > 0)
            .join(" · ") || t('status.done');
        this.upsertTask({ id, label: receipt, progress: 100, source: "moss", parentId });
        // After a brief dwell, the receipt recedes back to the pill.
        // Tracked in `staleTimers` so `destroy()` cancels it.
        this.clearStaleTimer(id);
        this.staleTimers.set(
          id,
          setTimeout(() => this.removeTask(id), 4000),
        );
        break;
      }
      case "failed": {
        this.awaitingJobs.delete(id);
        // NEW wiring (i): store the Job's failure as an advisory so groupNotices
        // surfaces a broken notice (Blocking → broken tier). Previously the panel
        // only upserted a task row; the notice rail was blind to Job failures.
        this.advisories.set(id, {
          task: "job",
          item: null,
          reason: state.advisory.what,
          action: state.advisory.action,
          severity: state.advisory.severity,
          scope: state.advisory.scope,
        });
        // Set lastFailure for a publish/deploy Job so the
        // red status line persists and checkAutoClose is suppressed.
        if (wire.kind === "deploy" || wire.kind === "publish") {
          this.lastFailure = state.advisory.what || t('preview.job_failed');
        }
        // Remove the old task row (if any); the notice rail owns failure surfaces.
        this.clearStaleTimer(id);
        this.tasks.delete(id);
        this.renderPanel();
        break;
      }
      case "cancelled": {
        this.awaitingJobs.delete(id);
        this.removeTask(id);
        break;
      }
    }
  }

  /**
   * Ingest the top-level Build parent Job. The Build Job is the
   * receipt HEADER, never a task row:
   *   • `running` → remember its id (`buildJobId`) so media children + the
   *     fine-grained `VideoConversionProgress` detail nest under it. No row, no
   *     surface (the "Building…" header + the Sync hairline own the in-progress
   *     moment — success makes no sound).
   *   • `succeeded` → compose `Built · N pages · X.Xs` from the parent Job's
   *     verb·amount·elapsed and drive it as the header (reconciled with the
   *     legacy `BuildComplete` listener via `buildReceipt` — one header slot, so
   *     no double receipt). Then forget the id (the build is over).
   *   • `failed`/`cancelled` → forget the id; a build error surfaces via its
   *     advisory, not a Build-Job row.
   */
  private handleBuildJobUpdate(id: string, wire: PanelTaskWire): void {
    const state = wire.state;
    switch (state.state) {
      case "running":
        this.buildJobId = id;
        break;
      case "succeeded": {
        const verb = localizeVerb(wire.verb) || t("verb.built");
        const amount = formatAmount(wire.amount);
        const elapsed = formatElapsed(wire.elapsed_ms);
        const receipt =
          [verb, amount, state.receipt ?? undefined, elapsed]
            .filter((p) => p && p.length > 0)
            .join(" · ") || t('verb.built');
        // Stash the parent Job's receipt (see `buildReceipt`) and seed
        // `lastBuild` so the dwell/retract machinery (R11) applies unchanged.
        this.buildReceipt = receipt;
        this.lastBuild = { elapsedMs: wire.elapsed_ms, pageCount: this.pendingPageCount };
        this.pendingPageCount = null;
        this.buildJobId = null;
        this.showCompletionReceipt();
        break;
      }
      case "failed":
      case "cancelled":
        this.buildJobId = null;
        break;
    }
  }

  // ── Narrated/Awaiting handlers (non-preview) ──────

  /**
   * Ingest a non-preview `tone==="narrated"` PanelTaskUpdate into the
   * narration store.  The narrated task is surfaced in the pill popover via
   * `getNarratedTasks()` (consumed by the renderer via `renderPanel`).
   *
   * A `failed` narrated task PERSISTS until explicitly cancelled —
   * it receives no fade timer. `succeeded`/`cancelled` tasks get a short
   * dwell then are removed.  Tone escalation (a task's tone changes away from
   * "narrated") drops the task immediately.
   */
  private handleNarratedUpdate(wire: PanelTaskWire): void {
    const key = `narrated:${wire.id}`;

    // Tone escalation: the wire now has a different tone but was tracked here —
    // shouldn't happen on this path (caller already checked tone === "narrated"),
    // but guard defensively.
    if (wire.tone !== "narrated") {
      this.dropNarratedTask(key);
      return;
    }

    const state = wire.state;

    if (state.state === "cancelled") {
      this.dropNarratedTask(key);
      return;
    }

    const existing = this.narratedTasks.get(key);
    const startedAtMs = existing
      ? existing.startedAtMs
      : wire.started_at_ms;

    const task: NarratedTask = {
      id: key,
      title: wire.title,
      startedAtMs,
      state,
    };
    this.narratedTasks.set(key, task);

    // Failed tasks must NOT be auto-removed (clear any stale timer from a
    // previous succeeded state on the same id, just in case).
    if (state.state === "failed") {
      const existing_timer = this.narratedFadeTimers.get(key);
      if (existing_timer) {
        clearTimeout(existing_timer);
        this.narratedFadeTimers.delete(key);
      }
    } else if (state.state === "succeeded") {
      // Cancel any pre-existing timer (idempotent) then re-arm.
      const existing_timer = this.narratedFadeTimers.get(key);
      if (existing_timer) clearTimeout(existing_timer);
      const t = setTimeout(() => {
        this.narratedFadeTimers.delete(key);
        this.narratedTasks.delete(key);
        this.renderPanel();
      }, ProgressPanel.NARRATION_FADE_MS);
      this.narratedFadeTimers.set(key, t);
    } else {
      // Running/awaiting: cancel any pending fade (task is active again).
      const existing_timer = this.narratedFadeTimers.get(key);
      if (existing_timer) {
        clearTimeout(existing_timer);
        this.narratedFadeTimers.delete(key);
      }
    }

    this.renderPanel();
  }

  /** Remove a narrated task (and its fade timer) immediately. */
  private dropNarratedTask(key: string): void {
    const t = this.narratedFadeTimers.get(key);
    if (t) { clearTimeout(t); this.narratedFadeTimers.delete(key); }
    this.narratedTasks.delete(key);
    this.renderPanel();
  }

  /**
   * Ingest a non-preview `tone==="awaiting"` PanelTaskUpdate. The awaiting
   * narration drives the amber "needs-you" seam dot (via a synthetic advisory
   * entry keyed to the task) and surfaces as a popover task row carrying the
   * escape action.
   *
   * Terminal states (succeeded/failed/cancelled) clear the task and remove
   * the amber dot.
   */
  private handleAwaitingNarrationUpdate(wire: PanelTaskWire): void {
    const key = `awaiting-narration:${wire.id}`;
    const state = wire.state;

    // Terminal → clear everything.
    if (
      state.state === "succeeded" ||
      state.state === "failed" ||
      state.state === "cancelled"
    ) {
      this.deleteAdvisory(key);
      // Also clear any existing task row for this awaiting task from the primary
      // tasks map (it may have been upserted as an awaiting row below).
      this.tasks.delete(key);
      this.awaitingJobs.delete(key);
      this.renderPanel();
      return;
    }

    if (state.state !== "awaiting") {
      // Running/resumed: clear the amber dot (awaiting is over).
      this.deleteAdvisory(key);
      this.tasks.delete(key);
      this.awaitingJobs.delete(key);
      this.renderPanel();
      return;
    }

    // NOTE: the matters publish room's "waiting for you" surface is the
    // moss-owned `.moss-matters-bar` title bar, NOT this
    // awaiting-narration row. The tone guard upstream forces a `Syndicate` job in
    // the Awaiting state to `TaskTone::Inline`, so its wire never reaches this
    // handler.

    // Active awaiting state — light the amber dot via a synthetic advisory.
    // We reuse the advisory path so the seam dot fires through the existing
    // `topSeverityCallbacks` without duplicating that infrastructure.
    this.advisories.set(key, {
      task: "awaiting-narration",
      item: null,
      reason: formatAwaitingCopy(state),
      action: state.action,
      // NeedsAction drives the amber dot. It does NOT satisfy shouldSurface()
      // (which requires Blocking or an awaitingJob) — so this stays a quiet
      // hairline dot and doesn't auto-pop the panel.
      severity: "NeedsAction",
    });

    // Also surface as a task row carrying the directive and escape action,
    // so the popover shows "Waiting for you to … [Cancel]".
    this.awaitingJobs.add(key);
    this.upsertTask({
      id: key,
      label: formatAwaitingCopy(state),
      progress: null,
      source: "moss",
      action: state.action,
    });
    // The interrupt persists — clear the stale-removal timer upsertTask armed.
    this.clearStaleTimer(key);
  }

  /**
   * Return the current narrated (non-preview) tasks as `ProgressTask[]` so the
   * renderer and tests can access them without knowledge of the internal
   * `NarratedTask` shape.
   *
   * All narrated tasks are returned (the popover shows all of them). Terminal
   * tasks remain visible so failed tasks stay in the list.
   */
  getNarratedTasks(): ProgressTask[] {
    if (this.narratedTasks.size === 0) return [];
    return Array.from(this.narratedTasks.values()).map((t) => ({
      id: t.id,
      label: formatNarration(t),
      progress: null,
      source: "moss" as const,
    }));
  }

  /** Handle plugin progress event: upsert on updates, remove on completion */
  handlePluginProgress(pp: PluginProgressEvent): void {
    // The `process` hook (e.g. the Matters import) is Workspace-scoped
    // background work that surfaces via showToast, not the preview progress
    // panel — keep it out of here. Deploy / syndicate and the setup hooks
    // still render here.
    if (pp.hook_name === "process") return;

    const taskId = `plugin:${pp.plugin_name}:${pp.hook_name}`;

    if (pp.completed) {
      // If the row was scheduled but never painted (the hook finished inside
      // the grace window — the common incremental-rebuild case), cancel the
      // pending upsert so nothing ever flashes. removeTask is a safe no-op
      // when the row was never created.
      const pending = this.deferredUpserts.get(taskId);
      if (pending) {
        clearTimeout(pending.timer);
        this.deferredUpserts.delete(taskId);
      }
      this.removeTask(taskId);
      return;
    }

    const task: ProgressTask = {
      id: taskId,
      label: pp.plugin_name,
      detail: pp.message || pp.phase,
      progress: pp.total > 0 ? Math.round((pp.current / pp.total) * 100) : null,
      source: "plugin",
      pluginName: pp.plugin_name,
    };

    // Publish / distribution hooks (deploy, syndicate) are user-triggered and
    // genuinely slow — paint immediately, there is no flash to suppress.
    if (ProgressPanel.PUBLISH_HOOKS.has(pp.hook_name)) {
      this.upsertTask(task);
      return;
    }

    // Every other hook (configure_domain, check_setup, login) is bracketed by
    // start/done around a step that is usually near-instant. Defer the first
    // row so a sub-window hook cancels (in the `completed` branch above)
    // before painting; a genuinely slow one surfaces once the grace period
    // elapses.
    // Once the row IS live, later non-completion updates flow straight through
    // so progress keeps ticking without re-deferring.
    if (this.tasks.has(taskId)) {
      this.upsertTask(task);
      return;
    }
    const existing = this.deferredUpserts.get(taskId);
    if (existing) {
      // Refresh the captured task with the latest counts/detail but keep the
      // original timer — re-arming on every update would push a steadily
      // self-updating hook past 250ms forever and it would never paint. The
      // timer reads the latest task from the map when it fires.
      existing.task = task;
      return;
    }
    const timer = setTimeout(() => {
      const pending = this.deferredUpserts.get(taskId);
      this.deferredUpserts.delete(taskId);
      if (pending) this.upsertTask(pending.task);
    }, ProgressPanel.DEFER_UPSERT_MS);
    this.deferredUpserts.set(taskId, { timer, task });
  }

  /** Clear (and forget) any stale-removal timer armed for `taskId`. */
  private clearStaleTimer(taskId: string): void {
    const timer = this.staleTimers.get(taskId);
    if (timer) clearTimeout(timer);
    this.staleTimers.delete(taskId);
  }

  removeTask(taskId: string): void {
    if (!this.tasks.has(taskId)) return;

    this.clearStaleTimer(taskId);
    // A removal by ANY path (timer, cancel, completion) must drop the
    // Job from `awaitingJobs` — otherwise a stale id pins `shouldSurface()` true
    // and the panel re-pops on the next unrelated rebuild.
    this.awaitingJobs.delete(taskId);
    this.arrivedTasks.delete(taskId); // a restarted task is a fresh arrival again

    this.tasks.delete(taskId);
    this.renderPanel();

    if (this.tasks.size === 0) {
      this.fireAllComplete();
    }

    this.checkAutoClose();
  }

  failTask(taskId: string): void {
    if (!this.tasks.has(taskId)) return;

    this.clearStaleTimer(taskId);
    this.awaitingJobs.delete(taskId);

    this.tasks.delete(taskId);

    for (const cb of this.taskFailedCallbacks) {
      cb(taskId);
    }

    this.renderPanel();

    if (this.tasks.size === 0) {
      this.fireAllComplete();
    }

    this.checkAutoClose();
  }

  /**
   * Derive whether the panel body is hidden from the reveal state. Read-only — it is
   * the enum's projection (`minimized`). The MANY `if (this._isHidden) …` read
   * sites keep working unchanged; every WRITE goes through `setRevealState()`.
   */
  private get _isHidden(): boolean {
    return this.revealState === "minimized";
  }

  /**
   * The ONE writer of `revealState` (single owner). Re-renders only on an actual
   * transition so a no-op call (e.g. `surface()` when already shown) doesn't
   * thrash the DOM. Keeps the seam's advisory dot in lockstep via renderPanel().
   */
  private setRevealState(next: RevealState): void {
    if (this.revealState === next) return;
    this.revealState = next;
    this.renderPanel();
  }

  /**
   * Minimizing sets `minimized` and retracts the panel body to the seam; the
   * persistent seam + advisory dot REMAIN (they're titlebar-layer siblings, not
   * panel children). Wired to the ⌄ control. Advisories are PRESERVED — minimize
   * keeps the job running; they clear only at build-start. The ⌄ is
   * also the ONLY way to close a `pinned` panel.
   */
  hide(): void {
    this.cancelPeekTimers();
    this.setRevealState("minimized");
  }

  /**
   * Explicit user-close — the ▾/▴ handle and Esc
   * route here, never straight to `hide()` (every other `minimized` path —
   * `checkAutoClose`, the peek close-timer, `setScreenUp` — is the panel
   * getting out of the way, not the user acknowledging anything). Records
   * every Blocking key showing right now in `dismissedAdvisoryKeys` (keyed
   * by advisory KEY, so a count-only change stays dismissed), THEN
   * collapses — so the next routine rebuild's `shouldSurface()` doesn't
   * reopen for the same problem. `deleteAdvisory` drops the acknowledgement
   * once a key leaves `advisories`, so a fresh key or this one returning
   * later still surfaces.
   */
  dismiss(): void {
    for (const [key, entry] of this.advisories) {
      if (entry.severity === "Blocking") this.dismissedAdvisoryKeys.add(key);
    }
    this.hide();
  }

  /**
   * Pushed by `PreviewManager.applyUnshowableReason` on every ladder
   * reconcile. On the rising edge (false → true) this panel closes itself —
   * it anchors above `--z-toast`, higher than the waiting screen can go
   * without outranking real notifications, so one already open would sit on
   * top of it. The falling edge is silent: whatever the cloudSync
   * subscription would have rendered while quiet is stale and gets
   * refreshed by the next tick, same as before this refactor.
   */
  setScreenUp(up: boolean): void {
    if (up && !this.screenUp) this.hide();
    this.screenUp = up;
  }

  /**
   * Tell the caller whenever `buildStageKnownLabel()` changes its answer.
   *
   * This panel is silent for the whole time a full-window screen is up
   * (`setScreenUp`), which is right — but it is also the only thing that
   * KNOWS what stage the build is in, and the screen's stage line is read
   * from here. Nothing else repaints that screen: a build that finishes
   * without changing the preview's status emits no new `PreviewStatus`, so
   * `renderStatus` never runs and the line stays frozen at whatever was known
   * when the screen went up — which, for a page created a moment ago, is
   * nothing at all. So the panel says when the sentence changed and the
   * ladder decides what to do about it.
   */
  setOnStageLabelChanged(cb: (() => void) | null): void {
    this.onStageLabelChanged = cb;
    this.lastStageLabel = this.buildStageKnownLabel();
  }

  private onStageLabelChanged: (() => void) | null = null;
  private lastStageLabel: string | null = null;

  private notifyStageLabelChanged(): void {
    if (!this.onStageLabelChanged) return;
    const label = this.buildStageKnownLabel();
    if (label === this.lastStageLabel) return;
    // Recorded BEFORE the callback: the listener reconciles the ladder, which
    // can call back into this panel (`setScreenUp` → `hide()` → `renderPanel`)
    // and re-enter here. With the new label already stored, that re-entry is
    // a no-op instead of a loop.
    this.lastStageLabel = label;
    this.onStageLabelChanged();
  }

  /**
   * Auto-surface (`→ surfaced`) — the panel pushed itself for a consequential
   * EVENT (a blocking advisory, an engaged publish, or a non-ambient task). Used by
   * the event-driven un-hide sites. Never DOWNGRADES a `peek`/`pinned` panel:
   * only a `minimized` panel surfaces (a shown panel is left as-is so a click-pin
   * isn't clobbered by a later event).
   */
  show(): void {
    if (this.revealState === "minimized") this.setRevealState("surfaced");
  }

  /**
   * Pin (`→ pinned`) — the user explicitly opened the panel (click/Enter on the
   * seam). A pinned panel survives mouse-leave AND advisory-clear
   * auto-close; it closes only via ⌄ minimize. Pinnable from any shown OR
   * minimized state.
   */
  pin(): void {
    this.cancelPeekTimers();
    this.setRevealState("pinned");
  }

  /**
   * Peek (`→ peek`) opens a transient hover reveal. Only from `minimized` (a
   * hover never downgrades a pinned/surfaced panel). Auto-closes on leave/settle.
   */
  peek(): void {
    if (this.revealState === "minimized") this.setRevealState("peek");
  }

  // ── Seam input handlers drive reveal transitions ──────────────────────────────────────
  // The ProgressSeam emits raw hover/click inputs (it holds NO visibility
  // state); these three methods are the ONLY place an input is mapped to a
  // RevealState transition (single owner). PreviewManager wires them to the
  // seam's `setCallbacks({ onHoverEnter, onHoverLeave, onClick })`.

  /**
   * Seam hover-enter → after the open-delay, `peek` (only from `minimized`). The
   * delay is the pass-through guard: a cursor merely crossing the ~14-16px seam
   * zone on its way elsewhere fires hover-leave before the delay elapses, so the
   * panel never blooms. A pending close-timer (from a just-prior leave) is
   * cancelled — a re-entry keeps the peek open.
   */
  onSeamHoverEnter(): void {
    // Re-entry cancels a pending close. (Also clear a stale open-timer so we
    // don't stack two.)
    this.cancelPeekTimers();
    // Only a minimized panel peeks; a surfaced/pinned panel ignores hover.
    if (this.revealState !== "minimized") return;
    this.peekOpenTimer = setTimeout(() => {
      this.peekOpenTimer = null;
      this.peek();
    }, ProgressPanel.PEEK_OPEN_DELAY_MS);
  }

  /**
   * Seam hover-leave → after the close-delay, retract a `peek` to `minimized`.
   * Cancels the OPEN-timer outright (a leave before the open-delay = pass-through
   * → never peeks). The close is cancelled by a re-enter, a click-to-pin, or an
   * interrupt-promote (`surfaceOnInterrupt()`), so a panel a click/interrupt just
   * claimed is never shut by a stale pending close.
   */
  onSeamHoverLeave(): void {
    // A leave before the open-delay fires cancels the pending peek (pass-through).
    if (this.peekOpenTimer) {
      clearTimeout(this.peekOpenTimer);
      this.peekOpenTimer = null;
      return;
    }
    // Only a peek auto-closes on leave; a pinned/surfaced panel ignores it.
    if (this.revealState !== "peek") {
      return;
    }
    if (this.peekCloseTimer) clearTimeout(this.peekCloseTimer);
    this.peekCloseTimer = setTimeout(() => {
      this.peekCloseTimer = null;
      // Re-check: an interrupt/click may have changed the state since.
      if (this.revealState === "peek") this.hide();
    }, ProgressPanel.PEEK_CLOSE_DELAY_MS);
  }

  /**
   * Pointer enter/leave on the open panel GLASS — the hover UNION (the flicker
   * fix). The peeked panel opens UNDER the very pointer that triggered it: its
   * glass top welds to the seam (top = pill.bottom − 1, z:toast) and covers the
   * hit band's lower half (z:2), so the band fires mouseleave the moment the
   * peek blooms. Without these, nothing cancels the pending close — the panel
   * retracts under the cursor, the band re-enters, re-peeks: a perpetual
   * ~600ms open/close oscillation. "Inside" must mean band ∪ panel.
   */
  onPanelHoverEnter(): void {
    // Pointer is inside the union — a pending close (from the band's leave)
    // is cancelled exactly like a band re-entry.
    this.cancelPeekTimers();
  }

  onPanelHoverLeave(): void {
    // Leaving the glass re-arms the same close discipline as leaving the band
    // (only a peek auto-closes; pass-through open-timer case can't occur here
    // since a visible panel implies no pending open).
    this.onSeamHoverLeave();
  }

  /**
   * Durable BODY hover enter (NOT the seam — the panel
   * glass itself). Suppresses `checkAutoClose()` while the pointer is on the glass.
   * Wired from the renderer's body `pointerenter` event.
   */
  onPanelBodyHoverEnter(): void {
    this.bodyHovered = true;
    // Cancel any pending peek close from the seam hover-leave path.
    // (The union logic already does this via onPanelHoverEnter; body-level
    // callbacks also feed it so focus-then-hover scenarios don't leak.)
    this.cancelPeekTimers();
  }

  /**
   * Durable BODY hover leave. Re-arms the auto-close
   * discipline so the panel can settle normally once the pointer truly leaves.
   */
  onPanelBodyHoverLeave(): void {
    this.bodyHovered = false;
    // Re-run auto-close in case all other suppressors are gone.
    this.checkAutoClose();
  }

  /**
   * Durable BODY focus in. Suppresses `checkAutoClose()`
   * while any descendant has focus. Wired from the renderer's body `focusin`.
   */
  onPanelBodyFocusIn(): void {
    this.bodyFocused = true;
  }

  /**
   * Durable BODY focus out. Clears the focus guard and
   * re-runs auto-close so the panel can settle once focus truly leaves.
   */
  onPanelBodyFocusOut(): void {
    this.bodyFocused = false;
    this.checkAutoClose();
  }

  /**
   * Escape key handler. Collapses a pinned panel to
   * minimized via `dismiss()` (this is the user closing it, same as the ▾
   * handle). A no-op when the panel is not pinned (surfaced/peek panels
   * auto-close on their own; pressing Esc on a non-pinned panel would be
   * confusing and is not needed).
   */
  onEscapeKey(): void {
    if (this.revealState === "pinned") {
      this.dismiss();
    }
  }

  /**
   * Seam click / Enter / Space → `surfaced` (from minimized; no-op if already
   * shown). Pin is handle-only (the ▾/▴ button at the bottom of the
   * panel glass). The seam click surfaces the panel and moves keyboard focus
   * into it so the user can tab through the content without hover.
   * Cancels any pending hover timers (same as `show()` for a minimized click).
   */
  onSeamClick(): void {
    this.cancelPeekTimers();
    // Promote to surfaced from minimized OR peek (a click during the peek
    // close-delay commits the panel to surfaced so it doesn't slip shut).
    if (this.revealState === "minimized" || this.revealState === "peek") {
      this.setRevealState("surfaced");
    }
    // Move focus into the panel glass so the Esc / Tab path works on keyboard.
    // The renderer exposes the root element; if focus already lives inside, skip.
    const panelEl = this.renderer.getPanelEl();
    if (panelEl && !panelEl.contains(document.activeElement)) {
      // Focus the first focusable descendant, or the panel itself if none.
      const firstFocusable = panelEl.querySelector<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      (firstFocusable ?? panelEl).focus();
    }
  }

  /**
   * Auto-surface for a consequential INTERRUPT (a blocking advisory, an engaged
   * publish, a non-ambient task) — the panel pushed itself unbidden. Distinct
   * from `show()`: an interrupt also CANCELS a pending peek close-timer and
   * promotes `peek → surfaced` before a pending hover-close can run — a
   * blocking advisory that lands while the close-timer is pending must not let
   * that timer shut the panel the interrupt just claimed. A `pinned` panel stays
   * pinned (the user already owns it).
   */
  private surfaceOnInterrupt(): void {
    this.cancelPeekTimers();
    if (this.revealState === "minimized" || this.revealState === "peek") {
      this.setRevealState("surfaced");
    }
  }

  /** Idempotently cancel both pending hover timers (open + close). */
  private cancelPeekTimers(): void {
    if (this.peekOpenTimer) {
      clearTimeout(this.peekOpenTimer);
      this.peekOpenTimer = null;
    }
    if (this.peekCloseTimer) {
      clearTimeout(this.peekCloseTimer);
      this.peekCloseTimer = null;
    }
  }

  get isHidden(): boolean {
    return this._isHidden;
  }

  /** Read the current reveal state, the single source of truth. */
  getRevealState(): RevealState {
    return this.revealState;
  }

  hasActiveTasks(): boolean {
    return this.tasks.size > 0;
  }

  getActiveTasks(): ProgressTask[] {
    return Array.from(this.tasks.values());
  }

  getAdvisories(): AdvisoryEntry[] {
    return Array.from(this.advisories.values());
  }

  /**
   * Expose the current media encoding status for
   * buildViewModel and tests.
   */
  getMediaStatus(): { verb: string; subject: string; current: number; total: number } | null {
    return this.mediaStatus;
  }

  onAllComplete(callback: () => void): void {
    this.allCompleteCallbacks.push(callback);
  }

  onTaskFailed(callback: (taskId: string) => void): void {
    this.taskFailedCallbacks.push(callback);
  }

  /**
   * Register a sink for the top-severity value emitted by the
   * view-model. PreviewManager wires this to `seam.setTopSeverity(s)` so the
   * seam dot reflects the highest advisory tier without the seam duplicating
   * the advisory store. Fires immediately with the current value.
   */
  onTopSeverityChange(callback: (severity: "broken" | "degraded" | "none") => void): void {
    this.topSeverityCallbacks.push(callback);
    callback(this.lastTopSeverity);
  }

  /** Emit top-severity to observers, de-duped to actual transitions. */
  private emitTopSeverity(severity: "broken" | "degraded" | "none"): void {
    if (severity === this.lastTopSeverity) return;
    this.lastTopSeverity = severity;
    for (const cb of this.topSeverityCallbacks) cb(severity);
  }

  /** Register a sink for SEAM-VISIBILITY changes (the seam shows ONLY when
   * there's content). Fires immediately so a late observer is in sync. */
  onContentChange(callback: (present: boolean) => void): void {
    this.contentChangeCallbacks.push(callback);
    callback(this.hasContent());
  }

  /** Is there anything to surface — a task, an active stage, or an advisory?
   * Drives the seam's visibility (and mirrors the renderer's own panel-exists
   * gate): no content ⇒ no panel, no line, no reveal handle. */
  private hasContent(): boolean {
    return (
      this.getActiveTasks().length > 0 ||
      this.narratedTasks.size > 0 ||
      this.advisories.size > 0 ||
      // A terminal failure keeps the seam + reveal handle alive even after the
      // body is collapsed. `computePublishLabel({stage:"failed"})` returns "",
      // so activePrimaryLabel() can't carry it — read lastFailure directly.
      // Belt-and-suspenders alongside the failure advisory: a red status must
      // never become a one-way collapse trapdoor (the re-expand gate).
      this.lastFailure != null ||
      this.activePrimaryLabel().text !== ""
    );
  }

  /** Emit content-presence to observers, de-duped to actual transitions. */
  private emitContentPresence(): void {
    const present = this.hasContent();
    if (present === this.lastContentPresent) return;
    this.lastContentPresent = present;
    for (const cb of this.contentChangeCallbacks) cb(present);
  }

  destroy(): void {
    // Unsubscribe from all Tauri event listeners
    for (const unlisten of this.unlistenFns) {
      unlisten();
    }
    this.unlistenFns = [];

    // Clear all stale timers
    for (const timer of this.staleTimers.values()) clearTimeout(timer);
    this.staleTimers.clear();

    // Clear the completion-receipt retract timer (R11).
    this.clearCompletionReceipt();
    this.clearPublishReceipt();

    // Clear any pending deferred-upsert timers (fast build hooks scheduled
    // but not yet painted) so they don't fire after teardown.
    for (const { timer } of this.deferredUpserts.values()) clearTimeout(timer);
    this.deferredUpserts.clear();

    // Clear both pending hover-reveal timers so a peek open/close doesn't
    // fire after teardown.
    this.cancelPeekTimers();

    // Clear narration fade timers.
    for (const t of this.narratedFadeTimers.values()) clearTimeout(t);
    this.narratedFadeTimers.clear();
    this.narratedTasks.clear();
    this.tasks.clear();
    this.advisories.clear();
    this.dismissedAdvisoryKeys.clear();
    this.allCompleteCallbacks = [];
    this.taskFailedCallbacks = [];
    this.cloudFileDetails.reset();
    this.renderer.destroy();
  }

  /**
   * Handle asset_ready events by relaying to the preview iframe.
   *
   * All media placeholders (images, videos, thumbnails) live inside the
   * preview iframe. We relay every AssetReady event via postMessage to
   * the iframe-bridge script which handles the actual DOM swap.
   */
  private handleAssetReady(asset: AssetReady): void {
    if (this.iframe?.contentWindow) {
      this.iframe.contentWindow.postMessage(
        {
          type: "moss-asset-ready",
          assetType: asset.asset_type,
          path: asset.path,
        },
        "*",
      );
      trace(
        `[ProgressPanel] Posted ${asset.asset_type} ready to iframe: ${asset.path}`,
      );
    } else {
      trace(
        `[ProgressPanel] Cannot relay ${asset.asset_type} to iframe: no contentWindow`,
      );
    }
  }

  /**
   * Show the `Built · N pages · X.Xs` completion receipt in the panel header.
   * To keep routine work quiet, it renders ONLY when the
   * panel is already shown — a routine rebuild's completion is the pill flash,
   * not a grown header. The max-dwell is armed BEFORE that check because it
   * expires the receipt STATE, not its pixels: returning early left a quiet
   * rebuild's receipt immortal, and a hover-peek an hour later still headlined a
   * build long gone. Its other half is in `checkAutoClose`: this does not hold
   * the glass.
   */
  private showCompletionReceipt(): void {
    // Hard max-dwell: clear the receipt then re-render + try to auto-close so
    // the "Built · …" header retracts to the pill rather than sitting forever.
    if (this.receiptTimer) clearTimeout(this.receiptTimer);
    this.receiptTimer = setTimeout(() => {
      this.receiptTimer = null;
      this.lastBuild = null;
      this.buildReceipt = null;
      this.renderPanel();
      this.checkAutoClose();
    }, ProgressPanel.RECEIPT_DWELL_MS);
    // Silence (R11/R12): no receipt grows the glass on a quiet rebuild.
    if (this._isHidden) return;
    this.renderPanel();
  }

  /**
   * Show `Published · 58.0 MB · 12.4s` and retract it on the same timer every
   * other receipt uses.
   *
   * Unlike the build receipt this does NOT obey the silence discipline: a
   * publish is something the author asked for and waited on, so its
   * confirmation is never an unbidden interruption. The panel is open anyway —
   * the publish kept it open.
   */
  private showPublishReceipt(text: string): void {
    this.clearPublishReceipt();
    this.publishReceipt = text;
    this.publishReceiptTimer = setTimeout(() => {
      this.publishReceiptTimer = null;
      this.publishReceipt = null;
      this.renderPanel();
      // The receipt was the last thing holding the panel open; now let it go.
      this.checkAutoClose();
    }, ProgressPanel.RECEIPT_DWELL_MS);
  }

  /** Drop the publish receipt (state + pending retract timer). */
  private clearPublishReceipt(): void {
    if (this.publishReceiptTimer) {
      clearTimeout(this.publishReceiptTimer);
      this.publishReceiptTimer = null;
    }
    this.publishReceipt = null;
  }

  /**
   * The completion-receipt text, formatted at render (no pre-formatted strings
   * on data — Hickey): `Amount{count, noun: "pages"}` + `Duration(elapsedMs)`,
   * both read off the `lastBuild` snapshot. Returns null when there is no
   * receipt to show (no completed build yet, no html_pages total seen when it
   * was minted, or it has retracted).
   */
  private completionReceiptText(): string | null {
    // A media build's parent Job is the authoritative receipt; the legacy
    // `BuildComplete` path below is the receipt only for a media-LESS build
    // (invariant #6). One header slot — see `buildReceipt`.
    if (this.buildReceipt != null) return this.buildReceipt;
    if (this.lastBuild == null) return null;
    const count = this.lastBuild.pageCount;
    if (count == null) return null;
    // The Job path's own three helpers, so the two receipt sources agree on
    // every segment: the localized verb, the localized noun with its singular
    // rule, and a span that drops `· X.Xs` rather than rendering "NaNs"/"0.0s".
    const amount = formatAmount({ count, noun: "pages" });
    return [t("verb.built"), amount, formatElapsed(this.lastBuild.elapsedMs)]
      .filter((p) => p.length > 0)
      .join(" · ");
  }

  /** Drop the completion receipt (state + pending retract timer). */
  private clearCompletionReceipt(): void {
    if (this.receiptTimer) {
      clearTimeout(this.receiptTimer);
      this.receiptTimer = null;
    }
    this.lastBuild = null;
    // The parent-Job receipt + its parent-id anchor are per-build
    // state — a fresh build must not inherit the previous Build Job's receipt or
    // nest new children under a stale parent.
    this.buildReceipt = null;
    this.buildJobId = null;
  }

  /**
   * Build the full PanelViewModel per the Input→home map.
   *
   * STATUS priority (single-line) is owned by `buildStatusInput` — one written
   * copy, because this second one had already drifted off the code.
   *
   * The "running job/plugin" tier: any this.tasks entry that is running (progress
   * != 100 or null, not done) AND NOT in awaitingJobs AND has a label feeds the
   * status line. Tiebreak = most-recently-updated (lastUpdatedAt) wins.
   *
   * NOTICES = groupNotices(getAdvisories()) [excludes awaiting-narration via the
   * isAwaitingNarration predicate] PLUS the awaiting bridge: one degraded NoticeVM
   * per this.tasks entry whose id ∈ awaitingJobs.
   *
   * TOPSEVERITY = max tier across notices; emitted via onTopSeverityChange.
   */
  buildViewModel(): PanelViewModel {
    // ── STATUS ──────────────────────────────────────────────────────────────
    const statusInput = this.buildStatusInput();
    const status = formatStatus(statusInput);

    // ── NOTICES ─────────────────────────────────────────────────────────────
    // 1. From advisories (BackgroundProgress, Job failures, PreviewServerFailed):
    const notices: NoticeVM[] = groupNotices(this.getAdvisories());
    const cloudNotice = notices.find((notice) => notice.key.startsWith(`${CLOUD_UNAVAILABLE_ADV_KEY}::`));
    if (cloudNotice) {
      const snapshot = cloudSync.getUnavailableSnapshot();
      cloudNotice.cloudFiles = this.cloudFileDetails.view(snapshot.count);
    }

    // 2. Awaiting bridge: one degraded notice per awaitingJobs entry. This covers:
    //    - Preview-scope Awaiting Jobs (job:* in awaitingJobs)
    //    - awaiting-narration:* entries (also in awaitingJobs via handleAwaitingNarrationUpdate)
    //    groupNotices already excluded the awaiting-narration SYNTHETIC ADVISORY
    //    (via isAwaitingNarration predicate), so these appear once here — never twice.
    for (const awaitingId of this.awaitingJobs) {
      const awaitingTask = this.tasks.get(awaitingId);
      if (!awaitingTask) continue;
      notices.push({
        key: `awaiting::${awaitingId}`,
        severity: "degraded",
        reason: awaitingTask.label,
        files: [],
        hiddenFiles: [],
        action: awaitingTask.action,
      });
    }

    // ── TOPSEVERITY ─────────────────────────────────────────────────────────
    let topSeverity: "broken" | "degraded" | "none" = "none";
    for (const n of notices) {
      if (n.severity === "broken") { topSeverity = "broken"; break; }
      if (n.severity === "degraded") topSeverity = "degraded";
    }

    // ── RECEIPT CLEAR FLAG ───────────────────────────────────────────────────
    // A green receipt requires zero unresolved notices. When notices
    // are present, the receipt is neutral (no accent colour).
    if (status.kind === "receipt") {
      status.clear = notices.length === 0;
    }

    return {
      status,
      notices,
      reveal: this.revealState,
      topSeverity,
    };
  }

  private renderPanel(): void {
    this.notifyStageLabelChanged();
    // Emit top-severity for the seam dot color (broken/degraded/none).
    // Derive from the full buildViewModel so the seam reflects the actual
    // notice tier (incl. Job failures and PreviewServerFailed).
    const model = this.buildViewModel();
    this.emitTopSeverity(model.topSeverity);
    // Seam visibility tracks "is there content" — the seam (line + reveal
    // handle) is hidden at rest, so an idle pill shows no line and the reveal
    // has no dead-end (a persistent green line reads as stuck progress).
    this.emitContentPresence();

    this.renderer.render(model);

    // Task rows: ONLY narrated (tone==="narrated") tasks render as rows.
    // All other active tasks (plugin/job/video) map to the status line or
    // notices rail via buildViewModel — renderNarrated handles only the
    // narrated store so each channel renders exactly once.
    this.renderer.renderNarrated(this.getNarratedTasks(), this._isHidden);
  }

  /**
   * Build the status-line input by priority.
   * Priority: failed → publish stage → publish receipt → media (mediaStatus) →
   *           cloud download → build receipt → build stage → job/plugin → none.
   *
   * The "running job/plugin" tier (wiring iii): reads any this.tasks entry that
   * has progress != 100 (or null = indeterminate) AND is NOT in awaitingJobs
   * AND has a label. Tiebreak = most-recently-updated (lastUpdatedAt) wins.
   * Covers the Domain/Deploy running Job AND every `plugin:*` hook.
   */
  private buildStatusInput(): StatusInput {
    // lastFailure: set by PreviewServerFailed and by a failed publish deploy.
    // Highest priority: red status overrides everything.
    if (this.lastFailure != null) {
      return { kind: "failed", text: this.lastFailure };
    }

    // Active publish stage (anything except idle/complete/failed)
    if (
      this.currentPublishStage !== "idle" &&
      this.currentPublishStage !== "complete" &&
      this.currentPublishStage !== "failed"
    ) {
      const label = this.activePrimaryLabel();
      // The REBUILD is what pauses — the server keeps serving the last
      // generation, so a mid-publish edit is saved and still browsable at the old
      // bytes until the catch-up build lands.
      //
      // It rides its own CONDITION line, not the
      // `subject` slot. In the slot it was the only shrinkable child of a full
      // nowrap row, so it ellipsized to nothing whenever there were numbers to
      // show — which is every upload. On its own line it finally has room to
      // say why, so it says the whole sentence.
      const paused = t("pstage.rebuild_paused_full");
      // Upload numerics live in the COUNT slot, not next to the pill: the
      // pill cluster is shrink-to-fit, so text there would move the pill as
      // the numbers change. The count slot is
      // right-anchored + tabular-nums in CSS.
      if (this.currentPublishStage === "uploading") {
        const ledger = this.publish.ledger(label.text, paused);
        if (ledger) return ledger;
      }
      return { kind: "running", verb: label.text, condition: paused };
    }

    // A publish that just landed. Placed directly under the active-publish tier
    // so it outranks the BUILD receipt below: after a publish, the rebuild that
    // fed it is not the result the author is waiting to hear about.
    if (this.publishReceipt != null) {
      return { kind: "receipt", text: this.publishReceipt };
    }

    // Media (video conversion) in progress
    if (this.mediaStatus !== null) {
      return {
        kind: "running",
        verb: this.mediaStatus.verb,
        subject: this.mediaStatus.subject,
        current: this.mediaStatus.current,
        total: this.mediaStatus.total,
      };
    }

    // Outstanding cloud downloads outrank the completion receipt: "Built · 47
    // pages" while 553 sources are still in the cloud is true about the build
    // and false about the site. Ordering decides only who
    // owns the line; the receipt still expires on its own dwell, seen or not.
    if (this.cloudOutstanding()) {
      return { kind: "running", verb: this.activePrimaryLabel().text };
    }

    // Text still being read from a site's documents outranks the receipt, and
    // with it the build stage below: "Built" is true of the build and false of
    // the page whose text has not landed. The read starts as the build
    // completes, while the stage still reads as compiling, so in the running
    // tier at the bottom it would never be shown.
    const reading = this.tasks.get("bg:derive");
    if (reading?.label && reading.progress !== 100) {
      return { kind: "running", verb: reading.label };
    }

    // Completion receipt — checked BEFORE the active build stage because the
    // receipt fires (via BuildComplete/Job Succeeded) while the stage is still
    // "compiling" (stage doesn't become "ready" until the next ProgressUpdate).
    // Receipt wins over the "still running" stage label so "Built · N pages"
    // appears immediately at the moment of completion.
    const receipt = this.completionReceiptText();
    if (receipt != null) {
      return { kind: "receipt", text: receipt };
    }

    // Active build stage
    const stageLabel = this.activePrimaryLabel();
    if (stageLabel.text !== "") {
      return { kind: "running", verb: stageLabel.text };
    }

    // Wiring (iii): active running `job:*`/`plugin:*`/`bg:*` tasks in the
    // lowest running tier — fires when build is idle (no stage label) and no
    // receipt. `bg:*` tasks are included: without them, a bgTaskRowDecision row
    // sat in `this.tasks` (satisfying hasContent(), so the seam un-hid) but
    // never reached the status line the panel shows on hover — the
    // hairline-hover render gate caught the seam opening onto empty content.
    // Pick the most-recently-updated (tiebreak) task from the running set.
    let bestTask: ProgressTask | null = null;
    let bestUpdatedAt = -1;
    for (const t of this.tasks.values()) {
      // Must be running (not done = progress 100, not awaiting)
      if (t.progress === 100) continue;
      // Must not be awaiting
      if (this.awaitingJobs.has(t.id)) continue;
      // Must be a job:*, plugin:* or bg:* task (the running tier covers these)
      if (!t.id.startsWith("job:") && !t.id.startsWith("plugin:") && !t.id.startsWith("bg:")) continue;
      // Must have a label
      if (!t.label) continue;
      const updatedAt = t.lastUpdatedAt ?? 0;
      if (updatedAt > bestUpdatedAt) {
        bestUpdatedAt = updatedAt;
        bestTask = t;
      }
    }
    if (bestTask !== null) {
      return { kind: "running", verb: bestTask.label };
    }

    return { kind: "none" };
  }

  /**
   * Set the current pipeline stage. Called by PreviewManager at the start
   * of startPreview (stage='opening') and again at server-ready (stage='ready').
   * Clearing-stages (initial/ready) reset the context accumulator.
   */
  setStage(stage: PreviewStage): void {
    this.currentStage = stage;
    if (stage === "initial" || stage === "ready") {
      // A download outlives the build that noticed it: the supervisor keeps
      // draining while this build goes ready, so `ready` must not wipe a
      // readout that is still true. `initial` is a new session/folder and
      // clears everything — that is where a stale download is dropped.
      const { provider, downloaded, total, remaining } = this.stageContext;
      this.stageContext =
        stage === "ready" && (remaining ?? 0) > 0
          ? { provider, downloaded, total, remaining }
          : {};
    }
    if (stage === "initial") {
      // New session — clear stale publish state so the panel doesn't
      // open the new project showing "Waiting for build…" or whatever
      // publish stage was last seen.
      this.currentPublishStage = "idle";
      this.publish.reset();
      this.clearPublishReceipt();
    }
    if (stage === "opening" || stage === "initial") {
      // Build-start supersedes the previous build: the stale advisory dot of a
      // fixed-and-rebuilt file, the lingering "Built · …" receipt, and the
      // previous publish's failure — the user is taking action by
      // rebuilding. The failure is TWO fields, so both go.
      this.clearBuildAdvisories();
      this.clearCompletionReceipt();
      this.lastFailure = null;
      this.clearPublishFailure();
      // The counter clamps within a pass, so a new build starts from nothing
      // or the last build's high-water mark would floor it.
      this.stageContext.pages = undefined;
    }
    if (stage === "ready") {
      // A successful server-ready clears the preview-server-error failure
      // SCOPED — that advisory key plus lastFailure, NEVER a blanket
      // advisories.clear(): BackgroundProgress ShippedDegraded/NeedsAction
      // advisories must persist through ready and clear only at opening/initial.
      if (this.advisories.has(PREVIEW_SERVER_ADV_KEY)) {
        this.deleteAdvisory(PREVIEW_SERVER_ADV_KEY);
        // Ours to clear only if PreviewServerFailed set it, not a concurrent
        // publish/deploy failure. The prefix comes from the SAME key with an
        // empty {msg}, so the test is locale-correct — never the English literal.
        if (this.lastFailure?.startsWith(t('preview.server_failed', { msg: '' }))) {
          this.lastFailure = null;
          this.clearPublishFailure();
        }
      }
    }
    if (stage === "ready" || stage === "initial") {
      // Build settled — let auto-close handle dismissal if nothing else
      // is active.
      this.renderPanel();
      this.checkAutoClose();
      return;
    }
    // Explicit build-start lifecycle (opening/scanning/…): surface from
    // minimized. `show()` leaves a peek/pinned panel as-is (no downgrade).
    this.show();
    this.renderPanel();
  }

  /**
   * Build-start clearing preserves sticky advisories: the
   * publish-verify narrative describes the LAST PUBLISH, which a rebuild does
   * not answer. Sticky entries leave only via CLEAR_NOTICE or destroy().
   */
  private clearBuildAdvisories(): void {
    for (const [key, entry] of this.advisories) {
      if (!outlivesBuildStart(entry)) this.deleteAdvisory(key);
    }
  }

  /**
   * A publish failure is ONE condition recorded in THREE places — the terminal
   * `currentPublishStage === "failed"`, the red `lastFailure` text (the callers
   * clear that), and the amber `publish-error` advisory — so whoever clears one
   * clears them all. Half-cleared the panel goes mute and never settles:
   * `activePrimaryLabel()` takes the publish branch but a "failed" stage has no
   * label so the build is never narrated, `checkAutoClose()` refuses to retract,
   * and `shouldSurface()` still calls the panel consequential — so every rebuild
   * yanks a minimized panel open on that blank line. The advisory alone does all
   * three, and it outlives the `=== "failed"` guard in `advancePublishStage`
   * that was its only other cleanup. Reset ONLY from "failed": another stage
   * means a publish is in flight, and a folder-open must not cancel it.
   */
  private clearPublishFailure(): void {
    if (this.currentPublishStage !== "failed") return;
    this.currentPublishStage = "idle";
    this.publish.reset();
    this.deleteAdvisory(this.PUBLISH_ADV_KEY);
  }

  private advanceStage(event: StageEvent): void {
    const next = nextStage(this.currentStage, event);
    if (next !== this.currentStage) {
      // No blanket clear at build-start: `nextStage` never regresses, so once the
      // stage reaches `ready` a later `scanning` target is never a transition.
      // Each `BackgroundProgress` task reconciles its own advisories at its own
      // `completed:true` tick (`staleTaskAdvisoryKeys`, swept in the listener
      // below), which also survives a partial rebuild that never touches this
      // stage machine.
      this.currentStage = next;
      // Defer to the author's focus. A routine
      // advisory-free, publish-idle rebuild (scanning→compiling→ready) must
      // NOT grow the panel: now that the panel extrudes from the nav pill over
      // the preview, force-showing it on every keystroke-rebuild would be the
      // loud opposite of "success makes no sound." The Sync hairline (green
      // fill) + the pill flash own that moment. So un-hide on a stage
      // transition ONLY when this rebuild is consequential — an advisory is
      // present, OR publish/deploy is engaged. Otherwise stay quiet. (The
      // separate "late event after a monotonic-refused transition" guard is
      // preserved: this branch only runs on an ACTUAL transition.)
      // A consequential rebuild interrupts routine work — surface from
      // minimized and cancel a pending peek close-timer when promoting peek.
      if (this.shouldSurface()) this.surfaceOnInterrupt();
    }
    this.renderPanel();
  }

  /**
   * The silence-discipline predicate. A routine, advisory-free,
   * publish-idle build is NON-SURFACING — it lives and dies at the Sync
   * hairline + pill flash, never growing the glass over the preview. The panel
   * un-hides (extrudes) only when the build is consequential-by-content
   * (an advisory the user may need to act on) OR consequential-by-intent
   * (publish/deploy is engaged).
   *
   * Consequential-by-CONTENT means a BLOCKING advisory, not merely "any
   * advisory." A `ShippedDegraded` (video shipped unoptimized) or non-blocking
   * `NeedsAction` (install a tool) advisory earns a quiet hairline dot but must
   * NOT auto-pop the panel — so the advisory term filters on
   * `severity === "Blocking"`. (`Severity` is `ShippedDegraded | NeedsAction |
   * Blocking`; the `Awaiting` interrupt is a task-state, not an advisory
   * severity.)
   *
   * Note: this gates the EVENT-DRIVEN auto-show during build progress
   * (`advanceStage` / the materializing path), the advisory-add un-hide in the
   * BackgroundProgress handler, AND the un-hide of AMBIENT `upsertTask` rows
   * (routine media sub-jobs). It deliberately does NOT touch a non-ambient
   * `upsertTask` (deploy/dns/error tasks are themselves consequential) or
   * `show()`/`setStage('opening')` (explicit lifecycle calls).
   *
   * A Blocking key the user has already dismissed (`dismissedAdvisoryKeys`,
   * see `dismiss()`) does not count — otherwise every routine per-edit
   * rebuild would re-run this check against the same still-present row and
   * pop the panel back open.
   */
  private shouldSurface(): boolean {
    const hasBlockingAdvisory = Array.from(this.advisories.entries()).some(
      ([key, a]) => a.severity === "Blocking" && !this.dismissedAdvisoryKeys.has(key),
    );
    // An `Awaiting` Job is the one interrupt — it needs the
    // user's action ("Verify your email to finish connecting example.test"), so it
    // satisfies this predicate, auto-promoting a mid-peek panel and surfacing
    // unbidden. `awaitingJobs` is maintained by `handleJobUpdate`.
    const hasAwaitingJob = this.awaitingJobs.size > 0;
    return (
      hasBlockingAdvisory ||
      hasAwaitingJob ||
      this.currentPublishStage !== "idle"
    );
  }

  /** Current publish stage (idle when no publish flow is active). */
  getPublishStage(): PublishStage {
    return this.currentPublishStage;
  }

  /** Latest upload counts from deploy-progress events; null when not uploading. */
  getPublishUpload(): { current: number; total: number } | null {
    return this.publish.counts();
  }

  private advancePublishStage(
    event: Parameters<typeof nextPublishStage>[1],
    // The failure detail from `deploy-progress { stage:"failed", message }`
    // (deploy.rs passes the error string as `message`). Only meaningful for a
    // `failed` event; carried into the advisory DETAIL below.
    detail?: string | null,
  ): void {
    // If the current stage is "failed"
    // (a terminal with no idle-reset path), a new waiting/progress event means a
    // fresh publish is engaging. Reset to idle and clear lastFailure BEFORE
    // calling nextPublishStage, so the stage machine starts from a clean slate.
    if (
      this.currentPublishStage === "failed" &&
      (event.kind === "waiting" || event.kind === "progress")
    ) {
      // A fresh publish supersedes the previous failure, all three recordings.
      this.lastFailure = null;
      this.clearPublishFailure();
    }

    // Capture idle state AFTER the failure-reset above (so a re-publish that just
    // reset "failed"→"idle" counts as engaging) but BEFORE the stage advance —
    // this is the idle→engaged EDGE. The tail auto-surface fires only on this
    // edge (see below); otherwise the 4 Hz deploy ticker would re-surface a panel
    // the user just minimized on every tick (deploy.rs emits at 4 Hz).
    const wasIdle = this.currentPublishStage === "idle";

    const next = nextPublishStage(this.currentPublishStage, event);
    if (next !== this.currentPublishStage) {
      this.currentPublishStage = next;
    }

    if (wasIdle && next !== "idle") {
      // The publish has engaged. Stamp the start for the receipt's elapsed
      // span, and drop any pending BUILD receipt: after a publish the rebuild
      // that fed it is not what the author wants confirmed, and leaving it
      // pending is exactly how "Built · N pages · 3.2s" ended up being the last
      // word on the publish result.
      this.publish.begin(performance.now());
      this.clearCompletionReceipt();
      this.clearPublishReceipt();
    }

    if (next === "complete") {
      // Mint the publish receipt BEFORE the context is wiped — bytes sent and
      // elapsed are the two things only the publish knows, and one line later
      // they are gone.
      this.showPublishReceipt(this.publish.receipt(performance.now()));
      // Successful publish: reset publish state, clear lastFailure, let auto-close
      // handle panel dismissal (when no secondary tasks remain).
      this.currentPublishStage = "idle";
      this.publish.reset();
      this.lastFailure = null;
      // Scoped-clear a prior failure's advisory so a success doesn't leave a
      // stale amber dot behind (mirrors the preview-server-error self-clear).
      this.deleteAdvisory(this.PUBLISH_ADV_KEY);
      this.renderPanel();
      this.checkAutoClose();
      return;
    }

    if (next === "failed") {
      // Failed does NOT idle-reset and does NOT auto-close. Persist
      // the failure state so the user can see the red status. The currentPublishStage
      // stays "failed" (terminal) until a re-publish event resets it above.
      // lastFailure drives the single-line red status; keep the generic label
      // there (the status line has no room for the full error).
      this.lastFailure = t('preview.publish_failed');
      // The uploading stage is over: drop the estimator window, the sticky
      // rate and the announced milestones so a retry starts clean.
      this.publish.resetUpload();
      // Emit an advisory carrying the REAL error detail so the STANDARD advisory
      // surface renders: severity dot + collapsible notice (with the detail) +
      // a persistent re-expand handle. Without this, a deploy failure was
      // orphaned off the advisory system — topSeverity="none" (no dot) and
      // hasContent()=false (no seam/handle, and a one-way collapse trapdoor).
      // Mirrors the PreviewServerFailed wiring (lastFailure + advisory +
      // surfaceOnInterrupt), but at the amber/degraded tier: a deploy failure is
      // typically retryable (network/endpoint), so severity NeedsAction + scope
      // Remote → noticeTier → "degraded" (amber dot), not "broken" (red).
      const detailText = (detail ?? "").trim();
      this.advisories.set(this.PUBLISH_ADV_KEY, {
        task: "publish-error",
        item: null,
        // groupNotices splits `reason` at the first " — " into head (title) +
        // detail. Omit the delimiter when there's no detail so we don't render
        // an empty detail line.
        reason: detailText
          ? `${t('preview.publish_failed')} — ${detailText}`
          : t('preview.publish_failed'),
        severity: "NeedsAction",
        scope: "Remote",
      });
      // Surface the panel so the user sees the amber status + notice.
      this.surfaceOnInterrupt();
      // Keep publish stage as "failed" (do NOT idle-reset).
      this.renderPanel();
      // Do NOT call checkAutoClose here — lastFailure suppresses it anyway.
      return;
    }

    // Publish is consequential-by-intent — the idle→engaged EDGE is an interrupt:
    // surface from minimized and promote a pending peek, cancelling its
    // close-timer. Gated to the edge (wasIdle) so an in-flight publish's 4 Hz
    // progress ticks do NOT re-yank a panel the user has minimized — the pill
    // hairline keeps advancing while the panel stays collapsed.
    if (wasIdle && next !== "idle") this.surfaceOnInterrupt();
    this.renderPanel();
  }

  /**
   * Are files for this folder still coming down from the cloud?
   *
   * The authority is `remaining`, not `downloaded < total`: the counts are a
   * snapshot of one supervisor tick, and `remaining === 0` is the only thing
   * the backend says that means "the episode is over".
   */
  private cloudOutstanding(): boolean {
    return (this.stageContext.remaining ?? 0) > 0;
  }

  /** Forget the download, so the chrome stops reporting one. */
  private clearCloudReadout(): void {
    this.stageContext.remaining = undefined;
    this.stageContext.downloaded = undefined;
    this.stageContext.total = undefined;
  }

  /**
   * Pick build label or publish label depending on which half is active.
   *
   * Outstanding downloads outrank the build stage. The stage machine is
   * monotonic and `materializing` ranks below `compiling`/`ready`, so once a
   * build moved past it an ongoing bulk download had nowhere to be said —
   * Reporting it from here rather than by
   * rewinding the stage keeps the stage machine's invariant intact and reaches
   * every consumer of the primary label at once (the bar, `hasContent`'s seam
   * visibility, the status line).
   *
   * Publish still wins: it is a foreground action the user just took, and the
   * download keeps its place in the status line below (`computeStatusInput`).
   */
  private activePrimaryLabel(): StageLabel {
    if (this.currentPublishStage !== "idle") {
      return computePublishLabel(this.buildPublishLabelInput());
    }
    if (this.cloudOutstanding()) {
      return computeStageLabel(this.cloudLabelInput());
    }
    return computeStageLabel(this.buildStageLabelInput());
  }

  /** The download readout, independent of which stage is current. */
  private cloudLabelInput(): StageLabelInput {
    const ctx = this.stageContext;
    return {
      stage: "materializing",
      provider: ctx.provider ?? "cloud",
      downloaded: ctx.downloaded ?? 0,
      total: ctx.total ?? 0,
    };
  }

  private buildStageLabelInput(): StageLabelInput {
    const ctx = this.stageContext;
    switch (this.currentStage) {
      case "initial":
        return { stage: "initial" };
      case "opening":
      // The pre-build threshold states narrate on their own full-window
      // surface (threshold-screen.ts); the panel's label stays at the
      // opening form rather than inventing a second teller.
      case "checking":
      case "threshold":
        return { stage: "opening" };
      case "scanning":
        return { stage: "scanning", found: ctx.found };
      case "materializing":
        return this.cloudLabelInput();
      case "compiling":
        return compilingLabelInput(ctx.pages);
      case "ready":
        return { stage: "ready" };
    }
  }

  private buildPublishLabelInput(): PublishLabelInput {
    return this.publish.labelInput(this.currentPublishStage);
  }

  /**
   * What rank 7 ("building") of `resolveUnshowableReason` shows on the
   * full-window screen — `null` when nothing is known yet (rank 8, "opening",
   * takes over).
   *
   * Deliberately NOT `buildStageLabelInput()`/`cloudLabelInput()`: those
   * report by `remaining`, which counts every outstanding file, not only the
   * ones the current render is waiting on. Rank 7 is only reached once
   * `blocking === 0` (rank 5 "downloading" claims every tick where it isn't),
   * so a naive reuse here would show "Downloading X of Y" for files the
   * render does not need — a bug that would come back through the
   * door. The `materializing` case below is therefore always the
   * fixed wording ("Building your site"), never a count.
   */
  buildStageKnownLabel(): string | null {
    switch (this.currentStage) {
      case "scanning":
        return computeStageLabel({ stage: "scanning", found: this.stageContext.found }).text;
      case "materializing":
        return t("preview.waiting_building");
      case "compiling":
        return computeStageLabel(compilingLabelInput(this.stageContext.pages)).text;
      case "initial":
      case "opening":
      case "checking":
      case "threshold":
      case "ready":
        return null;
    }
  }


  private fireAllComplete(): void {
    for (const cb of this.allCompleteCallbacks) {
      cb();
    }
  }

  /**
   * Check if panel should auto-close. Closes when no secondary tasks remain
   * AND no publish flow is in progress AND build has reached ready/initial.
   * Every `return` below names the suppressor it is.
   *
   * Auto-close preserves the user's explicit choice to keep a `pinned` panel open. The
   * user explicitly pinned it (click/Enter); only the ⌄ minimize / Esc closes it.
   * A `surfaced`/`peek` panel auto-closes as before (it pushed itself / is a
   * transient hover, so the work settling is the cue to retract). An already
   * `minimized` panel is a no-op.
   */
  private checkAutoClose(): void {
    if (this.revealState === "minimized" || this.revealState === "pinned") {
      return;
    }
    // Durable hover/focus on the panel body suppress the auto-close.
    if (this.bodyHovered || this.bodyFocused) return;
    // An unresolved publish failure persists until the user acts.
    if (this.lastFailure != null) return;
    if (this.hasActiveTasks()) return;
    // A narrated task persists until dismissed (failed forever; running/
    // succeeded until their fade timer). Mirrors hasContent()'s narratedTasks.
    if (this.narratedTasks.size > 0) return;
    if (this.advisories.size > 0) return;
    if (this.currentPublishStage !== "idle") return;
    // A publish receipt is live: hold the glass until it retracts, or the author
    // never sees the confirmation of the thing they asked for; its dwell timer
    // re-runs this. There is deliberately NO matching guard for the BUILD
    // receipt — the two run opposite disciplines, each owning one half. A
    // publish is foreground, so it paints unconditionally and is held here. A
    // build is ambient, so `showCompletionReceipt` refuses to paint unless the
    // glass is already open (R11/R12), and a receipt that only paints into an
    // open panel does not also get to keep it open — that would refuse to
    // retract at the very moment the author's site appears. R11 gives that
    // moment the pill flash instead.
    if (this.publishReceipt != null) return;
    // An outstanding download holds the panel open past a settled stage:
    // retracting takes down the one line saying the site is still filling in.
    if (this.cloudOutstanding()) return;
    // Build is "active" in scanning/materializing/compiling — hide only after.
    if (
      this.currentStage !== "initial" &&
      this.currentStage !== "ready"
    ) {
      return;
    }
    this.hide();
  }
}
