// Preview System - Iframe-based Management
// Handles preview window lifecycle, progress display, and build workflow
//
// Architecture:
// - PreviewState: Pure state machine for preview lifecycle (preview-state.ts)
// - getFileChangeAction: Pure function for file change handling (preview-actions.ts)
// - extractPathFromUrl: Pure path utilities (path-matcher.ts)
// - PreviewManager: Thin view layer that controls the preview iframe (this file)
//
// Iframe Architecture:
// - Single "preview" webview: Contains toolbar, control panel, and <iframe>
// - Preview content: Loads in the iframe via iframe.src
// - Communication: Direct iframe.src manipulation for navigation

import type { ProgressUpdate, BuildComplete, DeployProgress, BackgroundProgress, EngineResult } from "@symbiosis-lab/moss-ui";
import { host } from "../host";
import { onMossEvent } from "../utils/moss-events";
import { FullWindowScreen, type ScreenContent } from "./screen/surface";
import { markStartup } from "../log";
import { ThresholdPass } from "./threshold/threshold-pass";
import { type ThresholdPrompt } from "./threshold/seam";
import { OverlaySlot } from "./screen/overlay-slot";
import { ScreenHold } from "./screen/screen-hold";
import { cloudSync, type CloudSyncPayload } from "./cloud/cloud-sync-store";
import { ProgressPanel } from './progress/progress-panel';
import { PreviewHairline } from './preview-hairline';
import { deployFraction } from './progress/hairline-pacing';
import { eventBus, Events, type DeployProgressEvent } from '../context/event-bus';
import { ProgressSeam } from './progress/progress-seam';
import { NavigationManager, type ActivePage, type NavigationResultInfo } from "./navigation-manager";
import { PreviewState, PreviewStatus, resolveUnshowableReason, type UnshowableReason } from "./preview-state";
import { getFileChangeAction, decideNewPageWait, type NewPageWaitState, FileChangeEvent } from "./preview-actions";
import { extractPathFromUrl, sourcePathToHtmlPath } from "./path-matcher";
import { withShellMarker } from "./shell-marker";
import { BuildSingleflight } from "./build-singleflight";
import { info, warn, error } from "../log";
import { setOverlayUp } from "../shared/ui/toast/overlay-toast-gate";
import { t } from "../i18n/index";
import { PreviewZoomController, ZoomBadge } from './preview-zoom';
import { toRelative } from '../editor/path-domain';
import { bindPreviewInstruments, type PreviewInstruments } from './preview-instruments';
import { count as diagCount } from "../lib/diag";
import { formatRefreshDecisionLine, implicitRefreshContext, type RefreshTrigger, type RefreshOutcome, type RefreshDecisionContext } from './preview-refresh-decision';
import { engine } from '../engine';

/** What a guard command answers in a host with no nested-site guard; nothing asks one there. */
const NO_GUARD: Promise<EngineResult<null>> = Promise.resolve({ status: 'error', error: 'this host has no nested-site guard' });

export type { BuildComplete };

/**
 * Main preview system manager.
 *
 * This is a thin view layer that:
 * 1. Delegates state management to PreviewState (pure state machine)
 * 2. Delegates file change logic to getFileChangeAction (pure function)
 * 3. Controls the preview iframe for navigation
 *
 * Iframe architecture:
 * - This code runs in the main "preview" webview
 * - Preview content loads in an <iframe> element
 * - Uses iframe.src manipulation for navigation
 *
 * All business logic is in pure, testable modules.
 */
export class PreviewManager {
  private state: PreviewState;
  private navigationManager: NavigationManager;
  private currentPreviewUrl: string | null = null;
  private rebuildUnsub: (() => void) | null = null;
  private iframe: HTMLIFrameElement | null = null;
  private overlay: HTMLElement | null = null;
  // When the waiting/error screen was last raised, for the shown/dismissed
  // duration logged from `applyUnshowableReason`.
  private waitingScreenShownAt: number | null = null;
  private folderPath: string | null = null;
  /**
   * The wait/loading screen's title and location-line inputs — `VaultRoot::
   * name()`, the home-abbreviated parent (`redact_home_dir`), and a detected
   * cloud provider label, threaded from Rust alongside `folderPath` itself
   * (see `describe_vault_location` in the host location resolver) rather than
   * re-derived here. Set together with `folderPath` in `runBuild`; read by
   * `unshowableContentFor` and the threshold pass's `showChecking`.
   */
  private folderName: string | null = null;
  /**
   * This folder's site has not drawn in the preview yet. A screen that is
   * told to leave meanwhile stays until the iframe has loaded, so the window
   * goes from the screen to the site instead of through an empty frame.
   */
  private firstPaint: 'awaiting' | 'drop-on-load' | 'done' = 'done';
  /** The folder of an open that failed before this page had one. */
  private failedOpenPath: string | null = null;
  private parentDisplay: string | null = null;
  private cloudProviderLabel: string | null = null;
  private configReadyUnlisten: (() => void) | null = null;
  private instruments: PreviewInstruments | null = null;
  /** Unlisten for the deploy-progress→hairline wiring; removed in destroy(). */
  private deployHairlineUnlisten: (() => void) | null = null;
  private busDeployUnsub: (() => void) | null = null;
  /** Preview zoom controller — manages Cmd+=/−/0 scale on the iframe. */
  private zoom: PreviewZoomController | null = null;
  /** Transient "125%" badge over the preview corner; click resets to 100%. */
  private zoomBadge: ZoomBadge | null = null;
  /** While a feature holds the screen the folder's own status and rebuild
   *  traffic must not repaint it — see `screen/screen-hold.ts`. */
  readonly screenHold = new ScreenHold(() => ({ container: this.overlay, slot: this.slot }));

  private pageRetryTimer: number | null = null;
  private static readonly PAGE_SETTLE_RETRIES = 8;
  private static readonly PAGE_SETTLE_DELAY_MS = 350;

  /** In-flight new-page wait (null when none). A create navigates the preview
   *  to the page's build-independent URL before any build has produced it, so
   *  the URL serves the synthetic 404; this covers that window with the one
   *  waiting surface. */
  private newPageWait: NewPageWaitState = null;
  /** Pending deadline for the armed new-page wait. */
  private newPageWaitTimer: number | null = null;
  /** A deadline changes the wait to a recoverable stalled state, never a 404. */
  private static readonly PAGE_PENDING_TIMEOUT_MS = 20_000;
  /** Pathname of the wait most recently REPLACED by a newer one; see
   *  `decideNewPageWait`'s `superseded` rule. */
  private supersededPageWaitPath: string | null = null;
  /** True between the first `uploading` event and the next complete/failed —
   *  lets the syncing→uploading transition reset the bar exactly once. */
  private uploadingStarted = false;

  /** A build pass is mid-flight (set/cleared by handleProgressUpdate). */
  private buildPassActive = false;

  /** A machine-scoped download currently borrows the hairline (facade below). */
  private downloadPassActive = false;
  /** The overlay's blueprint grid — one per overlay, not one per screen; see
   *  screen/overlay-slot.ts. */
  private readonly slot = new OverlaySlot();
  private progressPanel: ProgressPanel | null = null;
  /** The rebuild hairline breathes on the nav pill while a rebuild runs. */
  private readonly hairline = new PreviewHairline();
  /**
   * The seam hit-band serves two tellers, never both at once: panel content
   * (hover = peek, the panel narrates) or an ambient download's label (hover =
   * tooltip, there is no panel behind it). These two fields are the inputs;
   * syncSeamSurface() is the single reconciler that maps them onto the seam's
   * visibility + tooltip so no path can strand either.
   */
  private panelContentPresent = false;
  private ambientTooltip: string | null = null;
  /**
   * Background tasks that finish what a page shows (`PAGE_FINISHING_TASKS`)
   * still in flight. The blocking build finishing (percentage=100) does not
   * mean the page is ready: image webp, video transcode and a document's text
   * run in the BACKGROUND after. While any is running the hairline keeps
   * advancing and the blocking-done dissolve is deferred until every one
   * reports completed. The dissolveAfter timer armed at blocking-done stays as
   * the backstop for a dropped `completed:true` tick.
   */
  private readonly bgPageTasksInFlight = new Set<string>();
  /**
   * The persistent seam owns the quiet resting line and advisory dot on the
   * pill-sibling titlebar layer. Both
   * survive the panel's hide() and teardown (minimize ≠ forget). The transient
   * `hairline` above OVERLAYS this during a build; this one is declarative (no
   * timers). Its dot is driven solely by the panel's advisory-presence callback
   * (the panel is the single owner of "is there an advisory"). */
  private readonly seam = new ProgressSeam();
  /** Timestamp of the last refresh() that actually ran (for double-fire coalescing). */
  private lastRefreshAt = 0;
  /** A terminal build can finish before the first bridge has loaded. */
  private refreshAfterFirstNavigation = false;
  /**
   * Trailing-edge follow-up for a refresh() call coalesced away by
   * REFRESH_COALESCE_MS. At most one is ever pending — see
   * `refresh()` for why a wall-clock window must not silently drop a call.
   */
  private trailingRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  private pendingTargetFile: string | null = null;
  /** Preserve bridge navigation order across asynchronous Tauri invokes. */
  private previewRequirementDispatch: Promise<void> = Promise.resolve();
  /** Invalidates queued calls when this manager changes folder or closes. */
  private previewRequirementScope: object = {};
  /**
   * Fired on a *completed* build whose folder is empty (`is_empty === true`).
   * The app shell wires this to auto-open the editor in folder-mode (the
   * onboarding surface) — the preview itself just shows the bare
   * empty-home artifact (no overlay). See onboarding design Principles 4 + 6.
   */
  private onEmptyFolder: (() => void) | null = null;
  /**
   * Escape key handler (WCAG 2.1.1) reference stored so we can
   * remove the listener on destroy() without leaking. Wires document keydown →
   * panel.onEscapeKey() which collapses a pinned panel to minimized.
   */
  private escapeKeyHandler: ((e: KeyboardEvent) => void) | null = null;

  /**
   * Did *this* build raise the gate? Distinct from the store's `gateUp`, which
   * is sticky across builds. A build that stops on a file the cloud withheld emits
   * `home_waiting` and then rejects — `build::pipeline::run` reads the deferred
   * verdict off `BuildStopped` and calls
   * `cloud_readiness::raise_gate_for_a_deferred_build`. Both signals
   * describe the same build, so the `'error'` branch below uses this one to
   * pick the screen. Reset at the start of every build, so a stale gate can
   * never mask a new, genuine failure.
   */
  private homeGateRaisedThisBuild = false;
  /** Keeps a folder-open from launching two rival pipelines. */
  private readonly buildSingleflight = new BuildSingleflight();
  /**
   * Unsubscribe from `cloudSync`; removed in destroy(). The provider/counts
   * copies and the folder filter this used to keep locally now live once in
   * `cloud-sync-store.ts` — see its module doc for why that consolidation
   * happened (two surfaces each deriving "is this news?" from their
   * own copy of the same event could disagree with each other).
   */
  private homeGateUnlisten: (() => void) | null = null;
  /**
   * The one full-window "nothing to look at" surface — renders whichever
   * `UnshowableReason` `resolveUnshowableReason` names, plus the nested-site
   * walk's live count and every plain notice moss has to put up.
   * `revealFolder`/`chooseFolder` are shared across every reason that offers
   * them: a folder that is unreachable and a folder that is merely slow both
   * point the user at the same two exits. A host without one of these exits
   * gets a screen without its button.
   */
  private readonly waitingScreen = new FullWindowScreen({
    revealFolder: host.reveal && (() => {
      if (!this.folderPath) return;
      // Best effort: the folder itself may be gone (the `unreachable` case),
      // but Finder can often still land the user near where it was.
      host.reveal?.(this.folderPath).catch((err) => {
        error(`❌ [PreviewManager] Waiting-screen reveal-in-finder failed: ${err}`);
      });
    }),
    retryOpen: () => {
      if (this.folderPath) void this.startBuild(this.folderPath);
      else if (this.failedOpenPath) void host.launcher?.openRecent(this.failedOpenPath);
    },
    sendLogs: host.sendLogs && (() => void host.sendLogs?.()),
    retryPage: () => {
      const wait = this.newPageWait;
      if (!wait) return;
      this.armNewPageWait(wait.pathname);
      this.loadPreview(wait.targetUrl);
      if (this.folderPath) {
        const request = this.navigationManager.getRequestRevision();
        void this.startBuild(this.folderPath).then(() => {
          if (request === this.navigationManager.getRequestRevision() && this.newPageWait) {
            this.loadPreview(this.newPageWait.targetUrl);
          }
        });
      }
    },
    chooseFolder: host.launcher && (() => {
      host.launcher?.show().catch((err) => {
        error(`❌ [PreviewManager] Waiting-screen show-launcher failed: ${err}`);
      });
    }),
    // The walk's escape is an ANSWER, not a navigation: the backend is parked
    // on this question, so abandoning it has to submit a cancel or the guard
    // waits for a screen that is gone.
    cancelChecking: () => this.threshold.decide({ kind: 'cancel' }),
  });
  /** Re-entrancy guard; see the null-transition branch in `applyUnshowableReason`. */
  private renderingStatus = false;

