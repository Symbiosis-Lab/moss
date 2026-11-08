/**
 * Tests for the shell-side swipe tracker — the phase-driven gesture state
 * machine behind the preview's back/forward arrows.
 *
 * Physics constants come from WebKit's own SwipeProgressTrackerMac.mm
 * (published source): commit iff |progress| + velocity·0.3s ≥
 * min(187.5px/width, 0.5); 15px start hysteresis; |dy| ≥ 0.5|dx| cancels a
 * pending swipe (ViewGestureController.cpp).
 *
 * Native NSEvent sign convention: fingers moving right produce POSITIVE
 * scrollingDeltaX (AppKit), the inverse of DOM wheel deltaX. The tracker
 * consumes NATIVE deltas; fingers-right = +deltaX = back.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { createSwipeTracker, SWIPE_COMMIT_ABS_PX } from "../swipe/swipe-tracker";

type Dir = "back" | "forward";

describe("swipe-tracker", () => {
  let progress: Array<{ dir: Dir; p: number }>;
  let committed: Dir[];
  let cancelled: Dir[];
  let canNav: ReturnType<typeof vi.fn>;
  let tracker: ReturnType<typeof createSwipeTracker>;
  const WIDTH = 1000;

  beforeEach(() => {
    progress = [];
    committed = [];
    cancelled = [];
    canNav = vi.fn(() => true);
    tracker = createSwipeTracker({
      width: () => WIDTH,
      canNavigate: canNav,
      onProgress: (dir, p) => progress.push({ dir, p }),
      onCommit: (dir) => committed.push(dir),
      onCancel: (dir) => cancelled.push(dir),
    });
  });

  function phase(
    phase: string,
    deltaX = 0,
    deltaY = 0,
    t = 0,
    momentumPhase = "none"
  ): void {
    tracker.handlePhaseEvent({ deltaX, deltaY, phase, momentumPhase, timeStampMs: t });
  }

  /** fingers-down swipe toward `back` (positive native deltaX) */
  function drag(perEvent: number, count: number, t0 = 1000): number {
    phase("began", 0, 0, t0);
    tracker.noteWheelReport("back", false);
    let t = t0 + 8;
    for (let i = 0; i < count; i++) {
      phase("changed", perEvent, 0, t);
      t += 8;
    }
    return t;
  }

  it("commits 'back' when released past the distance threshold", () => {
    const t = drag(30, 10); // 300px > 187.5px
    phase("ended", 0, 0, t);
    expect(committed).toEqual(["back"]);
    expect(cancelled).toEqual([]);
  });

  it("cancels when released short of threshold with no velocity", () => {
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("back", false);
    // 100px over a long, slow drag → final velocity ~0.
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 10, 0, t);
      t += 200;
    }
    phase("ended", 0, 0, t + 200);
    expect(committed).toEqual([]);
    expect(cancelled).toEqual(["back"]);
  });

  it("commits a short fast flick via velocity projection", () => {
    // 100px in 40ms = 2500px/s; projected = 100 + 2500*0.3 = 850px >> 187.5
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("back", false);
    let t = 1008;
    for (let i = 0; i < 5; i++) {
      phase("changed", 20, 0, t);
      t += 8;
    }
    phase("ended", 0, 0, t);
    expect(committed).toEqual(["back"]);
  });

  it("reports progress while tracking", () => {
    drag(30, 10);
    expect(progress.length).toBeGreaterThan(0);
    const last = progress[progress.length - 1];
    expect(last.dir).toBe("back");
    expect(last.p).toBeCloseTo(300 / WIDTH, 2);
  });

  it("does not arm below the 15px start hysteresis", () => {
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("back", false);
    phase("changed", 5, 0, 1008);
    phase("changed", 5, 0, 1016);
    expect(progress).toEqual([]);
    phase("ended", 0, 0, 1024);
    expect(committed).toEqual([]);
    expect(cancelled).toEqual([]);
  });

  it("a vertically dominant pending gesture never arms", () => {
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("back", false);
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 10, 30, t); // dy ≥ 0.5·dx cumulative
      t += 8;
    }
    phase("ended", 0, 0, t);
    expect(progress).toEqual([]);
    expect(committed).toEqual([]);
  });

  it("requires a bridge wheel report (gesture over the preview): none → never arms", () => {
    // No report at all — e.g. scrolling the editor pane.
    phase("began", 0, 0, 1000);
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 30, 0, t);
      t += 8;
    }
    phase("ended", 0, 0, t);
    expect(committed).toEqual([]);
    expect(progress).toEqual([]);
  });

  it("stays idle when the bridge reports the wheel as ceded to page content", () => {
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("back", true); // ceded
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 30, 0, t);
      t += 8;
    }
    phase("ended", 0, 0, t);
    expect(committed).toEqual([]);
  });

  it("stays idle when history has no entry in that direction", () => {
    canNav.mockImplementation((d: Dir) => d !== "back");
    const t = drag(30, 10);
    phase("ended", 0, 0, t);
    expect(committed).toEqual([]);
    expect(progress).toEqual([]);
  });

  it("ignores momentum events after a commit (no double navigation)", () => {
    const t = drag(30, 10);
    phase("ended", 0, 0, t);
    expect(committed).toEqual(["back"]);
    // Momentum tail keeps flowing…
    phase("none", 40, 0, t + 20, "began");
    phase("none", 40, 0, t + 40, "changed");
    phase("none", 40, 0, t + 60, "ended");
    expect(committed).toEqual(["back"]);
  });

  it("a new began after momentum ends starts a fresh, working gesture", () => {
    const t = drag(30, 10);
    phase("ended", 0, 0, t);
    phase("none", 40, 0, t + 20, "began");
    phase("none", 10, 0, t + 40, "ended");
    const t2 = drag(30, 10, t + 500);
    phase("ended", 0, 0, t2);
    expect(committed).toEqual(["back", "back"]);
  });

  it("a new began DURING momentum interrupts it and tracks normally", () => {
    const t = drag(30, 10);
    phase("ended", 0, 0, t);
    phase("none", 40, 0, t + 20, "began");
    // Fingers touch down again mid-momentum — this is the case the v1
    // heuristics could never distinguish.
    const t2 = drag(30, 10, t + 60);
    phase("ended", 0, 0, t2);
    expect(committed).toEqual(["back", "back"]);
  });

  it("maps negative native deltaX (fingers left) to 'forward'", () => {
    phase("began", 0, 0, 1000);
    tracker.noteWheelReport("forward", false);
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", -30, 0, t);
      t += 8;
    }
    phase("ended", 0, 0, t);
    expect(committed).toEqual(["forward"]);
  });

  it("cancelled phase cancels an armed gesture", () => {
    drag(30, 10);
    phase("cancelled", 0, 0, 2000);
    expect(committed).toEqual([]);
    expect(cancelled).toEqual(["back"]);
  });

  it("exposes the commit threshold for the arrow UI", () => {
    // min(187.5/width, 0.5) on a 1000px container
    expect(SWIPE_COMMIT_ABS_PX / WIDTH).toBeCloseTo(0.1875, 4);
  });
});

