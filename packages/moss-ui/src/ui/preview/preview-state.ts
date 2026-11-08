// Preview state machine - pure state management without DOM dependencies
// All state transitions are explicit and testable

import type { ProgressUpdate } from '@symbiosis-lab/moss-ui';
import { warn } from "../log";
import { t } from "../i18n/index";

/**
 * Coarse-grained pipeline stage, used by the progress panel to show a
 * single top-level label ("Scanning files…", "Downloading from iCloud…",
 * "Generating pages…") independent of the detailed task list.
 *
 * State transitions:
 * - initial -> opening (client-side, on folder pick)
 * - opening -> checking (nested-site detection walk runs; pre-build, nothing
 *   written yet — threshold narration with a live folder count)
 * - checking -> threshold (detection found nested site(s); the threshold
 *   screen holds until the user's one GuardDecision)
 * - checking -> scanning (detection found nothing: straight into the build,
 *   no interstitial flash)
 * - threshold -> scanning (decision made, build proceeds in the chosen root)
 * - opening -> scanning (on first scan-progress event)
 * - scanning -> materializing (on scan-complete with evicted_count > 0)
 * - scanning -> compiling (on scan-complete with evicted_count == 0)
 * - materializing -> compiling (on first BackgroundProgress event)
 * - compiling -> ready (on serverUrl set)
 */
export type PreviewStage =
  | 'initial'
  | 'opening'
  | 'checking'
  | 'threshold'
  | 'scanning'
  | 'materializing'
  | 'compiling'
  | 'ready';

/**
 * Possible preview status states.
 *
 * State transitions:
 * - initial -> loading (on first progress update)
 * - loading -> loading (on subsequent progress updates)
 * - loading -> ready (on completion — the served page is always real,
 *   even for an empty folder: `empty_home.rs` synthesizes a bare
 *   folder-name homepage. The empty-folder onboarding UI lives in the
 *   editor action-panel folder-mode, NOT as a preview overlay.)
 * - ready -> ready (silent rebuild, no change until complete)
 *
 * There is no longer an `empty` preview status — the preview always
 * loads the served homepage artifact. `is_empty` is surfaced separately
 * via `lastCompletionWasEmpty()` so the app shell can auto-open the
 * editor in folder-mode (the onboarding surface), without painting over
 * the preview canvas.
 */
export type PreviewStatus =
  | { type: 'initial' }
  | { type: 'loading'; message: string; percentage: number }
  | { type: 'ready'; url: string }
  | { type: 'error'; message: string; verdict: PreviewErrorVerdict };

/**
 * Where an `error` status came from — told, or guessed.
 *
 * A build that did not start the preview server carries no port, so a completion without one
 * is not evidence that the server failed. Reading it as one would take the window from a live
 * download screen.
 *
 * - `reported`: something failed and said so — the build threw, the iframe is
 *   missing, the page would not load, or Rust emitted
 *   `MossEvent::PreviewServerFailed`. This is a verdict, and a verdict may take
 *   the window from any other surface (see `preview-manager`'s error branch).
 * - `unverified`: nothing failed out loud; moss only noticed something absent.
 *   Worth showing when there is nothing better on screen, never worth
 *   destroying a screen that is explaining a real, ongoing download.
 */
export type PreviewErrorVerdict = 'reported' | 'unverified';

/**
 * The one contest for the preview window, when there is nothing of the
 * site's own to show, resolved by one ordered ladder.
 *
 *   rank  reason       raised when
 *    1    unreachable  folder health = unavailable
 *    2    gate         build produced no home page (sticky)
 *    3    error:hard   status error, verdict !== 'unverified'
 *    4    page-pending a just-created page is not in the build output yet
 *    5    stalled      blocking > 0 and 90s of silence
 *    6    downloading  blocking > 0 (never for blocking === 0)
 *    7    error:soft   status error, verdict === 'unverified'
 *    8    building     a build stage is known
 *    9    opening      nothing served and nothing else known
 *
 * `null` means a site is served and the window belongs to it. Ranks 1, 2, 3
 * and 4 are checked before that: an unreachable folder or a stale gate can take
 * the window back from an already-served site (there may be a 404 or an
 * arbitrarily stale copy underneath), and so can a REPORTED error — a build
 * that just failed outranks whatever an earlier, now-untrustworthy build left
 * on screen (folded into ranks 5-7 below).
 *
 * Rank 4 is checked before `previewBlank` for the same reason rank 2 is: what
 * is served underneath it is a 404. The author just created a page and the
 * preview jumped to its build-independent URL before any build ran, so a site being
 * served is exactly the condition, not a disqualification.
 *
 * Ranks 5 through 9 apply
 * only while there is nothing of the preview's own on screen
 * (`previewBlank`) — an UNVERIFIED error is one of them (rank 7): a mere
 * guess must not interrupt a live download or a served site, but it is
 * still shown rather than stranding the user on a stalled progress bar once
 * the window is otherwise empty.
 */