  /** Unsubscribe from `FolderHealthChanged`; removed in destroy(). */
  private folderHealthUnlisten: (() => void) | null = null;
  /**
   * The sweep's verdict: the vault root has been unreadable for
   * consecutive sweep passes, no rebuilds run, and the preview underneath may
   * be arbitrarily stale. Rank 1 in `resolveUnshowableReason` — it outranks
   * every other reason, the cloud gate included.
   */
  private folderUnreachable = false;
  /**
   * The ladder's last answer — `null` means a site is served and the window
   * belongs to it. Tracked here (not re-derived per event) so every listener
   * that can change an input reconciles through the same method,
   * `reconcileUnshowable`, and the raised/lowered edges are computed exactly
   * once no matter which input changed (one level up: now
   * there is only one screen to raise or lower).
   */
  private unshowableReason: UnshowableReason | null = null;

  /** True while the threshold checking narration is borrowing the ambient
   *  hairline channel, so ending it never tears down someone else's pass. */
  private thresholdNarrating = false;
  /** The nested-site threshold pass: guard events → screen in the waiting
   *  screen's overlay slot → one decision back (threshold/threshold-pass.ts
   *  owns the lifecycle; transport subscribed in `initialize()`). */
  private readonly threshold = new ThresholdPass({
    raiseOverlay: () => this.raiseOverlay(),
    clearOverlay: () => this.clearOverlay(),
    getStage: () => this.state.getStage(),
    setStage: (stage) => this.state.setStage(stage),
    dropWaitingScreen: () => this.waitingScreen.drop(),
    showChecking: (dirsVisited) => {
      const overlay = this.raiseOverlay();
      if (overlay) {
        this.waitingScreen.show(overlay, {
          kind: 'checking',
          visited: dirsVisited,
          ...this.vaultLocationFields(),
        });
      }
    },
    onError: (message) => error(`❌ [PreviewManager] ${message}`),
    // Only a host with a nested-site guard sends the events that ask these.
    submitDecision: (decision, token) => host.nestedSiteGuard?.decide(decision, token) ?? NO_GUARD,
    acknowledgePrompt: (token) => host.nestedSiteGuard?.acknowledge(token) ?? NO_GUARD,
    // Routes the checking narration into the hairline advisory channel
    // ("the hairline answers what is moss doing right now"). Indeterminate on
    // purpose: a walk has no fraction, and a fabricated one would be a bar
    // that lies.
    narrate: (text) => {
      if (text !== null) {
        if (this.thresholdNarrating) {
          this.setAmbientDownloadProgress(null, text);
        } else {
          this.thresholdNarrating = this.beginAmbientDownload(text);
        }
      } else if (this.thresholdNarrating) {
        this.thresholdNarrating = false;
        this.endAmbientDownload(true);
      }
    },
  });

  constructor(iframeId: string = "moss-preview-iframe") {
    // Initialize pure state machine
    this.state = new PreviewState();

    // Get iframe element
    this.iframe = document.getElementById(iframeId) as HTMLIFrameElement | null;
    if (!this.iframe) {
      warn(`⚠️ [PreviewManager] Iframe with id "${iframeId}" not found`);
    }

    // Get overlay element for progress/status display
    this.overlay = document.getElementById('moss-preview-overlay');
    // index.html paints "Starting moss" before any script; from here on it is
    // the screen's own state, so it leaves the way every other state does.
    if (this.overlay?.querySelector('.moss-waiting-screen')) {
      this.waitingScreen.show(this.raiseOverlay()!, { kind: 'starting' });
    }

    this.iframe?.addEventListener('load', () => {
      const dropping = this.firstPaint === 'drop-on-load';
      this.firstPaint = 'done';
      if (dropping) this.hideOverlay();
    });

    // Initialize zoom controller (reads persisted factor from localStorage).
    this.zoom = new PreviewZoomController(this.iframe);

    // Create progress panel in the preview container
    // Skip progress panel during E2E tests to prevent z-index blocking of modals
    const previewContainer = document.getElementById('moss-preview-container');
    if (previewContainer) {
      // Transient zoom feedback; shown by adjustZoom on every zoom change.
      this.zoomBadge = new ZoomBadge(previewContainer, () => this.adjustZoom(0));
    }
    if (previewContainer && !import.meta.env.E2E_TEST_MODE) {
      this.progressPanel = new ProgressPanel(previewContainer, this.iframe);
      // Mount the persistent seam on the titlebar layer (a no-op when
      // the chrome isn't present, e.g. test mode), and let the panel — the SINGLE
      // owner of advisory presence — drive the at-rest advisory dot. The seam
      // keeps no copy of the advisory store; it's told whether one exists.
      this.seam.mount();
      // Top-severity drives the seam dot color.
      this.progressPanel.onTopSeverityChange((severity) => this.seam.setTopSeverity(severity));
      // The full-window screen carries the build's stage line, and the panel
      // is where that sentence comes from. While a screen is up the panel
      // renders nothing, so this is the only route the line has to the author
      // — without it "Preparing this page" says nothing else for the whole
      // build, which is LESS than the corner panel showed before the screen
      // existed.
      this.progressPanel.setOnStageLabelChanged(() => {
        if (this.unshowableReason !== null && !this.renderingStatus) {
          this.reconcileUnshowable();
        }
      });
      // The seam (quiet line + reveal hit-region) shows ONLY when there's
      // content (a build/job/advisory) or a borrowed ambient pass — never on an
      // idle pill. A persistent green line reads as "stuck progress", and an
      // always-present reveal handle has no panel behind it at rest. Routed
      // through the single reconciler so the ambient-download tooltip cannot
      // land on a display:none band.
      this.progressPanel.onContentChange((present) => {
        this.panelContentPresent = present;
        this.syncSeamSurface();
      });
      // The seam emits reveal inputs (hover/click); the panel is the
      // SINGLE owner that maps each input to a RevealState transition (hover →
      // peek after the open-delay, leave → minimized after the close-delay,
      // click/Enter → surfaced). Pin is the handle button only (▾/▴).
      // The seam holds no visibility state.
      const panel = this.progressPanel;
      // The content guard lives HERE, not in the panel: the band can be
      // visible for an ambient download alone (its tooltip is the teller
      // there), and blooming a contentless glass box under that same hover
      // would be the tooltip+popover double-fire. The panel's reveal
      // mechanics stay unconditional; the manager owns the two-teller rule.
      this.seam.setCallbacks({
        onHoverEnter: () => { if (this.panelContentPresent) panel.onSeamHoverEnter(); },
        onHoverLeave: () => panel.onSeamHoverLeave(),
        onClick: () => { if (this.panelContentPresent) panel.onSeamClick(); },
      });

      // Wire Escape → panel.onEscapeKey() to collapse
      // a pinned panel. Stored in a field so destroy() can remove it.
      this.escapeKeyHandler = (e: KeyboardEvent) => {
        if (e.key === "Escape") panel.onEscapeKey();
      };
      document.addEventListener("keydown", this.escapeKeyHandler);
    }

    // Initialize navigation manager with iframe reference
    this.navigationManager = new NavigationManager(this.iframe);
    this.instruments = bindPreviewInstruments(this.navigationManager, this.iframe, () => {
      const file = this.navigationManager.getActivePage()?.sourceFile;
      return file && this.folderPath ? toRelative(file, this.folderPath) : null;
    });

    // The current request owns any covered generation-publication rechecks.
    this.navigationManager.setNavigationResultListener((info) =>
      this.onNavigationResult(info)
    );
    this.navigationManager.setPreviewRequirementListener((urlPath) => {
      const folder = this.folderPath;
      if (!folder) return; // the native open path already seeded cold targets
      const scope = this.previewRequirementScope;
      const request = this.navigationManager.getRequestRevision();
      this.previewRequirementDispatch = this.previewRequirementDispatch.then(async () => {
        if (scope !== this.previewRequirementScope || folder !== this.folderPath || request !== this.navigationManager.getRequestRevision()) return;
        const result = await host.previewRequirement?.(folder, urlPath);
        if (result?.status === "error") warn(`[Preview] Could not prioritize requested page: ${result.error}`);
      }).catch((err) => {
        warn(`[Preview] Could not prioritize requested page: ${err}`);
      });
    });

    // Re-apply zoom whenever the device-toggle pill changes mode.
    // adjustZoom reads isMobilePreview() fresh from the navigation manager so
    // the correct transform branch (desktop: scale+origin:0 0; mobile: translate+scale)
    // is always used. No step is applied (delta=0 would reset) — we call
    // setMobileMode() on the zoom controller directly to re-apply current factor.
    this.navigationManager.setDeviceModeChangeListener((isMobile) => {
      this.zoom?.setMobileMode(isMobile);
    });
  }

  /**
   * Forget the waiting screen, because its markup is about to be written over.
   * Every path that replaces the overlay's contents calls this: keys left set
   * behind a screen that is gone make the next tick with the same numbers a
   * no-op, so it never comes back — while the gate suppresses every render.
   */
  private dropWaitingScreen(): void {
    this.waitingScreen.drop();
    // Same contract for the threshold surface: its re-render guard would
    // otherwise suppress the next legitimate show.
    this.threshold.clear();
  }

  // ── Nested-site threshold (pre-build for the folder being opened; nothing
  //    is written until the user commits). Lifecycle and stage bookkeeping
  //    live in threshold/threshold-pass.ts; these delegates keep the public
  //    surface where callers and tests already look. ──

  /** Detection narration: live folder count on the overlay, mirrored to the
   *  hairline advisory. Safe to call on every progress tick. */
  showThresholdChecking(dirsVisited: number, opts: { guard?: boolean } = {}): void {
    this.threshold.showChecking(dirsVisited, opts);
  }

  /** The threshold question — cases (a)/(b)/(d). */
  showThresholdPrompt(
    prompt: ThresholdPrompt,
    stillChecking = false,
    opts: { guard?: boolean } = {},
  ): void {
    this.threshold.showPrompt(prompt, stillChecking, opts);
  }

  /** Decision made (or detection found nothing): the slot goes back to the
   *  ordinary build lifecycle. When nothing was found the checking screen
   *  simply continues into the build's loading overlay — no interstitial
   *  flash when the preview becomes ready. */
  clearThresholdScreen(): void {
    this.threshold.clear();
  }

  /** The guard pass ended (`ThresholdResolved`); see `ThresholdPass.close`. */
  closeThresholdPass(cancelled: boolean): void {
    this.threshold.close(cancelled);
  }

