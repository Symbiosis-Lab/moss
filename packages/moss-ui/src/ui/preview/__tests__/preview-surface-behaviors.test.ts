/**
 * Preserved-behavior STATE tests for ProgressPanel.
 *
 * These tests assert behavioral contracts of ProgressPanel that must hold across
 * any redesign. They are intentionally state-level (no DOM class assertions on
 * symbols a redesign may remove).
 *
 * Run with: pnpm vitest run packages/moss-ui/src/ui/preview/__tests__/preview-surface-behaviors.test.ts
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProgressPanel } from "../progress/progress-panel";
import type { ProgressTask } from "../progress/progress-panel";
import { host } from "../../host";

// ─── helpers ──────────────────────────────────────────────────────────────────

function makePanel(): { panel: ProgressPanel; container: HTMLElement } {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const panel = new ProgressPanel(container);
  return { panel, container };
}

function task(overrides: Partial<ProgressTask> = {}): ProgressTask {
  return {
    id: "task-1",
    label: "Test Task",
    progress: 50,
    source: "moss",
    ...overrides,
  };
}

/** Fire a BackgroundProgress moss-event via the init() listener chain. */
async function initAndGetFireFn(panel: ProgressPanel) {
  const listenCallbacks = new Map<string, Array<(ev: any) => void>>();
  host.engine.on = async (name: string, cb: any) => {
    const list = listenCallbacks.get(name) ?? [];
    list.push(cb);
    listenCallbacks.set(name, list);
    return () => {};
  };
  await panel.init();

  const fireMossEvent = (payload: any) => {
    for (const cb of listenCallbacks.get("moss-event") ?? []) {
      cb(payload);
    }
  };
  const fireDeployProgress = (payload: any) => {
    for (const cb of listenCallbacks.get("deploy-progress") ?? []) {
      cb(payload);
    }
  };
  return { fireMossEvent, fireDeployProgress, listenCallbacks };
}

// ─── 1. Awaiting persists past the stale-removal timeout ─────────────────────