/**
 * Corroboration race: the bridge report
 * may arrive BEFORE the native began (postMessage beats the Rust channel)
 * or only AFTER the tracker crosses its hysteresis. Neither ordering may
 * kill the gesture: a forward swipe right after a back navigation must still commit.
 */
describe("swipe-tracker report ordering", () => {
  let committed: Array<"back" | "forward">;
  let tracker: ReturnType<typeof createSwipeTracker>;

  beforeEach(() => {
    committed = [];
    tracker = createSwipeTracker({
      width: () => 1000,
      canNavigate: () => true,
      onProgress: () => {},
      onCommit: (d) => committed.push(d),
      onCancel: () => {},
    });
  });

  it("arms when the report arrives only after the hysteresis was crossed", () => {
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "began", momentumPhase: "none", timeStampMs: 1000 });
    let t = 1008;
    // Cross hysteresis with NO report yet — must stay pending, not die…
    for (let i = 0; i < 5; i++) {
      tracker.handlePhaseEvent({ deltaX: -30, deltaY: 0, phase: "changed", momentumPhase: "none", timeStampMs: t });
      t += 8;
    }
    // …then the report lands (bridge was slower than the native channel).
    tracker.noteWheelReport("forward", false);
    for (let i = 0; i < 5; i++) {
      tracker.handlePhaseEvent({ deltaX: -30, deltaY: 0, phase: "changed", momentumPhase: "none", timeStampMs: t });
      t += 8;
    }
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "ended", momentumPhase: "none", timeStampMs: t });
    expect(committed).toEqual(["forward"]);
  });

  it("corroborates from per-event reports interleaved with native events", () => {
    // Production shape: the bridge posts one report per horizontal wheel
    // event, so reports keep arriving throughout the gesture regardless of
    // which stream wins any single race.
    tracker.noteWheelReport("back", false); // may even precede began
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "began", momentumPhase: "none", timeStampMs: 1000 });
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      tracker.handlePhaseEvent({ deltaX: 30, deltaY: 0, phase: "changed", momentumPhase: "none", timeStampMs: t });
      tracker.noteWheelReport("back", false);
      t += 8;
    }
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "ended", momentumPhase: "none", timeStampMs: t });
    expect(committed).toEqual(["back"]);
  });

  it("a stale report alone (nothing during the gesture) never corroborates", () => {
    tracker.noteWheelReport("back", false); // stamped on a pristine clock
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "began", momentumPhase: "none", timeStampMs: 50000 });
    let t = 50008;
    for (let i = 0; i < 10; i++) {
      tracker.handlePhaseEvent({ deltaX: 30, deltaY: 0, phase: "changed", momentumPhase: "none", timeStampMs: t });
      t += 8;
    }
    tracker.handlePhaseEvent({ deltaX: 0, deltaY: 0, phase: "ended", momentumPhase: "none", timeStampMs: t });
    expect(committed).toEqual([]);
  });
});

