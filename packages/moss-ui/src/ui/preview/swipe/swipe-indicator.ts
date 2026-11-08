/**
 * Swipe arrow indicator — the visible half of preview swipe navigation.
 *
 * Renders two translucent arrow chips (the Chromium/Firefox grammar: a
 * circle + chevron sliding in from the edge, brightening with progress and
 * locking past the commit threshold) as SHELL chrome over the preview
 * container — never injected into the previewed page. The gesture itself is
 * decided by swipe-tracker.ts, fed by the native NSEvent phase stream
 * (preview_swipe_phase_stream command); the preview iframe bridge
 * corroborates that the wheel events happened over the preview and that
 * page content didn't claim them (per-event `moss-swipe-wheel` reports).
 *
 * On platforms without the native stream the indicator stays inert and the
 * v1 wheel-heuristic path in the bridge (`moss-swipe-nav`) remains the
 * fallback — NavigationManager consults `nativeActive` to suppress it when
 * the interactive path owns the gesture.
 */
import {
  createSwipeTracker,
  SWIPE_COMMIT_ABS_PX,
  SWIPE_COMMIT_MAX_RATIO,
  type SwipeDir,
} from "./swipe-tracker";

/** Wire shape of the native phase stream (snake_case per moss bindings). */
export interface SwipePhasePayload {
  delta_x: number;
  delta_y: number;
  phase: string;
  momentum_phase: string;
  location_x: number;
  location_y: number;
  timestamp_ms: number;
  swipe_tracking_enabled: boolean;
  direction_inverted_from_device: boolean;
}

export interface SwipeIndicatorOptions {
  container: HTMLElement;
  iframe: HTMLIFrameElement | null;
  navigate: {
    canGoBack(): boolean;
    canGoForward(): boolean;
    goBack(): Promise<void> | void;
    goForward(): Promise<void> | void;
  };
  /**
   * Subscribes to the native phase stream; resolves true when streaming.
   * Provided by the caller (NavigationManager wires the
   * preview_swipe_phase_stream command), so this module reaches no host
   * directly.
   */
  subscribe: (onEvent: (e: SwipePhasePayload) => void) => Promise<boolean>;
}

/** Chip travel from hidden to fully slid-in. Deliberately SHORT (28px, vs
 * Chromium's ~70pt): the chip must read as materializing just inside the
 * preview's edge, not as sliding out from under the chrome that floats at
 * the container boundary (control pills on the right). Opacity carries most
 * of the entrance. */
const ARROW_TRAVEL_PX = 28;
const COMMIT_FADE_MS = 400;
const CANCEL_RETRACT_MS = 150;

export class SwipeIndicator {
  private options: SwipeIndicatorOptions;
  private tracker: ReturnType<typeof createSwipeTracker>;
  private arrows: Record<SwipeDir, HTMLElement | null> = {
    back: null,
    forward: null,
  };
  private hideTimers: Record<SwipeDir, ReturnType<typeof setTimeout> | null> = {
    back: null,
    forward: null,
  };
  private messageListener: ((event: MessageEvent) => void) | null = null;
  private disposed = false;

  /** True when the native phase stream drives the gesture (macOS). */
  nativeActive = false;

  constructor(options: SwipeIndicatorOptions) {
    this.options = options;
    this.tracker = createSwipeTracker({
      width: () => this.options.container.clientWidth || window.innerWidth,
      canNavigate: (dir) =>
        dir === "back"
          ? this.options.navigate.canGoBack()
          : this.options.navigate.canGoForward(),
      onProgress: (dir, p) => this.renderProgress(dir, p),
      onCommit: (dir) => this.commit(dir),
      onCancel: (dir) => this.retract(dir),
    });

    // Bridge corroboration: stateless per-wheel-event reports prove the
    // gesture is over the preview and report content-cede. Only the preview
    // iframe may send them.
    this.messageListener = (event: MessageEvent) => {
      if (event.data?.type !== "moss-swipe-wheel") return;
      if (event.source !== this.options.iframe?.contentWindow) return;
      const dir = event.data.direction;
      if (dir !== "back" && dir !== "forward") return;
      this.tracker.noteWheelReport(dir, event.data.ceded === true);
    };
    window.addEventListener("message", this.messageListener);
  }