describe("Behavior: Awaiting interrupt persists — not removed by stale-timer", () => {
  it("an Awaiting Job stays in getActiveTasks() well past STALE_TIMEOUT_MS", () => {
    vi.useFakeTimers();
    try {
      const { panel, container } = makePanel();
      panel.handleJobUpdate({
        id: "aw-1",
        window_id: "main",
        scope: "preview",
        kind: "deploy",
        tone: "ambient",
        title: "Verify your email",
        fade_after_ms: null,
        started_at_ms: 0,
        verb: null,
        amount: null,
        elapsed_ms: null,
        parent: null,
        state: { state: "awaiting", directive: "Click the link in your email", action: "None" },
      });

      expect(panel.getActiveTasks()).toHaveLength(1);

      // 2× the default 15s stale timeout — a non-awaiting row would have been culled.
      vi.advanceTimersByTime(30_000);
      expect(panel.getActiveTasks()).toHaveLength(1);

      panel.destroy();
      document.body.removeChild(container);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ─── 2. Ambient task does NOT auto-surface the panel ─────────────────────────

describe("Behavior: ambient task does NOT auto-surface panel", () => {
  it("upsertTask with ambient=true does not un-hide a settled panel", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    // Settle the panel to a hidden state.
    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    // VideoConversionProgress is ambient — it must NOT pop the panel.
    fireMossEvent({
      kind: "VideoConversionProgress",
      payload: {
        phase: "encoding",
        current_video: 1,
        total_videos: 3,
        filename: "clip.mp4",
        overall_fraction: 0.33,
      },
    });

    expect(panel.isHidden).toBe(true);
    // The task IS recorded (state tracking), just not surfaced.
    expect(panel.getActiveTasks().some((t) => t.id === "bg:videos")).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 3. shouldSurface / checkAutoClose gating ────────────────────────────────

describe("Behavior: shouldSurface / checkAutoClose gating", () => {
  it("pinned panel does NOT auto-close when advisory clears (the race fix)", () => {
    const { panel, container } = makePanel();
    panel.pin();
    // With publish idle, no tasks, settled stage — checkAutoClose would normally
    // close a surfaced panel. A pinned panel must resist.
    panel.setStage("ready");
    expect(panel.getRevealState()).toBe("pinned");
    expect(panel.isHidden).toBe(false);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("surfaced panel DOES auto-close after last task removed and stage settles", () => {
    const { panel, container } = makePanel();
    panel.show(); // surfaced
    panel.upsertTask(task({ id: "t1" }));
    panel.removeTask("t1");
    // No tasks, no advisories, no publish — auto-close should have fired.
    expect(panel.isHidden).toBe(true);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("checkAutoClose is suppressed while advisories are present", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    // Raise a ShippedDegraded advisory to fill advisories.size.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "clip.MOV",
          what: "unsupported codec",
          action: "None",
        }],
      },
    });

    panel.show(); // ensure it's surfaced
    // Settle the build stage.
    panel.setStage("ready");
    // Advisory is still present → checkAutoClose must NOT close the panel.
    expect(panel.isHidden).toBe(false);
    expect(panel.getAdvisories()).toHaveLength(1);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("checkAutoClose is suppressed while publish stage is active", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // Engage publish.
    fireDeployProgress({ stage: "uploading", current: 3, total: 10, message: "" });
    expect(panel.getPublishStage()).toBe("uploading");
    expect(panel.isHidden).toBe(false);

    // Attempt to auto-close via a settled build stage.
    // setStage("ready") calls checkAutoClose internally.
    panel.setStage("ready");
    // Publish still running → must stay open.
    expect(panel.isHidden).toBe(false);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 4. Narration Law-2: failed narrated task never auto-fades ───────────────

describe("Behavior: narration Law-2 — failed narrated task never auto-fades", () => {
  it("failed narrated task persists past 2s (linger+fade window)", () => {
    vi.useFakeTimers();
    try {
      const { panel, container } = makePanel();
      panel.handleJobUpdate({
        id: "narrated-fail",
        window_id: "main",
        scope: "workspace",
        kind: "import",
        tone: "narrated",
        title: "Sync to Matters",
        fade_after_ms: null,
        started_at_ms: 0,
        verb: null,
        amount: null,
        elapsed_ms: null,
        parent: null,
        state: {
          state: "failed",
          advisory: {
            scope: "Config",
            severity: "Blocking",
            item: null,
            what: "network down",
            action: "None",
          },
        },
      });

      // Advance well past any linger+fade window.
      vi.advanceTimersByTime(5_000);

      const narrated = panel.getNarratedTasks();
      const failedTask = narrated.find((t) => t.id.includes("narrated-fail"));
      expect(failedTask).toBeDefined();

      panel.destroy();
      document.body.removeChild(container);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ─── 5. Advisory-dot survives panel teardown ──────────────────────────────────

describe("Behavior: advisory-dot survives panel teardown (seam sibling, not panel child)", () => {
  it("the advisory dot is NOT inside the panel element", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    // Raise an advisory to set the dot.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "clip.MOV",
          what: "unsupported codec",
          action: "None",
        }],
      },
    });

    // The panel element must NOT own the advisory dot.
    const panelEl = container.querySelector(".moss-progress-panel");
    if (panelEl) {
      expect(panelEl.querySelector(".moss-advisory-dot")).toBeNull();
    }

    // Advisory is in the store — it can drive the seam dot via onAdvisoryChange.
    expect(panel.getAdvisories()).toHaveLength(1);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("onTopSeverityChange fires 'degraded' when advisory added, 'none' when cleared at build-start", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    const changes: string[] = [];
    panel.onTopSeverityChange((severity) => changes.push(severity));
    // onTopSeverityChange fires immediately with current state ("none").
    expect(changes).toEqual(["none"]);

    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "clip.MOV",
          what: "x",
          action: "None",
        }],
      },
    });
    expect(changes).toEqual(["none", "degraded"]);

    // Build-start clears advisories → severity goes to "none".
    panel.setStage("opening");
    expect(changes).toEqual(["none", "degraded", "none"]);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 6. Deploy / DNS / preview-server-error surfaces the panel ───────────────

describe("Behavior: consequential events surface the panel from hidden", () => {
  it("an engaged deploy (deploy-progress uploading) surfaces the panel", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    fireDeployProgress({ stage: "uploading", current: 1, total: 5, message: "" });
    expect(panel.isHidden).toBe(false);
    expect(panel.getPublishStage()).toBe("uploading");

    panel.destroy();
    document.body.removeChild(container);
  });

  // PreviewServerFailed does not add a task row.
  // It surfaces via lastFailure (red status) + a broken advisory. Panel still surfaces.
  it("PreviewServerFailed surfaces the panel (not silenced even from hidden) — via advisory", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    fireMossEvent({
      kind: "PreviewServerFailed",
      payload: { message: "Address already in use (port 1430)" },
    });

    expect(panel.isHidden).toBe(false);
    // A broken advisory is shown, not a task row.
    expect(panel.getActiveTasks().some((t) => t.id === "preview-server-error")).toBe(false);
    expect(panel.getAdvisories().some((a) => a.task === "preview-server-error")).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("a Preview-scope domain Job (running) surfaces the panel", () => {
    const { panel, container } = makePanel();
    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    panel.handleJobUpdate({
      id: "domain-1",
      window_id: "main",
      scope: "preview",
      kind: "deploy",
      tone: "ambient",
      title: "Connecting example.test",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: { state: "running", fraction: 0.3, message: null },
    });

    // Domain Job is a consequential (non-ambient) task — surfaces the panel.
    expect(panel.isHidden).toBe(false);
    const tasks = panel.getActiveTasks();
    expect(tasks.some((t) => t.label === "Connecting example.test")).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 7. Narrated rows still render ───────────────────────────────────────────

describe("Behavior: narrated task rows render via getNarratedTasks()", () => {
  it("non-preview narrated task appears in getNarratedTasks(), not getActiveTasks()", () => {
    const { panel, container } = makePanel();
    panel.handleJobUpdate({
      id: "narrated-1",
      window_id: "main",
      scope: "workspace",
      kind: "import",
      tone: "narrated",
      title: "Connecting to Matters",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: { state: "running", fraction: null, message: null },
    });

    const narrated = panel.getNarratedTasks();
    expect(narrated.length).toBeGreaterThan(0);
    expect(narrated[0].label).toBe("Connecting to Matters");
    // Must NOT also appear in getActiveTasks().
    expect(panel.getActiveTasks().some((t) => t.id.includes("narrated-1"))).toBe(false);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 8. ShippedDegraded advisory survives setStage('ready'); clears at 'opening' ──

describe("Behavior: ShippedDegraded BackgroundProgress advisory lifecycle", () => {
  it("advisory survives setStage('ready') and persists — clears only at setStage('opening')", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    // Raise a ShippedDegraded advisory.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "clip.MOV",
          what: "unsupported codec",
          action: "None",
        }],
      },
    });
    expect(panel.getAdvisories()).toHaveLength(1);

    // setStage('ready') — build settled. Advisory must SURVIVE.
    panel.setStage("ready");
    expect(panel.getAdvisories()).toHaveLength(1);

    // setStage('opening') — next build starts. Advisory is cleared here.
    panel.setStage("opening");
    expect(panel.getAdvisories()).toHaveLength(0);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("advisory survives multiple setStage('ready') calls (no premature clear)", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "images",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "Environment",
          severity: "NeedsAction",
          item: null,
          what: "FFmpeg not available",
          action: "None",
        }],
      },
    });
    expect(panel.getAdvisories()).toHaveLength(1);

    // Repeat setStage('ready') — the advisory was NOT from a "ready" clear path.
    panel.setStage("ready");
    panel.setStage("ready");
    expect(panel.getAdvisories()).toHaveLength(1);

    // Clears at build-start via scanning transition (equivalent to 'opening').
    panel.setStage("opening");
    expect(panel.getAdvisories()).toHaveLength(0);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("advisory clears at setStage('initial') (new session reset)", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "v.mov",
          what: "x",
          action: "None",
        }],
      },
    });
    expect(panel.getAdvisories()).toHaveLength(1);

    panel.setStage("initial");
    expect(panel.getAdvisories()).toHaveLength(0);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 9. ShippedDegraded specifically does NOT auto-surface the panel ──────────

describe("Behavior: ShippedDegraded advisory is a quiet dot (no auto-surface)", () => {
  it("ShippedDegraded advisory does not un-hide a hidden settled panel", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "ShippedDegraded",
          item: "clip.MOV",
          what: "unsupported codec",
          action: "None",
        }],
      },
    });

    // Panel stays hidden (non-Blocking advisory = quiet dot only).
    expect(panel.isHidden).toBe(true);
    // Advisory IS recorded (seam dot can fire).
    expect(panel.getAdvisories()).toHaveLength(1);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("Blocking advisory DOES auto-surface from hidden (contrast check)", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);

    panel.setStage("ready");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "videos",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "Environment",
          severity: "Blocking",
          item: null,
          what: "FFmpeg not available",
          action: "None",
        }],
      },
    });

    expect(panel.isHidden).toBe(false);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 10. RevealState: pinned / peek / surfaced / minimized ───────────────────

describe("Behavior: RevealState invariants", () => {
  it("pin() transitions to 'pinned'; setStage('ready') cannot auto-close it", () => {
    const { panel, container } = makePanel();
    panel.pin();
    expect(panel.getRevealState()).toBe("pinned");
    panel.setStage("ready");
    expect(panel.getRevealState()).toBe("pinned");
    expect(panel.isHidden).toBe(false);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("hide() sets state to 'minimized'; isHidden is true", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task());
    panel.hide();
    expect(panel.getRevealState()).toBe("minimized");
    expect(panel.isHidden).toBe(true);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("a consequential upsert (non-ambient) while minimized → 'surfaced'", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task({ id: "t1" }));
    panel.hide();
    expect(panel.getRevealState()).toBe("minimized");

    // A deploy-task is consequential (non-ambient).
    panel.upsertTask(task({ id: "deploy:push", label: "Deploying" }));
    expect(panel.getRevealState()).toBe("surfaced");
    expect(panel.isHidden).toBe(false);

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── 11. Publish complete → auto-close; failed → currently auto-closes too ───

describe("Behavior: publish stage terminal states", () => {
  it("deploy-progress 'complete' resets to idle and triggers auto-close when settled", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    fireDeployProgress({ stage: "uploading", current: 3, total: 5, message: "" });
    expect(panel.getPublishStage()).toBe("uploading");
    expect(panel.isHidden).toBe(false);

    // The publish receipt holds the glass for its dwell
    // — a publish is something the author asked for and waited on, so its
    // confirmation gets the same 2.5s every other receipt gets. THEN it closes.
    vi.useFakeTimers();
    try {
      fireDeployProgress({ stage: "complete", current: 0, total: 0, message: "" });
      expect(panel.getPublishStage()).toBe("idle");
      expect(panel.isHidden).toBe(false);
      vi.advanceTimersByTime(3_000);
      expect(panel.isHidden).toBe(true);
    } finally {
      vi.useRealTimers();
    }

    panel.destroy();
    document.body.removeChild(container);
  });

});

// ─── 12. Awaiting job keeps panel open via hasActiveTasks() ──────────────────

describe("Behavior: Awaiting job keeps panel open (hasActiveTasks() stays true)", () => {
  it("panel stays open while an Awaiting Job is in the task store", () => {
    vi.useFakeTimers();
    try {
      const { panel, container } = makePanel();
      panel.handleJobUpdate({
        id: "aw-email",
        window_id: "main",
        scope: "preview",
        kind: "deploy",
        tone: "ambient",
        title: "Verify your email",
        fade_after_ms: null,
        started_at_ms: 0,
        verb: null,
        amount: null,
        elapsed_ms: null,
        parent: null,
        state: { state: "awaiting", directive: "Click the link", action: "None" },
      });

      expect(panel.hasActiveTasks()).toBe(true);
      // Advance past stale timeout — awaiting must still be present.
      vi.advanceTimersByTime(30_000);
      expect(panel.hasActiveTasks()).toBe(true);

      panel.destroy();
      document.body.removeChild(container);
    } finally {
      vi.useRealTimers();
    }
  });
});

// ─── Failure-persist + failure-clear + re-publish-after-failure ────

describe("failure-persist — deploy-progress failed → panel stays open, red status", () => {
  it("deploy-progress 'failed' does NOT auto-close; lastFailure persists", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // Engage a publish
    fireDeployProgress({ stage: "uploading", current: 3, total: 5, message: "" });
    expect(panel.isHidden).toBe(false);

    // Fire failed — this must NOT idle-reset / auto-close
    fireDeployProgress({ stage: "failed", current: 0, total: 0, message: "" });

    // The panel must REMAIN open (not hidden by auto-close)
    expect(panel.isHidden).toBe(false);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("deploy-progress 'failed' → lastFailure is set", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    fireDeployProgress({ stage: "uploading", current: 3, total: 5, message: "" });
    fireDeployProgress({ stage: "failed", current: 0, total: 0, message: "" });

    // The panel has lastFailure set
    expect((panel as any).lastFailure).not.toBeNull();

    panel.destroy();
    document.body.removeChild(container);
  });
});

describe("publish-failure clean-up — a successful 'complete' clears the red", () => {
  it("deploy-progress 'complete' after failure clears lastFailure and auto-closes", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    fireDeployProgress({ stage: "uploading", current: 3, total: 5, message: "" });
    fireDeployProgress({ stage: "failed", current: 0, total: 0, message: "" });
    expect((panel as any).lastFailure).not.toBeNull();

    // Re-publish happy path: fail → start over
    fireDeployProgress({ stage: "waiting", current: 0, total: 0, message: "" });
    vi.useFakeTimers();
    try {
      fireDeployProgress({ stage: "complete", current: 0, total: 0, message: "" });

      // complete path: lastFailure cleared, auto-close fires once the publish
      // receipt has had its dwell.
      expect((panel as any).lastFailure).toBeNull();
      vi.advanceTimersByTime(3_000);
      expect(panel.isHidden).toBe(true);
    } finally {
      vi.useRealTimers();
    }

    panel.destroy();
    document.body.removeChild(container);
  });
});

describe("Esc on pinned collapses the panel", () => {
  it("onEscapeKey() when pinned → minimized", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task());
    panel.pin();
    expect(panel.getRevealState()).toBe("pinned");

    panel.onEscapeKey();

    expect(panel.getRevealState()).toBe("minimized");

    panel.destroy();
    document.body.removeChild(container);
  });

  it("onEscapeKey() when surfaced (not pinned) is a no-op", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task());
    panel.show();
    expect(panel.getRevealState()).toBe("surfaced");
    panel.onEscapeKey();
    // surfaced stays surfaced
    expect(panel.getRevealState()).toBe("surfaced");
    panel.destroy();
    document.body.removeChild(container);
  });

  // Closing the panel with Esc, like the ▾ handle, must acknowledge the Blocking advisory
  // showing at that moment,
  // or the very next routine rebuild tick pops it back open.
  it("onEscapeKey() acknowledges the Blocking advisory showing at that moment", async () => {
    const { panel, container } = makePanel();
    const { fireMossEvent } = await initAndGetFireFn(panel);
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
    panel.pin();

    panel.onEscapeKey();
    expect(panel.getRevealState()).toBe("minimized");

    // A routine per-edit rebuild tick — same problem, already dismissed.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] } });
    expect(panel.isHidden).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });
});