  /**
   * Hide the overlay, revealing the iframe beneath.
   *
   * Refuses while a ladder reason still holds the window. The screen and
   * `unshowableReason` are one fact stated twice, and only the ladder changes
   * it: a navigation UNDERNEATH a raised screen — the new-page wait's own
   * recheck reload, a rename retry, a cache-busting refresh during an error —
   * is not a decision that the reason is over. It used to be treated as one,
   * leaving the state saying "screen up" with nothing on screen and no
   * repaint in sight (a finished rebuild emits no fresh `ready` status, so no
   * reconcile follows).
   *
   * The reason's own fall goes through `applyUnshowableReason(null)`, which
   * clears the field BEFORE calling here, so ordinary teardown is unaffected.
   */
  private hideOverlay(): void {
    if (!this.overlay || this.screenHold.active) return;
    if (this.unshowableReason !== null) return;
    if (this.firstPaint !== 'done') {
      this.firstPaint = 'drop-on-load';
      return;
    }
    markStartup("main", "ready");
    this.dropWaitingScreen();
    this.clearOverlay();
  }

  private raiseOverlay(): HTMLElement | null {
    return this.overlay ? this.slot.raise(this.overlay) : null;
  }

  private clearOverlay(): void {
    this.slot.clear(this.overlay);
  }

  /**
   * Recompute `resolveUnshowableReason` and repaint if the answer changed.
   *
   * The single reconciler: every listener that can move one of the ladder's
   * inputs — the folder-health sweep, the cloud-sync store, a preview-state
   * transition, the build-stage readout — calls this instead of deciding for
   * itself, so the raised/lowered edges (the progress panel's silence, the
   * unreachable editor close) are computed exactly once no matter which input moved.
   */
  private reconcileUnshowable(): void {
    if (this.screenHold.active) return;
    // Once an error rank takes the window, it stays until a NEW PreviewStatus
    // arrives — routed through `renderStatusInner`'s own 'error'/'loading'/
    // 'ready' cases, which compute a fresh reason and call
    // `applyUnshowableReason` directly. A cloud tick or folder-health tick
    // reaching THIS reconciler must not silently replace or clear a message
    // the user has not been given a new status to explain — that is what
    // clearing `previewBlank` in `clearHomeGate` arms otherwise (a later
    // download tick would resolve 'downloading' and paint over "Address
    // already in use"). The one exception is rank 1: the folder can still go
    // unreachable out from under a displayed error, and that outranks
    // everything.
    if (
      (this.unshowableReason === 'error-hard' || this.unshowableReason === 'error-soft') &&
      !this.folderUnreachable
    ) {
      return;
    }
    const cloud = cloudSync.getState();
    const status = this.state.getStatus();
    const reason = resolveUnshowableReason({
      unreachable: this.folderUnreachable,
      gateUp: cloud.gateUp,
      previewBlank: cloud.previewBlank,
      errorVerdict: status.type === 'error' ? status.verdict : null,
      pagePending: this.pagePendingActive(),
      blocking: cloud.blocking,
      stalled: cloud.stalled,
      buildStageLabel: this.progressPanel?.buildStageKnownLabel() ?? null,
    });
    this.applyUnshowableReason(reason, cloud, status);
  }

  /**
   * Apply an already-decided reason (the ladder's ordinary path is
   * `reconcileUnshowable`; the error branch of `renderStatusInner` computes
   * its own reason first — see the comment there — and hands it here).
   */
  private applyUnshowableReason(
    reason: UnshowableReason | null,
    cloud: ReturnType<typeof cloudSync.getState>,
    status: PreviewStatus,
  ): void {
    const prev = this.unshowableReason;
    this.unshowableReason = reason;

    // Shown/dismissed logging, keyed on the reason actually changing (a
    // reapplication of the same reason is not a new occurrence). Covers all
    // three transition shapes: null→reason (shown only), reason→null
    // (dismissed, endedBy=resolved), reason→otherReason (dismissed with
    // endedBy=otherReason, then shown otherReason).
    if (prev !== reason) {
      if (prev !== null) {
        const durationMs = this.waitingScreenShownAt !== null ? Date.now() - this.waitingScreenShownAt : null;
        void info(
          `[waiting-screen] dismissed reason=${prev} duration_ms=${durationMs ?? 'unknown'} ended_by=${reason ?? 'resolved'}`,
        );
      }
      if (reason !== null) {
        this.waitingScreenShownAt = Date.now();
        void info(`[waiting-screen] shown reason=${reason}`);
      } else {
        this.waitingScreenShownAt = null;
      }
    }

    // The waiting screen paints the preview region only (`#moss-preview-overlay`
    // is inset by `--action-panel-width`) and the editor is a separate native
    // webview beside it, so no reason here closes the editor: the panel
    // belongs to the editing session. The one exception is
    // `unreachable`, where an editor left open would keep auto-saving into a
    // folder whose root doesn't stat.
    if (reason === 'unreachable' && prev !== 'unreachable') {
      void info('[editor] closed by preview: folder unreachable');
      host.editorPanel?.close().catch(() => {});
    }
    // The corner panel anchors above `--z-toast` (200), higher than this
    // screen can go without outranking real notifications, so one already
    // open would sit on top of it — closing it is `setScreenUp`'s job on the
    // rising edge. Outside that it stays silent while a reason holds the
    // window and speaks for itself once it doesn't.
    this.progressPanel?.setScreenUp(reason !== null);
    // Same edge, narrower condition: only a WAIT queues toasts. An error
    // screen isn't one — it can hold the window indefinitely and the author
    // is already looking at the app — so entering one flushes instead.
    setOverlayUp(reason !== null && reason !== 'error-hard' && reason !== 'error-soft');

    if (reason === null) {
      // The falling edge can arrive from OUTSIDE `renderStatus` — a pure
      // cloud tick (`home_ready` clearing a sticky gate, say) with no new
      // PreviewStatus of its own. `renderStatusInner`'s own ranks handle
      // their own fall (e.g. 'ready' calls `loadPreview` right after this
      // reconcile), so only re-invoke it here when this reconcile is NOT
      // already running inside one — otherwise the site the gate was
      // covering never gets `loadPreview`d, because the 'ready' paint that
      // would have done it was suppressed by the gate at the time.
      if (prev !== null) {
        this.hideOverlay();
        // `page-pending` falls when the page it was waiting for arrived, and
        // the preview is already sitting on it. Re-rendering the status would
        // run the 'ready' case's `loadPreview(serverRoot)` and pull the
        // preview back to the home page — the exact home-jump
        // PreviewFollower's no-'/'-fallback rule exists to prevent.
        // A bridge-echoed target is already in the iframe. Re-loading the
        // server root after its exact seal clears the gate would jump away
        // from the requested article or generated index.
        const targetAlreadyShown = prev === 'gate' && this.navigationManager.getActivePage() !== null;
        if (targetAlreadyShown && !this.renderingStatus) {
          this.state.setStage('ready');
          this.progressPanel?.setStage('ready');
        }
        if (!this.renderingStatus && prev !== 'page-pending' && !targetAlreadyShown) {
          this.renderStatus(this.state.getStatus());
        }
      }
      return;
    }
    if (!this.overlay) return;
    if (reason === 'error-hard' || reason === 'error-soft') {
      // A failure is not a wait, but it IS this window with nothing in it:
      // same screen, with why and the ways on.
      this.showFailure(status.type === 'error' ? status.message : '');
      return;
    }
    const overlay = this.raiseOverlay()!;
    // Keyed on the CONTENT, not the reason. Rank 4 yields its pixels to a
    // real download (see `unshowableContentFor`), and that screen is a
    // full-window one: it carries a meter and "Reveal folder"/"Choose folder"
    // buttons, and the right control column sat clickable on top of them —
    // exactly the regression layout-zones I1c fixed. What opts out of owning
    // the window is the "Preparing this page" screen itself, which now says so
    // in its own spec — this call just names the content.
    this.waitingScreen.show(overlay, this.unshowableContentFor(reason, cloud));
  }

  /**
   * What the screen actually draws for a given reason. `unreachable` has its
   * own visual; every other reason — `gate` included — draws whatever is
   * really happening underneath it (a blocking download, the build's own
   * stage, or nothing yet): `gate` has no visual of its own, so its content
   * is the ladder's lower half re-run with `unreachable`/`gateUp` suppressed.
   */
  private unshowableContentFor(
    reason: UnshowableReason,
    cloud: ReturnType<typeof cloudSync.getState>,
  ): ScreenContent {
    if (reason === 'unreachable') return { kind: 'unreachable' };
    const buildStageLabel = this.progressPanel?.buildStageKnownLabel() ?? null;
    const sub = reason === 'gate' || reason === 'page-pending'
      ? resolveUnshowableReason({
          unreachable: false,
          gateUp: false,
          previewBlank: true,
          errorVerdict: null,
          pagePending: false,
          blocking: cloud.blocking,
          stalled: cloud.stalled,
          buildStageLabel,
        })
      : reason;
    // `page-pending` outranks the cloud rungs by position, but a real download
    // is both truer and more actionable than "preparing this page" — so it
    // yields to one, and otherwise says its own sentence with the build's
    // stage line underneath. A bare `building` does NOT displace it: naming
    // the page the author just made is the more specific truth, and both are
    // true at once.
    if (reason === 'page-pending' && sub !== 'stalled' && sub !== 'downloading') {
      return { kind: 'page-pending', stageText: buildStageLabel ?? '', stalled: this.newPageWait?.stalled };
    }
    switch (sub) {
      case 'stalled':
        return { kind: 'stalled', downloaded: cloud.downloaded, total: cloud.total, provider: cloud.provider };
      case 'downloading':
        return {
          kind: 'downloading',
          downloaded: cloud.downloaded,
          total: cloud.total,
          provider: cloud.provider,
          ...this.vaultLocationFields(),
        };
      case 'building':
        return { kind: 'building', stageText: buildStageLabel ?? '', ...this.vaultLocationFields() };
      default:
        return { kind: 'opening', ...this.vaultLocationFields() };
    }
  }

  /**
   * The wait/loading screen's title + location-line inputs, read off the
   * fields `runBuild` set alongside `folderPath` itself. `undefined` (not
   * `null`) for the two strings when absent, so `ScreenContent`'s optional
   * fields are simply missing rather than present-but-null — `specFor`'s
   * `nameTitle` falls back to the old generic title on a missing name.
   */
  private vaultLocationFields(): {
    folderName?: string;
    parentDisplay?: string;
    cloudProviderLabel?: string | null;
  } {
    return {
      folderName: this.folderName ?? undefined,
      parentDisplay: this.parentDisplay ?? undefined,
      cloudProviderLabel: this.cloudProviderLabel,
    };
  }

  /**
   * Expose state for testing purposes.
   */
  getState(): PreviewState {
    return this.state;
  }

  /**
   * Register a callback fired when a completed build reports an empty
   * folder. The app shell uses this to auto-open the editor in
   * folder-mode (the onboarding surface). Idempotency / "don't reopen if
   * already open" is the caller's responsibility.
   */
  setOnEmptyFolder(cb: () => void): void {
    this.onEmptyFolder = cb;
  }

  /**
   * Navigate the preview to a specific page by URL path.
   * Used for Editor → Preview sync when the user selects a file in the editor tree.
   */
  navigateToPage(urlPath: string): void {
    this.navigationManager.navigateToPage(urlPath);
  }