export interface UnshowableInputs {
  unreachable: boolean;
  gateUp: boolean;
  previewBlank: boolean;
  errorVerdict: PreviewErrorVerdict | null;
  /** A page the author just created has been navigated to before the build
   *  that produces it has landed, and the wait is still armed. */
  pagePending: boolean;
  blocking: number;
  stalled: boolean;
  /** Non-null text for the stage a build has reached (rank 7); null when
   *  nothing is known yet (rank 8) or the site is already served. */
  buildStageLabel: string | null;
}

export type UnshowableReason =
  | 'unreachable'
  | 'gate'
  | 'error-hard'
  | 'page-pending'
  | 'stalled'
  | 'downloading'
  | 'error-soft'
  | 'building'
  | 'opening';

export function resolveUnshowableReason(inputs: UnshowableInputs): UnshowableReason | null {
  if (inputs.unreachable) return 'unreachable';
  if (inputs.gateUp) return 'gate';
  if (inputs.errorVerdict === 'reported') return 'error-hard';
  if (inputs.pagePending) return 'page-pending';
  if (!inputs.previewBlank) return null;
  if (inputs.blocking > 0 && inputs.stalled) return 'stalled';
  if (inputs.blocking > 0) return 'downloading';
  if (inputs.errorVerdict === 'unverified') return 'error-soft';
  if (inputs.buildStageLabel) return 'building';
  return 'opening';
}

/**
 * Pure state machine for preview lifecycle.
 *
 * This class manages the preview state independently of the DOM.
 * All state transitions are explicit and can be tested without mocking.
 *
 * Key behaviors:
 * 1. Initial build: Shows loading progress, then ready/empty on completion
 * 2. Silent rebuilds: Once server is ready, progress updates are ignored
 *    until completion - this prevents UI flicker during rapid rebuilds
 * 3. Empty <-> Ready transitions: Handles sites that become empty or gain content
 */
export class PreviewState {
  private status: PreviewStatus = { type: 'initial' };
  private serverUrl: string | null = null;
  private stage: PreviewStage = 'initial';
  /**
   * Tracks whether the most recent *completed* build reported an empty
   * folder (`is_empty === true`). Independent of `status` (which is now
   * always `ready` once the server is up — the served empty-home page is
   * real content). Consumed by the app shell to decide whether to
   * auto-open the editor in folder-mode (the onboarding surface).
   */
  private lastEmpty = false;
  /**
   * The reason Rust gave for the preview server not starting, if it gave one.
   *
   * Set from `MossEvent::PreviewServerFailed`, which the backend emits from the
   * `Err` arm of `start_preview_server` — before the build runs, so it always
   * precedes the completion it explains. Its presence is what turns a portless
   * completion from a guess into a verdict; see [`PreviewErrorVerdict`].
   */
  private serverFailure: string | null = null;

  /**
   * Check if server URL has been established.
   * Once true, rebuilds are silent (no progress shown).
   */
  isServerReady(): boolean {
    return this.serverUrl !== null;
  }

  /**
   * Get current server URL (null if not ready).
   */
  getServerUrl(): string | null {
    return this.serverUrl;
  }

  /**
   * Get current preview status.
   */
  getStatus(): PreviewStatus {
    return this.status;
  }

  /**
   * Get current pipeline stage.
   */
  getStage(): PreviewStage {
    return this.stage;
  }

  /**
   * Whether the most recent *completed* build reported an empty folder.
   * Drives the app shell's auto-open-editor-in-folder-mode decision.
   * Reset to `false` on `reset()` (folder switch / first build).
   */
  lastCompletionWasEmpty(): boolean {
    return this.lastEmpty;
  }

  /**
   * Set the current pipeline stage.
   */
  setStage(stage: PreviewStage): void {
    this.stage = stage;
  }