describe("durable bodyHovered / bodyFocused suppress auto-collapse", () => {
  it("bodyHovered = true suppresses checkAutoClose", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task({ id: "t1" }));
    panel.show();
    // Simulate body hover start
    panel.onPanelBodyHoverEnter();
    // Remove task — normally auto-closes; but body is hovered
    panel.removeTask("t1");
    // Panel should remain open
    expect(panel.isHidden).toBe(false);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("bodyFocused = true suppresses checkAutoClose", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task({ id: "t1" }));
    panel.show();
    // Simulate body focus in
    panel.onPanelBodyFocusIn();
    // Remove task — normally auto-closes; but body has focus
    panel.removeTask("t1");
    expect(panel.isHidden).toBe(false);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("bodyHovered clears on leave; auto-close proceeds normally", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task({ id: "t1" }));
    panel.show();
    panel.onPanelBodyHoverEnter();
    panel.removeTask("t1");
    expect(panel.isHidden).toBe(false); // hover suppresses
    panel.onPanelBodyHoverLeave();
    // Now auto-close can fire
    expect(panel.isHidden).toBe(true);
    panel.destroy();
    document.body.removeChild(container);
  });

  it("bodyFocused clears on focusout; auto-close proceeds normally", () => {
    const { panel, container } = makePanel();
    panel.upsertTask(task({ id: "t1" }));
    panel.show();
    panel.onPanelBodyFocusIn();
    panel.removeTask("t1");
    expect(panel.isHidden).toBe(false); // focus suppresses
    panel.onPanelBodyFocusOut();
    expect(panel.isHidden).toBe(true);
    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── Only the idle→engaged transition can auto-show the panel ─────────────────
//
// The Rust deploy ticker emits `deploy-progress` at 4 Hz (deploy.rs). If every
// tick re-surfaced the panel, a user who minimizes mid-publish would see it pop
// back open within ~250 ms — the collapse can never stick. advancePublishStage
// must surface ONLY on the idle→engaged edge, not on every subsequent tick.

describe("Upload ticks at 4 Hz keep a minimized panel minimized", () => {
  it("subsequent deploy-progress 'uploading' ticks do NOT re-surface a minimized panel", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // First uploading tick engages the publish (idle→uploading) and surfaces.
    fireDeployProgress({ stage: "uploading", current: 1, total: 20, message: "" });
    expect(panel.getPublishStage()).toBe("uploading");
    expect(panel.isHidden).toBe(false);

    // User minimizes mid-publish.
    panel.hide();
    expect(panel.isHidden).toBe(true);

    // The ticker keeps firing at 4 Hz. None of these already-engaged ticks may
    // yank the panel back open.
    fireDeployProgress({ stage: "uploading", current: 2, total: 20, message: "" });
    fireDeployProgress({ stage: "uploading", current: 6, total: 20, message: "" });
    fireDeployProgress({ stage: "uploading", current: 11, total: 20, message: "" });

    expect(panel.isHidden).toBe(true);
    expect(panel.getRevealState()).toBe("minimized");
    // The publish is still live (only the surface was gated, not the progress).
    expect(panel.getPublishStage()).toBe("uploading");

    panel.destroy();
    document.body.removeChild(container);
  });

  it("a re-publish after failure re-engages from the idle reset and surfaces again", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // Engage, minimize, then fail.
    fireDeployProgress({ stage: "uploading", current: 1, total: 5, message: "" });
    panel.hide();
    fireDeployProgress({ stage: "failed", current: 0, total: 0, message: "boom" });

    // The user minimizes the failure advisory.
    panel.hide();
    expect(panel.isHidden).toBe(true);

    // Re-publish: a fresh `waiting` resets the failed stage to idle, so this is a
    // NEW idle→engaged edge — it must pop the panel again.
    fireDeployProgress({ stage: "waiting", current: 0, total: 0, message: "" });
    expect(panel.isHidden).toBe(false);
    expect(panel.getRevealState()).toBe("surfaced");

    panel.destroy();
    document.body.removeChild(container);
  });
});