  /**
   * Say that the navigation about to happen is to a page the author has only
   * just created, so the build that produces it cannot have run yet.
   *
   * Must be called BEFORE `navigateToPage`. The screen goes up optimistically
   * — not on the bridge's first `notFound` echo — because by the time that
   * echo arrives the 404 has already painted, and the paint is the bug. That
   * is safe only because the callers are the build-INDEPENDENT navigations,
   * which exist precisely because the page is known not to be there yet.
   *
   * The wait ends when the page serves, a new navigation replaces it, or
   * a reported build error takes the window.
   */
  armNewPageWait(urlPath: string): void {
    const targetUrl = this.navigationManager.resolveFullUrl(urlPath);
    if (!targetUrl) return; // no iframe origin yet — nothing to wait on
    let pathname: string;
    try {
      pathname = new URL(targetUrl).pathname;
    } catch {
      return;
    }
    // Not under a message the author has to read. An error rank is sticky
    // until a new PreviewStatus arrives, so `reconcileUnshowable` would raise
    // nothing — but the wait would still be armed underneath, suppressing the
    // smart-diff refresh and reloading the preview behind a
    // screen that says the build failed.
    if (this.unshowableReason === 'error-hard' || this.unshowableReason === 'error-soft') {
      return;
    }
    const superseded = this.newPageWait;
    this.clearNewPageWaitTimer();
    this.clearPageRetryTimer();
    // Remember only a wait for a DIFFERENT page: re-arming the same pathname
    // (two creates of the same file) has no stale echo to distinguish.
    this.supersededPageWaitPath =
      superseded && superseded.pathname !== pathname ? superseded.pathname : null;
    this.newPageWait = { pathname, targetUrl, rechecks: 0 };
    this.newPageWaitTimer = window.setTimeout(
      () => {
        this.newPageWaitTimer = null;
        if (!this.newPageWait) return;
        this.newPageWait = { ...this.newPageWait, stalled: true };
        this.reconcileUnshowable();
      },
      PreviewManager.PAGE_PENDING_TIMEOUT_MS,
    );
    this.reconcileUnshowable();
  }

  /** Reveal a served or superseded page without an artificial delay. */
  private endNewPageWait(): void {
    if (this.newPageWait === null) return;
    this.cancelNewPageWait();
    this.reconcileUnshowable();
  }

  /** Drop the wait without repainting, for callers about to paint anyway. */
  private cancelNewPageWait(): void {
    this.newPageWait = null;
    this.clearNewPageWaitTimer();
    this.clearPageRetryTimer();
    this.supersededPageWaitPath = null;
  }

  private pagePendingActive(): boolean {
    return this.newPageWait !== null;
  }

  private clearNewPageWaitTimer(): void {
    if (this.newPageWaitTimer !== null) {
      clearTimeout(this.newPageWaitTimer);
      this.newPageWaitTimer = null;
    }
  }

  /** Scroll the previewed page's footer into view and flash it — the answer to
   *  "I opened footer.md and nothing happened". */
  revealFooter(): Promise<void> {
    return this.navigationManager.revealFooter();
  }

  /** Scroll one element of the previewed page into view and flash it — the
   *  answer to "where does this frontmatter field end up?". Same primitive the
   *  footer reveal uses; a selector that matches nothing is a no-op. */
  revealSource(selector: string): Promise<void> {
    return this.navigationManager.revealSource(selector);
  }

  /** Flash every element `selector` matches WITHOUT scrolling — the hover
   *  strength of {@link revealSource} (chip hover → preview flash). */
  flashSource(selector: string): Promise<void> {
    return this.navigationManager.flashSource(selector);
  }

  /** Hover-out half of {@link flashSource}. */
  clearFlashSource(selector: string): Promise<void> {
    return this.navigationManager.clearFlashSource(selector);
  }

  /** Defer the footer reveal to the next navigation echo. Call before
   *  `navigateToPage()` when the footer to reveal is on another page. */
  armFooterReveal(urlPath: string): void {
    this.navigationManager.armFooterReveal(urlPath);
  }

  /** Pre-register an expected echo before initiating an editor-driven navigation.
   *  Must be called before `navigateToPage()` so the bounce-back moss-navigation
   *  is recognized as an echo acknowledgment, not a new user navigation.
   *  No-op if the URL cannot be resolved (iframe not yet mounted).
   */
  markPendingEcho(urlPath: string): void {
    const fullUrl = this.navigationManager.resolveFullUrl(urlPath);
    if (fullUrl) {
      this.navigationManager.markEditorNavigation(fullUrl);
    }
  }

  /**
   * Scroll the preview to the source line for editor-to-preview sync.
   * When atTop is true, the bridge short-circuits to scrollTop=0; when
   * atBottom is true it short-circuits to the preview's max scroll. In both
   * cases the line / fallbackAnchorId are informational only.
   */
  scrollToSourceLine(line: number, fallbackAnchorId: string | null, atTop = false, atBottom = false, onlyIfOffscreen = false): void {
    this.navigationManager.scrollToSourceLine(line, fallbackAnchorId, atTop, atBottom, onlyIfOffscreen);
  }

  /** Where the preview is right now, for an editor that has to follow it. */
  readScrollPosition(): Promise<{ line: number; atTop: boolean; atBottom: boolean } | null> {
    return this.navigationManager.readScrollPosition();
  }

  /**
   * Cache editor scroll position for post-rebuild restoration.
   * atTop / atBottom are cached so the replay path uses the same authoritative
   * edge signals as the original emit, rather than re-deriving them from the
   * line number.
   */
  setLastEditorPosition(line: number, anchorId: string | null, atTop = false, atBottom = false): void {
    this.navigationManager.setLastEditorPosition(line, anchorId, atTop, atBottom);
  }

  /** Reclaim scroll ownership for the editor. Call before any editor-driven scrollToSourceLine. */
  claimScrollForEditor(): void {
    this.navigationManager.claimScrollForEditor();
  }

  /**
   * Check if preview is in mobile mode
   */
  isMobilePreview(): boolean {
    return this.navigationManager.isMobilePreview();
  }

  /**
   * Adjust preview zoom by a discrete step.
   *
   * delta: +1 = zoom in, -1 = zoom out, 0 = reset to 100%.
   *
   * The zoom is a CSS transform: scale() applied to the <iframe> element.
   * The toolbar chrome (pill, publish button) lives in the shell and is NOT
   * affected. The per-site Aa font-scale pill (moss-font-scale) is a separate
   * layer inside the page CSS and is independent of this zoom.
   *
   * Persisted to localStorage['moss-preview-zoom'] across sessions.
   */
  adjustZoom(delta: number): void {
    // Sync mobile mode before stepping so applyZoomToIframe picks the right transform.
    this.zoom?.setMobileMode(this.isMobilePreview());
    this.zoom?.step(delta);
    // Show the level even when clamped at a boundary — "still 150%" is the
    // feedback that explains why the keystroke changed nothing.
    if (this.zoom) this.zoomBadge?.show(this.zoom.getFactor());
  }

  /**
   * Get the current preview URL path
   * Used by deploy handler for single-article syndication
   */
  getCurrentUrl(): string | null {
    return this.navigationManager.getCurrentUrl();
  }

  /** Get the canonical active page (URL + sourceFile) for identity validation. */
  getActivePage(): ActivePage | null {
    return this.navigationManager.getActivePage();
  }

  /**
   * Initialize preview system, and start the build if the folder is known.
   *
   * A window opened with its folder carries it in the init script
   * (`__MOSS_BUILD_CONFIG__`). A window opened before its folder was ready
   * (Rust shows it at once, on the shell script alone) keeps the "Starting
   * moss" screen until `config-ready` brings the folder. Both can reach a
   * page that has both; the build's singleflight makes that one build.
   */
  async initialize(): Promise<void> {
    // Setup file change event listener for live development
    this.setupFileChangeListener();

    // Initialize progress panel event listeners
    await this.progressPanel?.init();

    // Setup config-ready event listener (primary mechanism for first run)
    await this.setupConfigReadyListener();

    // Listen for navigate-to-target from backend (same-folder, skip-recompile fast path)
    await host.engine.on<{ target_file: string }>('navigate-to-target', (payload) => {
      info(`[PreviewManager] Navigate to target: ${payload.target_file}`);
      this.navigateToTarget(payload.target_file);
    });

    // Listen for backend completion signal
    // Maximum Decoupling: Backend owns completion detection, frontend just displays.
    // BuildComplete fires NO toast — videos_converted is a routine ambient
    // success; symlink-skip warnings surface on the hairline dot via a
    // BackgroundProgress advisory emitted by the Rust backend (pipeline.rs).
    await onMossEvent("BuildComplete", (_msg) => {
      // Reload preview to pick up background-copied assets (images, HTML, JS)
      if (this.newPageWait) this.handleFileChangeEvent(null);
      else this.refresh('build-complete');
    });

    // Bug 1: drive the nav-pill hairline from the media-conversion BACKGROUND
    // stream (image webp / video transcode run after the blocking build) so the
    // bar keeps advancing while they run and only dissolves once they finish.
    await onMossEvent("BackgroundProgress", (msg) => {
      this.handleBackgroundProgress(msg.payload);
    });

    // Recorded, not rendered: the panel owns the detail. This is the evidence
    // separating a real server failure from a rebuild that never started one;
    // see `noteServerFailed`.
    await onMossEvent("PreviewServerFailed", (m) => this.state.noteServerFailed(m.payload.message));

    // Drive the nav-pill hairline from DEPLOY upload progress.
    await this.setupDeployHairlineListener();

    // Cloud-download home-page gate: full-window waiting screen
    // while the site's home page source is still a cloud-dataless
    // placeholder. Subscribes to `cloudSync` — the single owner of the
    // `icloud-sync` event, shared with `ProgressPanel`'s per-file provider
    // progress — and early-returns on the progress phases so the two never
    // fight; see `cloud-sync-store.ts`'s module doc.
    await this.setupHomeGateListener();

    // The progress panel's Download button asks for a rebuild over the bus.
    this.rebuildUnsub?.();
    this.rebuildUnsub = eventBus.on(Events.REBUILD_OPEN_FOLDER, () => {
      if (this.folderPath) void this.startBuild(this.folderPath);
    });

    // Nested-site threshold: the guard's question events (folder switch —
    // the one case where this webview exists while a guard runs).
    await this.threshold.start();

    // Folder health: the sweep's project-unavailable verdict →
    // full-window screen; transitions only, so the clear edge repaints.
    await this.setupFolderHealthListener();

    host.onOpenFailure?.((failed) => {
      this.failedOpenPath = failed.folder_path;
      this.showFailure(failed.detail, failed.folder_name);
    });
    const config = host.openedFolder?.() ?? null;
    if (config) {
      await this.startBuild(
        config.folder_path,
        config.auto_serve,
        config.target_file ?? undefined,
        config.folder_name,
        config.parent_display,
        config.cloud_provider_label,
      );
    }
  }

  /**
   * Setup listener for config-ready event from backend.
   * This is the primary mechanism for receiving config on first run,
   * fixing the race condition where polling starts before init script executes.
   */
  private async setupConfigReadyListener(): Promise<void> {
    try {
      const unlisten = await host.engine.on<{
        folder_path: string;
        auto_serve: boolean;
        target_file: string | null;
        folder_name?: string;
        parent_display?: string;
        cloud_provider_label?: string | null;
      }>(
        'config-ready',
        (payload) => {
          info(`✅ [PreviewManager] Received config-ready event: ${payload.folder_path}`);
          // Start build with config from event
          this.startBuild(
            payload.folder_path,
            payload.auto_serve,
            payload.target_file ?? undefined,
            payload.folder_name,
            payload.parent_display,
            payload.cloud_provider_label,
          );
        }
      );
      this.configReadyUnlisten = unlisten;
    } catch (err) {
      error(`❌ [PreviewManager] Failed to setup config-ready listener: ${err}`);
    }
  }

  /**
   * Drive the nav-pill hairline from DEPLOY upload progress.
   *
   * The hairline is the SOLE progress encoding for a deploy upload, mirroring
   * its role for a build (preview-hairline.ts header). `ProgressPanel` owns the
   * status-line TEXT from the same `deploy-progress` event; PreviewManager (the
   * hairline owner) owns the FILL — a deliberate separation of concerns, so the
   * two listeners never fight over one element. Concretely, the upload numerics
   * (`12.4/58.0 MB · 2.1 MB/s`) live in the status line's `__count` slot, which
   * ProgressPanel owns; the only text this listener writes is the hairline's
   * hover `data-tooltip` — the sole channel for those
   * numbers once the user minimizes the panel.
   *
   * Byte-based: the Rust ticker emits `bytes_uploaded`/`bytes_total`, and the
   * fill tracks the real bytes so a video-heavy publish crawls through the big
   * files instead of racing the small ones then freezing. Falls back to no fill
   * (the panel still shows the file count) when byte fields are absent — e.g. the
   * prebuilt CLI path or a commit-only deploy.
   */
  // ── Ambient machine-download facade ────────────────────────────────
  // Progress bars live in the hairline; toasts only announce state
  // changes. A machine-scoped background
  // download (e.g. a channel's companion stack) may borrow the hairline
  // while no site-owned work runs. Site work always wins: a build tick
  // or deploy upload preempts the pass; the producer retries next tick
  // and re-acquires once the site work ends.