  /**
   * Process a progress update and return the new status.
   *
   * State transition logic:
   * 1. If server is already ready (rebuild), only update on completion
   * 2. If server not ready (initial build), show loading progress
   * 3. On completion, transition to ready or empty based on is_empty
   *
   * Key behavior: On rebuild completion, we DON'T create a new 'ready' status
   * if already in 'ready' state. This prevents unnecessary reloads - the
   * file-changed event handler (smart diff) decides whether to refresh.
   *
   * @param update - Progress update from backend
   * @returns New status after processing the update
   */
  handleProgressUpdate(update: ProgressUpdate): PreviewStatus {
    const isComplete = update.completed && update.percentage >= 100;

    // A port is proof a preview server is up, and `serverFailure` is a record
    // that one did not start. Evidence beats a record, so the record is
    // retired here rather than only in `reset()` — `reset()` runs on a folder
    // switch, and a server that has since started in the SAME folder falsifies
    // it just as much. Deliberately above every branch below, including the
    // `serverUrl !== null` early return: `build.rs`'s build-start event
    // carries a port with `is_empty: None`, which the early-preview branch
    // does not accept, so that update proves the server is up without moving
    // the preview to `ready`. That is the state the stale record would still
    // be read in, and reading it there would upgrade the next portless
    // completion to `reported` — the verdict allowed to take the window from a
    // live download screen.
    if (update.port) {
      this.serverFailure = null;
    }

    // Once server is ready, rebuilds are silent (no status change until complete)
    if (this.serverUrl !== null) {
      if (isComplete) {
        // Record empty-ness for the app shell's onboarding decision, but
        // DON'T change the preview status: the served page is always real
        // (empty_home.rs synthesizes a bare folder-name homepage), so the
        // preview stays `ready` and keeps showing the artifact. The
        // file-changed event handler decides whether to refresh.
        this.lastEmpty = update.is_empty === true;
      }
      // Non-complete updates during rebuild are silent - no status change
      return this.status;
    }

    // Server not ready yet - this is initial build
    if (!isComplete) {
      // If port is available and site has content, show preview immediately.
      // This happens when site/ exists from a previous build — the server can
      // serve it right away while the rebuild continues in background.
      // Once serverUrl is set, subsequent progress events become "silent rebuild".
      if (update.port && update.is_empty === false) {
        this.serverUrl = `http://localhost:${update.port}`;
        this.status = { type: 'ready', url: this.serverUrl };
        return this.status;
      }
      // Show loading progress
      this.status = {
        type: 'loading',
        message: update.message,
        percentage: update.percentage,
      };
      return this.status;
    }

    // Initial build complete - extract server URL and set status.
    // Even an empty folder serves a real page now (empty_home.rs), so we
    // always go to `ready`. Empty-ness is recorded separately for the
    // app shell's onboarding decision.
    if (update.port) {
      this.serverUrl = `http://localhost:${update.port}`;
      this.lastEmpty = update.is_empty === true;
      this.status = { type: 'ready', url: this.serverUrl };
    } else if (this.serverFailure !== null) {
      // Told: Rust's `start_preview_server` returned Err and said so on the
      // typed bus. The detailed reason is the progress-panel's — it upserts a
      // row with `detail: msg` so the popover carries it — and the overlay
      // shows the generic structural message.
      if (this.status.type !== 'error') {
        this.status = {
          type: 'error',
          message: t('preview.server_unavailable'),
          verdict: 'reported',
        };
      }
    } else {
      // Guessed. A completion carries no port whenever the pipeline that sent
      // it did not start the server (watcher rebuild, cloud-supervisor arrival
      // rebuild, deploy pre-build) — and one of those completing says nothing
      // about the server another pipeline started. Two pipelines can race a folder-open,
      // so a portless completion from one must not call a live server dead.
      //
      // So the status is still an error — with nothing on screen the user
      // needs to be told something rather than left on a bar that has stopped
      // — but it is marked `unverified`, and `preview-manager` will not let an
      // unverified error destroy a surface that is explaining a real download.
      warn('⚠️ Progress update marked complete but no port provided');
      if (this.status.type !== 'error') {
        this.status = {
          type: 'error',
          message: t('preview.server_unavailable'),
          verdict: 'unverified',
        };
      }
    }

    return this.status;
  }

  /**
   * Set error state.
   *
   * @param message - Error message to display
   */
  setError(message: string): void {
    this.status = { type: 'error', message, verdict: 'reported' };
  }

  /**
   * Record that Rust reported the preview server failed to start.
   *
   * The message itself is the progress-panel's to surface; what this class
   * needs is only that a failure was REPORTED, which is what lets the next
   * portless completion be read as a verdict rather than a guess. Deliberately
   * does not set an error status of its own: the completion is the event that
   * ends the build, and the panel already owns the detail (the overlay listener
   * that used to double-signal here was removed).
   */
  noteServerFailed(message: string): void {
    this.serverFailure = message;
  }

  /**
   * Reset state to initial.
   * Useful for testing or reinitialization.
   */
  reset(): void {
    this.status = { type: 'initial' };
    this.serverUrl = null;
    this.stage = 'initial';
    this.lastEmpty = false;
    // A failure belongs to the server the previous folder was served by.
    // Carrying it across would make the new folder's first portless
    // completion a verdict about a server that was never asked to start.
    // Not the only exit: `handleProgressUpdate` also retires it the moment a
    // port contradicts it, so a same-folder recovery is not left latched.
    this.serverFailure = null;
  }
}