// The same edge rule applies at a third caller, after `advancePublishStage`;
// `upsertTask` still surfaced on every call, and because a publish pins
// `shouldSurface()` true for its whole duration, each plugin status message
// re-opened a minimized panel. Live-tested against an IPFS deploy plugin: the
// panel could not be kept shut for the length of a publish.

describe("plugin status updates do not re-yank a minimized panel mid-publish", () => {
  it("repeated updates to an existing task leave a minimized panel minimized", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // Engage the publish, which pins shouldSurface() true, then minimize.
    fireDeployProgress({ stage: "uploading", current: 1, total: 20, message: "" });
    panel.upsertTask(task({ id: "plugin:ipfs", label: "Adding to IPFS" }));
    panel.hide();
    expect(panel.getRevealState()).toBe("minimized");

    // The plugin keeps reporting. Same task id — routine updates, not news.
    panel.upsertTask(task({ id: "plugin:ipfs", label: "Adding to IPFS (2/9)" }));
    panel.upsertTask(task({ id: "plugin:ipfs", label: "Adding to IPFS (7/9)" }));
    panel.upsertTask(task({ id: "plugin:ipfs", label: "Pinning" }));

    expect(panel.getRevealState()).toBe("minimized");
    expect(panel.isHidden).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });

  it("a genuinely new task still surfaces the panel mid-publish", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    fireDeployProgress({ stage: "uploading", current: 1, total: 20, message: "" });
    panel.upsertTask(task({ id: "plugin:ipfs", label: "Adding to IPFS" }));
    panel.hide();
    expect(panel.getRevealState()).toBe("minimized");

    // A row that was not there before is news, and must still reach the user.
    panel.upsertTask(task({ id: "dns:check", label: "Checking DNS" }));
    expect(panel.getRevealState()).toBe("surfaced");

    panel.destroy();
    document.body.removeChild(container);
  });
});

