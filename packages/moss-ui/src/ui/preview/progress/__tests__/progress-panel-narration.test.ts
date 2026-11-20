/**
 * Tests for folding narrator/awaiting into the pill popover
 *
 * The `ProgressPanel` now ingests non-preview `tone==="narrated"` and
 * `tone==="awaiting"` PanelTaskUpdate wires alongside its existing
 * preview-scope Job handling.  All assertions are against the panel's
 * in-memory state (accessible via public getters) rather than DOM output,
 * mirroring the pattern in `progress-panel-job.test.ts`.
 *
 * These five invariants keep narration quiet and useful:
 *   (a) narrated running task (non-preview) → a popover row appears
 *   (b) failed narrated task PERSISTS past the fade window
 *   (c) awaiting task → amber "needs-you" advisory dot + directive+action row
 *   (d) clearing tasks returns pill to quiet
 *   (e) #moss-titlebar-plugin-section is never populated after init
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProgressPanel } from "../progress-panel";
import type { Action, PanelTaskWire, TaskState } from '@symbiosis-lab/moss-ui';

const CANCEL: Action = "None";
const RESEND: Action = {
  InApp: { op: "SignIn" as const, args: { reason: "resend" }, label: "Resend email" },
};
const RECHECK: Action = {
  InApp: { op: "RecheckDns" as const, args: null, label: "Recheck DNS" },
};

function narratedWire(opts: {
  id?: string;
  title?: string | null;
  started_at_ms?: number;
  state?: TaskState;
  scope?: PanelTaskWire["scope"];
}): PanelTaskWire {
  return {
    id: opts.id ?? "narrated-1",
    window_id: "main",
    scope: opts.scope ?? "workspace",
    kind: "deploy",
    tone: "narrated",
    title: opts.title ?? null,
    fade_after_ms: null,
    started_at_ms: opts.started_at_ms ?? 0,
    verb: null,
    amount: null,
    elapsed_ms: null,
    parent: null,
    state: opts.state ?? { state: "running", fraction: null, message: null },
  };
}

function awaitingWire(opts: {
  id?: string;
  directive?: string;
  action?: Action;
  state?: TaskState;
}): PanelTaskWire {
  return {
    id: opts.id ?? "awaiting-1",
    window_id: "main",
    scope: "workspace",
    kind: "deploy",
    tone: "awaiting",
    title: null,
    fade_after_ms: null,
    started_at_ms: 0,
    verb: null,
    amount: null,
    elapsed_ms: null,
    parent: null,
    state: opts.state ?? {
      state: "awaiting",
      directive: opts.directive ?? "click the link in your email",
      action: opts.action ?? CANCEL,
    },
  };
}

describe("ProgressPanel narration ingestion", () => {
  let container: HTMLElement;
  let panel: ProgressPanel;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
  });
  afterEach(() => {
    document.body.removeChild(container);
    vi.useRealTimers();
  });

  // ── (a) narrated running task → popover row ──────────────────────────

  it("(a) non-preview narrated running task → appears as a panel row", () => {
    panel.handleJobUpdate(
      narratedWire({ title: "Connecting example.test" }),
    );
    const tasks = panel.getNarratedTasks();
    expect(tasks.length).toBeGreaterThan(0);
    expect(tasks[0].label).toBe("Connecting example.test");
  });

  it("(a) narrated task with no title uses message", () => {
    panel.handleJobUpdate(
      narratedWire({
        state: { state: "running", fraction: null, message: "Importing posts" },
      }),
    );
    const tasks = panel.getNarratedTasks();
    expect(tasks[0].label).toBe("Importing posts");
  });

  it("(a) preview-scope jobs are unaffected (existing behavior preserved)", () => {
    panel.handleJobUpdate({
      id: "preview-job",
      window_id: "main",
      scope: "preview",
      kind: "deploy",
      tone: "ambient",
      title: "Deploying",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: { state: "running", fraction: 0.5, message: null },
    });
    // Preview job is in getActiveTasks(), not getNarratedTasks()
    expect(panel.getActiveTasks()).toHaveLength(1);
    expect(panel.getNarratedTasks()).toHaveLength(0);
  });

  // ── (b) failed narrated task persists ────────────────────────

  it("(b) failed narrated task persists past linger+fade window — not removed", () => {
    panel.handleJobUpdate(
      narratedWire({
        id: "fail-1",
        title: "Check site",
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
      }),
    );

    // Advance past the 1000ms linger + 400ms fade = 1400ms the old narrator used.
    vi.advanceTimersByTime(2000);

    const tasks = panel.getNarratedTasks();
    expect(tasks.length).toBeGreaterThan(0);
    const failedTask = tasks.find((t) => t.id.includes("fail-1"));
    expect(failedTask).toBeDefined();
    expect(failedTask?.label).toBe("Failed: network down");
  });

  it("(b) succeeded narrated task is eventually removed (only failed persists)", () => {
    panel.handleJobUpdate(
      narratedWire({
        id: "succ-1",
        state: { state: "succeeded", receipt: "All done" },
      }),
    );

    // Initially present
    expect(panel.getNarratedTasks().length).toBeGreaterThan(0);

    // After the linger+fade window, succeeded tasks should clear.
    vi.advanceTimersByTime(2000);
    const remaining = panel.getNarratedTasks().filter((t) => t.id.includes("succ-1"));
    expect(remaining).toHaveLength(0);
  });

  // ── (c) awaiting → amber dot + directive+action in popover ──────────

  it("(c) awaiting tone → amber advisory dot is set (via onTopSeverityChange)", () => {
    // onAdvisoryChange is deleted; the seam dot color is driven by topSeverity.
    // An awaiting-narration task synthesises a NeedsAction advisory → topSeverity = degraded.
    let topSeverity: "broken" | "degraded" | "none" = "none";
    panel.onTopSeverityChange((s) => { topSeverity = s; });

    panel.handleJobUpdate(awaitingWire({ directive: "click email link" }));

    expect(topSeverity).not.toBe("none");
  });

  it("(c) awaiting tone → directive appears in panel task row", () => {
    panel.handleJobUpdate(
      awaitingWire({ directive: "verify DNS in your registrar", action: RECHECK }),
    );
    // The awaiting task should be accessible via getActiveTasks() or a
    // dedicated getter — the directive is the task label.
    const allTasks = [...panel.getActiveTasks(), ...panel.getNarratedTasks()];
    const awaitTask = allTasks.find((t) =>
      t.label.includes("verify DNS") ||
      t.label.includes("Waiting for you to verify DNS"),
    );
    expect(awaitTask).toBeDefined();
  });

  it("(c) awaiting action is present on the task row for the escape affordance", () => {
    panel.handleJobUpdate(
      awaitingWire({ action: RESEND }),
    );
    const allTasks = [...panel.getActiveTasks(), ...panel.getNarratedTasks()];
    const awaitTask = allTasks.find((t) => t.action != null);
    expect(awaitTask).toBeDefined();
    expect(awaitTask?.action).toEqual(RESEND);
  });

  // The matters publish room's "waiting" surface is the moss-owned
  // `.moss-matters-bar` title bar, and the split is enforced
  // enforced upstream by the tone guard: a `Syndicate` Awaiting
  // job is forced to `TaskTone::Inline`, so its wire arrives `tone === "inline"`
  // and routes to the rail (inline-status.ts) — it never reaches
  // handleAwaitingNarrationUpdate. The matters room therefore needs no bail here.
  // covered by the Rust tone-guard test.

  // ── (d) clearing returns pill to quiet ──────────────────────────────

  it("(d) cancelling a narrated task removes it from the panel", () => {
    panel.handleJobUpdate(narratedWire({ id: "n-1", title: "Working" }));
    expect(panel.getNarratedTasks().length).toBeGreaterThan(0);

    panel.handleJobUpdate(
      narratedWire({ id: "n-1", state: { state: "cancelled" } }),
    );

    const remaining = panel.getNarratedTasks().filter((t) => t.id.includes("n-1"));
    expect(remaining).toHaveLength(0);
  });

  it("(d) cancelling an awaiting task clears the amber advisory dot (via onTopSeverityChange)", () => {
    // onAdvisoryChange is deleted; verify via topSeverity which reflects the
    // actual advisory store. After cancel the awaiting advisory is removed →
    // topSeverity should return to "none".
    let topSeverity: "broken" | "degraded" | "none" = "none";
    panel.onTopSeverityChange((s) => { topSeverity = s; });

    panel.handleJobUpdate(awaitingWire({ id: "aw-1" }));
    expect(topSeverity).not.toBe("none");

    panel.handleJobUpdate(
      awaitingWire({ id: "aw-1", state: { state: "cancelled" } }),
    );
    expect(topSeverity).toBe("none");
  });

  it("(d) tone escalation away from narrated removes the task", () => {
    panel.handleJobUpdate(narratedWire({ id: "n-1", title: "Working" }));
    expect(panel.getNarratedTasks().length).toBeGreaterThan(0);

    // Same task id, but tone changed to "ambient" — should drop from narrated map.
    panel.handleJobUpdate({
      id: "n-1",
      window_id: "main",
      scope: "workspace",
      kind: "deploy",
      tone: "ambient",
      title: "Working",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: { state: "running", fraction: null, message: null },
    });

    const remaining = panel.getNarratedTasks().filter((t) => t.id.includes("n-1"));
    expect(remaining).toHaveLength(0);
  });

  it.each(["ambient", "narrated"] as const)("resumed site work clears the awaiting dot before routing to %s", (tone) => {
    panel.handleJobUpdate(awaitingWire({ id: "site", directive: "Check DNS" }));
    expect(panel.buildViewModel().topSeverity).toBe("degraded");
    panel.handleJobUpdate({ ...narratedWire({ id: "site", title: "Checking site" }), tone });
    expect(panel.buildViewModel().topSeverity).toBe("none");
    expect(panel.getActiveTasks()).toEqual([]);
    expect(panel.getNarratedTasks()).toHaveLength(tone === "narrated" ? 1 : 0);
    if (tone === "ambient") {
      panel.setStage("ready");
      expect(panel.isHidden).toBe(true);
    }
  });

  // ── checkAutoClose respects narratedTasks ──────

  it("a running narrated task prevents checkAutoClose from hiding the panel", () => {
    // Surface the panel first (the resting default is collapsed now).
    panel.show();
    expect(panel.isHidden).toBe(false);

    // Add a running narrated task.
    panel.handleJobUpdate(narratedWire({ id: "law2-running", title: "Checking site" }));
    expect(panel.getNarratedTasks()).toHaveLength(1);

    // setStage('ready') drives checkAutoClose() — normally closes a surfaced panel
    // when there are no tasks, advisories, or publish stages. But a narrated task
    // must prevent closure (a task that has not ended keeps the panel).
    panel.setStage("ready");

    // Panel must NOT hide — the narrated row is still alive.
    expect(panel.isHidden).toBe(false);
    expect(panel.getNarratedTasks()).toHaveLength(1);
  });

  it("a failed narrated task ALSO prevents checkAutoClose from hiding the panel", () => {
    panel.show();
    expect(panel.isHidden).toBe(false);

    // A failed narrated task persists indefinitely (no fade timer).
    panel.handleJobUpdate(
      narratedWire({
        id: "law2-failed",
        title: "Check site",
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
      }),
    );
    expect(panel.getNarratedTasks()).toHaveLength(1);

    // Settle build + advance past linger window — still must NOT close.
    panel.setStage("ready");
    vi.advanceTimersByTime(2000);

    expect(panel.isHidden).toBe(false);
    // Failed narrated task still there (a failed task is never removed on its own).
    expect(panel.getNarratedTasks().some((t) => t.id.includes("law2-failed"))).toBe(true);
  });

  // ── (e) #moss-titlebar-plugin-section never populated ────────────────

  it("(e) #moss-titlebar-plugin-section is not populated after init", () => {
    // Append the titlebar slot to the container (not body directly) so it gets
    // cleaned up with the container in afterEach.
    const slot = document.createElement("div");
    slot.id = "moss-titlebar-plugin-section";
    slot.style.display = "none";
    container.appendChild(slot);

    panel.handleJobUpdate(narratedWire({ title: "Some task" }));
    panel.handleJobUpdate(awaitingWire({ directive: "do something" }));

    const found = container.querySelector("#moss-titlebar-plugin-section");
    // Should remain empty — the panel renders into its own container, not the titlebar slot.
    expect(found?.children.length).toBe(0);
    // Display should remain unchanged (panel doesn't touch this element).
    expect((found as HTMLElement | null)?.style.display).toBe("none");
  });
});