/**
 * Final-review regressions: velocity formula (summed positions inflated it
 * ~n-fold → slow releases committed) and gesture-scoped corroboration (a
 * fresh-but-previous-gesture preview report must not corroborate a gesture
 * over the editor pane).
 */
describe("swipe-tracker final-review regressions", () => {
  let committed: string[];
  let cancelled: string[];
  let tracker: ReturnType<typeof createSwipeTracker>;

  beforeEach(() => {
    committed = [];
    cancelled = [];
    tracker = createSwipeTracker({
      width: () => 1000,
      canNavigate: () => true,
      onProgress: () => {},
      onCommit: (d) => committed.push(d),
      onCancel: (d) => cancelled.push(d),
    });
  });

  function ev(phase: string, dx: number, t: number, momentum = "none"): void {
    tracker.handlePhaseEvent({ deltaX: dx, deltaY: 0, phase, momentumPhase: momentum, timeStampMs: t });
  }

  it("cancels a slow release even when samples are inside the velocity window", () => {
    ev("began", 0, 1000);
    // 60px over 360ms → true v≈167px/s → projected 60+50=110 < 187.5.
    // The old summed-positions formula inflated v ~6x and committed.
    let t = 1030;
    for (let i = 0; i < 12; i++) {
      ev("changed", 5, t);
      tracker.noteWheelReport("back", false);
      t += 30;
    }
    ev("ended", 0, t);
    expect(committed).toEqual([]);
    expect(cancelled).toEqual(["back"]);
  });

  it("a report from BEFORE this gesture never corroborates it (editor-pane gesture)", () => {
    // Previous activity over the preview: sub-hysteresis wiggle + reports
    // (stays pending, never arms — but reports are recorded).
    ev("began", 0, 1000);
    let t = 1008;
    for (let i = 0; i < 3; i++) {
      ev("changed", 4, t);
      tracker.noteWheelReport("back", false);
      t += 8;
    }
    ev("ended", 0, t);
    ev("changed", 5, t + 16, "ended"); // momentum closes → idle

    // New gesture 200ms later — over the EDITOR pane, so no new preview
    // reports arrive. The old ≤600ms freshness let the stale report
    // corroborate; gesture-scoping must reject it.
    const t2 = t + 200;
    ev("began", 0, t2);
    let u = t2 + 8;
    for (let i = 0; i < 12; i++) {
      ev("changed", 30, u);
      u += 8;
    }
    ev("ended", 0, u);
    expect(committed).toEqual([]);
  });
});