// ─── One click collapses a surfaced panel ─────────────────────────────────
//
// From the auto-`surfaced` state the handle used to PIN (keep-open) on the first
// click, so a user who wanted to minimize an auto-popped panel had to click the
// handle twice. A surfaced panel is already open — its handle must collapse it in
// one click, exactly like a pinned panel.

describe("One click collapses a surfaced panel", () => {
  it("a single handle click on an auto-surfaced publish panel minimizes it", async () => {
    const { panel, container } = makePanel();
    const { fireDeployProgress } = await initAndGetFireFn(panel);

    // A publish auto-surfaces the panel and paints the "Uploading …" status line,
    // so the disclosure handle is rendered while reveal === "surfaced".
    fireDeployProgress({ stage: "uploading", current: 1, total: 5, message: "" });
    expect(panel.getRevealState()).toBe("surfaced");

    // The bottom handle is the only disclosure control. From surfaced, ONE click
    // must collapse (not pin/keep-open — that trapped the user needing two).
    const handle = container.querySelector<HTMLButtonElement>("button.moss-surface-handle");
    expect(handle).not.toBeNull();
    handle!.click();

    expect(panel.getRevealState()).toBe("minimized");
    expect(panel.isHidden).toBe(true);

    panel.destroy();
    document.body.removeChild(container);
  });
});
