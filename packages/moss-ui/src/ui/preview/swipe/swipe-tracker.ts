/**
 * Shell-side swipe tracker: the phase-driven gesture state machine behind
 * the preview's back/forward navigation arrows.
 *
 * Input is the native NSEvent scroll-wheel phase stream (see
 * the host's macOS swipe-phase adapter) — DOM wheel events carry no
 * phase/momentum info, so a wheel heuristic cannot reliably tell finger-lift
 * from momentum. Sign convention is
 * AppKit's: fingers moving right produce POSITIVE deltaX (the inverse of
 * DOM wheel), and fingers-right means "back" on macOS.
 *
 * Physics are WebKit's own, from the published SwipeProgressTrackerMac.mm /
 * ViewGestureController.cpp sources:
 *   - 15 px cumulative horizontal start hysteresis
 *   - a pending gesture whose cumulative |dy| ≥ 0.5·|dx| is a scroll, not a
 *     swipe (checked before arming; once armed, vertical drift is ignored)
 *   - commit on finger-lift iff |progress| + velocity·0.3 s ≥
 *     min(187.5 px, width/2) / width
 *   - velocity from a trailing 150 ms window (libadwaita/WebKit practice)
 *   - momentum events never start a gesture; after lift they are swallowed
 *     until momentum ends — but a new fingers-down began interrupts them
 *     and tracks normally (the case wheel heuristics cannot distinguish).
 *
 * The gesture must also be corroborated by the preview iframe bridge:
 * stateless per-wheel-event `moss-swipe-wheel` reports prove the wheel
 * events happened over the preview (not the editor pane) and say whether
 * page content can consume the scroll (horizontally scrollable container,
 * wheel-cancelling site UI). `noteWheelReport` feeds them in. Reports are
 * per-event and age-based on purpose: the bridge has no phase info, so any
 * gesture segmentation it attempted raced the native stream (postMessage
 * vs Rust channel ordering is unknowable) and momentum chaining starved
 * whole gestures of their candidate. A pending gesture with no fresh matching report WAITS instead
 * of dying; reports older than the freshness window are ignored.
 */

export type SwipeDir = "back" | "forward";

export interface SwipePhaseInput {
  deltaX: number;
  deltaY: number;
  phase: string;
  momentumPhase: string;
  timeStampMs: number;
}

export interface SwipeTrackerOptions {
  /** Preview container width in px (progress denominator). */
  width(): number;
  /** Whether history can navigate in the given direction. */
  canNavigate(dir: SwipeDir): boolean;
  /** Continuous progress toward commit, 0..1+ of container width. */
  onProgress(dir: SwipeDir, progress: number): void;
  onCommit(dir: SwipeDir): void;
  onCancel(dir: SwipeDir): void;
}

/** WebKit swipeGestureMinimumHorizontalSwipeDistance. */
export const SWIPE_START_HYSTERESIS_PX = 15;
/** WebKit SwipeProgressTracker: commit iff projected ≥ min(187.5px, w/2)/w. */
export const SWIPE_COMMIT_ABS_PX = 187.5;
export const SWIPE_COMMIT_MAX_RATIO = 0.5;
/** WebKit kineticProjectionDuration (300 ms), in seconds. */
export const SWIPE_PROJECTION_S = 0.3;
/** Velocity estimation window (WebKit/libadwaita practice). */
const VELOCITY_WINDOW_MS = 150;
/** Reports older than this (tracker clock) never corroborate a gesture. */
const REPORT_FRESHNESS_MS = 600;

type State = "idle" | "pending" | "tracking" | "dead" | "swallowing";