  private siteWorkBusy(): boolean {
    return this.buildPassActive || this.uploadingStarted || this.bgPageTasksInFlight.size > 0;
  }

  /**
   * The ONE writer of the seam's visibility + tooltip. Two rules:
   *   • the band shows for panel content AND for an ambient pass — the
   *     download's label used to sit on a band the panel kept display:none,
   *     so the tooltip the facade exists to show could never be hovered;
   *   • one teller per hover: content present → the panel narrates (hover =
   *     peek) and the tooltip is cleared; only a pass with NO panel behind it
   *     puts its label on the band. Never both from one gesture.
   */
  private syncSeamSurface(): void {
    const ambient = this.downloadPassActive && this.ambientTooltip !== null;
    this.seam.setVisible(this.panelContentPresent || ambient);
    this.seam.setTooltip(ambient && !this.panelContentPresent ? this.ambientTooltip : null);
  }

  /**
   * Site work (a build tick or a deploy stage) preempts a borrowed pass. Label
   * teardown must ride along: endAmbientDownload's not-active guard means a
   * download that terminates AFTER being preempted never gets another chance
   * to clear its tooltip.
   */
  private preemptAmbientPass(): void {
    if (!this.downloadPassActive && this.ambientTooltip === null) return;
    this.downloadPassActive = false;
    this.ambientTooltip = null;
    this.dropAmbientTaskRow();
    this.syncSeamSurface();
  }

  /** The borrowed pass's textual task row in the progress panel — the SAME
   * label the hairline tooltip carries, as visible text, so the number is
   * never hover-only.
   *
   * `narrate: true`, deliberately NOT `ambient: true`. Ambient is right for
   * `bg:videos`, a sub-job of work already on screen; the one producer here is
   * a machine-scoped stack install — minutes long, explicitly clicked, and
   * invisible elsewhere once the catalog closes. Filed ambient it narrated
   * only to a hover on a 1.5px line, i.e. not at all. `narrate`
   * also tells the renderer the label is a stage SENTENCE, so it wraps within
   * a budget instead of eliding mid-word. */
  private upsertAmbientTaskRow(label: string | undefined, fraction: number | null): void {
    if (label === undefined) return;
    const progress = fraction === null ? null : Math.round(fraction * 100);
    this.progressPanel?.upsertTask({ id: "ambient-download", label, progress, source: "moss", narrate: true });
  }

  private dropAmbientTaskRow(): void {
    this.progressPanel?.removeTask("ambient-download");
  }

  /** Try to borrow the hairline for a machine-scoped download. */
  beginAmbientDownload(label?: string): boolean {
    if (this.siteWorkBusy()) return false;
    this.downloadPassActive = true;
    this.hairline.cancelPendingDissolve();
    // Determinate (no trickle): a download reports real byte fractions, so the
    // bar must track them rather than easing ahead to ~90%.
    this.hairline.startDeterminate();
    this.ambientTooltip = label ?? null;
    this.upsertAmbientTaskRow(label, null);
    this.syncSeamSurface();
    return true;
  }

  isAmbientDownloadActive(): boolean {
    return this.downloadPassActive;
  }

  /** `fraction: null` = the task is live but has no numeric claim yet (a
   * stack install before its first tick). The hairline holds its dot — a
   * fabricated 0-width fill would read as "stuck at 0%" — and the task row
   * renders its indeterminate form, mirroring the catalog ring. */
  setAmbientDownloadProgress(fraction: number | null, label?: string): void {
    if (!this.downloadPassActive) return;
    if (fraction !== null) this.hairline.setUploadProgress(fraction);
    this.upsertAmbientTaskRow(label, fraction);
    if (label !== undefined) {
      this.ambientTooltip = label;
      this.syncSeamSurface();
    }
  }

  /**
   * End the borrowed pass. `ok` fills the bar to full and dissolves —
   * deliberately WITHOUT the pill flash, which means "site updated";
   * `!ok` (failure, or the catalog opening and taking over as the
   * surface) fades the partial fill out.
   */
  endAmbientDownload(ok: boolean): void {
    // Label teardown runs even when the pass was already preempted — behind
    // the guard below it would strand a stale download tooltip forever. The
    // panel row rides along for the same reason.
    this.ambientTooltip = null;
    this.dropAmbientTaskRow();
    this.syncSeamSurface();
    if (!this.downloadPassActive) return;
    this.downloadPassActive = false;
    if (ok) {
      this.hairline.complete();
    } else {
      this.hairline.cancel();
    }
  }

  private async setupDeployHairlineListener(): Promise<void> {
    try {
      this.deployHairlineUnlisten = await host.engine.on<DeployProgress>(
        "deploy-progress",
        (p) => {
          if (!p) return;
          // Verifying arrives AFTER Complete (publish = verified live): the
          // pass is closed and the pill flash queued — falling through would
          // re-open the bar and kill the flash. Verification isn't ours.
          if (p.stage === "verifying") return;
          // Terminal: fill-to-full + pill flash on success; a bare fade on
          // failure (failure makes no green). The panel keeps its OWN rate
          // estimator; the manager's died with the hairline tooltip.
          if (p.stage === "complete" || p.stage === "failed") {
            this.uploadingStarted = false;
            if (p.stage === "complete") this.hairline.complete(() => this.flashPill());
            else this.hairline.cancel();
            return;
          }

          // ONE monotonic pass across the WHOLE deploy (progress/hairline-pacing.ts), not a
          // bar that shows only the upload and restarts at 0 to do it. Opened
          // once, at the FIRST stage seen, and via startDeterminate so no
          // NProgress trickle can race ahead of the real fraction.
          if (!this.uploadingStarted) {
            this.uploadingStarted = true;
            this.preemptAmbientPass(); // a deploy preempts a borrowed pass
            this.hairline.cancelPendingDissolve();
            this.hairline.startDeterminate();
          }

          const fraction = deployFraction(p);
          if (fraction !== null) this.hairline.setUploadProgress(fraction);
        },
      );

      // Backstop: some pushes RESOLVE with no Tauri terminal stage. Rust emits
      // `Preparing` before the site-id check, so `push_site` → NeedsSetup
      // returns Ok having opened the pass and emitting no complete/failed —
      // the bar sat as a live dot forever and siteWorkBusy() stayed true, which
      // starved beginAmbientDownload for the rest of the session. The COARSE
      // bus event from the publish button is the authoritative "a publish
      // is/isn't in flight", and it fires on EVERY resolution path
      // (setNormalState / setSuccessState), so it closes whatever the stage
      // stream leaked. After a real terminal stage this is a no-op.
      this.busDeployUnsub = eventBus.on<DeployProgressEvent>(
        Events.DEPLOY_PROGRESS,
        (e) => {
          if (!e || e.isDeploying || !this.uploadingStarted) return;
          this.uploadingStarted = false;
          if (e.status === "success") this.hairline.complete(() => this.flashPill());
          else this.hairline.cancel();
        },
      );
    } catch (err) {
      error(`❌ [PreviewManager] Failed to setup deploy-progress hairline listener: ${err}`);
    }
  }

  /**
   * Folder health: the sweep re-judges the folder every ~2s and
   * emits `FolderHealthChanged` to this webview on transitions only. The
   * `unavailable` half feeds rank 1 of `resolveUnshowableReason`; the
   * `degraded` half (overdue build) is the progress panel's corner advisory
   * — it holds its own subscription, since the two flags feed two different
   * surfaces that never need to agree on anything but the payload.
   */
  private async setupFolderHealthListener(): Promise<void> {
    try {
      this.folderHealthUnlisten = await onMossEvent("FolderHealthChanged", (msg) => {
        // A stale sweep's last tick can land just after a folder switch;
        // health for a folder this window no longer shows is not news.
        if (this.folderPath && msg.payload.folder !== this.folderPath) return;
        this.folderUnreachable = !!msg.payload.unavailable;
        if (!this.renderingStatus) this.reconcileUnshowable();
      });
    } catch (err) {
      error(`❌ [PreviewManager] Failed to setup folder-health listener: ${err}`);
    }
  }

  /**
   * Cloud-download home-page gate: react to `cloudSync`'s state, which owns
   * the folder filter and the provider/counts copy — see
   * `cloud-sync-store.ts`'s module doc for why. The store does no
   * I/O of its own (same discipline as moss-core), so this is still the one
   * place that registers the `icloud-sync` listener and feeds it via
   * `cloudSync.ingest`; every other surface only subscribes.
   *
   * This subscriber itself keeps only one piece of local bookkeeping
   * (`homeGateRaisedThisBuild`, for the error branch's stale-gate guard) and
   * otherwise defers the whole "what does the window show now" question to
   * `reconcileUnshowable` — see `clearHomeGate` for the other way an episode
   * ends.
   */
  private async setupHomeGateListener(): Promise<void> {
    let syncUnlisten: (() => void) | null = null;
    try {
      syncUnlisten = await host.engine.on<CloudSyncPayload>("icloud-sync", (payload) => {
        if (payload) cloudSync.ingest(payload);
      });
    } catch (err) {
      error(`❌ [PreviewManager] Failed to setup home-gate listener: ${err}`);
    }
    const storeUnsubscribe = cloudSync.subscribe((change) => {
      if (change.state.phase === "home_waiting") this.homeGateRaisedThisBuild = true;
      // Skipped when renderStatus is itself what changed the store — it
      // pushes `previewBlank`/calls `lowerGate` and the store notifies
      // synchronously, so re-entering here would repaint (or re-invoke
      // loadPreview) twice for one transition. `renderStatus` reconciles
      // the ladder itself once it is done.
      if (!this.renderingStatus) this.reconcileUnshowable();
    });
    this.homeGateUnlisten = () => {
      syncUnlisten?.();
      storeUnsubscribe();
    };
  }

  /**
   * Drop the full-window screen and whatever is holding it up.
   *
   * Told to the store rather than dropped locally. Two callers end the
   * episode: the store's own gate-clearing edge (Rust's `home_ready`, or the
   * last file arriving), and the build-error branch of `renderStatusInner`
   * taking the window to show a message the user must read instead. Rust
   * never hears about the second one, so if this surface kept its own copy
   * the store would still say the gate was up — and the progress panel,
   * which stays silent while a reason holds the window, would never report
   * the download that is still running. `lowerGate()` is a no-op once the
   * gate is already down, so the first path costs nothing.
   */
  private clearHomeGate(): void {
    cloudSync.lowerGate();
    // Take the overlay down, not just its marker class: leaving `visible` set
    // strands the screen there and inert — 'loading' early-returns *because*
    // the overlay is visible, and ticks are ignored once the gate is down.
    this.dropWaitingScreen();
    this.clearOverlay();
  }

  /**
   * Start build with progress updates
   */
  async startBuild(
    folderPath: string,
    autoServe: boolean = true,
    targetFile?: string,
    folderName?: string,
    parentDisplay?: string,
    cloudProviderLabel?: string | null,
  ): Promise<void> {
    this.pendingTargetFile = targetFile ?? null;

    // Same-folder + target file: navigate immediately using the already-running
    // server. The state machine won't re-enter 'ready' on a silent rebuild
    // (status object is reused), so pendingTargetFile would never be consumed.
    // Navigate now, then let the rebuild proceed for content freshness.
    if (targetFile && folderPath === this.folderPath && this.state.isServerReady()) {
      this.navigateToTarget(targetFile);
      this.pendingTargetFile = null;
    }

    // A folder-open reaches this twice by design; `build-singleflight.ts` says
    // who the two callers are and what a duplicate pipeline costs.
    return this.buildSingleflight.run(folderPath, () =>
      this.runBuild(folderPath, autoServe, folderName, parentDisplay, cloudProviderLabel),
    );
  }

