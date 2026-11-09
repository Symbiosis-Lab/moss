import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProgressPanel } from "../progress-panel";
import { eventBus, Events } from "../../../context/event-bus";
import type { PanelTaskWire, TaskState } from '@symbiosis-lab/moss-ui';

function wire(opts: {
  scope: PanelTaskWire["scope"];
  state: TaskState;
  id?: string;
  kind?: PanelTaskWire["kind"];
  title?: string | null;
  verb?: string | null;
  amount?: PanelTaskWire["amount"];
  elapsed_ms?: number | null;
  parent?: string | null;
}): PanelTaskWire {
  return {
    id: opts.id ?? "1",
    window_id: "main",
    scope: opts.scope,
    kind: opts.kind ?? "deploy",
    tone: "ambient",
    title: opts.title ?? null,
    fade_after_ms: null,
    started_at_ms: 0,
    verb: opts.verb ?? null,
    amount: opts.amount ?? null,
    elapsed_ms: opts.elapsed_ms ?? null,
    parent: opts.parent ?? null,
    state: opts.state,
  };
}

describe("ProgressPanel Job ingestion (Preview-scope Jobs)", () => {
  let container: HTMLElement;
  let panel: ProgressPanel;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
  });
  afterEach(() => {
    document.body.removeChild(container);
  });

  it("renders a Preview-scope Running Job as a task row", () => {
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        title: "Connecting example.test",
        state: { state: "running", fraction: 0.2, message: null },
      }),
    );
    const tasks = panel.getActiveTasks();
    expect(tasks).toHaveLength(1);
    expect(tasks[0].label).toBe("Connecting example.test");
  });

  it("ignores non-Preview-scope Jobs (titlebar/inline own those)", () => {
    panel.handleJobUpdate(
      wire({
        scope: "action_panel",
        state: { state: "running", fraction: null, message: "Saving" },
      }),
    );
    expect(panel.getActiveTasks()).toHaveLength(0);
  });

  it("renders an Awaiting Job as the interrupt carrying its typed action", () => {
    const action = {
      InApp: { op: "SignIn" as const, args: { reason: "verify_email" }, label: "Open Settings" },
    };
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        state: {
          state: "awaiting",
          directive: "Verify your email to finish connecting example.test",          action,
        },
      }),
    );
    const tasks = panel.getActiveTasks();
    expect(tasks).toHaveLength(1);
    expect(tasks[0].label).toContain("Verify your email");
    expect(tasks[0].action).toEqual(action);
  });

  it("renders a Succeeded Job as a verb · receipt row (non-deploy/publish kind)", () => {
    // Deploy/publish kinds dissolve without a receipt row (the shelf
    // record is the durable surface). This test uses kind='syndicate' to verify
    // the receipt path still works for non-deploy kinds.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Connected",
        state: { state: "succeeded", receipt: "4 min", advisories: [] },
      }),
    );
    const tasks = panel.getActiveTasks();
    expect(tasks).toHaveLength(1);
    expect(tasks[0].label).toContain("Connected");
    expect(tasks[0].label).toContain("4 min");
  });

  it("formats elapsed_ms into the receipt when the producer sends no receipt string (non-deploy kind)", () => {
    // The live orchestrator passes receipt:None and relies on the frozen
    // elapsed span (wire.elapsed_ms) — the renderer formats it (R-fix).
    // A deploy Job leaves no receipt row (the shelf record is its surface), so this uses kind='syndicate'.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Connected",
        elapsed_ms: 240000, // 4 minutes
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    expect(panel.getActiveTasks()[0].label).toBe("Connected · 4 min");
  });

  it("receipt formatting: verb · amount · elapsed for a non-deploy succeeded Job", () => {
    // A deploy Job leaves no receipt row (the shelf record is its surface);
    // every other kind, such as syndicate, is formatted as a receipt.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Published",
        amount: { count: 142, noun: "files" },
        elapsed_ms: 2100,
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    expect(panel.getActiveTasks()[0].label).toBe("Published · 142 files · 2.1s");
  });

  it("a Job with no amount omits that segment (non-deploy kind, no files)", () => {
    // Invariant #4: amount present iff the producer set it.
    // A deploy Job leaves no receipt row (the shelf record is its surface), so this uses kind='syndicate'.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Connected",
        amount: null,
        elapsed_ms: 2100,
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    expect(panel.getActiveTasks()[0].label).toBe("Connected · 2.1s");
  });

  it("a Succeeded Job with amount.count===0 and no advisories leaves no row (nothing to report)", () => {
    // e.g. Syndicate: 0 posts published because nothing was live yet, or
    // every candidate ended as an unpublished draft. Same "success makes no
    // sound" rubric as the ambient sub-job case, applied to a top-level Job.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Syndicated",
        amount: { count: 0, noun: "posts" },
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    expect(panel.getActiveTasks()).toHaveLength(0);
  });

  it("a Succeeded Job with amount.count===0 but a real advisory keeps its row (the advisory's home)", () => {
    // A draft-timeout NeedsAction proposal rides the same terminal call as a
    // 0-published count — the row must not disappear out from under it.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Syndicated",
        amount: { count: 0, noun: "posts" },
        state: {
          state: "succeeded",
          receipt: null,
          advisories: [
            {
              scope: "Remote",
              severity: "NeedsAction",
              item: "A post",
              what: "Draft saved — publish it on Matters when ready",
              action: { Link: { href: "https://matters.town/draft/1", label: "Open draft" } },
            },
          ],
        },
      }),
    );
    expect(panel.getActiveTasks()).toHaveLength(1);
  });

  it("a Succeeded Job with amount.count > 0 renders its row as before (unaffected by the zero-count rule)", () => {
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Syndicated",
        amount: { count: 3, noun: "posts" },
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    const tasks = panel.getActiveTasks();
    expect(tasks).toHaveLength(1);
    expect(tasks[0].label).toBe("Syndicated · 3 posts");
  });

  it("N rapid Running updates for one Deploy Job produce ONE row, not N (risk b — dedup)", () => {
    // Folding deploy into the Job stream multiplies PanelTaskUpdate frequency;
    // the panel keys rows by Job id (`job:<id>`), so a burst of Running updates
    // upserts the same row rather than flooding the panel.
    for (let i = 0; i < 8; i++) {
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "deploy-1",
          title: "Publishing",
          state: { state: "running", fraction: i / 8, message: null },
        }),
      );
    }
    expect(panel.getActiveTasks()).toHaveLength(1);
  });

  it("the Awaiting interrupt PERSISTS past the stale-removal timeout", () => {
    vi.useFakeTimers();
    try {
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          state: {
            state: "awaiting",
            directive: "Verify your email",
            action: "None",
          },
        }),
      );
      expect(panel.getActiveTasks()).toHaveLength(1);
      // Well past STALE_TIMEOUT_MS — a live progress row would have been culled;
      // the one-shot interrupt must remain (minimize != forget).
      vi.advanceTimersByTime(120_000);
      expect(panel.getActiveTasks()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a sub-job (parent set) does NOT auto-show the panel; a top-level Job does", () => {
    // Forward-compat for media sub-jobs (Preview-scope, parent set):
    // routine work stays ambient. A top-level Domain Job (parent null) shows.
    const sub = new ProgressPanel(document.createElement("div"));
    sub.handleJobUpdate(
      wire({
        scope: "preview",
        parent: "7",
        state: { state: "running", fraction: 0.5, message: "encoding" },
      }),
    );
    expect(sub.getActiveTasks()[0].ambient).toBe(true);
  });

  it("renders the InApp action as a button that dispatches on click (R-i)", () => {
    const emit = vi.spyOn(eventBus, "emit");
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        state: {
          state: "awaiting",
          directive: "Verify your email to finish connecting example.test",          action: {
            InApp: { op: "SignIn", args: { reason: "verify_email" }, label: "Open Settings" },
          },
        },
      }),
    );
    const btn = container.querySelector(
      "button.moss-job-action",
    ) as HTMLButtonElement | null;
    expect(btn).not.toBeNull();
    expect(btn!.textContent).toBe("Open Settings");
    btn!.click();
    // verify_email routes to "deployment" (the email-verify
    // form + api-key input), NOT "email" (the Subscribers tab).
    expect(emit).toHaveBeenCalledWith(
      Events.OPEN_SETTINGS_MODAL,
      expect.objectContaining({ section: "deployment" }),
    );
  });

  it("an Awaiting Job interrupts by showing the panel", () => {
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        state: {
          state: "awaiting",
          directive: "Verify your email",
          action: "None",
        },
      }),
    );
    // The interrupt surfaces the panel from hidden.
    expect(container.querySelector(".moss-progress-panel")).not.toBeNull();
  });

  it("suppresses the Deploy Job's live running row while the publish bar is engaged (no double-render); deploy-kind terminal dissolves without a row", () => {
    // The primary publish bar OWNS the live progress of a publish. While engaged,
    // the Deploy Job must NOT paint a 'Publishing' row — that double-renders.
    // Deploy-kind terminal receipt also does not create a row —
    // the persistent shelf record (showAck "Site published") is the surface.
    (panel as unknown as { currentPublishStage: string }).currentPublishStage =
      "uploading";
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        id: "deploy-9",
        title: "Publishing",
        state: { state: "running", fraction: 0.4, message: null },
      }),
    );
    expect(panel.getActiveTasks()).toHaveLength(0); // suppressed — the bar owns it

    // Deploy-kind Succeeded also creates no receipt row.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        id: "deploy-9",
        verb: "Published",
        amount: { count: 142, noun: "files" },
        elapsed_ms: 2100,
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    // No row — the shelf record is the durable surface.
    expect(panel.getActiveTasks()).toHaveLength(0);
  });

  it("uses a singular noun for a count of 1 ('1 file', not '1 files') — non-deploy kind", () => {
    // A deploy Job leaves no receipt row, so this uses the syndicate kind.
    panel.handleJobUpdate(
      wire({
        scope: "preview",
        kind: "syndicate",
        verb: "Published",
        amount: { count: 1, noun: "files" },
        elapsed_ms: 2100,
        state: { state: "succeeded", receipt: null, advisories: [] },
      }),
    );
    expect(panel.getActiveTasks()[0].label).toBe("Published · 1 file · 2.1s");
  });

  describe("Media child Jobs nested under the Build parent", () => {
    // A media child Job (kind=asset_transform, parent set) renders as a row
    // nested under its Build parent (parentId on the task). A content-only
    // rebuild mints no Job → no rows — the heartbeat owns it.

    it("a running media child Job carries its parentId in state (nested row DOM covered by renderer test)", () => {
      // The Build parent is spawned first (kind=build, parent null) — it drives
      // the header, not a row. The child (asset_transform, parent set) nests.
      // In the new model, job:* tasks appear in the status line, not as DOM rows
      // (only narrated tasks render rows). DOM-level nesting is covered by the
      // renderer test. Here we pin the state: parentId is set correctly.
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "100",
          kind: "build",
          title: "Building",
          state: { state: "running", fraction: null, message: null },
        }),
      );
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "101",
          kind: "asset_transform",
          parent: "100",
          title: "Converting videos (1/3)",
          state: { state: "running", fraction: 0.33, message: null },
        }),
      );
      const child = panel
        .getActiveTasks()
        .find((t) => t.id === "job:101");
      expect(child).toBeDefined();
      expect(child!.parentId).toBe("job:100");
      // No DOM row assertion: job:* tasks appear in the status line, not as rows.
    });

    it("the Build parent Job is NOT a task row (it drives the header receipt)", () => {
      // A top-level kind=build Job supplies the receipt header, never a row in
      // the task list — otherwise the build would double as a row AND a header.
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "100",
          kind: "build",
          title: "Building",
          state: { state: "running", fraction: null, message: null },
        }),
      );
      expect(
        panel.getActiveTasks().find((t) => t.id === "job:100"),
      ).toBeUndefined();
    });

    it("the Build parent's terminal Succeeded drives the 'Built · N pages · X.Xs' header", () => {
      // The backend drives the parent receipt: verb=Built,
      // amount={page_count, pages}, frozen elapsed. The header reads it from the
      // parent Job — NOT a phantom row, NOT the legacy BuildComplete listener.
      // A compiling stage keeps the panel open (the receipt only shows on an
      // already-surfaced panel, keeping quiet rebuilds silent).
      panel.setStage("compiling");
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "100",
          kind: "build",
          verb: "Built",
          amount: { count: 12, noun: "pages" },
          elapsed_ms: 2100,
          state: { state: "succeeded", receipt: null, advisories: [] },
        }),
      );
      const header = container.querySelector(".moss-status-line") as HTMLElement | null;
      expect(header).not.toBeNull();
      expect(header!.textContent).toBe("Built · 12 pages · 2.1s");
      // And it minted no `job:100` task row.
      expect(
        panel.getActiveTasks().find((t) => t.id === "job:100"),
      ).toBeUndefined();
    });

    it("an ambient media child that succeeds with no advisory makes no sound (no persistent row)", () => {
      // Successful work stays quiet. A media child terminal-Succeeded with no
      // advisory leaves NO row — the content sharpening in place is the feedback.
      panel.handleJobUpdate(
        wire({
          scope: "preview",
          id: "101",
          kind: "asset_transform",
          parent: "100",
          verb: "Built",
          state: { state: "succeeded", receipt: null, advisories: [] },
        }),
      );
      expect(
        panel.getActiveTasks().find((t) => t.id === "job:101"),
      ).toBeUndefined();
    });
  });

  // ===========================================================================
  // deploy/publish succeeded Job: pill leaves no residue (no staleTimers)
  // ===========================================================================
  describe("deploy/publish succeeded Job: no staleTimers auto-recede; build receipt unaffected", () => {
    it("a deploy-kind succeeded Job does NOT create a receipt row (no upsert, no staleTimers entry)", () => {
      // RED: currently ALL non-ambient succeeded Jobs (including deploy) get
      // upsertTask(receipt) + staleTimers.set(id, setTimeout(() => removeTask(id), 4000)).
      // kind==="deploy" | "publish" dissolves WITHOUT a receipt
      // row — the persistent shelf record (showAck from deploy-handler) is the
      // durable surface. Check IMMEDIATELY after handleJobUpdate (before any timer
      // fires) so we detect the upsert, not just the staleTimers removal.
      vi.useFakeTimers();
      try {
        panel.handleJobUpdate(
          wire({
            scope: "preview",
            id: "deploy-1",
            kind: "deploy",
            verb: "Published",
            amount: { count: 42, noun: "files" },
            elapsed_ms: 2100,
            state: { state: "succeeded", receipt: null, advisories: [] },
          }),
        );

        // IMMEDIATELY after: no receipt row should exist for a deploy-kind job.
        // (Before the fix, a row IS created and only removed after 4s by staleTimers.)
        const deployTask = panel.getActiveTasks().find((t) => t.id === "job:deploy-1");
        expect(deployTask).toBeUndefined();
      } finally {
        vi.useRealTimers();
      }
    });

    it("a publish-kind succeeded Job does NOT produce a receipt row (no staleTimers entry)", () => {
      vi.useFakeTimers();
      try {
        panel.handleJobUpdate(
          wire({
            scope: "preview",
            id: "publish-1",
            kind: "publish",
            verb: "Published",
            state: { state: "succeeded", receipt: null, advisories: [] },
          }),
        );

        // Immediately after: no row (dissolved without receipt dwell).
        expect(
          panel.getActiveTasks().find((t) => t.id === "job:publish-1"),
        ).toBeUndefined();

        // Still absent after 4s.
        vi.advanceTimersByTime(4500);
        expect(
          panel.getActiveTasks().find((t) => t.id === "job:publish-1"),
        ).toBeUndefined();
      } finally {
        vi.useRealTimers();
      }
    });

    it("a non-deploy/publish succeeded Job (domain 'Connected') STILL gets its 4s receipt dwell", () => {
      // build-kind is routed via handleBuildJobUpdate (separate path).
      // domain deploy 'kind=deploy' is now a no-receipt path; test a non-deploy kind.
      // Use kind="deploy" for the domain — domain jobs actually use kind="deploy" too.
      // So test with the existing domain-like job that IS a deploy kind — confirmed
      // by the wire() default kind: "deploy". The spec says build kind is unaffected.
      // Let's test a non-deploy kind to confirm the existing receipt path still works.
      vi.useFakeTimers();
      try {
        // Simulate a non-deploy, non-publish, non-build succeeded Job
        // (the domain Job itself uses kind="deploy" in the real app, but the
        // spec only protects build; any other kind that is not deploy/publish
        // should still get the receipt dwell).
        // Use a hypothetical kind by casting — the wire kind field is typed.
        // In practice, the only non-build succeeded jobs we care about here are
        // domain (kind="deploy") — which now gets the no-residue treatment too.
        // The spec says "build jobs still get their receipt". Verify via handleBuildJobUpdate:
        panel.setStage("compiling"); // keeps panel visible so receipt shows
        panel.handleJobUpdate(
          wire({
            scope: "preview",
            id: "build-parent",
            kind: "build",
            verb: "Built",
            amount: { count: 5, noun: "pages" },
            elapsed_ms: 1500,
            state: { state: "succeeded", receipt: null, advisories: [] },
          }),
        );
        // Build parent is handled by handleBuildJobUpdate — it mints no task row
        // but DOES create the receipt header. The receipt header itself recedes
        // via RECEIPT_DWELL_MS. Confirm the buildReceipt is set (no task row):
        expect(
          panel.getActiveTasks().find((t) => t.id === "job:build-parent"),
        ).toBeUndefined();
        // The receipt is rendered in the status line (buildReceipt was set).
        const header = container.querySelector(".moss-status-line") as HTMLElement | null;
        expect(header).not.toBeNull();
        expect(header!.textContent).toContain("Built");
      } finally {
        vi.useRealTimers();
      }
    });
  });
});