export function createSwipeTracker(options: SwipeTrackerOptions): {
  handlePhaseEvent(event: SwipePhaseInput): void;
  noteWheelReport(dir: SwipeDir, ceded: boolean): void;
} {
  let state: State = "idle";
  let accX = 0;
  let accY = 0;
  let dir: SwipeDir | null = null;
  let samples: Array<{ t: number; x: number }> = [];
  // Latest bridge report per direction, stamped with the tracker's own
  // clock (last native event time) at ARRIVAL. Cross-clock comparison is
  // avoided: both the stamp and the freshness check use native timestamps.
  const reports: Record<SwipeDir, { ceded: boolean; at: number } | null> = {
    back: null,
    forward: null,
  };
  let lastEventAt = 0;
  let beganAt = 0;

  function reset(): void {
    state = "idle";
    accX = 0;
    accY = 0;
    dir = null;
    samples = [];
  }

  function begin(): void {
    // A fresh fingers-down gesture supersedes anything in flight —
    // including the momentum tail of the previous gesture.
    reset();
    beganAt = lastEventAt;
    state = "pending";
  }

  function velocityPxPerS(now: number): number {
    const cutoff = now - VELOCITY_WINDOW_MS;
    const recent = samples.filter((s) => s.t >= cutoff);
    if (recent.length < 2) return 0;
    const dt = recent[recent.length - 1].t - recent[0].t;
    if (dt <= 0) return 0;
    const dx = recent[recent.length - 1].x - recent[0].x;
    return (dx / dt) * 1000;
  }

  function progressOf(x: number): number {
    const width = Math.max(1, options.width());
    const toward = dir === "back" ? x : -x;
    return Math.max(0, toward / width);
  }

  function commitThreshold(): number {
    const width = Math.max(1, options.width());
    return Math.min(SWIPE_COMMIT_ABS_PX / width, SWIPE_COMMIT_MAX_RATIO);
  }

  function endGesture(now: number): void {
    if (state === "tracking" && dir) {
      const width = Math.max(1, options.width());
      const towardVelocity =
        (dir === "back" ? velocityPxPerS(now) : -velocityPxPerS(now)) / width;
      const projected = progressOf(accX) + towardVelocity * SWIPE_PROJECTION_S;
      if (projected >= commitThreshold()) {
        options.onCommit(dir);
      } else {
        options.onCancel(dir);
      }
    }
    // Whatever happened, the momentum tail (if any) belongs to this
    // gesture and must not start another.
    reset();
    state = "swallowing";
  }

  function handlePhaseEvent(event: SwipePhaseInput): void {
    lastEventAt = event.timeStampMs;

    if (event.phase === "began") {
      begin();
      return;
    }

    if (state === "idle" || state === "swallowing") {
      // Momentum (or stray) events never start a gesture. Swallowing ends
      // when momentum reports ended/cancelled.
      if (
        state === "swallowing" &&
        (event.momentumPhase === "ended" || event.momentumPhase === "cancelled")
      ) {
        state = "idle";
      }
      return;
    }

    if (event.phase === "cancelled") {
      if (state === "tracking" && dir) options.onCancel(dir);
      reset();
      state = "swallowing";
      return;
    }

    if (event.phase === "ended") {
      if (state === "dead" || state === "pending") {
        reset();
        state = "swallowing";
        return;
      }
      endGesture(event.timeStampMs);
      return;
    }

    if (event.phase !== "changed") return;

    accX += event.deltaX;
    accY += event.deltaY;
    samples.push({ t: event.timeStampMs, x: accX });

    if (state === "pending") {
      // A predominantly vertical pending gesture is a scroll for its whole
      // life (WebKit deltaShouldCancelSwipe: |dy| ≥ 0.5·|dx|).
      if (Math.abs(accY) >= 0.5 * Math.abs(accX) && Math.abs(accY) > 10) {
        state = "dead";
        return;
      }
      if (Math.abs(accX) < SWIPE_START_HYSTERESIS_PX) return;

      const wantDir: SwipeDir = accX > 0 ? "back" : "forward";
      if (!options.canNavigate(wantDir)) {
        state = "dead";
        return;
      }
      const report = reports[wantDir];
      // The report must belong to THIS gesture: arrived at/after began
      // (small slack for the arrival-order race). Momentum stops when new
      // fingers touch down, so a gesture over the editor pane gets no
      // fresh preview reports and never corroborates — while a genuine
      // preview gesture's per-event reports land within a frame. When the
      // report is missing (still in flight) or ceded (possibly the
      // previous gesture's tail — reports carry no gesture identity),
      // WAIT and re-check on the next event rather than killing.
      const fromThisGesture =
        report !== null &&
        report.at >= beganAt - 50 &&
        event.timeStampMs - report.at <= REPORT_FRESHNESS_MS;
      if (!fromThisGesture || report.ceded) {
        return;
      }
      dir = wantDir;
      state = "tracking";
    }

    if (state === "tracking" && dir) {
      options.onProgress(dir, progressOf(accX));
    }
  }

  function noteWheelReport(reportDir: SwipeDir, ceded: boolean): void {
    // Stamped with the tracker clock at arrival. Reports are per-event, so
    // even if one report loses a race (or stamps on a pristine clock), the
    // next one — a frame later — corroborates; pending waits for it.
    reports[reportDir] = { ceded, at: lastEventAt };
  }

  return { handlePhaseEvent, noteWheelReport };
}