  /** The build body. Guarded by the singleflight above — do not call directly. */
  private async runBuild(
    folderPath: string,
    autoServe: boolean,
    folderName?: string,
    parentDisplay?: string,
    cloudProviderLabel?: string | null,
  ): Promise<void> {
    try {
      // Detect same-folder rebuild: the preview is already showing this folder's
      // content, so we can keep it visible instead of hiding it behind an overlay.
      // For folder switches (different path), we must show the overlay because
      // the current preview belongs to a different site.
      const isSameFolderRebuild = folderPath === this.folderPath
        && this.state.isServerReady();

      // A new build gets to speak for itself: whatever the last one said about
      // the cloud must not decide what this one's failure looks like.
      this.homeGateRaisedThisBuild = false;

      if (isSameFolderRebuild) {
        // Same-folder rebuild: keep serverUrl so PreviewState treats progress
        // updates as a "silent rebuild" (no loading→ready transitions that
        // would trigger overlay display in renderStatus).
      } else {
        // Different folder or first build: full reset clears serverUrl so the
        // new build shows progress instead of being treated as a silent rebuild.
        this.state.reset();
        this.firstPaint = 'awaiting';
        // The new folder's health starts healthy (its sweep emits transitions
        // only), so a predecessor's `unreachable` must not survive the switch
        // — reset it BEFORE `clearHomeGate` below, whose `lowerGate()` notify
        // runs `reconcileUnshowable` synchronously and would otherwise read
        // the old folder's stale value.
        this.folderUnreachable = false;
        // A leftover gate from a previous folder must not block this one.
        // `clearHomeGate` rather than the marker class alone: an overlay left
        // `visible` strands folder A's waiting screen — its counts, its live
        // "Choose a different folder" button — over folder B's build, because
        // the 'loading' render early-returns on an already-visible overlay.
        this.clearHomeGate();
      }

      if (!isSameFolderRebuild) {
        // Different folder or first build: reset navigation (old paths are invalid)
        this.previewRequirementScope = {};
        this.refreshAfterFirstNavigation = false;
        this.navigationManager.reset();
      }
      // Same-folder rebuild: keep navigation position so user stays on their
      // current article page while the rebuild happens in the background.

      // Store folder path for scraping
      this.folderPath = folderPath;
      // The wait/loading screen's title + location-line inputs, alongside the
      // path they describe — see `vaultLocationFields`.
      this.folderName = folderName ?? null;
      this.parentDisplay = parentDisplay ?? null;
      this.cloudProviderLabel = cloudProviderLabel ?? null;
      // `cloudSync` is the single filter on `icloud-sync` now — see its
      // `setFolder` doc — so this scopes both this listener and the panel's.
      cloudSync.setFolder(folderPath);
      // Re-scoping the panel's half of the payload drops a predecessor's
      // overdue-build advisory, whose all-clear will never arrive (the old
      // sweep is cancelled before it can emit one).
      this.progressPanel?.setFolder(folderPath);

      // Set folder path for article detection in navigation manager
      this.navigationManager.setFolderPath(folderPath);

      if (isSameFolderRebuild) {
        // Same-folder rebuild: keep old preview visible, just show progress.
        // The user sees their site with a non-blocking progress indicator
        // instead of a blank overlay for 1-5 seconds.
        this.progressPanel?.show();
      } else {
        // First build or folder switch: nothing valid to show, so raise the
        // screen at rank 8 ('opening' — nothing served, nothing known yet).
        // INVARIANT: this block MUST remain synchronous (no await before the
        // `upsertTask` below) — the screen must be visible before the IPC
        // round-trip to Rust, so the user never stares at a blank window.
        cloudSync.setPreviewBlank(true);
        this.reconcileUnshowable();
        this.state.setStage('opening');
        this.progressPanel?.show();
      }

      this.progressPanel?.setStage('opening');

      // Start build with progress updates and file watching enabled
      if (!host.build) throw new Error('this host cannot build a folder');
      await host.build(folderPath, autoServe, (update) => {
        this.handleProgressUpdate(update);
      });
    } catch (err) {
      error(`❌ 📦 [Preview] Build failed: ${err}`);
      this.state.setError(t('preview.build_failed', { err: String(err) }));
      // Route through renderStatus so the new `case 'error'` path runs:
      // it removes the build task, drops the compiling stage, and shows
      // the failure in the overlay. Previously this wrote the overlay
      // separately, which left the build task pinned in the panel.
      this.renderStatus(this.state.getStatus());
    }
  }

  /**
   * Handle progress updates from build.
   *
   * Delegates to PreviewState for state transitions, then renders the result.
   * PreviewState handles:
   * - Initial build: shows loading progress
   * - Silent rebuilds: once server ready, progress is ignored until completion
   * - Empty/ready transitions: based on is_empty flag
   */
  private handleProgressUpdate(update: ProgressUpdate): void {
    // The nav-pill hairline is the SOLE progress encoding for EVERY build
    // because the panel has no internal stage/per-task bars. We reuse
    // the already-arriving progress stream to drive it for both the INITIAL
    // build (serverUrl still null) and every REBUILD (serverUrl set):
    //   • in flight → GROW the hairline 0→90% with the build's real percentage;
    //   • the terminal completed update → schedule a complete()+dissolve.
    // For a REBUILD the on-screen change lands a beat later (FileChanged →
    // refresh()/morph), so refresh() cancels this pending dissolve and instead
    // completes the bar exactly when the morph finishes (bar-full + pill-flash
    // coincide with the update). For the INITIAL build no refresh follows, so
    // this fallback completes the bar after the served preview loads.
    const buildDone = update.completed && update.percentage >= 100;
    this.buildPassActive = !buildDone;
    if (this.uploadingStarted) {
      // A deploy pass OWNS the hairline. `Preparing` fires before the deploy
      // drains in-flight work, so a pre-publish rebuild's ticks arrive here
      // mid-publish — letting them through drove the bar to ~90, flashed
      // "site updated", dissolved, and handed the deploy a restarted bar
      // (0 → 90 → 100+flash → 27, within one publish). State/panel updates
      // below still run; only the hairline is off-limits.
    } else if (buildDone) {
      this.hairline.dissolveAfter(1500);
    } else {
      // A build tick preempts any borrowed download pass (site work wins);
      // the download producer re-acquires after the build completes.
      this.preemptAmbientPass();
      // The first event (often ~0%) starts the bar as a dot at the left.
      this.hairline.setBuildProgress((update.percentage ?? 0) / 100);
    }

    // Get previous status for comparison
    const previousStatus = this.state.getStatus();

    // Let state machine handle the transition
    const newStatus = this.state.handleProgressUpdate(update);

    // Render if status changed (object reference comparison is sufficient
    // since state machine returns new objects on change)
    if (previousStatus !== newStatus) {
      this.renderStatus(newStatus);
    }

    // On a completed build of an empty folder, notify the app shell so it
    // can auto-open the editor in folder-mode (the onboarding surface).
    // The preview itself shows the served empty-home artifact (no overlay).
    // WATCH mode re-completes empty on every rebuild, so this fires
    // repeatedly; the app-shell handler (`openEditorForOnboarding`) is
    // guarded against re-entry (one-shot latch + in-flight guard) and the
    // Rust action-panel creation is idempotent, so repeated empty
    // completions never reopen or duplicate the editor webview.
    const isComplete = update.completed && update.percentage >= 100;
    if (isComplete && this.state.lastCompletionWasEmpty()) {
      this.onEmptyFolder?.();
    }
  }

  /**
   * Drive the hairline from the BACKGROUND work that finishes a page.
   * The blocking build reaching 100% only means the HTML/text is ready; image
   * webp + video transcode run in the background AFTER (blocking.rs registers
   * them Pending, a BackgroundHandle worker encodes them, emitting Tier-2
   * `BackgroundProgress{task:"images"|"videos"}`), as a document's text read
   * does (`derive`). While any such task is in
   * flight the bar keeps advancing; the completed path dissolves it (1.5s)
   * once every one reports done.
   *
   * Backstop: a `completed:true` tick is NOT guaranteed — the image
   * worker returns early without one when a rebuild supersedes an in-flight
   * encode (build/media/image.rs:1612-1618), routine in preview mode. So the
   * completed branch must not be the only thing that arms the dissolve: EACH
   * in-flight tick re-arms a bounded watchdog (`dissolveAfter` clears+resets
   * its own pending timer) instead of hanging the bar until the next build.
   * The normal completed path can still dissolve sooner (1.5s < watchdog).
   *
   * Only work that finishes what a page shows touches the bar: the media
   * tasks and `derive`, a document's text read after the build that wrote its
   * page. comment-sync, plugin-init, rss and notebooks are unrelated.
   *
   * A DEPLOY PASS OWNS THE HAIRLINE, here as in handleProgressUpdate and
   * onRefreshApplied. `Preparing` drains in-flight conversion, so an
   * encode ticks INSIDE the deploy pass at a fraction of the ENCODE, and the
   * watchdog's `dissolveAfter` would `complete()` the bar mid-upload. Both
   * guards sit after the in-flight bookkeeping (siteWorkBusy() reads it).
   * Repro + numbers: preview-manager.test.ts, "neither a draining rebuild NOR".
   */
  /** Watchdog window (see the backstop above). Longer than a normal encode plus
   *  the completed-path 1.5s dissolve, so it only fires as a last resort. */
  private static readonly BG_WATCHDOG_MS = 15000;
  private static readonly PAGE_FINISHING_TASKS = new Set(["images", "videos", "derive"]);

  private handleBackgroundProgress(bg: BackgroundProgress): void {
    if (!PreviewManager.PAGE_FINISHING_TASKS.has(bg.task)) return;

    if (!bg.completed) {
      this.bgPageTasksInFlight.add(bg.task);
      if (this.uploadingStarted) return; // see the guard note above
      // RE-ARM the watchdog (never a bare cancel) — see the backstop above.
      this.hairline.dissolveAfter(PreviewManager.BG_WATCHDOG_MS);
      const fraction = bg.total > 0 ? bg.current / bg.total : 0;
      this.hairline.setBuildProgress(fraction);
    } else {
      this.bgPageTasksInFlight.delete(bg.task);
      if (this.uploadingStarted) return; // see the guard note above
      // Dissolve only once ALL in-flight tasks have reported done.
      if (this.bgPageTasksInFlight.size === 0) {
        this.hairline.dissolveAfter(1500);
      }
    }
  }

  /**
   * Render a preview status.
   * Uses Tauri commands to control the preview webview.
   */
  private renderStatus(status: PreviewStatus): void {
    if (this.screenHold.active) return;
    // The project-unavailable screen outranks every status paint: while it
    // is up nothing may repaint the overlay (moss cannot vouch for what a
    // repaint would reveal), and its own clear edge re-invokes renderStatus
    // with the current status — so nothing here is lost, only delayed. The
    // same shape as the cloud gate's stickiness, one rank higher.
    if (this.folderUnreachable) return;
    this.renderingStatus = true;
    try { this.renderStatusInner(status); } finally { this.renderingStatus = false; }
  }

