// One guard pass on the preview overlay: the controller that connects the
// transport (guard events in, one decision out — seam.ts) to the threshold
// screen surface (threshold-screen.ts) and to the stage machine it borrows
// the overlay from.
//
// The threshold states live strictly BEFORE the build of the folder being
// opened — but the webview receiving them may be showing a LIVE site: the
// guard only runs while a preview webview exists on a folder-switch open.
// So a guard-driven paint remembers the covered stage and restores it when
// the pass resolves cancelled; a committed pass leaves the screen up for the
// new open's overlay to paint over, preventing an interstitial flash.

import { t } from "../../i18n/index";
import { STAGE_RANK } from '../progress/progress-panel-stage';
import type { PreviewStage } from '../preview-state';
import {
  createThresholdTransport,
  isThresholdRefusal,
  type ThresholdPrompt,
} from './seam';
import type { AcknowledgeThresholdPrompt, GuardDecision, SubmitThresholdDecision } from "../../host";
import { ThresholdScreenSurface } from './threshold-screen';

export interface ThresholdPassDeps {
  /** The overlay slot (`#moss-preview-overlay`), made ready per call — the
   *  manager owns the element's lifetime and the blueprint grid behind it, so
   *  this pass never starts or stops one. */
  raiseOverlay(): HTMLElement | null;
  /** Take the overlay back to empty (grid included). */
  clearOverlay(): void;
  getStage(): PreviewStage;
  setStage(stage: PreviewStage): void;
  /** Forget the shared full-window screen before the prompt takes the slot. */
  dropWaitingScreen(): void;
  /** Paint the walk's live count on the shared full-window screen — the one
   *  surface that owns waiting, so this pass owns only the question. */
  showChecking(dirsVisited: number): void;
  /** Checking-narration echo for the hairline advisory (null = ended). */
  narrate(text: string | null): void;
  onError(message: string): void;
  /** The decision command, injected by the manager from the host's
   *  nested-site guard. */
  submitDecision: SubmitThresholdDecision;
  /** The prompt-receipt ack — `commands.acknowledgeThresholdPrompt`, injected
   *  for the same reason. The guard's bounded ack wait is what turns a
   *  prompt emitted at an unmounted listener into a dialog fallback instead
   *  of a hang. */
  acknowledgePrompt: AcknowledgeThresholdPrompt;
}

export class ThresholdPass {
  /** The screen surface; exposed so the manager's overlay-replacing paths
   *  can `drop()` it (same contract as the waiting screen). */
  readonly screen: ThresholdScreenSurface;

  /** What the stage machine showed before a guard pass borrowed the overlay.
   *  null = no pass in flight. */
  private prevStage: PreviewStage | null = null;

  /** The live guard pass's generation token (null = no pass adopted).
   *  Tokens are minted monotonically backend-side, so "newest wins": an
   *  event from an older pass is ignored, and a late ThresholdResolved for
   *  an old token never tears down the current screen. */
  private token: number | null = null;

  private readonly transport: ReturnType<typeof createThresholdTransport>;

  constructor(private readonly deps: ThresholdPassDeps) {
    this.transport = createThresholdTransport(
      {
        showChecking: (dirs, token) => {
          if (!this.adopt(token)) return;
          this.showChecking(dirs, { guard: true });
        },
        showPrompt: (prompt) => {
          if (!this.adopt(prompt.token)) return;
          this.showPrompt(prompt, false, { guard: true });
        },
        close: (cancelled, token) => {
          if (this.token === null || token !== this.token) return; // stale resolve
          this.close(cancelled);
        },
      },
      deps.submitDecision,
      deps.acknowledgePrompt,
    );
    this.screen = new ThresholdScreenSurface({
      // Success does NOT clear the screen: the guard answers with a
      // ThresholdResolved event, which `close` handles — one teardown path
      // whichever side ends the pass.
      decide: (decision: GuardDecision) => this.decide(decision),
    });
  }