  /** Subscribe to the native stream; inert (fallback mode) on failure. */
  async start(): Promise<void> {
    this.nativeActive = await this.options.subscribe((e) => {
      // Kill-switch parity with the fallback path (morph convention).
      if (localStorage.getItem("moss.preview.swipe") === "0") return;
      // Ring buffer for live sign/phase calibration via the MCP bridge
      // (window.__mossSwipeDebug in the shell webview).
      const dbg = ((window as unknown as { __mossSwipeDebug?: unknown[] }).__mossSwipeDebug ??= []);
      dbg.push({ t: e.timestamp_ms, p: e.phase, m: e.momentum_phase, dx: e.delta_x, dy: e.delta_y });
      if (dbg.length > 200) dbg.splice(0, dbg.length - 200);
      this.tracker.handlePhaseEvent({
        deltaX: e.delta_x,
        deltaY: e.delta_y,
        phase: e.phase,
        momentumPhase: e.momentum_phase,
        timeStampMs: e.timestamp_ms,
      });
    });
  }

  private commitThresholdProgress(): number {
    const width = this.options.container.clientWidth || window.innerWidth;
    return Math.min(SWIPE_COMMIT_ABS_PX / Math.max(1, width), SWIPE_COMMIT_MAX_RATIO);
  }

  private arrow(dir: SwipeDir): HTMLElement {
    let el = this.arrows[dir];
    if (el) return el;
    el = document.createElement("div");
    el.className = `moss-swipe-arrow moss-swipe-arrow--${dir}`;
    // Chevron pointing toward the navigation direction.
    el.innerHTML =
      dir === "back"
        ? `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.5 6 9 12l5.5 6" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>`
        : `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9.5 6 15 12l-5.5 6" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    this.options.container.appendChild(el);
    this.arrows[dir] = el;
    return el;
  }

  private renderProgress(dir: SwipeDir, progress: number): void {
    if (this.disposed) return;
    const timer = this.hideTimers[dir];
    if (timer) {
      clearTimeout(timer);
      this.hideTimers[dir] = null;
    }
    const el = this.arrow(dir);
    const fraction = Math.min(progress / this.commitThresholdProgress(), 1);
    el.classList.add("is-visible");
    el.classList.remove("is-committing");
    el.classList.toggle("is-armed", fraction >= 1);
    // Slide in from the edge proportionally to progress-toward-commit;
    // brightness follows via CSS on the custom property.
    el.style.setProperty("--swipe-fraction", fraction.toFixed(4));
    el.style.transform = this.transformFor(dir, fraction);
  }

  private transformFor(dir: SwipeDir, fraction: number): string {
    const travel = ARROW_TRAVEL_PX * fraction;
    return dir === "back"
      ? `translate(${(-ARROW_TRAVEL_PX + travel).toFixed(1)}px, -50%)`
      : `translate(${(ARROW_TRAVEL_PX - travel).toFixed(1)}px, -50%)`;
  }

  private commit(dir: SwipeDir): void {
    if (this.disposed) return;
    const el = this.arrow(dir);
    el.classList.add("is-armed", "is-committing");
    void (dir === "back"
      ? this.options.navigate.goBack()
      : this.options.navigate.goForward());
    this.scheduleHide(dir, COMMIT_FADE_MS);
  }

  private retract(dir: SwipeDir): void {
    this.scheduleHide(dir, CANCEL_RETRACT_MS);
  }

  private scheduleHide(dir: SwipeDir, afterMs: number): void {
    const el = this.arrow(dir);
    el.classList.remove("is-visible");
    el.style.transform = this.transformFor(dir, 0);
    const timer = this.hideTimers[dir];
    if (timer) clearTimeout(timer);
    this.hideTimers[dir] = setTimeout(() => {
      this.hideTimers[dir] = null;
      el.classList.remove("is-armed", "is-committing");
    }, afterMs);
  }

  /**
   * Detach from the window and stop driving navigation. The Channel itself
   * cannot be unsubscribed; the disposed flag makes late events inert, and
   * any later indicator's subscription swaps the Rust-side sink anyway.
   */
  dispose(): void {
    this.disposed = true;
    if (this.messageListener) {
      window.removeEventListener("message", this.messageListener);
      this.messageListener = null;
    }
    for (const dir of ["back", "forward"] as const) {
      const timer = this.hideTimers[dir];
      if (timer) clearTimeout(timer);
      this.hideTimers[dir] = null;
      this.arrows[dir]?.remove();
      this.arrows[dir] = null;
    }
    this.nativeActive = false;
  }
}