  private renderStatusInner(status: PreviewStatus): void {
    switch (status.type) {
      case 'initial':
        // Nothing to render yet - preview webview shows loading page
        break;

      case 'loading': {
        // Half of "does something already own the window?", and the half
        // only this surface knows. `reconcileUnshowable` paints whichever
        // reason resolves — the sticky gate, a download, the build's own
        // stage, or (rank 8) nothing known yet. No separate "just show the bare grid"
        // fallback exists; the screen yields on its own once `previewBlank` goes false.
        cloudSync.setPreviewBlank(true);
        this.reconcileUnshowable();
        break;
      }

      case 'ready':
        // Sticky home-page gate: a build that finished WITHOUT a home page
        // must not reveal the 404 underneath until a build reports
        // `home_ready` — the only way out, and it re-invokes renderStatus with
        // this SAME status, so nothing here is lost — only delayed.
        //
        //
        // This line comes FIRST, before that return: the preview has a served
        // site whatever the gate says. Clearing it only after the guard left
        // it stuck true for the whole gated stretch, so `home_ready` dropped
        // the gate and the screen stayed up anyway on the download half —
        // with no further status change coming to repaint it.
        cloudSync.setPreviewBlank(false);
        this.reconcileUnshowable();
        // `gateUp`, NOT the general reason: only the build's verdict is
        // sticky. A servable site outranks a progress count, so the
        // download-in-flight half yields here and the corner panel carries
        // the rest — which is what the line after the guard says.
        if (cloudSync.getState().gateUp) {
          return;
        }
        // setStage('ready') clears the PREVIEW_SERVER_ADV_KEY advisory and
        // lastFailure automatically (via ProgressPanel.setStage). No removeTask
        // needed — preview-server-error was never a task row (it was an advisory).
        this.state.setStage('ready');
        this.progressPanel?.setStage('ready');

        // Set server URL for asset placeholder swapping
        if (status.url) {
          try {
            const serverUrl = new URL(status.url).origin;
            this.progressPanel?.setServerBaseUrl(serverUrl);
          } catch {}
        }

        this.loadPreview(status.url);
        if (this.pendingTargetFile) {
          this.navigateToTarget(this.pendingTargetFile);
          this.pendingTargetFile = null;
        }
        break;

      case 'error': {
        error(`❌ [Preview] Error: ${status.message}`);
        // The build that would have produced the waited-for page just failed
        // out loud. "Preparing this page" over that is a promise moss has
        // already broken; rank 3 outranks rank 4 and says what happened.
        this.cancelNewPageWait();
        // One screen wins, and WHICH is one question with one owner — the
        // ladder, run here with a narrower `gateUp` than the general
        // reconciler uses: a gate raised by an EARLIER build in this folder
        // must not mask a new, genuine failure this build never touched the
        // cloud to produce (`homeGateRaisedThisBuild`, reset at the start of
        // every build). Losing the contest still ends the build; the window
        // just stays with the truer surface.
        const cloud = cloudSync.getState();
        const reason = resolveUnshowableReason({
          unreachable: this.folderUnreachable,
          gateUp: cloud.gateUp && this.homeGateRaisedThisBuild,
          previewBlank: cloud.previewBlank,
          errorVerdict: status.verdict,
          pagePending: this.pagePendingActive(),
          blocking: cloud.blocking,
          stalled: cloud.stalled,
          buildStageLabel: this.progressPanel?.buildStageKnownLabel() ?? null,
        });
        this.state.setStage('ready');
        this.progressPanel?.setStage('ready');
        if (reason !== 'error-hard' && reason !== 'error-soft') {
          this.reconcileUnshowable();
          break;
        }
        // Both halves of the claim, including a merely-ARMED download screen
        // (blank preview, nothing in flight) the next tick would raise over it.
        this.clearHomeGate();
        this.applyUnshowableReason(reason, cloudSync.getState(), status);
        break;
      }
    }
  }

  /**
   * Navigate the preview to a target file after build completes.
   * Resolves the file path to a URL and navigates after a short delay
   * to let the initial page load settle in the iframe.
   */
  private navigateToTarget(targetFile: string): void {
    const request = this.navigationManager.getRequestRevision();
    const navigate = async () => {
      try {
        let urlPath: string;
        const folder = this.folderPath ?? '';
        const fullPath = folder.endsWith('/')
          ? `${folder}${targetFile}`
          : `${folder}/${targetFile}`;
        if (!targetFile.endsWith('.md') && !targetFile.endsWith('.markdown')) {
          const result = await engine.resolveUrlForFolder(fullPath);
          if (result.status !== 'ok' || !result.data) {
            warn(`[navigateToTarget] resolveUrlForFolder returned no match for "${fullPath}"`);
            return;
          }
          urlPath = result.data;
        } else {
          const result = await engine.resolveUrlForFile(fullPath);
          if (result.status !== 'ok' || !result.data) {
            warn(`[navigateToTarget] resolveUrlForFile returned no match for "${fullPath}"`);
            return;
          }
          urlPath = result.data;
        }
        if (request !== this.navigationManager.getRequestRevision()) return;
        setTimeout(() => {
          if (request === this.navigationManager.getRequestRevision()) this.navigationManager.navigateToPage(urlPath);
        }, 300);
      } catch (err) {
        warn(`Could not navigate to target file: ${err}`);
      }
    };
    navigate();
  }

  /**
   * Navigate the preview iframe to a URL.
   * Sets iframe.src to load the content.
   */
  private navigatePreviewIframe(url: string): void {
    if (!this.iframe) {
      error("❌ [Preview] Cannot navigate: iframe not found");
      this.state.setError(t('preview.iframe_unavailable'));
      return;
    }

    try {
      this.navigationManager.beginNavigation(url);
      this.currentPreviewUrl = url;
      // Hide overlay when navigating to actual content
      this.hideOverlay();
      // Stamp the shell marker so the served HTML carries `moss-shell-frame`
      // before first paint — eliminates the 48px chrome-clearance flash
      // that was visible while build progress was showing. See
      // `packages/moss-ui/src/ui/preview/shell-marker.ts` for the full design.
      this.iframe.src = withShellMarker(url);
    } catch (err) {
      error(`❌ [Preview] Failed to navigate preview iframe: ${err}`);
      this.state.setError(t('preview.load_failed', { err: String(err) }));
    }
  }

  /**
   * Load preview URL in iframe (public API).
   * Adds cache-busting when URL matches current src to force reload
   * (e.g., when switching folders but preview server is on same port).
   */
  loadPreview(url: string): void {
    // Add cache-busting if URL matches current iframe src
    // This forces reload when switching folders (same port, different content)
    try {
      const currentSrc = this.iframe?.src;
      if (currentSrc) {
        const newUrl = new URL(url);
        const currentUrl = new URL(currentSrc);
        // If same origin and path, add cache bust to force reload
        if (newUrl.origin === currentUrl.origin && newUrl.pathname === currentUrl.pathname) {
          newUrl.searchParams.set("_t", Date.now().toString());
          url = newUrl.toString();
        }
      }
    } catch {
      // URL parsing failed, proceed without cache busting
    }
    this.navigatePreviewIframe(url);
  }

  /** The open stopped: say so in this window, with why. */
  private showFailure(detail: string, folderName = this.folderName ?? folderBaseName(this.folderPath)): void {
    const overlay = this.raiseOverlay();
    if (!overlay) return;
    this.waitingScreen.show(overlay, { kind: 'failed', folderName, detail });
  }

  /**
   * The morph kill-switch disables the default in-place update (on-device
   * sign-off complete): refresh morphs the preview in place rather than
   * reloading the cross-origin iframe (which caused the WebKit blank-until-
   * scroll, #135275). The localStorage flag survives as a kill-switch — set
   * `localStorage['moss.preview.morph'] = '0'` (or
   * `window.__moss_debug.setMorphEnabled(false)`) to fall back to reload. Read
   * fresh on every refresh so the toggle takes effect without a reload.
   */
  static isMorphEnabled(): boolean {
    try {
      return localStorage.getItem("moss.preview.morph") !== "0";
    } catch {
      return true;
    }
  }

  /**
   * Coalesce window for refresh(). One rebuild emits FileChanged AND
   * BuildComplete; both call refresh(). Skip a refresh landing within this many
   * ms of the previous one so the rapid pair becomes one morph. Kept BELOW the
   * 250ms watcher debounce so two distinct rebuilds are never merged, yet a
   * genuinely-later BuildComplete (after async asset encoding) still refreshes.
   */
  private static readonly REFRESH_COALESCE_MS = 200;

  /** Emit the one `[preview-refresh]` INFO line for a decision, at the point it's final. */
  private logRefreshDecision(trigger: RefreshTrigger, ctx: RefreshDecisionContext, outcome: RefreshOutcome): void {
    void info(formatRefreshDecisionLine(trigger, ctx, outcome));
  }

  /**
   * Refresh current preview.
   * Uses in-place reload via RPC (preserves scroll position),
   * falling back to cache-busting iframe.src for non-HTML content.
   *
   * @param source Which signal triggered this call (default `'other'` for the
   *   redirect/rename fallback call sites). Only used to decide whether a
   *   call coalesced away by REFRESH_COALESCE_MS is worth a trailing-edge
   *   follow-up — see the coalescing block below.
   * @param diffContext The smart-diff decision behind this call, for `[preview-refresh]`.
   */
  async refresh(source: 'file-changed' | 'build-complete' | 'other' = 'other', diffContext?: RefreshDecisionContext): Promise<void> {
    if (this.newPageWait) return;
    const request = this.navigationManager.getRequestRevision();
    const isCurrent = () => request === this.navigationManager.getRequestRevision();
    const ctx = diffContext ?? implicitRefreshContext(this.navigationManager.getCurrentUrl());
    // A contentWindow exists before its bridge. Keep the terminal refresh
    // until that bridge announces navigation, rather than sending an RPC
    // nobody can answer and spending the morph timeout. Early preview may
    // contain the preceding generation, so the terminal refresh is retained.
    if (source === 'build-complete' && !this.navigationManager.getActivePage()) {
      this.refreshAfterFirstNavigation = true;
      this.logRefreshDecision(source, ctx, 'deferred(first navigation)');
      return;
    }

    // Coalesce the double-fire: one rebuild emits BOTH FileChanged
    // (preview-manager listener) AND BuildComplete, each calling refresh() —
    // two same-origin fetches, two morphs and two pill flashes per edit. Skip
    // a refresh that lands within REFRESH_COALESCE_MS of the previous one. The
    // window (< the 250ms watcher debounce) merges the rapid FileChanged+
    // BuildComplete pair of a TEXT edit (1 morph), while a genuinely-later
    // BuildComplete after async asset encoding (seconds) is NOT coalesced — so
    // image-adding edits still get the placeholder→real-asset refresh. Distinct
    // rebuilds are >=250ms apart (debounce) so they are never merged.
    //
    // A COALESCED FileChanged must not be silently lost. FileChanged carries a
    // real smart-diff decision; BuildComplete carries none — it fires on EVERY
    // rebuild of EVERY site (`BuildTerminalBarrier`, the single post-join
    // point), so a plain unrelated text edit routinely produces a
    // FileChanged+BuildComplete pair. Arming a trailing follow-up for every
    // coalesced call would turn that routine pair into a second flash/morph
    // ~200ms later. So only a coalesced FileChanged (or a direct 'other'
    // caller — redirect/rename fallbacks, which also represent a real state
    // change) arms the trailing timer; a coalesced BuildComplete-only
    // duplicate is dropped (it never carries a diff, and a later one outside
    // the window still refreshes). This covers the back-to-back-rebuild race:
    // an image lands as its own rebuild, then the edit referencing it
    // autosaves ~800ms later, and its real FileChanged can land within 200ms
    // of the first rebuild's BuildComplete.
    const now = Date.now();
    const elapsed = now - this.lastRefreshAt;
    if (elapsed < PreviewManager.REFRESH_COALESCE_MS) {
      const arm = source !== 'build-complete' && this.trailingRefreshTimer === null;
      if (arm) {
        this.trailingRefreshTimer = setTimeout(() => {
          this.trailingRefreshTimer = null;
          if (isCurrent()) void this.refresh('file-changed', ctx);
        }, PreviewManager.REFRESH_COALESCE_MS - elapsed);
      }
      this.logRefreshDecision(source, ctx, arm ? 'trailing-armed' : 'coalesced');
      return;
    }
    this.lastRefreshAt = now;

    // A rebuild produced changes, so the hairline accompanies a morph/reload
    // is about to apply. Cancel the no-change fallback so the bar instead
    // completes at each exit below — exactly when the morph/reload finishes, so
    // the bar reaching full length + the pill flash coincide with the on-screen
    // update.
    this.hairline.cancelPendingDissolve();

    // Use in-place idiomorph updates by default; moss.preview.morph=0 opts out. The bridge
    // re-fetches its own URL same-origin and reconciles the DOM in place,
    // eliminating the WebKit blank-until-scroll a cross-origin iframe reload
    // causes (#135275). On failure/timeout it returns false → we fall through
    // to reload() below, so behavior never regresses when the morph misfires.
    let outcome: RefreshOutcome | undefined, morphAttempted = false;
    const yielded = this.navigationManager.isNavigationPending();
    if (PreviewManager.isMorphEnabled()) {
      morphAttempted = true;
      diagCount('preview.morph');
      if (await this.navigationManager.morph()) outcome = 'morph-applied';
      if (!isCurrent()) return;
    }

    // Try in-place reload (preserves scroll position) if morph didn't land.
    if (!outcome && await this.navigationManager.reload()) {
      // reload() reports success without acting while a navigation is pending.
      outcome = yielded
        ? 'skipped(navigation pending)'
        : morphAttempted ? 'morph-failed→reload' : 'reload';
    }
    if (!isCurrent()) return;

    // Fallback: cache-busting navigation (for non-HTML content without bridge)
    if (!outcome) {
      const currentUrl = this.navigationManager.getCurrentUrl()
        || this.iframe?.src
        || this.currentPreviewUrl;
      if (currentUrl) {
        try {
          const url = new URL(currentUrl);
          url.searchParams.set("_t", Date.now().toString());
          this.navigatePreviewIframe(url.toString());
        } catch {
          if (this.currentPreviewUrl) this.navigatePreviewIframe(this.currentPreviewUrl);
        }
      }
      outcome = currentUrl ? 'iframe-nav-fallback' : 'skipped(no page)';
    }
    this.logRefreshDecision(source, ctx, outcome);
    this.onRefreshApplied();
  }