  /**
   * Send one decision, from whichever screen asked for it — the prompt's
   * cards or the checking screen's escape. Both are answers to the same
   * parked question, so they go out the same door.
   */
  decide(decision: GuardDecision): void {
    const token = this.token;
    if (token === null) {
      // No adopted pass — nothing backend-side awaits this. Treat like a dead
      // question rather than leaving a disarmed screen up.
      this.close(true);
      return;
    }
    this.transport.decide(decision, token).catch((err) => {
      const message = err instanceof Error ? err.message : String(err);
      if (isThresholdRefusal(message)) {
        // The question is dead backend-side (answered, superseded, or nobody
        // awaiting) — the screen has nothing left to ask.
        this.close(true);
      } else {
        // Transient failure: the question is still parked backend-side, so
        // re-arm the controls instead of stranding an invisible pending
        // question behind a dead screen.
        this.deps.onError(`Threshold decision failed: ${message}`);
        this.screen.rearm();
      }
    });
  }

  /** Adopt a pass token, newest-wins. False = the event is from an older
   *  pass and must be dropped. */
  private adopt(token: number): boolean {
    if (this.token !== null && token < this.token) return false;
    this.token = token;
    return true;
  }

  /** Subscribe to the guard's events; returns the combined unlisten. */
  start(): Promise<() => void> {
    return this.transport.start();
  }

  /**
   * A guard-driven paint belongs to a NEW open (the guard only runs before
   * anything is written), so it may cover a live site. It remembers the
   * covered stage so a cancel puts the machine back where it was. Non-guard
   * callers keep the monotonic gate: a straggler tick landing after the
   * build started must not repaint "Checking…" over the build overlay.
   */
  private beginsPass(guard: boolean): boolean {
    if (!guard) {
      return STAGE_RANK[this.deps.getStage()] < STAGE_RANK['scanning'];
    }
    if (this.prevStage === null) {
      this.prevStage = this.deps.getStage();
    }
    return true;
  }

  /** Detection narration: live folder count on the overlay, mirrored to the
   *  hairline advisory. Safe to call on every progress tick. */
  showChecking(dirsVisited: number, opts: { guard?: boolean } = {}): void {
    // The gate FIRST: raising the overlay puts a canvas up and starts the
    // grid, so asking it before `beginsPass` would let a straggler tick paint
    // over a live site with the very screen it is being refused. This also
    // means `beginsPass`'s covered-stage bookkeeping now runs before the
    // overlay is resolved — which is the right order (a pass that is refused
    // records nothing), and the overlay is resolved at initialize() anyway.
    if (!this.beginsPass(opts.guard === true)) return;
    const overlay = this.deps.raiseOverlay();
    if (!overlay) return;
    this.deps.setStage('checking');
    // The prompt surface must let go of the slot: waiting and asking are two
    // screens, and only one of them is up at a time.
    this.screen.drop();
    this.deps.showChecking(dirsVisited);
    this.deps.narrate(t('threshold.checking', { count: String(dirsVisited) }));
  }

  /** The threshold question — cases (a)/(b)/(d). `stillChecking` keeps the
   *  muted footer up while the walk is still filling in candidates. */
  showPrompt(prompt: ThresholdPrompt, stillChecking = false, opts: { guard?: boolean } = {}): void {
    if (!this.beginsPass(opts.guard === true)) return;
    const overlay = this.deps.raiseOverlay();
    if (!overlay) return;
    this.deps.setStage('threshold');
    this.deps.narrate(null);
    this.deps.dropWaitingScreen();
    this.screen.showPrompt(overlay, prompt, stillChecking);
  }

  /** Decision made (or detection found nothing): the slot goes back to the
   *  ordinary build lifecycle. */
  clear(): void {
    this.deps.narrate(null);
    this.deps.dropWaitingScreen();
    this.screen.drop();
  }

  /**
   * The guard pass ended (`ThresholdResolved`, or a failed decision send).
   * A cancelled question restores what the overlay covered — the open never
   * happened, so the prior site's state stands. A committed one leaves the
   * screen up for the new open's overlay to paint over — that open replaces
   * or reloads this webview anyway.
   */
  close(cancelled: boolean): void {
    const prev = this.prevStage;
    this.prevStage = null;
    this.token = null;
    if (!cancelled) return;
    this.clear();
    this.deps.clearOverlay();
    if (prev !== null) {
      this.deps.setStage(prev);
    }
  }
}