  /**
   * The morph/reload landed: complete the rebuild hairline to full length, and
   * flash the whole nav pill moss-green to confirm "updated" — the flash fires
   * via the hairline's reached-full callback so it lands right AFTER the
   * progress bar finishes, not during the grow.
   */
  private onRefreshApplied(): void {
    this.instruments?.refresh();
    // Mid-publish, the deploy pass owns the hairline — a draining rebuild's
    // morph must not fill the bar and flash "site updated" while the upload
    // is at 27%. The deploy's own complete() carries the flash.
    if (this.uploadingStarted) return;
    this.hairline.complete(() => this.flashPill());
  }

  /** Flash the whole nav pill moss-green (rebuild applied). */
  private flashPill(): void {
    const pill = document.querySelector<HTMLElement>('.moss-nav-pill');
    if (!pill) return;
    pill.classList.remove('refreshing');
    void pill.offsetWidth; // force reflow so the animation re-triggers
    pill.classList.add('refreshing');
  }

  /**
   * Setup event listener for file change events from backend
   */
  private async setupFileChangeListener(): Promise<void> {
    try {
      await onMossEvent("FileChanged", (msg) => {
        this.handleFileChangeEvent(msg.payload);
      });
    } catch (err) {
      error(`❌ [Preview] Failed to setup file change listener: ${err}`);
    }
  }

  /**
   * Handle file change events during live development.
   *
   * Delegates to getFileChangeAction (pure function) for determining
   * what action to take, then executes that action.
   */
  private handleFileChangeEvent(changeEvent: FileChangeEvent | null): void {
    const changedCount = changeEvent?.changed_output_files?.length ?? 0;

    // Only output for this request justifies reloading its covered document.
    // An unrelated rebuild cannot decide that the requested page is missing.
    if (this.newPageWait) {
      const wait = this.newPageWait;
      const targetPath = extractPathFromUrl(wait.targetUrl);
      const action = getFileChangeAction(changeEvent, targetPath);
      if (action.type === 'update-url' || action.type === 'redirect-home') {
        this.executeFileChangeAction(action, { action: action.type, path: targetPath, changedCount });
        return;
      }
      if (!changeEvent || (targetPath !== null && changeEvent.changed_output_files?.includes(targetPath))) {
        this.newPageWait = { ...wait, rechecks: wait.rechecks + 1 };
        this.logRefreshDecision('newPageWait-recheck', { action: 'recheck', path: wait.pathname, changedCount }, 'page-recheck');
        this.loadPreview(wait.targetUrl);
      }
      return;
    }

    // Get current URL from NavigationManager (tracks actual navigation, not iframe.src)
    // iframe.src doesn't update when user navigates via links within the iframe
    const currentUrl = this.navigationManager.getCurrentUrl();
    const currentPath = currentUrl ? extractPathFromUrl(currentUrl) : null;

    // Get action from pure function
    const action = getFileChangeAction(changeEvent, currentPath);
    const ctx: RefreshDecisionContext = { action: action.type, path: currentPath, changedCount };

    // 'none' is final right here — folds the old DEBUG [SmartDiff] line in.
    // 'refresh' logs from inside refresh() once its mechanism is known;
    // 'redirect-home'/'update-url' log from their own handler's success
    // branch, because each has a fallback (no known URL) that calls
    // refresh() itself — logging here too would double it.
    if (action.type === 'none') this.logRefreshDecision('file-changed', ctx, 'none(page unaffected)');

    // Execute the action
    this.executeFileChangeAction(action, ctx);
  }

  /**
   * Execute a file change action.
   */
  private executeFileChangeAction(action: ReturnType<typeof getFileChangeAction>, ctx: RefreshDecisionContext): void {
    switch (action.type) {
      case 'none':
        break;

      case 'refresh':
        this.refresh('file-changed', ctx);
        break;

      case 'redirect-home':
        this.redirectToHomepage(ctx);
        break;

      case 'update-url':
        this.updateUrlForRename(action.newPath, ctx);
        break;
    }
  }

  /**
   * Redirect to homepage when current page is deleted
   */
  private redirectToHomepage(ctx: RefreshDecisionContext): void {
    try {
      const iframeSrc = this.iframe?.src;
      const baseUrl = iframeSrc || this.currentPreviewUrl;

      if (baseUrl) {
        const homepageUrl = `${new URL(baseUrl).origin}/index.html`;
        this.logRefreshDecision('file-changed', ctx, 'redirect-home');
        this.loadPreview(homepageUrl);
      } else {
        // No known URL — refresh() is the sole [preview-refresh] line here.
        this.refresh();
      }
    } catch (err) {
      error(`❌ [Preview] Failed to redirect to homepage: ${err}`);
      this.refresh();
    }
  }

  /**
   * Clean up event listeners and owned resources. Call when the PreviewManager
   * is torn down (e.g. the window is being reused for a different folder).
   */
  destroy(): void {
    this.instruments?.destroy();
    this.instruments = null;
    // Remove the document-level Escape key listener (ref stored to
    // avoid creating a new closure that won't match the original for removal).
    if (this.escapeKeyHandler) {
      document.removeEventListener("keydown", this.escapeKeyHandler);
      this.escapeKeyHandler = null;
    }
    this.configReadyUnlisten?.();
    this.configReadyUnlisten = null;
    this.deployHairlineUnlisten?.();
    this.deployHairlineUnlisten = null;
    this.busDeployUnsub?.();
    this.busDeployUnsub = null;
    this.rebuildUnsub?.();
    this.rebuildUnsub = null;
    this.homeGateUnlisten?.();
    this.homeGateUnlisten = null;
    this.folderHealthUnlisten?.();
    this.folderHealthUnlisten = null;
    if (this.trailingRefreshTimer !== null) {
      clearTimeout(this.trailingRefreshTimer);
      this.trailingRefreshTimer = null;
    }
    // Reset so a reused instance (window re-init) starts the next deploy's bar
    // cleanly even if a prior deploy was abandoned before complete/failed.
    this.uploadingStarted = false;
    this.progressPanel?.destroy();
    this.zoomBadge?.destroy();
    this.zoomBadge = null;
    this.dropWaitingScreen();
    // Not just the screens: the blueprint grid belongs to the overlay, and
    // its rAF loop and three listeners outlive this manager unless cleared.
    this.clearOverlay();
    this.cancelNewPageWait();
    this.refreshAfterFirstNavigation = false;
    this.navigationManager.setNavigationResultListener(null);
    this.navigationManager.setPreviewRequirementListener(null);
    this.previewRequirementScope = {};
    this.navigationManager.setDeviceModeChangeListener(null);
    this.navigationManager.destroy();
    this.zoom?.destroy();
    this.zoom = null;
    // The hairline owns timers that RE-MOUNT it when they fire (`complete()`
    // calls `start()`), so leaving them running past teardown is not a stray
    // tick — it rebuilds a bar into a document this manager no longer drives.
    this.hairline.destroy();
  }

  private updateUrlForRename(newPath: string, ctx: RefreshDecisionContext): void {
    try {
      const iframeSrc = this.iframe?.src;
      const baseUrl = iframeSrc || this.currentPreviewUrl;

      if (baseUrl) {
        const newUrl = `${new URL(baseUrl).origin}/${sourcePathToHtmlPath(newPath)}`;
        // Arm the 404-retry BEFORE navigating: the moved page is published by a
        // detached seal+generation-swap that can land just after this nav, so
        // the first hit may 404. The same wait covers creation and output moves.
        this.armNewPageWait(new URL(newUrl).pathname);
        this.logRefreshDecision('file-changed', ctx, 'url-moved');
        this.loadPreview(newUrl);
      } else {
        // No known URL — refresh() is the sole [preview-refresh] line here.
        this.refresh();
      }
    } catch (err) {
      error(`❌ [Preview] Failed to update URL for rename: ${err}`);
      this.refresh();
    }
  }

  /** Recheck a requested route through its generation publication window. */
  private onNavigationResult(info: NavigationResultInfo): void {
    if (this.refreshAfterFirstNavigation) {
      this.refreshAfterFirstNavigation = false;
      void this.refresh('build-complete');
    }
    if (!this.newPageWait) return;
    // info.url is already shell-marker-stripped by NavigationManager (see
    // NavigationResultInfo contract); we only need its pathname here.
    let pathname: string;
    try {
      pathname = new URL(info.url).pathname;
    } catch {
      return;
    }
    if (this.newPageWait) {
      const waitDecision = decideNewPageWait(
        this.newPageWait,
        { pathname, notFound: info.notFound },
        this.supersededPageWaitPath,
      );
      if (waitDecision.type === 'arrived') {
        const served = !info.notFound && pathname === this.newPageWait.pathname;
        this.endNewPageWait();
        // The build that produced this page never reached `refresh()` — the
        // wait took the ordinary smart-diff action off the table and reloaded
        // itself instead — so the rebuild hairline would dissolve on its
        // fallback timer instead of filling, and the "site updated" pill
        // flash would be lost for the one build the author is watching for.
        // Arrival IS the moment the update landed on screen; say so.
        if (served) this.onRefreshApplied();
      }
    }
    const wait = this.newPageWait;
    if (!wait || pathname !== wait.pathname || !info.notFound || wait.rechecks >= PreviewManager.PAGE_SETTLE_RETRIES || this.pageRetryTimer !== null) return;
    const request = this.navigationManager.getRequestRevision();
    this.pageRetryTimer = window.setTimeout(() => {
      this.pageRetryTimer = null;
      if (request !== this.navigationManager.getRequestRevision() || this.newPageWait !== wait) return;
      this.newPageWait = { ...wait, rechecks: wait.rechecks + 1 };
      this.loadPreview(wait.targetUrl);
    }, PreviewManager.PAGE_SETTLE_DELAY_MS);
  }

  private clearPageRetryTimer(): void {
    if (this.pageRetryTimer !== null) {
      clearTimeout(this.pageRetryTimer);
      this.pageRetryTimer = null;
    }
  }

}

/** The last segment of a folder path, for a title when no name was given. */
function folderBaseName(path: string | null): string {
  return path?.split(/[\\/]/).filter(Boolean).pop() ?? '';
}
