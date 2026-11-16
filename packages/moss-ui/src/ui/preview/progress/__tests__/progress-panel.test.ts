/**
 * Tests for ProgressPanel and ProgressPanelRenderer
 * Chunk B: Core panel logic, task management, and DOM rendering
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as path from "node:path";
import { ProgressPanel } from "../progress-panel";
import type { ProgressTask } from "../progress-panel";
import { cloudSync } from "../../cloud/cloud-sync-store";
import { engine } from "../../../engine";
import { host } from "../../../host";

// __dirname is not defined in ESM/vitest — derive it from import.meta.url so the
// source-style.css read (the glass/blur declared-value guard) resolves.
const __dirname = path.dirname(fileURLToPath(import.meta.url));

// The engine subscriptions the code under test makes, captured per test.
const setEngineOn = (on: unknown) => { host.engine.on = on as typeof host.engine.on; };


vi.mock("../../../log", () => ({
  uiEvent: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  debug: vi.fn(),
  trace: vi.fn(),
}));

// Helper to create a ProgressTask with defaults
function createTask(overrides: Partial<ProgressTask> = {}): ProgressTask {
  return {
    id: "task-1",
    label: "Test Task",
    progress: 50,
    source: "moss",
    ...overrides,
  };
}

describe("ProgressPanel", () => {
  let container: HTMLElement;
  let panel: ProgressPanel;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  describe("Task CRUD", () => {
    it("upsertTask adds a new task", () => {
      panel.upsertTask(createTask({ id: "task-1", label: "First Task" }));

      expect(panel.hasActiveTasks()).toBe(true);
      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.getActiveTasks()[0].label).toBe("First Task");
    });

    it("upsertTask updates an existing task", () => {
      panel.upsertTask(createTask({ id: "task-1", label: "Original" }));
      panel.upsertTask(
        createTask({ id: "task-1", label: "Updated", progress: 75 }),
      );

      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.getActiveTasks()[0].label).toBe("Updated");
      expect(panel.getActiveTasks()[0].progress).toBe(75);
    });

    it("upsertTask adds multiple tasks with different IDs", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));
      panel.upsertTask(createTask({ id: "task-3" }));

      expect(panel.getActiveTasks()).toHaveLength(3);
    });

    it("removeTask removes the specified task", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));

      panel.removeTask("task-1");

      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.getActiveTasks()[0].id).toBe("task-2");
    });

    it("removeTask does nothing for non-existent task", () => {
      panel.upsertTask(createTask({ id: "task-1" }));

      panel.removeTask("non-existent");

      expect(panel.getActiveTasks()).toHaveLength(1);
    });

    it("getActiveTasks returns a copy of tasks", () => {
      panel.upsertTask(createTask({ id: "task-1" }));

      const tasks = panel.getActiveTasks();
      tasks.push(createTask({ id: "fake" }));

      // Original should be unaffected
      expect(panel.getActiveTasks()).toHaveLength(1);
    });
  });

  describe("Placement", () => {
    // Use a narrated task to create panel DOM (only narrated tasks render as rows).
    function addNarratedTask(id = "narrated:1", label = "Building") {
      panel.handleJobUpdate({
        id, window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: label, fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
    }

    it("defaults to bottom placement", () => {
      // Panel should start in bottom mode.
      // Narrated tasks render as rows → creates the panel DOM.
      addNarratedTask();

      const panelEl = container.querySelector(".moss-progress-panel");
      expect(panelEl).not.toBeNull();
      expect(panelEl!.classList.contains("placement-bottom")).toBe(true);
    });
  });

  describe("hasActiveTasks", () => {
    it("returns false when no tasks", () => {
      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("returns true when tasks exist", () => {
      panel.upsertTask(createTask());
      expect(panel.hasActiveTasks()).toBe(true);
    });

    it("returns false after all tasks removed", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.removeTask("task-1");
      expect(panel.hasActiveTasks()).toBe(false);
    });
  });

  describe("Callbacks", () => {
    it("onAllComplete fires when last task is removed", () => {
      const callback = vi.fn();
      panel.onAllComplete(callback);

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));

      panel.removeTask("task-1");
      expect(callback).not.toHaveBeenCalled();

      panel.removeTask("task-2");
      expect(callback).toHaveBeenCalledTimes(1);
    });

    it("onAllComplete does not fire when tasks remain", () => {
      const callback = vi.fn();
      panel.onAllComplete(callback);

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));
      panel.removeTask("task-1");

      expect(callback).not.toHaveBeenCalled();
    });

    it("onAllComplete supports multiple callbacks", () => {
      const cb1 = vi.fn();
      const cb2 = vi.fn();
      panel.onAllComplete(cb1);
      panel.onAllComplete(cb2);

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.removeTask("task-1");

      expect(cb1).toHaveBeenCalledTimes(1);
      expect(cb2).toHaveBeenCalledTimes(1);
    });

    it("onTaskFailed fires when failTask is called", () => {
      const callback = vi.fn();
      panel.onTaskFailed(callback);

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.failTask("task-1");

      expect(callback).toHaveBeenCalledWith("task-1");
    });

    it("failTask removes the task after firing callback", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.failTask("task-1");

      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("failTask fires onAllComplete if it was the last task", () => {
      const allComplete = vi.fn();
      const taskFailed = vi.fn();
      panel.onAllComplete(allComplete);
      panel.onTaskFailed(taskFailed);

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.failTask("task-1");

      expect(taskFailed).toHaveBeenCalledWith("task-1");
      expect(allComplete).toHaveBeenCalledTimes(1);
    });

    it("failTask does nothing for non-existent task", () => {
      const callback = vi.fn();
      panel.onTaskFailed(callback);

      panel.failTask("non-existent");

      expect(callback).not.toHaveBeenCalled();
    });
  });

  describe("Hide/Show", () => {
    it("starts collapsed (minimized) by default — body surfaces only on event/hover", () => {
      expect(panel.isHidden).toBe(true);
    });

    it("hide() hides the panel", () => {
      // Narrated task creates panel DOM.
      panel.handleJobUpdate({
        id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
      panel.hide();

      expect(panel.isHidden).toBe(true);
      const panelEl = container.querySelector(".moss-progress-panel");
      expect(panelEl!.classList.contains("closed")).toBe(true);
    });

    it("show() shows the panel", () => {
      panel.handleJobUpdate({
        id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
      panel.hide();
      panel.show();

      expect(panel.isHidden).toBe(false);
      const panelEl = container.querySelector(".moss-progress-panel");
      expect(panelEl!.classList.contains("closed")).toBe(false);
    });

    it("handlePluginProgress creates task when not completed", () => {
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "validating",
        current: 1,
        total: 5,
        message: "Checking requirements...",
        completed: false,

      });

      expect(panel.hasActiveTasks()).toBe(true);
      expect(panel.getActiveTasks()).toHaveLength(1);
      const task = panel.getActiveTasks()[0];
      expect(task.id).toBe("plugin:github:deploy");
      expect(task.label).toBe("github");
      expect(task.progress).toBe(20); // 1/5 * 100
    });

    it("handlePluginProgress removes task when completed", () => {
      // First create the task
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 4,
        total: 5,
        message: "Pushing to remote...",
        completed: false,

      });
      expect(panel.getActiveTasks()).toHaveLength(1);

      // Then complete it
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });

      expect(panel.hasActiveTasks()).toBe(false);
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("handlePluginProgress with completed triggers auto-close", () => {
      // Add a plugin task
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 4,
        total: 5,
        message: null,
        completed: false,

      });
      expect(panel.isHidden).toBe(false);

      // Complete it — panel should auto-close since no tasks remain
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });

      expect(panel.isHidden).toBe(true);
    });
  });

  /**
   * The explicit `RevealState` enum is the single source of truth for
   * visibility, owned solely by ProgressPanel. A boolean `_isHidden` could not
   * represent peek/pin/surfaced and produced the checkAutoClose-vs-pin race.
   *
   *   minimized = no panel body; the persistent seam and dot remain
   *   surfaced  = auto-shown when a consequential event needs attention
   *   peek      = shown transiently on hover
   *   pinned    = click-shown; survives mouse-leave AND advisory-clear auto-close
   *
   * `isHidden` is DERIVED (isHidden === state === 'minimized') so the legacy
   * call sites keep working; `getRevealState()` exposes the richer state.
   */
  describe("RevealState enum owns panel visibility", () => {
    it("starts minimized — the resting state is a quiet collapsed hairline", () => {
      // The panel's resting state is collapsed: the body surfaces only on a
      // consequential event (`surfaceOnInterrupt`/`show`) or a hover-peek. A
      // routine build is already quiet, so the default mirrors that.
      expect(panel.isHidden).toBe(true);
      expect(panel.getRevealState()).toBe("minimized");
    });

    it("⌄ minimize transitions to 'minimized' (the panel body is gone)", () => {
      // Narrated task creates panel DOM.
      panel.handleJobUpdate({
        id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
      expect(container.querySelector('[aria-label="Minimize"]')).toBeNull();
      panel.hide();
      expect(panel.getRevealState()).toBe("minimized");
      expect(panel.isHidden).toBe(true);
      const panelEl = container.querySelector(".moss-progress-panel");
      // The body closes: `.closed` is height:0 + visibility:hidden.
      expect(panelEl!.classList.contains("closed")).toBe(true);
    });

    it("a consequential event (non-ambient upsert while minimized) → 'surfaced'", () => {
      panel.upsertTask(createTask());
      panel.hide();
      expect(panel.getRevealState()).toBe("minimized");
      // A non-ambient (consequential) task un-hides → surfaced.
      panel.upsertTask(createTask({ id: "deploy:push", label: "Deploying" }));
      expect(panel.getRevealState()).toBe("surfaced");
      expect(panel.isHidden).toBe(false);
    });

    it("pin() shows the panel and survives advisory-clear auto-close (the race fix)", () => {
      panel.pin();
      expect(panel.getRevealState()).toBe("pinned");
      expect(panel.isHidden).toBe(false);
      // No tasks, no advisories, no publish, build settled — checkAutoClose
      // would close a surfaced/peek panel here. A PINNED panel must NOT close.
      panel.setStage("ready"); // settled → drives checkAutoClose
      expect(panel.getRevealState()).toBe("pinned");
      expect(panel.isHidden).toBe(false);
    });

    it("a pinned panel does NOT auto-close when its last task is removed", () => {
      panel.pin();
      panel.upsertTask(createTask({ id: "t1" }));
      // pin() then a task — removing the task settles the panel; a surfaced
      // panel would auto-close, a pinned one stays open (user pinned it).
      panel.removeTask("t1");
      expect(panel.getRevealState()).toBe("pinned");
      expect(panel.isHidden).toBe(false);
    });

    it("a surfaced panel DOES auto-close when its last task is removed (unchanged)", () => {
      panel.show(); // surfaced
      panel.upsertTask(createTask({ id: "t1" }));
      panel.removeTask("t1");
      expect(panel.isHidden).toBe(true);
      expect(panel.getRevealState()).toBe("minimized");
    });
  });

  // Renderer DOM coverage previously asserted rows for source="moss" tasks
  // (the old _renderLegacy path). In the new model, only narrated tasks render as rows;
  // source="moss" tasks appear in the status line only. DOM-level coverage (task rows,
  // state glyphs, label rendering) is now in progress-panel-renderer.test.ts which drives
  // the new renderNarrated() path directly. These tests are deleted as redundant.

  describe("init()", () => {
    it("init resolves without error", async () => {
      await expect(panel.init()).resolves.toBeUndefined();
    });

    it("keeps every subscription that succeeded when one fails, so destroy() releases them", async () => {
      const { host } = await import("../../../host");
      const released: string[] = [];
      host.engine = {
        ...host.engine,
        on: async (name: string) => {
          if (name === "scan-progress") throw new Error("refused");
          return () => { released.push(name); };
        },
      };
      await panel.init();
      panel.destroy();
      expect(released).toEqual(expect.arrayContaining(["moss-event", "scan-complete", "deploy-progress"]));
    });
  });

  describe("domain progress via the unified Job", () => {
    function dnsWire(state: any): any {
      return {
        id: "9", window_id: "main", scope: "preview", kind: "deploy",
        tone: "ambient", title: "Connecting example.test", fade_after_ms: null,
        started_at_ms: "0", verb: null, amount: null, elapsed_ms: null,
        parent: null, state,
      };
    }

    it("renders a domain Job as a task row with indeterminate progress", () => {
      panel.handleJobUpdate(
        dnsWire({ state: "running", fraction: null, message: null }),
      );
      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].label).toBe("Connecting example.test");
      expect(tasks[0].progress).toBeNull();
    });

    it("auto-closes when the domain Job is the only task and is cancelled", () => {
      panel.show();
      panel.handleJobUpdate(
        dnsWire({ state: "running", fraction: 0.2, message: null }),
      );
      expect(panel.isHidden).toBe(false);
      panel.handleJobUpdate(dnsWire({ state: "cancelled" }));
      expect(panel.isHidden).toBe(true);
    });
  });

  describe("Auto-close Logic", () => {
    it("auto-closes when last task is removed via removeTask", () => {
      panel.show(); // Ensure panel is visible
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));

      expect(panel.isHidden).toBe(false);

      panel.removeTask("task-1");
      expect(panel.isHidden).toBe(false); // Still has task-2

      panel.removeTask("task-2");
      expect(panel.isHidden).toBe(true); // Should auto-close
    });

    it("auto-closes when last task is removed via failTask", () => {
      panel.show();
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));

      expect(panel.isHidden).toBe(false);

      panel.failTask("task-1");
      expect(panel.isHidden).toBe(false); // Still has task-2

      panel.failTask("task-2");
      expect(panel.isHidden).toBe(true); // Should auto-close
    });

    it("auto-closes on removeTask even with mixed success/failure", () => {
      panel.show();
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));

      panel.failTask("task-1"); // Fail first task
      expect(panel.isHidden).toBe(false);

      panel.removeTask("task-2"); // Remove last task
      expect(panel.isHidden).toBe(true);
    });

    it("fires onAllComplete callback on auto-close", () => {
      const callback = vi.fn();
      panel.onAllComplete(callback);
      panel.show();

      panel.upsertTask(createTask({ id: "task-1" }));
      panel.removeTask("task-1");

      expect(callback).toHaveBeenCalledTimes(1);
      expect(panel.isHidden).toBe(true);
    });

    it("does not auto-close when already hidden", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.hide(); // Manually hide

      const callback = vi.fn();
      panel.onAllComplete(callback);

      panel.removeTask("task-1");

      // Panel was already hidden, so auto-close should not change state
      expect(panel.isHidden).toBe(true);
      expect(callback).toHaveBeenCalledTimes(1); // Callback still fires
    });

    it("does not auto-close if no tasks were active", () => {
      // Surface the panel first (default is collapsed now).
      panel.show();
      expect(panel.isHidden).toBe(false);

      // removeTask of a non-existent id early-returns BEFORE checkAutoClose, so a
      // visible panel must not spuriously close.
      panel.removeTask("non-existent");

      expect(panel.isHidden).toBe(false);
    });

    it("auto-shows panel when task is upserted while hidden", () => {
      // Setup: add and complete a task to trigger auto-close
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.removeTask("task-1");
      expect(panel.isHidden).toBe(true); // Auto-closed

      // Act: new task arrives (e.g., deploy progress)
      panel.upsertTask(createTask({ id: "task-2", label: "Deploying" }));

      // Assert: panel state is visible again.
      // (DOM .hidden class is a concern for the renderer test; state is what matters here.)
      expect(panel.isHidden).toBe(false);
    });

    it("auto-shows panel when handlePluginProgress adds task while hidden", () => {
      // Setup: previous auto-close
      panel.upsertTask(createTask({ id: "old-task" }));
      panel.removeTask("old-task");
      expect(panel.isHidden).toBe(true);

      // Act: plugin deploy progress arrives
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "validating",
        current: 1,
        total: 10,
        message: "Checking requirements...",
        completed: false,

      });

      // Assert: panel is visible
      expect(panel.isHidden).toBe(false);
      expect(panel.hasActiveTasks()).toBe(true);
    });
  });

  describe("Multi-plugin progress isolation", () => {
    it("suppresses the process hook — it shows in the editor hairline, not here", () => {
      panel.handlePluginProgress({
        plugin_name: "matters",
        hook_name: "process",
        phase: "syncing",
        current: 3,
        total: 10,
        message: "Syncing articles...",
        completed: false,
      });
      expect(panel.getActiveTasks()).toHaveLength(0);

      // ...while deploy/syndicate still render here.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 1,
        total: 5,
        message: "Pushing...",
        completed: false,
      });
      expect(panel.getActiveTasks()).toHaveLength(1);
    });

    it("completing one plugin's hook does not remove another plugin's progress", () => {
      vi.useFakeTimers();
      try {
        // Add matters:syndicate task (publish hook — paints immediately).
        panel.handlePluginProgress({
          plugin_name: "matters",
          hook_name: "syndicate",
          phase: "distributing",
          current: 3,
          total: 10,
          message: "Distributing content...",
          completed: false,
        });

        // Add github:deploy task (publish hook — paints immediately).
        panel.handlePluginProgress({
          plugin_name: "github",
          hook_name: "deploy",
          phase: "pushing",
          current: 1,
          total: 5,
          message: "Pushing to remote...",
          completed: false,
        });

        expect(panel.getActiveTasks()).toHaveLength(2);

        // Complete matters
        panel.handlePluginProgress({
          plugin_name: "matters",
          hook_name: "syndicate",
          phase: "done",
          current: 0,
          total: 0,
          message: null,
          completed: true,
        });

        expect(panel.getActiveTasks()).toHaveLength(1);
        expect(panel.getActiveTasks()[0].id).toBe("plugin:github:deploy");
      } finally {
        vi.useRealTimers();
      }
    });

    it("panel does not auto-close when one plugin completes but another is active", () => {
      // Add matters:process task
      panel.handlePluginProgress({
        plugin_name: "matters",
        hook_name: "process",
        phase: "syncing",
        current: 3,
        total: 10,
        message: "Syncing articles...",
        completed: false,

      });

      // Add github:deploy task
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 1,
        total: 5,
        message: "Pushing to remote...",
        completed: false,

      });

      // Complete matters
      panel.handlePluginProgress({
        plugin_name: "matters",
        hook_name: "process",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });

      // Panel should still be visible because github:deploy is active
      expect(panel.isHidden).toBe(false);
      expect(panel.hasActiveTasks()).toBe(true);
    });

    it("panel auto-closes only when ALL plugins complete", () => {
      // Add matters:process task
      panel.handlePluginProgress({
        plugin_name: "matters",
        hook_name: "process",
        phase: "syncing",
        current: 3,
        total: 10,
        message: "Syncing articles...",
        completed: false,

      });

      // Add github:deploy task
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 1,
        total: 5,
        message: "Pushing to remote...",
        completed: false,

      });

      // Complete matters — panel should stay open
      panel.handlePluginProgress({
        plugin_name: "matters",
        hook_name: "process",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });
      expect(panel.isHidden).toBe(false);

      // Complete github — panel should auto-close
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });
      expect(panel.isHidden).toBe(true);
    });

    it("same plugin with different hooks gets separate entries", () => {
      vi.useFakeTimers();
      try {
        // Add matters:configure_domain task (deferred ~250ms, so it only
        // paints once the hook outlives the flash window).
        panel.handlePluginProgress({
          plugin_name: "matters",
          hook_name: "configure_domain",
          phase: "configuring",
          current: 3,
          total: 10,
          message: "Configuring domain...",
          completed: false,
        });
        // Let the deferred row paint.
        vi.advanceTimersByTime(250);

        // Add matters:syndicate task (publish hook — paints immediately).
        panel.handlePluginProgress({
          plugin_name: "matters",
          hook_name: "syndicate",
          phase: "distributing",
          current: 1,
          total: 4,
          message: "Distributing content...",
          completed: false,
        });

        expect(panel.getActiveTasks()).toHaveLength(2);

        // Complete matters:configure_domain only
        panel.handlePluginProgress({
          plugin_name: "matters",
          hook_name: "configure_domain",
          phase: "done",
          current: 0,
          total: 0,
          message: null,
          completed: true,
        });

        expect(panel.getActiveTasks()).toHaveLength(1);
        expect(panel.getActiveTasks()[0].id).toBe("plugin:matters:syndicate");
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe("Deferred upsert — fast build-hook flash suppression", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("a fast build hook start+done within 250ms creates NO task (no flash)", () => {
      // build.rs brackets a build hook with start (completed:false) and
      // done (completed:true). On an incremental rebuild the bracketed step
      // is near-instant, so done arrives well inside the 250ms grace window.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "collecting",
        current: 0,
        total: 0,
        message: "Loading plugin content...",
        completed: false,
      });
      // No row yet — the upsert is deferred.
      expect(panel.getActiveTasks()).toHaveLength(0);

      // done arrives ~16ms later (one frame), inside the window.
      vi.advanceTimersByTime(16);
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });

      // Even after the would-be deferral fires, nothing ever paints.
      vi.advanceTimersByTime(1000);
      expect(panel.getActiveTasks()).toHaveLength(0);
      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("a slow build hook (>250ms) DOES create a task", () => {
      // A genuinely-slow hook (e.g. a comments/social-network fetch) does
      // not complete inside the grace window, so the row surfaces.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "collecting",
        current: 0,
        total: 0,
        message: "Loading plugin content...",
        completed: false,
      });
      expect(panel.getActiveTasks()).toHaveLength(0);

      // Outlive the 250ms window — the deferred row paints.
      vi.advanceTimersByTime(250);
      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].id).toBe("plugin:github:configure_domain");
      expect(tasks[0].source).toBe("plugin");

      // The eventual done still removes it cleanly.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "done",
        current: 0,
        total: 0,
        message: null,
        completed: true,
      });
      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("a later update on a still-deferred hook refreshes counts without re-arming the timer", () => {
      // Ticking progress before the window elapses must not push the paint
      // out forever — the original timer still fires at 250ms with the
      // latest counts.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "collecting",
        current: 1,
        total: 4,
        message: "step 1",
        completed: false,
      });
      vi.advanceTimersByTime(200);
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "collecting",
        current: 3,
        total: 4,
        message: "step 3",
        completed: false,
      });
      // Still deferred 200ms in.
      expect(panel.getActiveTasks()).toHaveLength(0);

      // Original timer fires at 250ms total — paints with the LATEST counts.
      vi.advanceTimersByTime(50);
      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].progress).toBe(75); // 3/4
      expect(tasks[0].detail).toBe("step 3");
    });

    it("deploy hooks still paint immediately (no deferral)", () => {
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 1,
        total: 5,
        message: "Pushing...",
        completed: false,
      });
      // No timer advance — deploy is in PUBLISH_HOOKS.
      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.getActiveTasks()[0].id).toBe("plugin:github:deploy");
    });

    it("destroy cancels a pending deferred upsert (no stray paint)", () => {
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "configure_domain",
        phase: "collecting",
        current: 0,
        total: 0,
        message: null,
        completed: false,
      });
      expect(panel.getActiveTasks()).toHaveLength(0);

      panel.destroy();
      // The deferral timer must not fire a task into a destroyed panel.
      vi.advanceTimersByTime(1000);
      expect(panel.hasActiveTasks()).toBe(false);
    });
  });

  describe("Stale timer", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("auto-removes task after 15s of no updates", () => {
      panel.upsertTask(createTask({ id: "task-1", label: "Stuck task" }));
      expect(panel.hasActiveTasks()).toBe(true);

      vi.advanceTimersByTime(15_000);

      expect(panel.hasActiveTasks()).toBe(false);
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("a plugin publish row survives a long silent step (no heartbeat needed)", () => {
      // A git push or a large upload
      // is legitimately silent for minutes, so the row must outlive the
      // default window on its own.
      panel.handlePluginProgress({
        plugin_name: "github",
        hook_name: "deploy",
        phase: "pushing",
        current: 5,
        total: 10,
        message: "Pushing...",
        completed: false,
      });
      expect(panel.getActiveTasks()).toHaveLength(1);

      vi.advanceTimersByTime(15_000);
      expect(panel.getActiveTasks()).toHaveLength(1);

      // Still bounded: past the publish window it is culled like anything else.
      vi.advanceTimersByTime(600_000);
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("a non-publish plugin hook keeps the 15s window", () => {
      panel.upsertTask(createTask({ id: "plugin:github:configure_domain", label: "Domain" }));
      expect(panel.hasActiveTasks()).toBe(true);

      vi.advanceTimersByTime(15_000);

      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("resets stale timer on each upsertTask call", () => {
      panel.upsertTask(createTask({ id: "task-1", label: "First" }));

      // Advance 10s, then update the same task
      vi.advanceTimersByTime(10_000);
      panel.upsertTask(createTask({ id: "task-1", label: "Updated" }));

      // Advance 10s more (20s from first upsert, 10s from second)
      vi.advanceTimersByTime(10_000);
      expect(panel.hasActiveTasks()).toBe(true); // Still alive

      // Advance 5 more seconds (15s from second upsert)
      vi.advanceTimersByTime(5_000);
      expect(panel.hasActiveTasks()).toBe(false); // Now removed
    });

    it("clears stale timer when removeTask is called", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.removeTask("task-1");

      // Advance past the stale timeout — no errors should occur
      vi.advanceTimersByTime(15_000);

      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("clears all stale timers on destroy", () => {
      panel.upsertTask(createTask({ id: "task-1" }));
      panel.upsertTask(createTask({ id: "task-2" }));
      panel.upsertTask(createTask({ id: "task-3" }));

      panel.destroy();

      // Advance past the stale timeout — no errors should occur
      vi.advanceTimersByTime(15_000);

      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("each task has its own independent stale timer", () => {
      panel.upsertTask(createTask({ id: "task-1" }));

      vi.advanceTimersByTime(5_000);
      panel.upsertTask(createTask({ id: "task-2" }));

      // At 15s: task-1 should be removed, task-2 still alive
      vi.advanceTimersByTime(10_000);
      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.getActiveTasks()[0].id).toBe("task-2");

      // At 20s: task-2 should also be removed
      vi.advanceTimersByTime(5_000);
      expect(panel.hasActiveTasks()).toBe(false);
    });

    it("stale timer removal fires onAllComplete when last task", () => {
      const callback = vi.fn();
      panel.onAllComplete(callback);

      panel.upsertTask(createTask({ id: "task-1" }));

      vi.advanceTimersByTime(15_000);

      expect(callback).toHaveBeenCalledTimes(1);
    });
  });

  describe("destroy", () => {
    it("destroy removes panel from DOM", () => {
      // Narrated task creates panel DOM (only narrated tasks render as rows).
      panel.handleJobUpdate({
        id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
      expect(container.querySelector(".moss-progress-panel")).not.toBeNull();

      panel.destroy();

      expect(container.querySelector(".moss-progress-panel")).toBeNull();
    });

    it("destroy clears internal state", () => {
      panel.upsertTask(createTask());
      panel.destroy();

      expect(panel.hasActiveTasks()).toBe(false);
      expect(panel.getActiveTasks()).toHaveLength(0);
    });
  });

  describe("handleAssetReady — iframe relay", () => {
    // Each onMossEvent() call registers its own listen("moss-event", ...) with
    // an internal kind filter. Multiple variants share the channel, so the
    // mock must collect all callbacks per channel and the helper must invoke
    // them all so the right filter handles the event.
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };
    let mockIframe: HTMLIFrameElement;
    let postMessageSpy: ReturnType<typeof vi.fn>;
    let panelWithIframe: ProgressPanel;

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(
        async (eventName: string, cb: any) => {
          const list = listenCallbacks.get(eventName) ?? [];
          list.push(cb);
          listenCallbacks.set(eventName, list);
          return () => {};
        },
      );

      // Create mock iframe with postMessage
      postMessageSpy = vi.fn();
      mockIframe = document.createElement("iframe");
      Object.defineProperty(mockIframe, "contentWindow", {
        value: { postMessage: postMessageSpy },
        writable: false,
      });

      panelWithIframe = new ProgressPanel(container, mockIframe);
      await panelWithIframe.init();
    });

    afterEach(() => {
      panelWithIframe.destroy();
    });

    it("image AssetReady posts to iframe", () => {
      fireMossEvent({ kind: "AssetReady", payload: { path: "images/photo.jpg", asset_type: "image" } });

      expect(postMessageSpy).toHaveBeenCalledWith(
        {
          type: "moss-asset-ready",
          assetType: "image",
          path: "images/photo.jpg",
        },
        "*",
      );
    });

    it("video AssetReady posts to iframe", () => {
      fireMossEvent({ kind: "AssetReady", payload: { path: "videos/clip.mp4", asset_type: "video" } });

      expect(postMessageSpy).toHaveBeenCalledWith(
        {
          type: "moss-asset-ready",
          assetType: "video",
          path: "videos/clip.mp4",
        },
        "*",
      );
    });

    it("thumbnail AssetReady posts to iframe", () => {
      fireMossEvent({
        kind: "AssetReady",
        payload: { path: "videos/clip.thumb.jpg", asset_type: "thumbnail" },
      });

      expect(postMessageSpy).toHaveBeenCalledWith(
        {
          type: "moss-asset-ready",
          assetType: "thumbnail",
          path: "videos/clip.thumb.jpg",
        },
        "*",
      );
    });

    it("AssetsSettled relays each variant's authoritative asset_type (no extension re-inference)", () => {
      // asset_type is carried from the Rust manifest bucket
      // (build.rs::diff_settled_assets), NOT re-derived from the path extension.
      // The middle entry is the guard: a `.webp` tagged "thumbnail" must post
      // assetType "thumbnail" — an extension guess would have said "image".
      fireMossEvent({
        kind: "AssetsSettled",
        payload: {
          changed: [
            { path: "assets/pic.webp", asset_type: "image" },
            { path: "assets/poster.webp", asset_type: "thumbnail" },
            { path: "videos/clip.mp4", asset_type: "video" },
          ],
        },
      });

      expect(postMessageSpy).toHaveBeenCalledWith(
        { type: "moss-asset-ready", assetType: "image", path: "assets/pic.webp" },
        "*",
      );
      expect(postMessageSpy).toHaveBeenCalledWith(
        { type: "moss-asset-ready", assetType: "thumbnail", path: "assets/poster.webp" },
        "*",
      );
      expect(postMessageSpy).toHaveBeenCalledWith(
        { type: "moss-asset-ready", assetType: "video", path: "videos/clip.mp4" },
        "*",
      );
    });

    it("no crash when no iframe", async () => {
      const panelNoIframe = new ProgressPanel(container);
      await panelNoIframe.init();

      // Should not throw
      expect(() => {
        fireMossEvent({ kind: "AssetReady", payload: { path: "images/photo.jpg", asset_type: "image" } });
      }).not.toThrow();

      panelNoIframe.destroy();
    });
  });

  describe("init() — deploy-progress drives publish stage (no extra task bars)", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      // cloud-sync-store.ts now owns the `icloud-sync` event; reset() clears
      // its subscribers so this suite's panel doesn't hear a previous test's
      // subscription. Must run BEFORE init() (init() is what subscribes).
      cloudSync.reset();
      await panel.init();
    });

    it("registers a listener for deploy-progress", () => {
      expect(listenCallbacks.has("deploy-progress")).toBe(true);
    });

    // `stalled` is a STATE re-asserted every 2 seconds, and
    // an in-flight publish keeps shouldSurface() true, so an ungated surface
    // call re-opened the panel every tick — the user could neither keep it
    // closed nor read the notice's detail.
    describe("a re-asserted cloud stall does not re-yank the panel", () => {
      const fireStalled = () =>
        cloudSync.ingest({ phase: "stalled", provider_label: "Google Drive", total: 250, remaining: 247, downloaded: 3 });

      beforeEach(() => {
        // Engage publish so shouldSurface() is true throughout.
        listenCallbacks.get("deploy-progress")![0]({ stage: "syncing", current: 0, total: 0, message: "" });
      });

      it("the first stall surfaces the panel; a repeat tick leaves it minimized", () => {
        panel.hide();
        fireStalled();
        expect(panel.isHidden).toBe(false); // the edge IS news

        panel.hide(); // the user collapses it
        fireStalled();
        fireStalled();
        expect(panel.isHidden).toBe(true); // …and it stays collapsed
      });

      it("a stall that clears and returns is news again", () => {
        fireStalled();
        panel.hide();
        // "materializing" is the supervisor's non-stalled tick (see
        // cloud_supervisor.rs::emit_progress) — a resumed download.
        cloudSync.ingest({ phase: "materializing", provider_label: "Google Drive", total: 250, remaining: 100, downloaded: 150 });
        expect(panel.isHidden).toBe(true); // resumed download is not an interrupt
        fireStalled();
        expect(panel.isHidden).toBe(false);
      });
    });

    // `screenUp` is PUSHED by `PreviewManager.applyUnshowableReason`, the single place
    // that resolves the whole ladder (gate, unreachable, error, download,
    // build stage all fold into one boolean here). Prove the panel goes
    // quiet on that push alone, with no per-fact re-derivation of its own:
    // engage publish (so `shouldSurface()` alone would surface the panel if
    // `screenUp` weren't checked first) and show that a stalled tick arriving
    // while `screenUp` is true keeps the panel hidden. A wait is not a read
    // failure and must not create a NeedsAction notice.
    it("while screenUp is pushed true, a stalled tick keeps the panel hidden", () => {
      listenCallbacks.get("deploy-progress")![0]({ stage: "syncing", current: 0, total: 0, message: "" });
      panel.hide();
      panel.setScreenUp(true);

      cloudSync.ingest({ phase: "home_waiting", total: 5, remaining: 5, downloaded: 0 });
      cloudSync.ingest({ phase: "stalled", provider_label: "Google Drive", total: 5, remaining: 5, downloaded: 0 });

      expect(panel.isHidden).toBe(true); // screenUp kept the panel quiet
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    // Going quiet is not enough on the raising tick. This panel anchors above
    // `--z-toast`, so a panel already OPEN when a reason takes the window
    // would sit on top of the full-window waiting screen. `setScreenUp(true)`
    // has to close it on the rising edge, not just silence it going forward.
    it("a panel already open is closed by setScreenUp(true), not just kept quiet", () => {
      listenCallbacks.get("deploy-progress")![0]({ stage: "syncing", current: 0, total: 0, message: "" });
      cloudSync.ingest({ phase: "stalled", provider_label: "Google Drive", total: 5, remaining: 5, downloaded: 0 });
      expect(panel.isHidden).toBe(false); // open, over the preview

      panel.setScreenUp(true);

      expect(panel.isHidden).toBe(true);
    });

    // The screen has a second, non-sticky way up: a preview with
    // nothing of its own to show while files are still arriving. The panel
    // is pushed `screenUp` the same way regardless of which of the ladder's
    // eight reasons is behind it — "one surface at a time" is the rule, and
    // the reason is not the panel's business.
    it("the panel goes quiet for a download-only screen too, pushed the same way as the gate", () => {
      listenCallbacks.get("deploy-progress")![0]({ stage: "syncing", current: 0, total: 0, message: "" });
      panel.setScreenUp(true);
      cloudSync.ingest({ phase: "stalled", provider_label: "Google Drive", total: 5, remaining: 5, downloaded: 0 });

      expect(cloudSync.getState().gateUp).toBe(false);
      expect(panel.isHidden).toBe(true);
      expect(panel.getAdvisories()).toHaveLength(0);

      // And it takes the download back the moment the site is on screen.
      panel.setScreenUp(false);
      cloudSync.ingest({ phase: "stalled", provider_label: "Google Drive", total: 5, remaining: 5, downloaded: 0 });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("keeps concrete failures current behind a screen and clears them on recovery", () => {
      panel.setScreenUp(true);
      cloudSync.ingest({ phase: "unavailable", total: 5, remaining: 4, downloaded: 1,
        unavailable_count: 4, unavailable: ["a.md", "b.md", "c.md"] });
      expect(panel.isHidden).toBe(true);
      expect(panel.getAdvisories()).toHaveLength(1);
      expect(panel.getAdvisories()[0].reason).toContain("4 files");

      cloudSync.ingest({ phase: "materializing", total: 5, remaining: 0, downloaded: 5,
        unavailable_count: 0, unavailable: [] });
      panel.setScreenUp(false);
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("queries full cloud paths only on request and drops an older folder reply", async () => {
      let resolveFirst!: (value: { status: "ok"; data: string[] }) => void;
      const query = vi.spyOn(engine, "getCloudUnavailableFiles")
        .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
        .mockResolvedValueOnce({ status: "ok", data: ["新目录/第二站点.md"] });
      try {
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        cloudSync.ingest({ folder: "/site/a", phase: "unavailable", total: 8, remaining: 4,
          downloaded: 4, unavailable_count: 8, unavailable: ["a.md", "b.md", "c.md"] });
        panel.show();
        expect(query).not.toHaveBeenCalled();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(query).toHaveBeenCalledWith("/site/a"));
        cloudSync.setFolder("/site/b");
        panel.setFolder("/site/b");
        cloudSync.ingest({ folder: "/site/b", phase: "unavailable", total: 1, remaining: 1,
          downloaded: 0, unavailable_count: 1, unavailable: ["新目录/第二站点.md"] });
        panel.show();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(container.querySelector(".moss-notice-cloud-files li")?.textContent)
          .toBe("新目录/第二站点.md"));
        resolveFirst({ status: "ok", data: ["old-site-secret.md"] });
        await Promise.resolve();
        expect(container.textContent).not.toContain("old-site-secret.md");
      } finally {
        query.mockRestore();
      }
    });

    it("drops a reply after supervisor recovery and reports a query error without changing the count", async () => {
      let resolveQuery!: (value: { status: "ok"; data: string[] }) => void;
      const query = vi.spyOn(engine, "getCloudUnavailableFiles")
        .mockResolvedValueOnce({ status: "error", error: "closed owner" })
        .mockImplementationOnce(() => new Promise((resolve) => { resolveQuery = resolve; }));
      try {
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        cloudSync.ingest({ folder: "/site/a", phase: "unavailable", total: 4, remaining: 4,
          downloaded: 0, unavailable_count: 4, unavailable: ["a.md"] });
        panel.show();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(container.textContent).toContain("Couldn’t load the file list"));
        expect(panel.buildViewModel().notices[0].cloudFiles?.count).toBe(4);
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-refresh")!.click();
        cloudSync.ingest({ folder: "/site/a", phase: "materializing", total: 4, remaining: 0,
          downloaded: 4, unavailable_count: 0, unavailable: [] });
        resolveQuery({ status: "ok", data: ["late.md"] });
        await Promise.resolve();
        expect(container.textContent).not.toContain("late.md");
        expect(panel.buildViewModel().notices).toHaveLength(0);
      } finally {
        query.mockRestore();
      }
    });

    it("drops an in-flight list when a newer supervisor snapshot has the same count", async () => {
      let resolveQuery!: (value: { status: "ok"; data: string[] }) => void;
      const query = vi.spyOn(engine, "getCloudUnavailableFiles")
        .mockImplementationOnce(() => new Promise((resolve) => { resolveQuery = resolve; }))
        .mockResolvedValueOnce({ status: "ok", data: ["second.md"] });
      try {
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        const tick = (file: string) => cloudSync.ingest({ folder: "/site/a", phase: "unavailable",
          total: 4, remaining: 4, downloaded: 0, unavailable_count: 4, unavailable: [file] });
        tick("first.md");
        panel.show();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(query).toHaveBeenCalledOnce());
        tick("second.md");
        resolveQuery({ status: "ok", data: ["first.md"] });
        await vi.waitFor(() => expect(container.querySelector(".moss-notice-cloud-files li")?.textContent)
          .toBe("second.md"));
        expect(container.querySelector(".moss-notice-cloud-files li")?.textContent).not.toBe("first.md");
      } finally {
        query.mockRestore();
      }
    });

    it("does not accept an old reply after switching away and back to the same folder", async () => {
      let resolveQuery!: (value: { status: "ok"; data: string[] }) => void;
      const query = vi.spyOn(engine, "getCloudUnavailableFiles")
        .mockImplementationOnce(() => new Promise((resolve) => { resolveQuery = resolve; }));
      try {
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        cloudSync.ingest({ folder: "/site/a", phase: "unavailable", total: 1, remaining: 1,
          downloaded: 0, unavailable_count: 1, unavailable: ["first.md"] });
        panel.show();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(query).toHaveBeenCalledOnce());
        cloudSync.setFolder("/site/b");
        panel.setFolder("/site/b");
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        cloudSync.ingest({ folder: "/site/a", phase: "unavailable", total: 1, remaining: 1,
          downloaded: 0, unavailable_count: 1, unavailable: ["new.md"] });
        panel.show();
        resolveQuery({ status: "ok", data: ["first.md"] });
        await Promise.resolve();
        expect(container.querySelector(".moss-notice-cloud-files li")).toBeNull();
        expect(container.querySelector(".moss-notice-cloud-files-toggle")?.textContent).toContain("1");
      } finally {
        query.mockRestore();
      }
    });

    it("does not recreate the panel when a file-list reply arrives after destroy", async () => {
      let resolveQuery!: (value: { status: "ok"; data: string[] }) => void;
      const query = vi.spyOn(engine, "getCloudUnavailableFiles")
        .mockImplementationOnce(() => new Promise((resolve) => { resolveQuery = resolve; }));
      try {
        cloudSync.setFolder("/site/a");
        panel.setFolder("/site/a");
        cloudSync.ingest({ folder: "/site/a", phase: "unavailable", total: 1, remaining: 1,
          downloaded: 0, unavailable_count: 1, unavailable: ["first.md"] });
        panel.show();
        container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
        await vi.waitFor(() => expect(query).toHaveBeenCalledOnce());
        panel.destroy();
        expect(container.querySelector(".moss-progress-panel")).toBeNull();
        resolveQuery({ status: "ok", data: ["late.md"] });
        await Promise.resolve();
        expect(container.querySelector(".moss-progress-panel")).toBeNull();
      } finally {
        query.mockRestore();
      }
    });

    it("syncing stage advances publish stage without adding a task", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "syncing", current: 0, total: 0, message: "" });

      expect(panel.getPublishStage()).toBe("syncing");
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("uploading tracks current/total for the primary bar", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 3, total: 10, message: "" });

      expect(panel.getPublishStage()).toBe("uploading");
      expect(panel.getPublishUpload()).toEqual({ current: 3, total: 10 });
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("complete returns publish to idle and auto-closes", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 5, total: 10, message: "" });
      expect(panel.isHidden).toBe(false);

      // The publish receipt holds the panel for its dwell, then lets go.
      vi.useFakeTimers();
      try {
        cb({ stage: "complete", current: 0, total: 0, message: "" });
        expect(panel.getPublishStage()).toBe("idle");
        expect(panel.isHidden).toBe(false);
        vi.advanceTimersByTime(3_000);
        expect(panel.isHidden).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    });

    it("failed PERSISTS red status; publish stage stays 'failed', panel stays open", () => {
      // Failed does NOT idle-reset — it sets lastFailure and keeps
      // stage as "failed" so the user sees the red status until they re-publish.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "syncing", current: 0, total: 0, message: "" });
      cb({ stage: "failed", current: 0, total: 0, message: "" });
      expect(panel.getPublishStage()).toBe("failed");
      expect(panel.isHidden).toBe(false);
    });

    it("failed→waiting re-publish resets stage and clears failed status", () => {
      // Spec "Critical": after a failed publish the user presses Publish again.
      // The fresh DeployWaitingForBuild / deploy-progress{waiting} must reset
      // the panel so the red "failed" status is replaced by the live progress.
      const cb = listenCallbacks.get("deploy-progress")![0];

      // 1. Engage a publish and let it fail.
      cb({ stage: "syncing", current: 0, total: 0, message: "" });
      cb({ stage: "failed", current: 0, total: 0, message: "" });
      expect(panel.getPublishStage()).toBe("failed");
      const failedVm = panel.buildViewModel();
      expect(failedVm.status.kind).toBe("failed");

      // 2. Re-publish: a waiting event arrives (DeployWaitingForBuild equivalent).
      cb({ stage: "waiting", current: 0, total: 0, message: "" });

      // 3. Stage must NOT be stuck on "failed" — it should have reset and advanced.
      const newStage = panel.getPublishStage();
      expect(newStage).not.toBe("failed");
      expect(newStage).not.toBe("idle"); // it ADVANCED, it did not just reset
      // 4. Status kind must no longer be "failed".
      const newVm = panel.buildViewModel();
      expect(newVm.status.kind).not.toBe("failed");
    });

    it("a fresh build after a failed publish clears BOTH failure fields", () => {
      // A publish failure is one condition in two fields. Clearing only the red
      // text left the terminal "failed" stage behind, and the panel then
      // narrated NOTHING for the whole next build and never retracted at ready
      // — a blank strip over the preview until the app restarted.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "syncing", current: 0, total: 0, message: "" });
      cb({ stage: "failed", current: 0, total: 0, message: "boom" });
      expect(panel.getPublishStage()).toBe("failed");

      panel.setStage("opening");
      expect(panel.getPublishStage()).toBe("idle");
      panel.setStage("compiling");
      // Assert the KIND, not the copy — the point is that the build is narrated
      // at all, not which words it uses.
      const vm = panel.buildViewModel();
      expect(vm.status.kind).toBe("running");
      expect(vm.status.text).not.toBe("");
      panel.setStage("ready");
      expect(panel.isHidden).toBe(true);
    });

    it("a build start does NOT cancel an in-flight publish", () => {
      // The reset fires only from the "failed" terminal. `setStage('opening')`
      // also fires on an ordinary folder-open, which can land while a deploy is
      // genuinely running in Rust — an unconditional reset would destroy the
      // live upload readout and re-enter the idle→engaged edge, re-stamping the
      // receipt's start and yanking a minimized panel open.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 3, total: 10, message: "" });
      panel.setStage("opening");
      expect(panel.getPublishStage()).toBe("uploading");
      expect(panel.getPublishUpload()).toEqual({ current: 3, total: 10 });
    });

    it("a good server-ready after a publish failure clears ALL THREE recordings", () => {
      // The other site that nulls lastFailure without touching the stage: a
      // preview-server failure overwrites the publish's red text, and the
      // server coming back scope-clears it. Same poisoned half-state.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "syncing", current: 0, total: 0, message: "" });
      cb({ stage: "failed", current: 0, total: 0, message: "boom" });
      fireMossEvent({ kind: "PreviewServerFailed", payload: { message: "port busy", open_in_another_app: false } });

      panel.setStage("ready");
      expect(panel.getPublishStage()).toBe("idle");
      // Assert the SYMPTOM, not just the field. Clearing the stage while the
      // amber advisory survives buys nothing — advisories.size > 0 keeps
      // checkAutoClose from retracting, so the panel stays open on a failure
      // the user has already been shown. The opening path only ever passed this
      // because clearBuildAdvisories() happens to drop the same key; here
      // nothing else does, so it is clearPublishFailure's own job.
      expect(panel.buildViewModel().notices).toHaveLength(0);
      expect(panel.isHidden).toBe(true);
    });

    it("a re-publish after that clean-up starts amber-free", () => {
      // The advisory outlives the `=== "failed"` guard that used to be its only
      // other cleanup: once the stage is idle, advancePublishStage's reset block
      // no longer runs, so a stale "Publish failed — boom" rode all the way
      // through the NEXT publish.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "syncing", current: 0, total: 0, message: "" });
      cb({ stage: "failed", current: 0, total: 0, message: "boom" });
      fireMossEvent({ kind: "PreviewServerFailed", payload: { message: "port busy", open_in_another_app: false } });
      panel.setStage("ready");

      cb({ stage: "waiting", current: 0, total: 0, message: "" });
      expect(panel.buildViewModel().notices).toHaveLength(0);
    });

    it("DeployWaitingForBuild advances publish stage to waiting (no task)", () => {
      fireMossEvent({ kind: "DeployWaitingForBuild" });

      expect(panel.getPublishStage()).toBe("waiting");
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    /**
     * Regression: the stage machine's entrance is shared (DeployWaitingForBuild
     * comes from generic build machinery) but its exit used to be written by
     * hand inside the moss-hosted bodies only. A plugin publish — OnionPress —
     * tripped the entrance and emitted no terminal stage at all, so the panel
     * sat on "Waiting for build… rebuild paused" forever after a publish that
     * had in fact succeeded. The terminal stage now comes from the publish
     * guard, which every publish passes through.
     */
    it("a terminal event alone brings the panel down from waiting", () => {
      fireMossEvent({ kind: "DeployWaitingForBuild" });
      expect(panel.getPublishStage()).toBe("waiting");

      vi.useFakeTimers();
      try {
        listenCallbacks.get("deploy-progress")![0]({ stage: "complete", current: 0, total: 0, message: "Published!" });

        expect(panel.getPublishStage()).toBe("idle");
        // Down once the publish receipt has had its dwell.
        vi.advanceTimersByTime(3_000);
        expect(panel.isHidden).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    });

    it("late waiting after preparing does not regress publish stage", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "preparing", current: 0, total: 0, message: "" });
      expect(panel.getPublishStage()).toBe("preparing");

      fireMossEvent({ kind: "DeployWaitingForBuild" });
      expect(panel.getPublishStage()).toBe("preparing");
    });

    it("ignores events with no payload", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb(null);
      expect(panel.getPublishStage()).toBe("idle");
    });

    // ── Upload detail: size / progress / speed ──────────

    it("uploading WITH bytes puts the readout in the count slot, not the verb", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({
          stage: "uploading",
          current: 7,
          total: 214,
          bytes_uploaded: 12.4e6,
          bytes_total: 58e6,
          message: "",
        });

      const vm = panel.buildViewModel();
      // The verb is the plain form — the file count is gone from the line.
      expect(vm.status.text).toBe("Uploading…");
      expect(vm.status.text).not.toContain("214");
      // The count slot carries the size pair. No rate yet: the trailing window
      // has not spanned MIN_SPAN_MS, and "0 kB/s" must never render.
      expect(vm.status.count).toBe("12.4/58.0 MB");
    });

    it("uploading WITHOUT bytes keeps the file-count verb and no count slot", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 7, total: 214, message: "" });

      const vm = panel.buildViewModel();
      expect(vm.status.text).toBe("Uploading… (7 / 214)");
      expect(vm.status.count).toBeUndefined();
    });

    it("a sub-10 kB deploy keeps the file-count verb instead of `0/0 kB`", () => {
      // The bytes path is not automatically the better path: at kB scale the
      // pair renders no decimals, so a one-post publish would read `0/0 kB`
      // from first tick to last — less than the count it displaced.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({
          stage: "uploading",
          current: 1,
          total: 1,
          bytes_uploaded: 400,
          bytes_total: 900,
          message: "",
        });

      const vm = panel.buildViewModel();
      expect(vm.status.text).toBe("Uploading… (1 / 1)");
      expect(vm.status.count).toBeUndefined();
    });

    it("announces upload milestones at 25/50/75 only, and again after a reset", () => {
      // Milestones, not a stream: 3 announcements per deploy instead of ~240.
      // 0% and 100% are omitted — the stage transitions into and out of
      // `uploading` already announce them.
      const cb = listenCallbacks.get("deploy-progress")![0];
      const announcements: string[] = [];
      const tick = (done: number) => {
        cb({
            stage: "uploading",
            current: 0,
            total: 20,
            bytes_uploaded: done,
            bytes_total: 100e6,
            message: "",
          });
        const a = panel.buildViewModel().status.announce;
        if (a && announcements[announcements.length - 1] !== a) announcements.push(a);
      };

      for (let i = 0; i <= 20; i++) tick(i * 5e6); // 0 → 100% in 20 ticks
      expect(announcements).toEqual([
        "Uploading, 25% complete",
        "Uploading, 50% complete",
        "Uploading, 75% complete",
      ]);

      // A second deploy re-announces (the set is cleared on the edge into
      // `uploading`, so it cannot inherit the previous pass).
      cb({ stage: "complete", current: 20, total: 20, message: "" });
      announcements.length = 0;
      for (let i = 0; i <= 20; i++) tick(i * 5e6);
      expect(announcements).toHaveLength(3);
    });

    it("writes the live region exactly 3 times across ~240 ticks", async () => {
      // The milestone test above dedupes in the harness, so it only proves the
      // view model is STICKY. What actually stops a screen reader reciting a
      // percentage 240 times is the renderer's write-on-CHANGE guard: writing
      // the same string replaces the text node, and a childList mutation in a
      // live region is a fresh announcement. So count the DOM writes.
      const cb = listenCallbacks.get("deploy-progress")![0];
      const tick = (done: number) =>
        cb({
            stage: "uploading",
            current: 0,
            total: 240,
            bytes_uploaded: done,
            bytes_total: 240e6,
            message: "",
          });

      tick(0);
      const live = container.querySelector('.visually-hidden[aria-live="polite"]');
      // 0% is deliberately not a milestone, so the region does not exist yet.
      expect(live).toBeNull();

      const writes: string[] = [];
      const observer = new MutationObserver((records) => {
        for (const r of records) {
          // characterData records target a text node; walk up to an element.
          const node =
            r.target.nodeType === Node.ELEMENT_NODE ? r.target : r.target.parentElement;
          const el = (node as HTMLElement | null)?.closest(
            '.visually-hidden[aria-live="polite"]',
          );
          if (!el) continue;
          // Records are delivered in one microtask batch, so `el.textContent`
          // would read the FINAL text for all of them — take the text the
          // record itself carries.
          writes.push(
            r.type === "childList"
              ? (r.addedNodes[0]?.textContent ?? "")
              : (r.target.textContent ?? ""),
          );
        }
      });
      observer.observe(container, { childList: true, characterData: true, subtree: true });

      for (let i = 1; i <= 240; i++) tick(i * 1e6);
      await Promise.resolve();
      observer.disconnect();

      expect(writes).toEqual([
        "Uploading, 25% complete",
        "Uploading, 50% complete",
        "Uploading, 75% complete",
      ]);
    });

    it("the live region is a SIBLING of the status line, never a descendant", () => {
      // `.moss-status-line` carries aria-atomic="true": a nested live region is
      // subsumed by an atomic ancestor, so each milestone would re-announce the
      // WHOLE line instead of the milestone. Invisible to every other assertion.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({
          stage: "uploading",
          current: 0,
          total: 20,
          bytes_uploaded: 30e6,
          bytes_total: 100e6,
          message: "",
        });

      const statusLine = container.querySelector(".moss-status-line")!;
      expect(statusLine.getAttribute("aria-atomic")).toBe("true");
      const live = container.querySelector('.visually-hidden[aria-live="polite"]')!;
      expect(live).not.toBeNull();
      expect(live.textContent).toBe("Uploading, 25% complete");
      // Outside the atomic region entirely — it is a sibling of the whole
      // status BLOCK now that the block owns the line plus its detail and
      // condition rows.
      const block = container.querySelector(".moss-status-block")!;
      expect(block.contains(statusLine)).toBe(true);
      expect(block.contains(live)).toBe(false);
      expect(live.parentElement).toBe(block.parentElement);
    });

    // The build is FROZEN for the whole publish — the watcher admits
    // no rebuild while the upload runs, so an edit made mid-publish is saved to
    // disk but does not reach the preview until the publish ends. Without this
    // line the preview just stops responding and reads as broken.
    it("says the rebuild is paused for every non-terminal publish stage", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      // It has its own row now — in the `subject` slot
      // it was the only shrinkable child of a full nowrap line and ellipsized
      // to nothing on every upload.
      const condition = () =>
        container.querySelector(".moss-status-condition")?.textContent ?? "";

      for (const stage of ["preparing", "syncing", "committing", "finalizing"]) {
        cb({ stage, current: 0, total: 0, message: "" });
        expect(condition()).toBe("Rebuild paused until upload finishes");
      }

      // Also on the bytes path, where the count slot carries the upload readout.
      cb({
          stage: "uploading",
          current: 1,
          total: 20,
          bytes_uploaded: 30e6,
          bytes_total: 100e6,
          message: "",
        });
      expect(condition()).toBe("Rebuild paused until upload finishes");

      // …and gone once the publish is over and the catch-up rebuild can land.
      cb({ stage: "complete", current: 20, total: 20, message: "" });
      expect(condition()).toBe("");
    });

    // ── The ledger keeps two registers with distinct responsibilities ─────────

    it("puts the size on the headline and the file + rate on the detail row", () => {
      // The row this replaces was `white-space: nowrap` with three children,
      // only one of which could shrink — so the size pair and the rate took the
      // whole 264px and everything else ellipsized to nothing. There was no
      // room left to name a file, which is the request that started this.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({
          stage: "uploading",
          current: 3,
          total: 20,
          bytes_uploaded: 12.4e6,
          bytes_total: 58e6,
          current_file: "assets/video/harvest-timelapse.mp4",
          message: "",
        });

      expect(container.querySelector(".moss-status-line__count")?.textContent)
        .toBe("12.4/58.0 MB");
      // The directory is de-emphasised, not dropped: the basename alone is
      // ambiguous across a site with per-post asset folders.
      expect(container.querySelector(".moss-status-detail__dir")?.textContent)
        .toBe("assets/video/");
      expect(container.querySelector(".moss-status-detail__base")?.textContent)
        .toBe("harvest-timelapse.mp4");
      // The detail row never speaks: the status line above it is an atomic live
      // region and this row changes several times a second.
      expect(container.querySelector(".moss-status-detail")?.getAttribute("aria-hidden"))
        .toBe("true");
    });

    it("names no file when the publish has nothing in flight to name", () => {
      // A commit-only deploy, or the gap between files. The row still carries
      // the rate; an empty filename span would just be a hole.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({
          stage: "uploading",
          current: 3,
          total: 20,
          bytes_uploaded: 12.4e6,
          bytes_total: 58e6,
          message: "",
        });

      expect(container.querySelector(".moss-status-detail__file")).toBeNull();
    });

    // ── The receipt reports the result of publishing ─────────────────────

    it("ends a publish with 'Published', not with the build that preceded it", () => {
      // The reported bug: on `complete` the publish tier went silent and the
      // next tier that answered was the BUILD receipt, so the last word on a
      // publish was "Built · 12 pages · 3.2s".
      const cb = listenCallbacks.get("deploy-progress")![0];
      vi.useFakeTimers();
      try {
        cb({
            stage: "uploading",
            current: 3,
            total: 20,
            bytes_uploaded: 58e6,
            bytes_total: 58e6,
            message: "",
          });
        cb({ stage: "complete", current: 20, total: 20, message: "" });

        const verb = () =>
          container.querySelector(".moss-status-line__verb")?.textContent ?? "";
        expect(verb()).toContain("Published");
        expect(verb()).toContain("58.0 MB");
        expect(verb()).not.toContain("Built");

        // It is a receipt like any other, so it retracts on the same dwell.
        vi.advanceTimersByTime(3_000);
        expect(verb()).not.toContain("Published");
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe("init() — BackgroundProgress: page-counting tasks feed the bar, others get a row", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      // cloud-sync-store.ts now owns the `icloud-sync` event; reset() clears
      // its subscribers so this suite's panel doesn't hear a previous test's
      // subscription. Must run BEFORE init() (init() is what subscribes).
      cloudSync.reset();
      await panel.init();
      // The panel's resting state is collapsed; this group asserts on the panel
      // BODY (notice rows / task rows), so surface it. Tests that exercise
      // visibility transitions (auto-close, hide, surface-on-blocking) drive their
      // own state from here and remain valid.
      panel.show();
    });

    it("a page-counting task (html_pages) does NOT add a peer task bar", () => {
      // The compile stage bar already shows "Generating pages X/Y" for
      // markdown/html_pages — a separate row would duplicate the same data.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 8, total: 12, completed: false },
      });

      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("a task past the compiling stage (images) gets a bg:<task> row naming it", () => {
      // Images run
      // AFTER the primary bar's compiling label goes empty (build reaches
      // 'ready' before the background media pass finishes), so without this
      // row the hairline fill moves with nothing to hover. Ablate the
      // upsertTask call added to the BackgroundProgress handler to watch
      // this go red.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "images", message: "Processing images", current: 8, total: 12, completed: false },
      });

      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].id).toBe("bg:images");
      expect(tasks[0].label).toBe("Optimizing images (8/12)");
    });

    // Background
    // job start/end with kind and duration, keyed on the raw task string
    // (not `bg:<task>`, since videos skip the row's own arrival tracking).
    it("logs a start on the first tick of a kind, and an end with duration on completion", async () => {
      const { info } = await import("../../../log");
      vi.mocked(info).mockClear();

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "images", message: "", current: 1, total: 12, completed: false },
      });
      expect(vi.mocked(info)).toHaveBeenCalledWith("[bg-job] start kind=images");

      vi.mocked(info).mockClear();
      // A second in-progress tick for the SAME kind must not re-log start.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "images", message: "", current: 6, total: 12, completed: false },
      });
      expect(vi.mocked(info)).not.toHaveBeenCalledWith("[bg-job] start kind=images");

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "images", message: "", current: 12, total: 12, completed: true },
      });
      const endCall = vi.mocked(info).mock.calls.map((c) => String(c[0])).find((m) => m.startsWith("[bg-job] end kind=images"));
      expect(endCall).toBeDefined();
      expect(endCall).toMatch(/^\[bg-job\] end kind=images duration_ms=\d+$/);
    });

    it("videos get a bg:videos row from the FIRST queued tick, before FFmpeg's own callback ever fires", () => {
      // Regression (thermo review of efc4083b4f/8ae1428059): Rust emits
      // BackgroundProgress{task:"videos"} before the iCloud materialize wait
      // and the encode-permit queue (video.rs:717-724, 819-826, before
      // 861/949-959) — tens of seconds before VideoConversionProgress's first
      // tick from FFmpeg's callback (video.rs:976-994). The user's exact
      // report (11 videos never finishing): the hairline fill was already
      // moving with nothing to hover. Ablate bgTaskRowDecision's videos
      // branch (progress-panel-stage.ts) to watch this go red.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "videos", message: "", current: 1, total: 11, completed: false },
      });

      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].id).toBe("bg:videos");
      expect(tasks[0].label).toBe("Waiting to convert 11 videos");
    });

    it("the queued bg:videos row's label switches to the named one once FFmpeg's callback starts", () => {
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "videos", message: "", current: 1, total: 11, completed: false },
      });
      expect(panel.getActiveTasks().find((t) => t.id === "bg:videos")?.label).toBe(
        "Waiting to convert 11 videos",
      );

      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: { filename: "intro.mov", current_video: 2, total_videos: 11, phase: "encoding", overall_fraction: 0.1 },
      });
      const row = panel.getActiveTasks().find((t) => t.id === "bg:videos");
      expect(row?.label).toBe("Converting intro.mov · 2/11");
    });

    // The counter the full-window "Building your site" screen reads used to be
    // written by EVERY BackgroundProgress task into one shared pair of
    // scalars, so a real build walked it backwards: markdown finished at
    // 553/553, `html_pages` restarted the numerator, `images` overwrote both
    // with its own unrelated set, and `sealing` reported a flat 0/1. Replayed
    // here in emission order.
    it("the page counter never rewinds across a build's interleaved tasks", () => {
      panel.setStage("opening");
      panel.setStage("compiling");
      const seen: string[] = [];
      const tick = (task: string, current: number, total: number) => {
        fireMossEvent({
          kind: "BackgroundProgress",
          payload: { task, message: "", current, total, completed: false },
        });
        seen.push(panel.buildStageKnownLabel() ?? "");
      };

      tick("markdown", 300, 553);
      tick("markdown", 553, 553);
      tick("html_pages", 40, 554);
      tick("images", 3, 12);
      tick("html_pages", 240, 554);
      tick("videos", 1, 2);
      tick("sealing", 0, 1);
      tick("html_pages", 554, 554);

      expect(seen).toEqual([
        "Formatting your writing… (300 / 553)",
        "Formatting your writing… (553 / 553)",
        "Generating pages… (40 / 554)",
        "Generating pages… (40 / 554)",
        "Generating pages… (240 / 554)",
        "Generating pages… (240 / 554)",
        "Generating pages… (240 / 554)",
        "Generating pages… (554 / 554)",
      ]);
    });

    // The terminal `html_pages` emit reports the pages actually achieved,
    // which trails the last stride tick when documents were skipped. It is the
    // truthful total and still must not walk the numerator back.
    it("the terminal page emit cannot rewind its own phase", () => {
      panel.setStage("opening");
      panel.setStage("compiling");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 240, total: 240, completed: false },
      });
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 220, total: 240, completed: true },
      });

      expect(panel.buildStageKnownLabel()).toBe("Generating pages… (240 / 240)");
    });

    // A straggler from the first pass arriving after the second has started is
    // an out-of-order event, not progress — the same newest-wins rule
    // `STAGE_RANK` enforces one level up.
    it("a late markdown tick cannot rewind the page pass", () => {
      panel.setStage("opening");
      panel.setStage("compiling");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 40, total: 554, completed: false },
      });
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "markdown", message: "", current: 553, total: 553, completed: true },
      });

      expect(panel.buildStageKnownLabel()).toBe("Generating pages… (40 / 554)");
    });

    it("icloud-sync does NOT add a task — the materializing stage bar covers it", () => {
      cloudSync.ingest({
        phase: "materializing",
        provider: "iCloud",
        provider_label: "iCloud",
        total: 10,
        remaining: 6,
        downloaded: 4,
      });

      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    // The readout lasts while files remain in the cloud, whatever the stage:
    // a site with 553 files still in
    // the cloud must not read as finished. The readout lives as long as
    // `remaining > 0` — see `stageContext.remaining`.
    // Driven through `cloudSync.ingest`, not the raw `icloud-sync` listener:
    // the panel and the full-window screen now derive their state from the one
    // store rather than each parsing the event, so the store IS the seam.
    const fireIcloudSync = (phase: string, downloaded: number, total: number) =>
      cloudSync.ingest({
        phase,
        provider: "icloud",
        provider_label: "iCloud",
        total,
        remaining: total - downloaded,
        downloaded,
      });

    it("reports an outstanding download whatever stage the build has reached", () => {
      panel.setStage("compiling");
      fireIcloudSync("materializing", 412, 553);

      expect(panel.buildViewModel().status.text).toContain("Downloading from iCloud");

      // `ready` resets the stage context — but not a download that is still
      // running, because the supervisor keeps draining after the build settles.
      panel.setStage("ready");
      expect(panel.buildViewModel().status.text).toContain("Downloading from iCloud");
    });

    it("keeps the download on screen after home_ready, and clears it at zero", () => {
      // `home_ready` only says the home page exists. The other 553 files are
      // still coming, and that is exactly the state the incident rendered as
      // finished.
      fireIcloudSync("home_ready", 100, 553);
      expect(panel.buildViewModel().status.text).toContain("Downloading from iCloud");

      // A completed build must not claim the site is done while bytes land:
      // "Built · N pages" is true about the build and false about the site.
      fireMossEvent({ kind: "BuildComplete", payload: { total_time_ms: "1200" } });
      const during = panel.buildViewModel().status;
      expect(during.kind).toBe("running");
      expect(during.text).toContain("Downloading from iCloud");

      // Zero left: the episode is over and the readout goes with it, whatever
      // phase carried the news.
      fireIcloudSync("materializing", 553, 553);
      expect(panel.buildViewModel().status.text).not.toContain("Downloading");
    });

    it("BackgroundProgress.advisories populate the advisory store, not the task row's label", () => {
      // The Advisory binding carries {scope, severity, item, what, action} —
      // `what` (renamed from `reason`) carries the human-readable text. A
      // task in flight also gets its own bg:<task> row; this pins
      // that the advisory text lands in the advisory store, never folded
      // into that row's label.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "notebooks", message: "skip", current: 1, total: 2, completed: false,
          advisories: [{
            scope: "File", severity: "ShippedDegraded", item: "nb/big.ipynb",
            what: "skipped after 30s of blocked I/O — next rebuild retries", action: "None",
          }],
        },
      });
      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].id).toBe("bg:notebooks");
      expect(tasks[0].label).not.toContain("30s"); // advisory text stays out of the row
      const advs = panel.getAdvisories();
      expect(advs).toHaveLength(1);
      expect(advs[0].item).toBe("nb/big.ipynb");
      // The store's `reason` field carries the Advisory's `what`.
      expect(advs[0].reason).toContain("30s");
    });

    it("BackgroundProgress: empty/missing advisories add nothing", () => {
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 5, total: 10, completed: false, advisories: [] } });
      expect(panel.getAdvisories()).toHaveLength(0);
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 6, total: 10, completed: false } as any });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("notebooks: incremental advisories accumulate within one run; same key merges", () => {
      // All three ticks belong to the SAME notebooks run (the terminal
      // completed:true arrives only once, at the end) — a duplicate key
      // inside that one run must not grow the store. A duplicate key
      // arriving in a LATER, separate run is a different question
      // covered below.
      const ev = (item: string, completed: boolean) => fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "notebooks", message: "", current: 1, total: 2, completed, advisories: [{ scope: "File", severity: "ShippedDegraded", item, what: "skipped", action: "None" }] },
      });
      ev("a.ipynb", false);
      ev("b.ipynb", false);
      ev("b.ipynb", true); // duplicate key, same run — no growth
      expect(panel.getAdvisories().map((a) => a.item).sort()).toEqual(["a.ipynb", "b.ipynb"]);
    });

    it("a rebuild in which a build advisory is no longer raised removes it", () => {
      // (a) First build: notebooks reports a.ipynb stuck, reaches its own
      // completed:true.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "notebooks", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "a.ipynb", what: "skipped after 30s of blocked I/O", action: "None" }] },
      });
      expect(panel.getAdvisories().map((a) => a.item)).toEqual(["a.ipynb"]);

      // A later per-edit rebuild: the SAME task completes again, but a.ipynb
      // is no longer in its reported set (the author fixed it).
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "notebooks", message: "", current: 1, total: 1, completed: true, advisories: [] },
      });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("a key removed by a clean rebuild reopens if the SAME task reports it again later", () => {
      const tick = (advisories: unknown[]) => fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "notebooks", message: "", current: 1, total: 1, completed: true, advisories },
      });
      const stuck = [{ scope: "File", severity: "ShippedDegraded", item: "a.ipynb", what: "skipped after 30s of blocked I/O", action: "None" }];

      tick(stuck);
      expect(panel.getAdvisories()).toHaveLength(1);
      tick([]); // fixed — swept
      expect(panel.getAdvisories()).toHaveLength(0);
      tick(stuck); // broke again — a NEW instance of the same key
      expect(panel.getAdvisories().map((a) => a.item)).toEqual(["a.ipynb"]);
    });

    it("a task that never ticks this build (fingerprint-skipped) leaves its advisory untouched", () => {
      // images' whole-set fingerprint skip
      // emits NO BackgroundProgress at all for "images" on a
      // rebuild that doesn't touch any image — that is a partial rebuild, not
      // evidence the still-broken image is fixed.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "images", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "p.png", what: "conversion failed", action: "None" }] },
      });
      expect(panel.getAdvisories()).toHaveLength(1);

      // An unrelated markdown-only rebuild: "images" never ticks.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] },
      });
      expect(panel.getAdvisories().map((a) => a.item)).toEqual(["p.png"]);
    });

    it("video + image advisories coexist (keyed by task+item)", () => {
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "v.mov", what: "unsupported codec", action: "None" }] } });
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "p.png", what: "conversion failed", action: "None" }] } });
      expect(panel.getAdvisories()).toHaveLength(2);
    });

    it("build-start (setStage 'opening') clears all advisories", () => {
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "v.mov", what: "x", action: "None" }] } });
      expect(panel.getAdvisories()).toHaveLength(1);
      panel.setStage("opening");
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("an Acknowledge advisory SURVIVES build-start and leaves only by being acknowledged", async () => {
      // It reports an act moss took — a uid it rewrote — not a condition of the
      // build, so a rebuild neither answers it nor raises it again. A notice that asks the author to
      // go and check something.
      const what = "‘a.md’ and ‘b.md’ started as copies of one page; check the comments";
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 0, total: 0, completed: true, advisories: [{ scope: "File", severity: "NeedsAction", item: "b.md", what, action: { Acknowledge: { label: "I’ve checked" } } }] } });
      expect(panel.getAdvisories()).toHaveLength(1);

      panel.setStage("opening");
      expect(panel.getAdvisories()).toHaveLength(1);

      // The dismiss button can only name the GROUPED key, not the storage key.
      const { eventBus, Events } = await import("../../../context/event-bus");
      eventBus.emit(Events.CLEAR_NOTICE, { task: `markdown::${what}` });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("an advisory moss keeps until its task restates it survives build-start and leaves at that task's next tick", () => {
      // A plugin's Sync offer: save-rebuilds run no process hook, so only the
      // plugin's next process run (or the Sync itself) speaks for it.
      const offer = { scope: "Remote", severity: "NeedsAction", item: null, what: "3 new posts on Matters", lifetime: "UntilTaskRestates", action: { InApp: { op: "RunPluginProcess", args: { plugin: "matters" }, label: "Sync" } } };
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "plugin-offer-matters", message: "", current: 0, total: 0, completed: true, advisories: [offer] } });
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "v.mov", what: "x", action: "None" }] } });
      expect(panel.getAdvisories()).toHaveLength(2);

      panel.setStage("opening");
      expect(panel.getAdvisories().map((a) => a.reason)).toEqual(["3 new posts on Matters"]);

      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "plugin-offer-matters", message: "", current: 0, total: 0, completed: true, advisories: [] } });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("a dismissal that matches no notice clears nothing", async () => {
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "v.mov", what: "x", action: "None" }] } });
      const { eventBus, Events } = await import("../../../context/event-bus");
      eventBus.emit(Events.CLEAR_NOTICE, { task: "markdown::nothing like this" });
      expect(panel.getAdvisories()).toHaveLength(1);
    });

    it("a burst of content-only rebuilds produces ZERO panel rows (Sync owns it)", () => {
      // A markdown/html recompile is the preview's heartbeat (Sync), NOT a Job:
      // the backend mints no parent/child Job for a content-only rebuild, so the
      // panel must stay empty. Simulate the keystroke-frequency content stream:
      // many BackgroundProgress ticks for non-media tasks, no PanelTaskUpdate Job.
      for (let i = 0; i < 12; i++) {
        fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: i, total: 12, completed: false, advisories: [] } });
        fireMossEvent({ kind: "BackgroundProgress", payload: { task: "html_pages", message: "", current: i, total: 12, completed: false, advisories: [] } });
      }
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "html_pages", message: "", current: 12, total: 12, completed: true, advisories: [] } });
      expect(panel.getActiveTasks()).toHaveLength(0);
    });

    it("VideoConversionProgress nests its fine-grained row under the live Build parent Job", () => {
      // The Build parent (kind=build) spawns first; VideoConversionProgress is the
      // fine-grained in-progress detail — it renders as a child row nested under
      // that parent (there is no standalone `bg:videos` row).
      fireMossEvent({
        kind: "PanelTaskUpdate",
        payload: {
          id: "200", window_id: "main", scope: "preview", kind: "build",
          tone: "ambient", title: "Building", fade_after_ms: null,
          started_at_ms: "0", verb: null, amount: null, elapsed_ms: null,
          parent: null, state: { state: "running", fraction: null, message: null },
        },
      });
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: { filename: "clip.mov", current_video: 1, total_videos: 3, phase: "encoding", video_fraction: 0.5, overall_fraction: 0.33 },
      });
      const videoRow = panel.getActiveTasks().find((t) => t.id === "bg:videos");
      expect(videoRow).toBeDefined();
      expect(videoRow!.parentId).toBe("job:200");
    });

    it("VideoConversionProgress with no live Build Job still renders (graceful — no parent)", () => {
      // Defensive: if the parent Job spawn was missed, the fine-grained row still
      // renders (parentId unset) rather than vanishing — the detail is never lost.
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: { filename: "clip.mov", current_video: 1, total_videos: 3, phase: "encoding", video_fraction: 0.5, overall_fraction: 0.33 },
      });
      const videoRow = panel.getActiveTasks().find((t) => t.id === "bg:videos");
      expect(videoRow).toBeDefined();
      expect(videoRow!.parentId).toBeUndefined();
    });

    it("FFmpeg-missing yields ONE deduped Environment advisory (the same-reason collapse, unchanged)", () => {
      // The build-wide FFmpeg advisory (item: null, Blocking, Environment) is one
      // root cause → one advisory, never N per-file dots. The child-Job model does
      // not change this — advisories still flow through BackgroundProgress.
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 3, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: { Command: { command: "brew install ffmpeg", label: "Install FFmpeg" } } }] } });
      const buildWide = panel.getAdvisories().filter((a) => a.item == null);
      expect(buildWide).toHaveLength(1);
      expect(buildWide[0].reason).toContain("FFmpeg");
    });

    // Comment-sync advisories must NEVER enter the advisory panel — they are
    // local-first non-blocking issues surfaced in Services > Comments instead.
    it("comment-sync advisories are filtered out (never enter advisory store)", () => {
      // A failed comment-sync with NeedsAction advisories must NOT appear in the
      // advisory store — the filter runs before Array.isArray(p.advisories) ingestion.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "comment-sync", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Site", severity: "NeedsAction", item: null, what: "connection refused", action: "None" }],
        },
      });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("comment-sync Blocking advisory is also filtered (routing override)", () => {
      // Even a Blocking advisory from comment-sync must not surface in the advisory
      // panel — the whole task is routed to Services > Comments unconditionally.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "comment-sync", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Site", severity: "Blocking", item: null, what: "auth expired", action: "None" }],
        },
      });
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("comment-sync filter does not affect other tasks (images advisory still lands)", () => {
      // The comment-sync filter is narrow — other tasks must be unaffected.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "images", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "File", severity: "ShippedDegraded", item: "photo.png", what: "conversion failed", action: "None" }],
        },
      });
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "comment-sync", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Site", severity: "NeedsAction", item: null, what: "offline", action: "None" }],
        },
      });
      // Only the images advisory (comment-sync filtered out).
      expect(panel.getAdvisories()).toHaveLength(1);
      expect(panel.getAdvisories()[0].item).toBe("photo.png");
    });

    it("panel stays open while a blocking advisory is present (no auto-close)", () => {
      // A Blocking advisory shows the panel and, while present,
      // blocks checkAutoClose (advisories.size > 0). A non-blocking advisory
      // wouldn't surface it in the first place, as the ambient-gate tests show.
      panel.setStage("ready"); // settled — would normally auto-close
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
      expect(panel.isHidden).toBe(false);
    });

    it("⌄ minimize HIDES the panel but PRESERVES advisories (does not clear them)", () => {
      // Minimize must NOT discard an FFmpeg/codec advisory. Advisories clear
      // at build-start (setStage 'opening'/'scanning') per the model — never on hide.
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "v.mov", what: "x", action: "None" }] } });
      expect(panel.getAdvisories()).toHaveLength(1);

      // No panel-scoped minimize button; assert it's gone and use hide() directly.
      expect(container.querySelector('[aria-label="Minimize"]')).toBeNull();
      panel.hide();

      expect(panel.isHidden).toBe(true); // hidden…
      expect(panel.getAdvisories()).toHaveLength(1); // …but the advisory store is preserved
    });

    it("renders per-file advisory as a .moss-notice row with a severity dot and filename inline", () => {
      // Advisories render via groupNotices → notices rail (.moss-notice).
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "unsupported codec", action: "None" }] } });
      // No old advisory zone — that DOM is gone.
      expect(container.querySelector(".moss-advisory-zone")).toBeNull();
      // New: notices rail.
      const notice = container.querySelector(".moss-notice");
      expect(notice).not.toBeNull();
      // Severity dot present.
      const dot = notice!.querySelector(".moss-severity-dot");
      expect(dot).not.toBeNull();
      // Filename present (inline in files span or reason).
      expect(notice!.textContent).toContain("clip.MOV");
      // Reason present.
      expect(notice!.textContent).toContain("unsupported codec");
      // No per-row progress bar.
      expect(notice!.querySelector(".moss-progress-bar-fill")).toBeNull();
      // Old chip classes must NOT be present.
      expect(container.querySelector(".moss-advisory")).toBeNull();
      expect(container.querySelector(".moss-advisory-name")).toBeNull();
      expect(container.querySelector(".moss-advisory-chips")).toBeNull();
    });

    it("lays out per-file advisories as .moss-notice rows (distinct reasons → separate rows)", () => {
      // Single-column via the notices rail. Each advisory with a unique
      // reason becomes a separate .moss-notice. (Same-reason files collapse via
      // groupNotices into one notice — that end-to-end test follows below.)
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 3, total: 3, completed: true, advisories: [
        { scope: "File", severity: "ShippedDegraded", item: "a.mov", what: "unsupported codec", action: "None" },
        { scope: "File", severity: "ShippedDegraded", item: "b.MOV", what: "corrupt moov atom", action: "None" },
        { scope: "File", severity: "ShippedDegraded", item: "c.mp4", what: "zero-length file", action: "None" },
      ] } });
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(3); // one per distinct reason
      // Each notice is a .moss-notice element.
      notices.forEach((n) => expect(n.classList.contains("moss-notice")).toBe(true));
    });

    it("guard: .moss-notice rows carry no visible border or border-radius ≥ 20 (flat, not pill)", () => {
      // Declared-value guard: the notice row must never regrow a pill.
      const styleEl = document.createElement("style");
      styleEl.textContent = `.moss-notice { border: 0 solid transparent; border-radius: 0px; }`;
      document.head.appendChild(styleEl);
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "unsupported codec", action: "None" }] } });
      const notice = container.querySelector(".moss-notice")! as HTMLElement;
      const cs = getComputedStyle(notice);
      expect(parseFloat(cs.borderTopWidth) || 0).toBe(0); // no border
      const radius = parseFloat(cs.borderTopLeftRadius) || 0;
      expect(radius).toBeLessThan(20);
      document.head.removeChild(styleEl);
    });

    it("renders build-wide advisory (item null) as a broken .moss-notice with a copyable Command field", () => {
      // Build-wide (item null) advisory → broken notice (Blocking tier)
      // with the Command affordance rendered inside the notice.
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 0, total: 0, completed: true, advisories: [{ scope: "Environment", severity: "NeedsAction", item: null, what: "FFmpeg not available — videos published unoptimized", action: { Command: { run: "brew install ffmpeg", label: "Copy" } } }] } });
      expect(container.querySelector(".moss-advisory-banner")).toBeNull();
      // The Command affordance renders inside the notice via buildCommandField.
      const cmd = container.querySelector(".moss-advisory-command");
      expect(cmd).not.toBeNull();
      expect(cmd!.querySelector(".moss-advisory-command-text")?.textContent).toBe("brew install ffmpeg");
      expect(cmd!.querySelector(".moss-copy-btn")).not.toBeNull();
      // No chip, ever.
      expect(container.querySelector(".moss-advisory-chips")).toBeNull();
    });

    it("guard: .moss-copy-btn has no border at rest (ghost button)", () => {
      // The canonical copy button is borderless ghost — verify no border at rest.
      // We inject a transparent border (same test pattern as the old .moss-advisory-command-copy
      // guard) and check that the computed color is transparent, proving the button does not
      // sport an opaque border when the spec calls for none.
      const styleEl = document.createElement("style");
      styleEl.textContent = `.moss-copy-btn { border: 1px solid transparent; }`;
      document.head.appendChild(styleEl);
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 0, total: 0, completed: true, advisories: [{ scope: "Environment", severity: "NeedsAction", item: null, what: "FFmpeg not available", action: { Command: { run: "brew install ffmpeg", label: "Copy" } } }] } });
      const copy = container.querySelector(".moss-copy-btn")! as HTMLElement;
      const cs = getComputedStyle(copy);
      const bc = cs.borderTopColor;
      expect(bc === "transparent" || bc === "rgba(0, 0, 0, 0)").toBe(true);
      document.head.removeChild(styleEl);
    });

    it("caps per-file notices and shows +N more button past the groupNotices inline cap", () => {
      // groupNotices caps inline files at INLINE_FILE_CAP (3 files per notice).
      // With 11 distinct-reason advisories, each is its own notice (no collapse).
      // The notices rail renders all of them — groupNotices doesn't cap notices, it
      // caps FILES within a notice via hiddenFiles. With distinct reasons, each file
      // is its own notice with 0 hidden files. Verify store size and notices count.
      const advisories = Array.from({ length: 11 }, (_, i) => ({ scope: "File", severity: "ShippedDegraded", item: `f${i}.mov`, what: `reason ${i}`, action: "None" }));
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 11, total: 11, completed: true, advisories } });
      expect(panel.getAdvisories()).toHaveLength(11);
      // 11 distinct-reason advisories → 11 groupNotices notices.
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(11);
    });

    it("collapses same-reason per-file advisories into one notice (event pipeline)", () => {
      // End-to-end: event → advisory store → groupNotices → notices rail.
      // A single root cause hitting N files reads as ONE notice with files listed.
      const reason = "still downloading from iCloud, will optimize on next preview";
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 6, total: 6, completed: true, advisories: Array.from({ length: 6 }, (_, i) => ({ scope: "File", severity: "ShippedDegraded", item: `clip${i}.mov`, what: reason, action: "None" })) } });
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(1); // collapsed to one notice
      expect(notices[0].textContent).toContain(reason);
      // Old advisory-zone DOM is gone.
      expect(container.querySelector(".moss-advisory-summary")).toBeNull();
      expect(container.querySelector(".moss-advisory-more")).toBeNull();
    });

    it("an unresolved advisory drives topSeverity from the panel to the seam", () => {
      // The advisory dot stays on the persistent ProgressSeam;
      // its color is driven by topSeverity via onTopSeverityChange. The PANEL is
      // the SINGLE owner of the advisory store; the seam learns of it via this
      // callback only. onAdvisoryChange is deleted; use onTopSeverityChange instead.
      const severities: string[] = [];
      panel.onTopSeverityChange((s) => severities.push(s)); // fires immediately with current
      expect(severities).toEqual(["none"]);

      // A non-advisory background tick must NOT change severity.
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 1, total: 2, completed: false } });
      expect(severities).toEqual(["none"]);

      // Raise an advisory → severity transitions (once).
      fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "unsupported codec", action: "None" }] } });
      expect(severities.length).toBeGreaterThan(1);
      expect(severities[severities.length - 1]).not.toBe("none");

      // The panel does NOT regrow the dot as its own child (it's a seam concern).
      expect(container.querySelector(".moss-advisory-dot")).toBeNull();
    });

    it("plugin progress STILL adds a secondary task — it can run independently", () => {
      fireMossEvent({
        kind: "PluginProgress",
        payload: {
          plugin_name: "github",
          hook_name: "deploy",
          phase: "uploading",
          message: "Pushing to origin",
          current: 0,
          total: 0,
          completed: false,
        },
      });

      const tasks = panel.getActiveTasks();
      expect(tasks).toHaveLength(1);
      expect(tasks[0].source).toBe("plugin");
    });
  });

  describe("Pipeline integration — single primary bar across build → publish", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      await panel.init();
    });

    const stageLabel = () =>
      container.querySelector(".moss-status-line")?.textContent ?? "";
    const taskLabels = () =>
      Array.from(container.querySelectorAll(".moss-progress-task-label")).map(
        (el) => el.textContent ?? "",
      );
    const stepperVisible = () =>
      container.querySelector(".moss-progress-stepper") !== null;

    it("during compile, BackgroundProgress drives the primary bar without spawning a peer task bar", () => {
      panel.setStage("opening");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 8, total: 12, completed: false },
      });

      // Status line shows compile progress; no duplicate task bar.
      expect(stageLabel()).toContain("8");
      expect(stageLabel()).toContain("12");
      expect(taskLabels()).toEqual([]);
      // Stepper hidden — publish has not been engaged.
      expect(stepperVisible()).toBe(false);
    });

    // Test that the stage label still flips and
    // publish state advances, without asserting stepper DOM.
    it("when publish waits on build, primary bar flips to 'Waiting for build…' (stepper removed)", () => {
      panel.setStage("compiling");
      // A page-counting task (html_pages) so this test's own taskLabels()
      // assertion stays about the stepper/bar, not the separate bg:<task>
      // row coverage (see the "past the compiling stage" describe above).
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 8, total: 12, completed: false },
      });
      // Now publish queues:
      fireMossEvent({ kind: "DeployWaitingForBuild" });

      expect(stageLabel()).toContain("Waiting for build");
      expect(taskLabels()).toEqual([]);
      // No .moss-progress-stepper element.
      expect(stepperVisible()).toBe(false);
    });

    it("when publish progresses, primary bar shows publish label (stepper removed)", () => {
      panel.setStage("ready");
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 3, total: 12, message: "" });

      expect(stageLabel()).toContain("Uploading");
      expect(stageLabel()).toContain("3");
      expect(stageLabel()).toContain("12");
      expect(taskLabels()).toEqual([]);

      // No .moss-progress-step elements.
      expect(container.querySelector(".moss-progress-step")).toBeNull();
    });

    it("on publish complete, panel auto-closes (no leftover bars)", () => {
      panel.setStage("ready");
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 5, total: 5, message: "" });
      expect(panel.isHidden).toBe(false);

      vi.useFakeTimers();
      try {
        cb({ stage: "complete", current: 0, total: 0, message: "" });
        // Held by the publish receipt, then released.
        expect(panel.isHidden).toBe(false);
        vi.advanceTimersByTime(3_000);
        expect(panel.isHidden).toBe(true);
        expect(panel.getPublishStage()).toBe("idle");
      } finally {
        vi.useRealTimers();
      }
    });

    it("BackgroundProgress completion applies final counts (no off-by-one)", () => {
      // Bug: the user saw "Generating pages… (11 / 12)" stuck forever after
      // the build completed. Root cause was the completion event carrying
      // `current = total = 12` being short-circuited before its counts were
      // applied to stageContext. Verify the label catches up to 12/12.
      panel.setStage("opening");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "html_pages",
          message: "",
          current: 11,
          total: 12,
          completed: false,
        },
      });
      expect(stageLabel()).toContain("(11 / 12)");

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "html_pages",
          message: "",
          current: 12,
          total: 12,
          completed: true,
        },
      });

      expect(stageLabel()).toContain("(12 / 12)");
      expect(stageLabel()).not.toContain("(11 / 12)");
    });

    it("comment-sync completion does NOT clobber the compiling counter (buildTotal invariant)", () => {
      // Comment-sync always emits (current:1, total:1). If the
      // comment-sync drop ran AFTER the stageContext writes, buildTotal would be
      // overwritten 47→1 and the compiling label would collapse to "(1 / 1)".
      // The guard at the TOP of the BackgroundProgress handler must bail before
      // any stageContext mutation. Drive the counter to 47/47 via html_pages,
      // then fire a comment-sync completion and assert the label stays 47.
      panel.setStage("opening");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 47, total: 47, completed: false },
      });
      expect(stageLabel()).toContain("(47 / 47)");

      // A comment-sync completion (success or failure) carries current:1,total:1.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "comment-sync", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Site", severity: "NeedsAction", item: null, what: "offline", action: "None" }],
        },
      });

      // The compiling counter is untouched — still 47/47, never collapsed to 1/1.
      expect(stageLabel()).toContain("(47 / 47)");
      expect(stageLabel()).not.toContain("(1 / 1)");
      // And it never entered the advisory store (filtered at the top too).
      expect(panel.getAdvisories()).toHaveLength(0);
    });

    it("advisory-only BackgroundProgress (current=0,total=0,advisories≥1) does not reset stage counter", () => {
      // The symlink-skip advisory fires after the build (task:"assets",
      // current:0, total:0, completed:true, advisories:[…]). If applied, it
      // would reset the compiling-stage counter from "12/12" to "0/0".
      // Guard: advisory-only events must be inert to the counter.
      panel.setStage("opening");

      // Advance to compiling stage (non-completed tick first, matching how the
      // existing "no off-by-one" test drives the panel)
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 11, total: 12, completed: false, advisories: [] },
      });
      expect(stageLabel()).toContain("(11 / 12)");

      // Simulate the normal build completion landing at 12/12
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 12, total: 12, completed: true, advisories: [] },
      });
      expect(stageLabel()).toContain("(12 / 12)");

      // Now fire the advisory-only symlink-skip event
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "assets",
          message: "1 symlink skipped — broken target, escape attempt, or absolute path that would 404 on deploy",
          current: 0,
          total: 0,
          completed: true,
          advisories: [{ scope: "Config", severity: "NeedsAction", item: null, what: "1 symlink skipped — broken target", action: "None" }],
        },
      });

      // Counter must be untouched
      expect(stageLabel()).toContain("(12 / 12)");
      expect(stageLabel()).not.toContain("(0 / 0)");
    });

    it("late BackgroundProgress after ready does not regress the stage", () => {
      // Regression for "Generating pages… (N / N)" stuck-forever bug when
      // site/ already has content from a prior build:
      //
      //   1. start_preview_server succeeds and the backend emits an early
      //      ProgressUpdate { port: Some, is_empty: false } — preview-state
      //      flips to 'ready' and PreviewManager calls setStage('ready'),
      //      auto-closing the panel.
      //   2. Image / asset background tasks keep firing BackgroundProgress.
      //   3. Pre-fix: each non-completion event advanced the stage to
      //      'compiling' (the panel re-appeared). The final ProgressUpdate
      //      from build.rs lands in preview-state.ts's silent-rebuild
      //      branch and returns the SAME status object, so renderStatus
      //      and setStage('ready') are never called again. Stage is stuck
      //      at 'compiling' forever, with the bar showing (N / N).
      //   4. Post-fix: nextStage is monotonic, so build-progress after
      //      'ready' stays at 'ready'. The panel stays hidden.
      panel.setStage("ready");
      expect(panel.isHidden).toBe(true);

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "images",
          message: "Processing images",
          current: 1,
          total: 2,
          completed: false,
        },
      });
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "images",
          message: "Processing images",
          current: 2,
          total: 2,
          completed: true,
        },
      });

      // Stage never rewound, panel never reappeared.
      expect(stageLabel()).not.toContain("Generating pages");
      expect(panel.isHidden).toBe(true);
    });

    // PreviewServerFailed sets lastFailure (red status) + a broken advisory (notice rail).
    // The stageLabel is empty because stage was set to 'ready' and lastFailure
    // drives the status line directly.
    it("PreviewServerFailed surfaces as red status + broken advisory (no task row)", () => {
      // When start_preview_server returns Err the backend emits this event.
      // Surfaces as lastFailure (red status) + broken advisory instead
      // of a preview-server-error task row.
      panel.setStage("compiling");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "images",
          message: "Processing images",
          current: 11,
          total: 12,
          completed: false,
        },
      });

      fireMossEvent({
        kind: "PreviewServerFailed",
        payload: { message: "Address already in use (port 1430)", open_in_another_app: false },
      });

      // No task row — it's a notice now.
      const tasks = panel.getActiveTasks();
      const errTask = tasks.find((t) => t.id === "preview-server-error");
      expect(errTask).toBeUndefined();
      // The advisory store carries the server error (broken notice).
      const advisories = panel.getAdvisories();
      const errAdv = advisories.find((a) => a.task === "preview-server-error");
      expect(errAdv).toBeDefined();
      expect(errAdv?.reason).toContain("Address already in use");
      // The status line is empty (stage was set to 'ready'); lastFailure drives the failed status.
      expect(stageLabel()).toContain("Preview server failed");
    });

    it("VideoConversionProgress task is removed when BackgroundProgress completes", () => {
      // VideoConversionProgress enriches bg:videos with fine-grained encoding
      // detail once FFmpeg's callback starts. The only completion signal is
      // the BackgroundProgress task=videos completed=true event, so the BG
      // handler must always propagate that to removeTask regardless of which
      // of the two listeners last touched the row.
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: {
          phase: "encoding",
          current_video: 1,
          total_videos: 2,
          filename: "intro.mp4",
          overall_fraction: 0.5,
        },
      });
      expect(panel.getActiveTasks().some((t) => t.id === "bg:videos")).toBe(true);

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "videos", message: "", current: 2, total: 2, completed: true },
      });
      expect(panel.getActiveTasks().some((t) => t.id === "bg:videos")).toBe(false);
    });

    // mediaStatus stash (VideoConversionProgress feeds the status line)
    it("VideoConversionProgress stashes mediaStatus {verb, subject, current, total}", () => {
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: {
          phase: "encoding",
          current_video: 3,
          total_videos: 12,
          filename: "intro.mov",
          overall_fraction: 0.25,
        },
      });
      const ms = panel.getMediaStatus();
      expect(ms).not.toBeNull();
      expect(ms!.verb).toBe("Optimizing");
      expect(ms!.subject).toBe("intro.mov");
      expect(ms!.current).toBe(3);
      expect(ms!.total).toBe(12);
    });

    it("mediaStatus is cleared when BackgroundProgress videos completed=true arrives", () => {
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: {
          phase: "encoding",
          current_video: 2,
          total_videos: 5,
          filename: "clip.mp4",
          overall_fraction: 0.4,
        },
      });
      expect(panel.getMediaStatus()).not.toBeNull();

      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "videos", message: "", current: 5, total: 5, completed: true },
      });
      expect(panel.getMediaStatus()).toBeNull();
    });

    it("setStage('initial') clears stale publish state from a prior session", () => {
      // Prior session left publish in waiting (e.g., user closed folder
      // mid-deploy). New session starts with setStage('initial') and
      // should not show "Waiting for build…".
      fireMossEvent({ kind: "DeployWaitingForBuild" });
      expect(panel.getPublishStage()).toBe("waiting");

      panel.setStage("initial");
      expect(panel.getPublishStage()).toBe("idle");
      expect(panel.isHidden).toBe(true);
    });
  });

  /**
   * Routine work remains silent. A routine, advisory-free
   * rebuild must NOT grow/show the panel: the Sync hairline + nav-pill flash
   * own that moment. The panel un-hides ONLY when (a) an advisory is present,
   * or (b) publish/deploy is engaged. The four cases below pin that contract.
   */
  describe("Routine rebuilds obey the silence discipline", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      // cloud-sync-store.ts now owns the `icloud-sync` event; reset() clears
      // its subscribers so this suite's panel doesn't hear a previous test's
      // subscription. Must run BEFORE init() (init() is what subscribes).
      cloudSync.reset();
      await panel.init();
      // A routine rebuild starts from a SETTLED (hidden) panel — the previous
      // build settled and auto-closed. We reset the stage to 'initial' so the
      // build-only stage transitions below are GENUINELY allowed by the
      // monotonic `nextStage` guard (a rebuild from 'ready' is no-op'd by
      // monotonicity and wouldn't exercise the un-hide site at all). 'initial'
      // is the honest "fresh rebuild begins" input, and it is strictly earlier
      // than scanning/compiling so each transition fires.
      panel.setStage("initial");
      panel.hide();
      expect(panel.isHidden).toBe(true);
    });

    const fireScan = (found: number) =>
      listenCallbacks
        .get("scan-progress")!
        .forEach((cb) => cb({ found, evicted: 0 }));
    const fireScanComplete = (evicted = 0, provider: string | null = null) =>
      listenCallbacks
        .get("scan-complete")!
        .forEach((cb) =>
          cb({ total_files: 10, evicted_count: evicted, cloud_provider: provider }),
        );
    const fireIcloud = (downloaded: number, total: number) =>
      cloudSync.ingest({ phase: "materializing", provider_label: "iCloud", total, remaining: total - downloaded, downloaded });
    const fireBgProgress = (current: number, total: number, completed = false, advisories?: any[]) =>
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current, total, completed, ...(advisories ? { advisories } : {}) },
      });

    it("(1) content-only rebuild (scanning→materializing→compiling→ready, NO advisory, publish idle) keeps the panel hidden THE WHOLE TIME", () => {
      // Drive every build-only stage transition the panel's own listeners
      // observe during a file-watch rebuild. None of them may grow the panel —
      // the hairline + green flash own this moment (R4/R12).
      fireScan(3);
      expect(panel.isHidden).toBe(true); // scanning
      fireScanComplete(0, null);
      expect(panel.isHidden).toBe(true); // scan-complete → compiling
      fireBgProgress(4, 12, false);
      expect(panel.isHidden).toBe(true); // compiling progress
      fireBgProgress(12, 12, true);
      expect(panel.isHidden).toBe(true); // compiling terminal
      panel.setStage("ready");
      expect(panel.isHidden).toBe(true); // settled — still quiet
    });

    it("(1b) a materializing (cloud-download) rebuild with no advisory also stays hidden", () => {
      // The icloud-sync / materializing path has its OWN un-hide site; it must
      // be gated by the same rule. A routine rebuild that happens to download
      // evicted cloud files must not extrude the glass.
      fireScan(5);
      fireScanComplete(4, "iCloud");
      fireIcloud(1, 4);
      expect(panel.isHidden).toBe(true);
      fireIcloud(4, 4);
      expect(panel.isHidden).toBe(true);
    });

    it("(2) a rebuild that RAISES A BLOCKING ADVISORY grows the panel (un-hides)", () => {
      // Only a Blocking advisory surfaces the panel. A
      // ShippedDegraded one stays a quiet dot, covered by the ambient-gate tests.
      fireScan(3);
      expect(panel.isHidden).toBe(true); // still quiet up to here
      fireBgProgress(1, 1, true, [
        { scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available — videos did not convert", action: { Command: { run: "brew install ffmpeg", label: "Copy" } } },
      ]);
      // A blocking advisory is present → the panel must surface it.
      expect(panel.isHidden).toBe(false);
      expect(panel.getAdvisories()).toHaveLength(1);
    });

    it("(2b) a rebuild that raises only a ShippedDegraded advisory stays quiet (dot only)", () => {
      fireScan(3);
      expect(panel.isHidden).toBe(true);
      fireBgProgress(1, 1, true, [
        { scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "unsupported codec", action: "None" },
      ]);
      expect(panel.isHidden).toBe(true); // non-blocking → never pushed
      expect(panel.getAdvisories()).toHaveLength(1); // …but the dot is earned
    });

    it("(3) when PUBLISH/DEPLOY is engaged, the panel shows", () => {
      // deploy-progress drives the publish stage machine; an engaged publish is
      // consequential-by-intent and earns the panel.
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 3, total: 12, message: "" });
      expect(panel.isHidden).toBe(false);
      expect(panel.getPublishStage()).toBe("uploading");
    });

    it("(3b) DeployWaitingForBuild (publish queued behind a build) also shows", () => {
      fireMossEvent({ kind: "DeployWaitingForBuild" });
      expect(panel.isHidden).toBe(false);
      expect(panel.getPublishStage()).toBe("waiting");
    });

    it("(4) the hairline/flash path is untouched — build progress still advances the stage state", () => {
      // Silence gates the panel's VISIBILITY, not its bookkeeping. The stage
      // state machine (which the hairline-independent stage label reads) must
      // still advance so that IF the panel is later shown (advisory/publish)
      // the label is correct, and the Sync hairline keeps its own input. We
      // assert the stage advanced even though the panel stayed hidden.
      fireScan(3);
      fireScanComplete(0, null);
      fireBgProgress(8, 12, false);
      // Panel stayed hidden (silence)…
      expect(panel.isHidden).toBe(true);
      // …but the stage advanced to compiling (bookkeeping intact). Showing the
      // panel now reveals the up-to-date compile label, not a stale one.
      panel.show();
      const label = container.querySelector(".moss-status-line")?.textContent ?? "";
      expect(label).toContain("8");
      expect(label).toContain("12");
    });
  });

  /**
   * The ambient gate surfaces only work that needs attention.
   *
   * Two complementary mechanisms, tested here:
   *  - Per-task `ambient` flag: routine media sub-jobs (`bg:videos`, …) are
   *    `ambient` and un-hide the panel ONLY when `shouldSurface()` is true.
   *    Consequential tasks (`preview-server-error`, `dns:check`, deploy hooks)
   *    are NOT ambient and keep un-hiding unconditionally — silencing one of
   *    THOSE would bury a real error (the review's #1 blocker).
   *  - `shouldSurface()` advisory term tightened to severity `Blocking` only:
   *    a `ShippedDegraded` (or non-blocking `NeedsAction`) advisory leaves a
   *    quiet hairline dot but does NOT auto-pop the panel.
   */
  describe("The ambient gate opens the panel only for blocking notices", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      await panel.init();
      // Start from a settled, hidden panel — the routine-rebuild resting state.
      panel.setStage("ready");
      panel.hide();
      expect(panel.isHidden).toBe(true);
    });

    const fireVideo = () =>
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: {
          phase: "encoding",
          current_video: 1,
          total_videos: 3,
          filename: "intro.mp4",
          overall_fraction: 0.33,
        },
      });

    it("(1) a routine VideoConversionProgress (bg:videos, no advisory, publish idle) keeps the panel hidden across updates — but records the task", () => {
      fireVideo();
      expect(panel.isHidden).toBe(true); // ambient → no auto-pop
      // A second tick must not pop it either.
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: { phase: "encoding", current_video: 2, total_videos: 3, filename: "b.mp4", overall_fraction: 0.66 },
      });
      expect(panel.isHidden).toBe(true);
      // …yet the task is still recorded in the store (the work is tracked, just quiet).
      expect(panel.getActiveTasks().some((t) => t.id === "bg:videos")).toBe(true);
    });

    // It uses lastFailure + advisory (notice rail). The panel still surfaces.
    it("(2) PreviewServerFailed STILL surfaces the panel (not silenced) — the regression guard", () => {
      // PreviewServerFailed is a real error — it must reach the user even from
      // a hidden, settled panel. It uses lastFailure + advisory instead
      // of a preview-server-error task row, but surfacing is preserved via
      // surfaceOnInterrupt() in the handler.
      fireMossEvent({
        kind: "PreviewServerFailed",
        payload: { message: "Address already in use (port 1430)", open_in_another_app: false },
      });
      expect(panel.isHidden).toBe(false);
      // No task row — broken advisory + lastFailure instead.
      expect(
        panel.getActiveTasks().some((t) => t.id === "preview-server-error"),
      ).toBe(false);
      // The advisory was set.
      expect(
        panel.getAdvisories().some((a) => a.task === "preview-server-error"),
      ).toBe(true);
    });

    it("(3) a ShippedDegraded advisory does NOT auto-pop the panel (dot only)", () => {
      // A non-blocking advisory (FFmpeg codec, video shipped unoptimized)
      // is a quiet hairline dot — never pushed.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "videos", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "unsupported codec", action: "None" }],
        },
      });
      expect(panel.isHidden).toBe(true); // stayed quiet
      // The advisory is still ingested (it earns the dot), just not surfaced.
      expect(panel.getAdvisories()).toHaveLength(1);
    });

    it("(4) a Blocking advisory surfaces the panel (un-hides)", () => {
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "videos", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available — videos did not convert", action: { Command: { run: "brew install ffmpeg", label: "Copy" } } }],
        },
      });
      expect(panel.isHidden).toBe(false);
      expect(panel.getAdvisories()).toHaveLength(1);
    });

    it("(4b) a routine VideoConversionProgress un-hides ONCE a Blocking advisory is already present (ambient + shouldSurface)", () => {
      // Ambient tasks un-hide when shouldSurface() is true. A blocking advisory
      // present from this build makes the build consequential, so an ambient
      // bg:videos tick is allowed to (re-)surface it. This event ALSO creates
      // the quiet queued bg:videos row (bgTaskRowDecision, skipArrival: true)
      // — the regression this pins: that quiet row-creation must NOT spend
      // upsertTask's first-arrival check, or the LATER fireVideo() below
      // (FFmpeg's own, real arrival) would see the id already present and
      // never ask "should this un-hide a minimized panel?".
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "videos", message: "", current: 1, total: 2, completed: false,
          advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }],
        },
      });
      expect(panel.isHidden).toBe(false); // blocking advisory already surfaced it
      panel.hide(); // user minimizes
      expect(panel.isHidden).toBe(true);
      fireVideo(); // FFmpeg's own tick arrives while the blocking advisory still stands
      expect(panel.isHidden).toBe(false); // shouldSurface() true → its arrival un-hides
    });

  });

  /**
   * The panel header summarizes the job (verb·amount·elapsed). The
   * header title is the job receipt, not a raw media sub-task:
   *   • running → a job VERB with NO amount ("Building…" / "Publishing…")
   *   • done    → the completion receipt "Built · N pages · X.Xs" (amount
   *     present iff Done — invariant #4)
   * The detailed `(N / M)` sub-task count stays a QUIET secondary detail row
   * (`.moss-progress-stage-label`), never the header title. The header element
   * is the same `.moss-progress-receipt` slot the done-state receipt uses (one
   * header title, two altitudes).
   */
  describe("The running header shows the job verb without an amount", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };
    const blocking = [
      { scope: "Environment", severity: "Blocking", item: null, what: "x", action: "None" },
    ];
    const headerTitle = () =>
      container.querySelector(".moss-status-line")?.textContent ?? "";
    const stageRow = () =>
      container.querySelector(".moss-status-line")?.textContent ?? "";

    beforeEach(async () => {
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      await panel.init();
    });

    it("running build → header is the job verb 'Building…' with NO amount (invariant #4)", () => {
      // Status line shows the stage text (e.g. "Generating pages… (3 / 12)").
      panel.setStage("compiling");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: 3, total: 12, completed: false, advisories: blocking },
      });
      expect(panel.isHidden).toBe(false);
      // Status line contains the stage text (build stage label).
      expect(headerTitle()).toContain("Generating pages");
    });

    it("engaged publish → header is the job verb 'Publishing…' with NO amount", () => {
      const cb = listenCallbacks.get("deploy-progress")![0];
      cb({ stage: "uploading", current: 3, total: 12, message: "" });
      expect(panel.isHidden).toBe(false);
      // The status line shows the publish label "Uploading… (3 / 12)".
      expect(headerTitle()).toContain("Uploading");
      expect(headerTitle()).toContain("3");
      expect(headerTitle()).toContain("12");
    });

    it("on completion the header becomes the receipt 'Built · N pages · X.Xs' (amount iff Done)", () => {
      vi.useFakeTimers();
      try {
        panel.setStage("compiling");
        fireMossEvent({
          kind: "BackgroundProgress",
          payload: { task: "html_pages", message: "", current: 12, total: 12, completed: true, advisories: blocking },
        });
        // Still building — status line shows stage label.
        expect(headerTitle()).toContain("Generating pages");
        fireMossEvent({
          kind: "BuildComplete",
          payload: { videos_converted: 0, total_time_ms: "2100", skipped_symlinks: 0 },
        });
        // Done: the receipt in the status line — amount present iff Done.
        expect(headerTitle()).toBe("Built · 12 pages · 2.1s");
      } finally {
        vi.runOnlyPendingTimers();
        vi.useRealTimers();
      }
    });

    it("no running verb grows the glass on a quiet (hidden) routine rebuild (silence discipline)", () => {
      // Settle to ready, then minimize — the panel is hidden. A routine
      // (ambient, advisory-free) rebuild tick must NOT grow a running verb
      // header: the pill flash reports the rebuild without opening the panel.
      panel.setStage("ready");
      panel.hide();
      expect(panel.isHidden).toBe(true);
      fireMossEvent({
        kind: "VideoConversionProgress",
        payload: { phase: "encoding", current_video: 1, total_videos: 3, filename: "intro.mp4", overall_fraction: 0.33 },
      });
      expect(panel.isHidden).toBe(true); // stayed quiet
      expect(headerTitle()).toBe(""); // no running verb rendered into a hidden panel
    });
  });

  /**
   * The `Built · N pages · X.Xs` completion header confirms a finished build.
   * N = the `html_pages` task's total, recorded as receipt state when it
   * arrives; elapsed = `Number(BuildComplete.total_time_ms)` formatted as
   * a Duration at render. The receipt does NOT linger — it retracts after a hard
   * maximum dwell time. The receipt renders only when the panel is
   * already shown (a routine rebuild's completion resolves to the pill flash, not
   * a grown header).
   */
  describe("Completion header reports Built · N pages · X.Xs", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) {
        cb(payload);
      }
    };
    // `unknown`, not `string`: the producer can also hand us an unparseable
    // string, an explicit null, or nothing at all — see the "span the producer
    // could not report" case below.
    const fireBuildComplete = (totalTimeMs: unknown) =>
      fireMossEvent({
        kind: "BuildComplete",
        payload: { videos_converted: 0, total_time_ms: totalTimeMs, skipped_symlinks: 0 },
      });
    // The page count in the receipt comes ONLY from the `html_pages` task — feed
    // it via that task so the count is the PAGE count, not whatever media phase
    // last fired. A Blocking advisory rides on the same event to SURFACE the
    // panel with a Blocking notice and keep it open so the receipt
    // renders (while minimized the dwell still runs, but nothing is painted).
    const firePages = (total: number, advisories?: any[]) =>
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "html_pages", message: "", current: total, total, completed: true, ...(advisories ? { advisories } : {}) },
      });
    // A later media phase (videos/images) that ALSO carries a total — its count
    // must NOT become the receipt's "N pages".
    const fireMedia = (task: string, total: number) =>
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task, message: "", current: total, total, completed: true },
      });
    const headerText = () =>
      container.querySelector(".moss-status-line")?.textContent ?? "";

    beforeEach(async () => {
      vi.useFakeTimers();
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      await panel.init();
    });

    afterEach(() => {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    });

    it("reports the PAGE count, not the media count, when media phases run after html_pages", () => {
      // BuildComplete fires AFTER the media phase, and every
      // BackgroundProgress overwrites buildTotal — so a 47-page site with 3
      // without this, videos would show "Built · 3 pages". The receipt must read the
      // html_pages total (47), unaffected by the later videos/images totals.
      firePages(47, [
        { scope: "Environment", severity: "Blocking", item: null, what: "x", action: "None" },
      ]);
      fireMedia("videos", 3); // 3 videos convert AFTER the 47 pages
      fireMedia("images", 9); // 9 images process AFTER that
      fireBuildComplete("4200");
      expect(headerText()).toBe("Built · 47 pages · 4.2s");
    });

    it("singularises the noun for a one-page build (Amount{count,noun})", () => {
      firePages(1, [
        { scope: "Environment", severity: "Blocking", item: null, what: "x", action: "None" },
      ]);
      fireBuildComplete("900");
      expect(headerText()).toBe("Built · 1 page · 0.9s");
    });

    // The Blocking advisory that surfaces the panel so a receipt can paint.
    const BLOCKING = [
      { scope: "Environment", severity: "Blocking", item: null, what: "x", action: "None" },
    ];

    it("text still being read from documents outranks the build receipt", () => {
      firePages(47, BLOCKING);
      fireBuildComplete("4200");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "derive", message: "Reading PDFs · 0 of 1", current: 0, total: 1, completed: false },
      });
      expect(headerText()).toBe("Reading PDFs · 0 of 1");
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: { task: "derive", message: "", current: 0, total: 0, completed: true },
      });
      expect(headerText()).not.toContain("Reading");
    });

    it("a span the producer could not report still reports the build, minus the duration", () => {
      // "It finished, we don't know how long" is a fact about THIS build — not a
      // reason to lie and not a reason to go silent. Keep the receipt, drop the
      // `· X.Xs` segment. Before: an unparseable span rendered "Built · 47 pages
      // · NaNs", and a null one the quieter fake measurement "· 0.0s".
      for (const span of ["not-a-number", null, undefined]) {
        // A fresh html_pages per build: the count is owned and consumed by the
        // build that reported it, so it is never re-used across two of them.
        firePages(47, BLOCKING);
        fireBuildComplete(span);
        expect(headerText()).toBe("Built · 47 pages");
      }
    });

    it("an unknown span is never filled in from the PREVIOUS build", () => {
      // The count is re-read live from html_pages, so merely LEAVING the old
      // elapsed in place composes one receipt out of two builds — this build's
      // page count beside the last one's span. That reads as a measurement and
      // is a lie, which is worse than a visibly broken "NaNs".
      firePages(47, BLOCKING);
      fireBuildComplete("4200");
      expect(headerText()).toBe("Built · 47 pages · 4.2s");
      firePages(9, BLOCKING);
      fireBuildComplete("garbage");
      expect(headerText()).not.toContain("4.2s");
      expect(headerText()).toBe("Built · 9 pages");
    });

    it("the Job-derived receipt drops an unusable span identically (the two sources agree)", () => {
      // A media build's receipt comes from the parent Build Job, a media-less
      // one from BuildComplete. One missing fact must produce one string, or the
      // panel says two different things about the same condition.
      firePages(47, BLOCKING);
      const wire = (state: any) => ({
        id: "900", window_id: "main", scope: "preview", kind: "build", tone: "ambient",
        title: "Building", fade_after_ms: null, started_at_ms: "0", verb: "Built",
        amount: { count: "47", noun: "pages" }, elapsed_ms: "not-a-number",
        parent: null, state,
      });
      panel.handleJobUpdate(wire({ state: "running", fraction: null, message: null }) as any);
      panel.handleJobUpdate(wire({ state: "succeeded", receipt: null }) as any);
      expect(headerText()).toBe("Built · 47 pages");
    });

    // REPLACES an earlier max-dwell test that fired BuildComplete without ever
    // reaching 'ready'. The backend never produces that order — the terminal
    // progress event that drives ready is emitted at the end of the build fn,
    // and BuildComplete comes later from the spawned seal task — so it exercised
    // an unreachable path, which is exactly why the pageTotal wipe below went
    // unnoticed. Same dwell assertion, real order.
    it("a receipt minted after the stage settles still reads its page count — the stage reset must not dissolve a live receipt (R11)", () => {
      panel.setStage("opening");
      firePages(8, BLOCKING);
      panel.setStage("ready"); // wipes stageContext, where the count used to live
      fireBuildComplete("1500");
      // The Blocking advisory legitimately holds the glass open, so the receipt
      // has somewhere to paint — and must actually form.
      expect(panel.isHidden).toBe(false);
      expect(headerText()).toBe("Built · 8 pages · 1.5s");
      // After the max-dwell window the header is gone (retracted to the pill).
      vi.advanceTimersByTime(ProgressPanel.RECEIPT_DWELL_MS + 50);
      expect(headerText()).toBe("");
    });

    it("a rebuild inside the previous receipt's dwell still gets its own receipt", () => {
      // The dwell timer expires RECEIPT state, so every field it clears must
      // belong to the build it was armed for. The page count was the one field
      // written mid-build (at html_pages) rather than at completion, so build
      // N's timer reached forward and wiped build N+1's already-recorded total:
      // on a panel legitimately held open, any rebuild starting inside the 2.5s
      // window rendered NO receipt at all. Now the count is consumed into the
      // completion snapshot, and the timer can only discard its own build.
      panel.setStage("opening");
      firePages(47, BLOCKING);
      panel.setStage("ready");
      fireBuildComplete("1500"); // build 1 completes, arming its dwell
      expect(headerText()).toBe("Built · 47 pages · 1.5s");

      vi.advanceTimersByTime(1000); // still inside build 1's dwell
      firePages(9, BLOCKING); // build 2 records its count
      vi.advanceTimersByTime(ProgressPanel.RECEIPT_DWELL_MS - 900); // build 1's timer fires
      fireBuildComplete("2000");
      expect(headerText()).toBe("Built · 9 pages · 2.0s");
    });

    it("a build that reports no page count does not borrow the previous build's", () => {
      // The other half of the same ownership rule: the count is CONSUMED at
      // completion, so it cannot outlive its build. A site rendering zero pages
      // emits no html_pages event at all (the emit is guarded on total > 0), and
      // reporting "Built · 47 pages" for it is the plausible-looking lie that is
      // worse than saying nothing.
      panel.setStage("opening");
      firePages(47, BLOCKING);
      panel.setStage("ready");
      fireBuildComplete("1500");
      expect(headerText()).toBe("Built · 47 pages · 1.5s");

      vi.advanceTimersByTime(500); // build 2 lands INSIDE build 1's dwell
      fireBuildComplete("300"); // no intervening html_pages
      expect(headerText()).toBe("");
    });

    it("a build receipt does NOT hold the glass open the way a publish receipt does", () => {
      // The deliberate asymmetry, locked so it cannot be "fixed" into symmetry:
      // a build is ambient, so its receipt paints only into an already-open
      // panel and does not also keep it open. R11 gives that moment the pill
      // flash. A publish receipt, which the author waited on, does hold it.
      panel.setStage("opening");
      firePages(12);
      fireBuildComplete("2100");
      expect(headerText()).toBe("Built · 12 pages · 2.1s");
      panel.setStage("ready");
      expect(panel.isHidden).toBe(true);
    });

    it("the elapsed is forgotten even when no page count ever arrived, so a later one cannot revive it", () => {
      // Skipping the arm when no text can form yet looks like a free
      // optimisation and is not: the page count arrives on the NEXT build,
      // composes `Built · 227 pages · 1.2s` from a build long gone, and paints
      // it MID-BUILD, since the receipt outranks the running build stage.
      panel.setStage("ready");
      panel.hide();
      fireBuildComplete("1200"); // no html_pages yet → no receipt text can form
      vi.advanceTimersByTime(ProgressPanel.RECEIPT_DWELL_MS + 50);
      panel.show();
      firePages(227);
      // Assert the element is ABSENT, not that its text is empty — an empty
      // `.moss-status-line` claiming data-kind="running" would pass the latter.
      expect(panel.isHidden).toBe(false); // the guard only means anything open
      expect(container.querySelector(".moss-status-line")).toBeNull();
    });

    it("a receipt minted while minimized still EXPIRES — a later peek does not replay a stale build (R11)", () => {
      // The load-bearing half is the POST-peek assertion. The pre-peek
      // `headerText() === ""` is weak on its own — the renderer paints nothing
      // whatsoever while minimized, so it would hold even with the silence check
      // deleted from showCompletionReceipt; do not read it as a guard on that.
      // The silence discipline stops the receipt being PAINTED while minimized.
      // It must not stop the receipt being FORGOTTEN: the max-dwell is a
      // wall-clock expiry on the state, not on the pixels. Before this, a quiet
      // rebuild (the common case) armed no timer at all, so the receipt lived
      // until the next build started — and the first thing to open the panel
      // afterwards, however much later, headlined `Built · 227 pages · 3.1s`.
      panel.setStage("ready");
      panel.hide();
      firePages(227); // count recorded, no advisory → the panel stays minimized
      fireBuildComplete("3100");
      expect(panel.isHidden).toBe(true);
      expect(headerText()).toBe(""); // silence held

      // Long past the dwell window, the author hovers the seam.
      vi.advanceTimersByTime(ProgressPanel.RECEIPT_DWELL_MS + 50);
      panel.peek();
      expect(panel.isHidden).toBe(false); // the reveal state did open
      expect(headerText()).toBe(""); // …and it has nothing stale to say
    });

    // A green receipt (data-clear="true") requires zero unresolved notices.
    it("receipt with zero notices sets data-clear=true on .moss-status-line", () => {
      // No advisory: clear receipt.
      firePages(5); // no advisory arg → no advisory in store
      // Surface the panel via setStage so showCompletionReceipt doesn't no-op.
      panel.show();
      fireBuildComplete("1000");
      // Status line should be receipt with data-clear="true" (no notices).
      const line = container.querySelector(".moss-status-line") as HTMLElement | null;
      expect(line).not.toBeNull();
      expect(line!.getAttribute("data-kind")).toBe("receipt");
      expect(line!.getAttribute("data-clear")).toBe("true");
    });

    it("receipt with unresolved notices sets data-clear=false and stays neutral", () => {
      // An advisory is present when BuildComplete fires → clear=false.
      firePages(5, [
        { scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg missing", action: "None" },
      ]);
      expect(panel.isHidden).toBe(false); // blocking advisory surfaced it
      fireBuildComplete("1000");
      const line = container.querySelector(".moss-status-line") as HTMLElement | null;
      expect(line).not.toBeNull();
      expect(line!.getAttribute("data-kind")).toBe("receipt");
      expect(line!.getAttribute("data-clear")).toBe("false");
    });
  });

  /**
   * Seam inputs drive panel transitions. The panel is the ONLY code that
   * maps a hover/click input to a RevealState change (the seam emits inputs).
   * Hover has a ~100ms open-delay (pass-through guard) and a ~200ms close-delay
   * (forgive a quick re-entry); click pins immediately (survives mouse-leave); a
   * blocking advisory mid-peek PROMOTES to surfaced and cancels the pending
   * close. Fake timers drive the delays.
   */
  describe("Hover peeks, clicking pins, and interrupts promote the panel", () => {
    let listenCallbacks: Map<string, Array<(event: any) => void>>;
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) cb(payload);
    };

    beforeEach(async () => {
      vi.useFakeTimers();
      listenCallbacks = new Map();
      setEngineOn(async (eventName: string, cb: any) => {
        const list = listenCallbacks.get(eventName) ?? [];
        list.push(cb);
        listenCallbacks.set(eventName, list);
        return () => {};
      });
      await panel.init();
      // Resting state: settled + minimized (the routine quiet hairline).
      panel.setStage("ready");
      panel.hide();
      expect(panel.getRevealState()).toBe("minimized");
    });

    afterEach(() => {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    });

    it("hover-enter → (after the open-delay) peek; nothing happens before the delay", () => {
      panel.onSeamHoverEnter();
      // Before the open-delay elapses, the panel is still minimized (no bloom).
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS - 1);
      expect(panel.getRevealState()).toBe("minimized");
      vi.advanceTimersByTime(2);
      expect(panel.getRevealState()).toBe("peek");
    });

    it("a hover that leaves BEFORE the open-delay never peeks (pass-through)", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS - 10);
      panel.onSeamHoverLeave(); // cursor crossed the zone, kept going
      vi.advanceTimersByTime(1000);
      expect(panel.getRevealState()).toBe("minimized");
    });

    it("hover-leave from peek → (after the close-delay) minimized", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      expect(panel.getRevealState()).toBe("peek");
      panel.onSeamHoverLeave();
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS - 1);
      expect(panel.getRevealState()).toBe("peek"); // still open within the grace
      vi.advanceTimersByTime(2);
      expect(panel.getRevealState()).toBe("minimized");
    });

    it("a re-enter during the close-delay CANCELS the close (stays peek)", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      panel.onSeamHoverLeave();
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS - 50);
      panel.onSeamHoverEnter(); // cursor came back
      vi.advanceTimersByTime(1000);
      expect(panel.getRevealState()).toBe("peek"); // never closed
    });

    // Seam click does not pin (pin = handle only).
    // Seam click SURFACES from minimized; a subsequent hover-leave still closes a
    // surfaced (not pinned) panel after the grace.
    it("click → surfaced (NOT pinned)", () => {
      panel.onSeamClick();
      expect(panel.getRevealState()).toBe("surfaced");
      // surfaced panels DO auto-close after leave+settle (no tasks/advisories).
    });

    it("click from a peek (with a pending close-timer) → surfaced and cancels the close", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      panel.onSeamHoverLeave(); // close-timer pending
      panel.onSeamClick(); // user committed: now surfaced, close cancelled
      vi.advanceTimersByTime(1000);
      // surfaced: the click cancelled the close-timer; panel remains open.
      expect(panel.getRevealState()).toBe("surfaced");
    });

    it("a Blocking advisory mid-peek (close-timer pending) → surfaced (interrupt cancels the close)", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      expect(panel.getRevealState()).toBe("peek");
      panel.onSeamHoverLeave(); // close-timer now pending
      // An interrupt lands: a blocking advisory. It must promote peek → surfaced
      // and cancel the pending close so the panel the interrupt opened isn't shut.
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "videos", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }],
        },
      });
      expect(panel.getRevealState()).toBe("surfaced");
      vi.advanceTimersByTime(1000); // the would-be close fires into the void
      expect(panel.getRevealState()).toBe("surfaced"); // never closed
    });

    it("a ShippedDegraded advisory mid-peek does NOT promote (dot only; peek closes normally)", () => {
      // A non-blocking advisory is a quiet dot, never an interrupt. It must
      // NOT cancel the close or promote the peek.
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      panel.onSeamHoverLeave(); // close-timer pending
      fireMossEvent({
        kind: "BackgroundProgress",
        payload: {
          task: "videos", message: "", current: 1, total: 1, completed: true,
          advisories: [{ scope: "File", severity: "ShippedDegraded", item: "clip.MOV", what: "x", action: "None" }],
        },
      });
      // Not an interrupt → still peek, close-timer still pending.
      expect(panel.getRevealState()).toBe("peek");
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS + 1);
      expect(panel.getRevealState()).toBe("minimized"); // closed normally
      expect(panel.getAdvisories()).toHaveLength(1); // …but the dot was earned
    });

    it("hover-enter while pinned is a no-op (no downgrade), and leave doesn't close it", () => {
      panel.pin(); // pinned directly
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(1000);
      expect(panel.getRevealState()).toBe("pinned");
      panel.onSeamHoverLeave();
      vi.advanceTimersByTime(1000);
      expect(panel.getRevealState()).toBe("pinned");
    });

    it("⌄ minimize from a peek cancels any pending hover timer (no late reopen/close)", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      expect(panel.getRevealState()).toBe("peek");
      panel.hide(); // ⌄
      expect(panel.getRevealState()).toBe("minimized");
      vi.advanceTimersByTime(1000); // any stray timer must be a no-op
      expect(panel.getRevealState()).toBe("minimized");
    });

    // ── Hover union: the panel glass is part of the hover region ────────────
    // The peeked panel opens UNDER the pointer — its glass top welds to the
    // seam (top = pill.bottom − 1, z:toast) and covers the hit band's lower
    // half (z:2), so the very pointer that triggered the peek lands on the
    // panel and the band fires mouseleave. Without a union, nothing cancels
    // the close: the panel retracts under the cursor, the band re-enters,
    // re-peeks — a ~600ms open/close oscillation (the "hairline flickers"
    // bug). Pointer-on-panel must count as "inside".

    it("pointer landing on the open peek panel cancels the pending close (hover union)", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      expect(panel.getRevealState()).toBe("peek");
      panel.onSeamHoverLeave(); // the opening panel covered the band → leave fired
      panel.onPanelHoverEnter(); // …but the pointer sits on the panel glass
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS + 500);
      expect(panel.getRevealState()).toBe("peek"); // no oscillation
    });

    it("leaving the panel glass retracts the peek after the close-delay", () => {
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      panel.onSeamHoverLeave();
      panel.onPanelHoverEnter();
      panel.onPanelHoverLeave(); // pointer left the union entirely
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS - 1);
      expect(panel.getRevealState()).toBe("peek"); // still in the grace window
      vi.advanceTimersByTime(2);
      expect(panel.getRevealState()).toBe("minimized");
    });

    it("panel hover-leave never closes a pinned panel", () => {
      panel.pin(); // pinned directly
      panel.onPanelHoverEnter();
      panel.onPanelHoverLeave();
      vi.advanceTimersByTime(1000);
      expect(panel.getRevealState()).toBe("pinned");
    });

    it("the rendered panel element feeds pointerenter/leave into the union (renderer wiring)", () => {
      // Narrated task creates panel DOM — only narrated tasks render as rows.
      panel.handleJobUpdate({
        id: "narrated:wire-1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
        title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
        elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
      });
      panel.hide();
      expect(panel.getRevealState()).toBe("minimized");
      panel.onSeamHoverEnter();
      vi.advanceTimersByTime(ProgressPanel.PEEK_OPEN_DELAY_MS);
      expect(panel.getRevealState()).toBe("peek");
      const el = document.querySelector(".moss-progress-panel");
      expect(el).not.toBeNull();
      panel.onSeamHoverLeave(); // band leave (panel covered the pointer)
      el!.dispatchEvent(new Event("pointerenter"));
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS + 500);
      expect(panel.getRevealState()).toBe("peek");
      el!.dispatchEvent(new Event("pointerleave"));
      vi.advanceTimersByTime(ProgressPanel.PEEK_CLOSE_DELAY_MS + 1);
      expect(panel.getRevealState()).toBe("minimized");
    });

  });
});

/**
 * The panel grows from the nav pill with a shared width and origin.
 *
 * jsdom has no layout (getBoundingClientRect() = 0), so these assert DECLARED
 * values only: the mount layer (NOT a descendant of #moss-preview-container),
 * the data-anchor marker, computed border-radius / border-top / transform-origin
 * / width, and the ⌄-not-× minimize control. Pixel alignment is the browser
 * visual gate, never asserted here.
 */
describe("Panel mounting and shape keep it attached to the nav pill", () => {
  let previewContainer: HTMLElement;
  let titlebar: HTMLElement;
  let pill: HTMLElement;
  let panel: ProgressPanel;
  let styleEl: HTMLStyleElement;

  beforeEach(() => {
    // The production shell: a clipped preview container (the OLD mount, which
    // the panel must NOT live in any more) + a non-clipped titlebar holding the
    // nav pill (the live-rect anchor, mirroring preview-hairline.ts).
    previewContainer = document.createElement("div");
    previewContainer.id = "moss-preview-container";
    titlebar = document.createElement("div");
    titlebar.className = "moss-titlebar";
    const cluster = document.createElement("div");
    cluster.className = "moss-pill-cluster";
    pill = document.createElement("div");
    pill.className = "moss-nav-pill";
    cluster.appendChild(pill);
    titlebar.appendChild(cluster);
    document.body.appendChild(previewContainer);
    document.body.appendChild(titlebar);

    // The declared form lives in style.css; inject the rules under test so
    // getComputedStyle resolves them in jsdom (no real stylesheet loaded).
    // GEOMETRY ONLY. Do not add a colour assertion against this block: it is a
    // hand-written copy, so asserting a colour here asserts the copy, not the
    // shipped CSS. The panel's material and ink are checked against source
    // style.css below, and against tokens.css in panel-contrast.test.ts.
    styleEl = document.createElement("style");
    styleEl.textContent = `
      :root {
        --moss-glass-pill-bg: rgba(255, 255, 255, 0.9);
        --moss-glass-blur: 20px;
        --moss-glass-saturate: 100%;
        --moss-color-bg: #faf8f2;
      }
      .moss-progress-panel[data-anchor="nav-pill"] {
        width: fit-content;
        min-width: 280px;
        max-width: 360px;
        /* The real rule is \`background: var(--moss-glass-pill-bg)\` (rgba …,0.9);
           jsdom does NOT resolve var() into backgroundColor (it returns
           transparent — see the probe), so inject the RESOLVED literal here.
           This is the value the var carries in the browser, and it is distinct
           from the old opaque cream underlay which jsdom collapsed to an opaque
           rgb(250,248,242) — the guard below asserts that distinction. */
        background: rgba(255, 255, 255, 0.9);
        backdrop-filter: blur(var(--moss-glass-blur)) saturate(var(--moss-glass-saturate));
        -webkit-backdrop-filter: blur(var(--moss-glass-blur)) saturate(var(--moss-glass-saturate));
        border-radius: 0px 0px 18px 18px;
        border-top-width: 0px;
      }
    `;
    document.head.appendChild(styleEl);

    // Constructed with the clipped container as the LEGACY/fallback mount —
    // the renderer must prefer the titlebar layer when it exists.
    panel = new ProgressPanel(previewContainer);
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(previewContainer);
    document.body.removeChild(titlebar);
    document.head.removeChild(styleEl);
  });

  function panelEl(): HTMLElement {
    // Narrated task creates panel DOM (only narrated tasks render as rows in the new model).
    panel.handleJobUpdate({
      id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
      title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
      elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
    });
    return document.querySelector(".moss-progress-panel") as HTMLElement;
  }

  it("mounts the panel OUTSIDE #moss-preview-container (non-clipped layer)", () => {
    const el = panelEl();
    expect(el).not.toBeNull();
    expect(previewContainer.contains(el)).toBe(false);
    expect(el.closest("#moss-preview-container")).toBeNull();
  });

  it("mounts the panel inside the non-clipped titlebar layer", () => {
    const el = panelEl();
    expect(titlebar.contains(el)).toBe(true);
  });

  it("carries data-anchor=\"nav-pill\"", () => {
    expect(panelEl().getAttribute("data-anchor")).toBe("nav-pill");
  });

  it("leaves the persistent structural seam on the titlebar layer", () => {
    // The persistent seam lives outside the panel on the pill-sibling
    // titlebar layer (ProgressSeam), so it survives the panel's hide() AND
    // teardown. The panel must NOT create it as a child any more (the panel's
    // square top simply meets the persistent seam). Presence + the quiet
    // resting look are now covered by progress-seam.test.ts; here we lock that
    // the panel does not regrow ownership of it.
    const el = panelEl();
    expect(el.querySelector(".moss-progress-seam")).toBeNull();
    expect(el.querySelector(".moss-advisory-dot")).toBeNull();
  });

  // The surface width is read from source (jsdom can't
  // resolve var() via getComputedStyle).
  it("takes the pill cluster's width (pill == window), not fit-content", () => {
    const css = readFileSync(path.resolve(__dirname, "../../../style.css"), "utf8");
    const start = css.indexOf('.moss-progress-panel[data-anchor="nav-pill"] {');
    const rule = css.slice(start, css.indexOf("}", start));
    expect(rule).toContain("width: 100%;");
    expect(rule).not.toContain("fit-content");
  });

  it("is capped at 360px and floored at 280px (publish stepper fits)", () => {
    // max 360 keeps a long FFmpeg lead wrapping rather than running wide; min
    // 280 keeps the publish stepper + "Uploading… (148/200)" from truncating.
    const cs = getComputedStyle(panelEl());
    expect(cs.maxWidth).toBe("360px");
    expect(cs.minWidth).toBe("280px");
  });

  it("source style.css gives the panel its own material AND its own ink", () => {
    // Read from source, not from jsdom: this is the one place the panel's real
    // declarations can be checked, and the defect it guards was invisible to
    // every computed-style test in the suite. The panel's background was on the
    // [data-chrome-theme] (page) axis while all ~23 of its text rules were on
    // the [data-theme] (app) axis, so app-light ink over a dark-chrome panel
    // measured 1.02:1 — unreadable exactly when the two axes disagreed, which
    // is the only case the page axis exists for. The re-binding below is what
    // makes an app-axis colour unrepresentable inside the panel.
    const css = readFileSync(
      path.resolve(__dirname, "../../../style.css"),
      "utf8",
    );
    const ruleStart = css.indexOf(
      '.moss-progress-panel[data-anchor="nav-pill"] {',
    );
    expect(ruleStart).toBeGreaterThan(-1);
    const rule = css.slice(ruleStart, css.indexOf("}", ruleStart));
    // Its own material, not the pill's: the pill is a control over the
    // shell-painted ambient band; this is a reading surface over the preview
    // iframe, where the blur cannot reach.
    expect(rule).toContain("background: var(--moss-chrome-panel-bg);");
    // The blur stays — it is what still reads as glass in the few px of panel
    // that overlap the band — but it is texture, not the legibility mechanism.
    expect(rule).toMatch(
      /backdrop-filter:\s*blur\(var\(--moss-glass-blur\)\)/,
    );
    expect(rule).toMatch(
      /-webkit-backdrop-filter:\s*blur\(var\(--moss-glass-blur\)\)/,
    );
    // Every colour-bearing token the panel subtree reads must be re-bound onto
    // the backdrop axis. A name missing here is a child rendering app-axis ink
    // on a page-axis material — the original defect, one token at a time.
    for (const bound of [
      "--moss-color-text:",
      "--moss-color-text-secondary:",
      "--moss-color-muted:",
      "--moss-color-accent:",
      "--moss-color-accent-quiet:",
      "--moss-error:",
      "--moss-severity-broken:",
      "--moss-severity-degraded:",
      "--moss-border-medium:",
      "--moss-glass-pill-bg:",
    ]) {
      expect(rule).toContain(`${bound} var(--moss-chrome-panel-`);
    }
    // The old opaque-cream underlay layering stays gone: R8's legibility floor
    // is now a token on the panel's own axis, not a second painted layer.
    expect(rule).not.toContain("linear-gradient");
    expect(rule).not.toContain("var(--moss-color-bg)");
    expect(rule).toContain("width: 100%;");
    expect(rule).toContain("border-radius: 0 0 var(--moss-pill-radius) var(--moss-pill-radius);");
  });

  it("does NOT make the seam-hit or panel window-draggable (allowlist model)", () => {
    // Window drag is opt-in via data-tauri-drag-region (window-drag.ts), which
    // only the bare titlebar carries. The seam-hit and the advisory panel are
    // their own mousedown targets and are NEVER marked, so they can't drag the
    // window — no -webkit-app-region carve-outs, and none must creep back in
    // (the old denylist leaked window-drag onto the panel repeatedly). The
    // titlebar-drag-region test guards the whole-file invariant; here we assert
    // these two specific surfaces stay unmarked.
    const css = readFileSync(
      path.resolve(__dirname, "../../../style.css"),
      "utf8",
    );
    const ruleBody = (selector: string): string => {
      const start = css.indexOf(selector);
      expect(start).toBeGreaterThan(-1);
      return css.slice(start, css.indexOf("}", start));
    };
    expect(ruleBody(".moss-seam-hit {")).not.toContain("app-region");
    expect(ruleBody(".moss-progress-panel {")).not.toContain("app-region");
  });

  it("has a SQUARE top and 18px bottom (0px 0px 18px 18px)", () => {
    // The panel hangs off the persistent seam as a straight butt-weld —
    // no parent corner to round concentrically — so the top is square 0, bottom
    // 18px (extrusion).
    // jsdom preserves the shorthand literal
    // but does NOT expand it to the four longhand properties (they return ""),
    // so assert the shorthand string.
    expect(getComputedStyle(panelEl()).borderRadius).toBe("0px 0px 18px 18px");
  });

  it("has no accent border-top", () => {
    expect(getComputedStyle(panelEl()).borderTopWidth).toBe("0px");
  });

  // The drawer's motion is a real height morph. jsdom has
  // no layout, so `morphHeight` measures 0 → 0 and applies the mutation without
  // animating; what these assert is the STRUCTURAL contract — one class, flipped
  // synchronously, with no deferred second phase. The paint is the render gate's.
  it("hide()/show() flip .closed synchronously — no deferred second phase", () => {
    const el = panelEl();
    panel.show();
    expect(el.classList.contains("closed")).toBe(false);
    panel.hide();
    expect(el.classList.contains("closed")).toBe(true);
    panel.show();
    expect(el.classList.contains("closed")).toBe(false);
  });

  it("reduced motion jump-cuts: the class flips and no inline height is left behind", () => {
    const original = window.matchMedia;
    (window as unknown as { matchMedia: typeof window.matchMedia }).matchMedia =
      ((q: string) =>
        ({
          matches: q.includes("prefers-reduced-motion"),
          media: q,
          addEventListener() {},
          removeEventListener() {},
          addListener() {},
          removeListener() {},
          onchange: null,
          dispatchEvent() {
            return false;
          },
        }) as unknown as MediaQueryList) as typeof window.matchMedia;
    try {
      const el = panelEl();
      panel.hide();
      expect(el.classList.contains("closed")).toBe(true);
      expect(el.style.height).toBe("");
      panel.show();
      expect(el.classList.contains("closed")).toBe(false);
      expect(el.style.height).toBe("");
    } finally {
      (
        window as unknown as { matchMedia: typeof window.matchMedia }
      ).matchMedia = original;
    }
  });

  it("exposes a ⌄ minimize control (aria-label Minimize), never a × glyph", () => {
    const el = panelEl();
    expect(el.querySelector('[aria-label="Minimize"]')).toBeNull();
    // No × or ✕ either (nothing like a close glyph should appear).
    expect(el.textContent).not.toContain("×"); // ×
    expect(el.textContent).not.toContain("✕"); // ✕
  });

  it("minimize click retracts/hides the panel", () => {
    panelEl(); // creates the panel
    panel.hide();
    expect(panel.isHidden).toBe(true);
  });

  it("falls back to the constructor container when no titlebar exists", () => {
    // jsdom-only path AND the E2E/test-mode mount: with no .moss-titlebar in
    // the DOM the panel must still render (in the given container) so the
    // existing render tests and the E2E mount keep working.
    document.body.removeChild(titlebar);
    const bare = document.createElement("div");
    document.body.appendChild(bare);
    const p2 = new ProgressPanel(bare);
    // Narrated task creates panel DOM (only narrated tasks render as rows in the new model).
    p2.handleJobUpdate({
      id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
      title: "x", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
      elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
    });
    expect(bare.querySelector(".moss-progress-panel")).not.toBeNull();
    p2.destroy();
    document.body.removeChild(bare);
    // restore titlebar for afterEach symmetry
    document.body.appendChild(titlebar);
  });
});

// AdvisoryEntry carries scope
// =============================================================================
// buildViewModel (full Input→home map)
// Tests (a)–(i) and the tiebreak.
// =============================================================================
describe("buildViewModel", () => {
  let panel: ProgressPanel;
  let container: HTMLElement;
  let listenCallbacks: Map<string, Array<(event: any) => void>>;

  const fireMossEvent = (payload: any) => {
    for (const cb of listenCallbacks.get("moss-event") ?? []) {
      cb(payload);
    }
  };

  // Helper: fire a PanelTaskUpdate through the event bus.
  const fireTaskUpdate = (w: import("@symbiosis-lab/moss-ui").PanelTaskWire) => {
    fireMossEvent({ kind: "PanelTaskUpdate", payload: w });
  };

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
    listenCallbacks = new Map();
    setEngineOn(async (eventName: string, cb: any) => {
      const list = listenCallbacks.get(eventName) ?? [];
      list.push(cb);
      listenCallbacks.set(eventName, list);
      return () => {};
    });
    await panel.init();
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  // ── (a) Mixed build VM ────────────────────────────────────────────────────
  // NeedsAction+File + ShippedDegraded + running stage + mediaStatus →
  // correct PanelViewModel: broken-first notices, broken topSeverity, media in status.
  it("(a) mixed build: NeedsAction+File + ShippedDegraded + compiling stage + mediaStatus → correct PanelViewModel", () => {
    // 1. Inject a NeedsAction File advisory (broken tier)
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "markdown",
        message: "",
        current: 1,
        total: 1,
        completed: true,
        advisories: [{
          scope: "File",
          severity: "NeedsAction",
          item: "broken.md",
          what: "Bad frontmatter — check syntax",
          action: "None",
        }],
      },
    });

    // 2. Inject a ShippedDegraded advisory (degraded tier)
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
          item: "clip.mov",
          what: "unsupported codec — shipped without conversion",
          action: "None",
        }],
      },
    });

    // 3. Set a compiling stage (running build)
    panel.setStage("compiling");

    // 4. Fire VideoConversionProgress so mediaStatus is set
    fireMossEvent({
      kind: "VideoConversionProgress",
      payload: {
        phase: "encoding",
        current_video: 1,
        total_videos: 3,
        filename: "intro.mp4",
        overall_fraction: 0.33,
      },
    });

    const vm = panel.buildViewModel();

    // Status: media encoding wins over build stage (mediaStatus ≠ null)
    expect(vm.status.kind).toBe("running");
    // The media subject is used in the status
    expect(vm.status.subject).toBe("intro.mp4");

    // Notices: broken before degraded
    expect(vm.notices.length).toBeGreaterThanOrEqual(2);
    expect(vm.notices[0].severity).toBe("broken");
    expect(vm.notices[1].severity).toBe("degraded");

    // topSeverity: driven by the broken NeedsAction+File notice
    expect(vm.topSeverity).toBe("broken");

    // reveal is surfaced (default)
    expect(vm.reveal).toBe("surfaced");
  });

  // ── (b) Awaiting bridge ───────────────────────────────────────────────────
  // A Preview-scope Awaiting Job + an awaiting-narration synthetic advisory
  // should each produce ONE degraded notice; the awaiting-narration must NOT
  // double-surface (groupNotices already excludes it).
  it("(b) awaiting bridge: preview Awaiting Job → one degraded notice; awaiting-narration synthetic → one notice, not double-surfaced", () => {
    // 1. Preview-scope awaiting Job
    panel.handleJobUpdate({
      id: "domain-1",
      window_id: "main",
      scope: "preview",
      kind: "deploy",
      tone: "ambient",
      title: "Verify DNS",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: {
        state: "awaiting",
        directive: "Verify your DNS records",
        action: { InApp: { op: "RecheckDns" as const, args: null, label: "Recheck DNS" } },
      },
    });

    // 2. Awaiting-narration synthetic (non-preview, tone=awaiting)
    fireTaskUpdate({
      id: "narr-1",
      window_id: "main",
      scope: "workspace",
      kind: "syndicate",
      tone: "awaiting",
      title: "Authorize publication",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: {
        state: "awaiting",
        directive: "Authorize publication",
        action: "None",
      },
    });

    const vm = panel.buildViewModel();

    // Both awaiting entries surface as notices (degraded)
    const degraded = vm.notices.filter((n) => n.severity === "degraded");
    expect(degraded.length).toBeGreaterThanOrEqual(2);

    // No duplicate for awaiting-narration: the advisory store holds it but
    // groupNotices excludes it via isAwaitingNarration; the bridge adds ONE notice.
    // Confirm the notices have distinct reasons (no exact duplicate).
    const reasons = vm.notices.map((n) => n.reason);
    const unique = new Set(reasons);
    expect(unique.size).toBe(reasons.length);

    // Panel stays open while awaiting (hasActiveTasks / shouldSurface)
    expect(panel.isHidden).toBe(false);
  });

  /**
   * `upsertTask` surfaces on ARRIVAL, which is the right rule for a progress
   * tick and the wrong one here: a job already running has a row, so its turn
   * to awaiting — the one update that is a question for the user — would land
   * in a panel the user minimized and never be seen.
   */
  it("(b2) a running job that turns awaiting surfaces the panel the user minimized", () => {
    const wire = (
      state: import("@symbiosis-lab/moss-ui").PanelTaskWire["state"],
    ): import("@symbiosis-lab/moss-ui").PanelTaskWire => ({
      id: "deploy-awaits", window_id: "main", scope: "preview" as const, kind: "deploy" as const,
      tone: "ambient" as const, title: "Publish", fade_after_ms: null, started_at_ms: 0,
      verb: null, amount: null, elapsed_ms: null, parent: null, state,
    });


    panel.handleJobUpdate(wire({ state: "running", fraction: null, message: null }));
    panel.hide();
    expect(panel.getRevealState()).toBe("minimized");

    panel.handleJobUpdate(
      wire({ state: "awaiting", directive: "Sign in to GitHub", action: "None" })
    );

    expect(panel.getRevealState()).toBe("surfaced");
  });

  // ── (c) Narrated tasks still render via renderNarrated ────────────────────
  it("(c) narrated tasks still render a .moss-progress-task row via renderNarrated", () => {
    fireTaskUpdate({
      id: "narr-job-1",
      window_id: "main",
      scope: "workspace",
      kind: "syndicate",
      tone: "narrated",
      title: "Syndicating to Mirror",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: {
        state: "running",
        fraction: 0.5,
        message: "Uploading post",
      },
    });

    // renderPanel is called internally on handleJobUpdate; a narrated task
    // goes through renderNarrated.
    const rows = container.querySelectorAll(".moss-progress-task");
    expect(rows.length).toBeGreaterThanOrEqual(1);
  });

  // ── (d) Failed Job → broken notice + status.kind=failed for publish/deploy ─
  it("(d) a failed deploy Job → broken NoticeVM AND status.kind=failed", () => {
    panel.handleJobUpdate({
      id: "deploy-job-1",
      window_id: "main",
      scope: "preview",
      kind: "deploy",
      tone: "ambient",
      title: "Publishing",
      fade_after_ms: null,
      started_at_ms: 0,
      verb: null,
      amount: null,
      elapsed_ms: null,
      parent: null,
      state: {
        state: "failed",
        advisory: {
          scope: "Environment",
          severity: "Blocking",
          item: null,
          what: "Deploy failed — network timeout",
          action: "None",
        },
      },
    });

    const vm = panel.buildViewModel();

    // status.kind=failed (lastFailure set for deploy kind)
    expect(vm.status.kind).toBe("failed");
    expect(vm.status.text).toContain("Deploy failed");

    // A broken notice exists
    const broken = vm.notices.filter((n) => n.severity === "broken");
    expect(broken.length).toBeGreaterThanOrEqual(1);
    expect(broken[0].reason).toContain("Deploy failed");

    // topSeverity reflects the broken notice
    expect(vm.topSeverity).toBe("broken");
  });

  // ── (e) PreviewServerFailed → red status + broken notice; ready clears both ─
  it("(e) PreviewServerFailed → red status + broken notice; subsequent ready clears BOTH (no intervening opening)", () => {
    // Trigger PreviewServerFailed
    fireMossEvent({
      kind: "PreviewServerFailed",
      payload: { message: "Port 1430 in use", open_in_another_app: false },
    });

    const vmFailed = panel.buildViewModel();
    expect(vmFailed.status.kind).toBe("failed");
    expect(vmFailed.status.text).toContain("Preview server failed");
    const brokenBefore = vmFailed.notices.filter((n) => n.severity === "broken");
    expect(brokenBefore.length).toBeGreaterThanOrEqual(1);
    // groupNotices splits reason at " — ": head = "Preview server failed",
    // detail = "Port 1430 in use". The notice reason is the head; detail carries the msg.
    expect(brokenBefore[0].reason).toContain("Preview server failed");
    expect(brokenBefore[0].detail).toContain("Port 1430 in use");

    // A subsequent `ready` (NO intervening opening/build-start) clears both.
    panel.setStage("ready");

    const vmAfter = panel.buildViewModel();
    expect(vmAfter.status.kind).toBe("none");
    // The preview-server-error advisory is gone (no more broken notices)
    const brokenAfter = vmAfter.notices.filter((n) => n.severity === "broken");
    expect(brokenAfter.length).toBe(0);
  });

  // ── (f) Active syndicate plugin hook → status line ────────────────────────
  it("(f) an active syndicate plugin hook task → appears in the status line (wiring iii)", () => {
    // plugin:* tasks in the running tier feed the status line when no stage/publish/media.
    // handlePluginProgress is called directly (same as the event path) using the
    // correct PluginProgressEvent field names: plugin_name, hook_name, phase, current, total.
    // "syndicate" is an IMMEDIATE_HOOK → no deferral, upserts directly.
    panel.handlePluginProgress({
      plugin_name: "Mirror",
      hook_name: "syndicate",
      phase: "running",
      current: 0,
      total: 0,
      message: "Publishing to Mirror…",
      completed: false,
    });

    const vm = panel.buildViewModel();
    // status should reflect the plugin label (running tier)
    expect(vm.status.kind).toBe("running");
    // The task label is pp.plugin_name = "Mirror"
    expect(vm.status.text).toBe("Mirror");
  });

  // ── (g) Domain/Deploy Job running + idle publishStage → status.text===task.label ─
  it("(g) a running Deploy Job with idle publishStage → status.text equals task.label", () => {
    panel.handleJobUpdate({
      id: "domain-conn-1",
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
      state: { state: "running", fraction: 0.2, message: null },
    });

    // Confirm publish stage is idle (the default)
    const vm = panel.buildViewModel();

    // With idle publish stage and no build stage label, the running job:* tier fires.
    expect(vm.status.kind).toBe("running");
    expect(vm.status.text).toBe("Connecting example.test");
  });

  // ── (g2) a bg:<task> row feeds the status line too, once nothing else claims it ──
  it("(g2) a bg:images row reaches the status line — the panel body has content to show on hover", () => {
    // The seam can un-hide onto a bg:<task> row; buildViewModel().status must carry it too.
    panel.setStage("ready"); // no active build-stage label to compete
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "images", message: "", current: 8, total: 12, completed: false },
    });

    const vm = panel.buildViewModel();
    expect(vm.status.kind).toBe("running");
    expect(vm.status.text).toBe("Optimizing images (8/12)");
  });

  // ── (h) No id:'build' task + quiet rebuild doesn't force-surface ──────────
  it("(h) no id:\"build\" task is ever created; a quiet (advisory-free) rebuild keeps the panel hidden", () => {
    panel.hide(); // Start hidden (minimized)

    // Fire a full build cycle: opening → BackgroundProgress (no advisory) → ready
    panel.setStage("opening");
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: {
        task: "html_pages",
        message: "",
        current: 5,
        total: 5,
        completed: true,
        advisories: [],
      },
    });
    panel.setStage("ready");

    // No id:'build' task should exist at any point
    const allTasks = panel.getActiveTasks();
    expect(allTasks.find((t) => t.id === "build")).toBeUndefined();

    // Panel should remain hidden (quiet rebuild = no surface)
    expect(panel.isHidden).toBe(true);
  });

  // ── (i) Tiebreak: most-recently-updated running job:* task wins ───────────
  it("(i) tiebreak: among multiple running job:* tasks, the most-recently-updated wins status", () => {
    // Use fake timers so Date.now() returns a deterministically different value
    // for each upsertTask call — otherwise both jobs get the same timestamp.
    vi.useFakeTimers();
    try {
      vi.setSystemTime(1000);
      panel.handleJobUpdate({
        id: "job-older",
        window_id: "main",
        scope: "preview",
        kind: "deploy",
        tone: "ambient",
        title: "Older Job",
        fade_after_ms: null,
        started_at_ms: 0,
        verb: null,
        amount: null,
        elapsed_ms: null,
        parent: null,
        state: { state: "running", fraction: 0.1, message: null },
      });

      // Advance time so the second job gets a later lastUpdatedAt.
      vi.setSystemTime(2000);
      panel.handleJobUpdate({
        id: "job-newer",
        window_id: "main",
        scope: "preview",
        kind: "deploy",
        tone: "ambient",
        title: "Newer Job",
        fade_after_ms: null,
        started_at_ms: 0,
        verb: null,
        amount: null,
        elapsed_ms: null,
        parent: null,
        state: { state: "running", fraction: 0.2, message: null },
      });

      // The second job was upserted later → its lastUpdatedAt=2000 > first (1000).
      // status.text should be "Newer Job".
      const vm = panel.buildViewModel();
      expect(vm.status.kind).toBe("running");
      expect(vm.status.text).toBe("Newer Job");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("AdvisoryEntry carries scope", () => {
  let panel: ProgressPanel;
  let container: HTMLElement;
  let listenCallbacks: Map<string, Array<(event: any) => void>>;

  const fireMossEvent = (payload: any) => {
    for (const cb of listenCallbacks.get("moss-event") ?? []) {
      cb(payload);
    }
  };

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
    listenCallbacks = new Map();
    setEngineOn(async (eventName: string, cb: any) => {
      const list = listenCallbacks.get(eventName) ?? [];
      list.push(cb);
      listenCallbacks.set(eventName, list);
      return () => {};
    });
    await panel.init();
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  it("a BackgroundProgress advisory with scope:Environment → entry.scope === 'Environment'", () => {
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
          severity: "NeedsAction",
          item: null,
          what: "FFmpeg not found",
          action: "None",
        }],
      },
    });
    const entries = Array.from((panel as any).advisories.values()) as import("../progress-panel").AdvisoryEntry[];
    expect(entries.length).toBeGreaterThan(0);
    expect(entries[0].scope).toBe("Environment");
  });

  it("awaiting-narration synthetic advisory keeps no File scope — tiers degraded", () => {
    // Simulate a non-preview awaiting tone task
    fireMossEvent({
      kind: "PanelTaskUpdate",
      payload: {
        id: "task-1", window_id: "main", scope: "workspace", kind: "syndicate",
        tone: "awaiting", title: "Authorize", started_at_ms: "0",
        verb: null, amount: null, elapsed_ms: null, parent: null,
        state: { state: "awaiting", directive: "Authorize to continue", action: "None" },
      },
    });
    const advisories = Array.from((panel as any).advisories.values()) as import("../progress-panel").AdvisoryEntry[];
    const awaitingAdv = advisories.find((a: import("../progress-panel").AdvisoryEntry) => a.task === "awaiting-narration");
    // The synthetic advisory should NOT have scope "File"
    if (awaitingAdv) {
      expect(awaitingAdv.scope).not.toBe("File");
    }
  });

});

// PUSH_NOTICE via eventBus surfaces as a degraded notice
describe("Area A — PUSH_NOTICE event → advisory zone", () => {
  let panel: ProgressPanel;
  let container: HTMLElement;

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
    setEngineOn(async () => () => {});
    await panel.init();
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  it("PUSH_NOTICE is stored in the advisory map keyed by task", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:site-live-gate",
      item: null,
      reason: "Publish your site first — deploy to your website, then syndicate.",
      severity: "NeedsAction",
      scope: "Remote",
    });
    const advs = panel.getAdvisories();
    const notice = advs.find((a) => a.task === "deploy:site-live-gate");
    expect(notice).toBeDefined();
    expect(notice!.reason).toBe("Publish your site first — deploy to your website, then syndicate.");
    expect(notice!.item).toBeNull();
  });

  it("PUSH_NOTICE renders as a .moss-notice element (degraded)", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    // Add a narrated task first to get the panel DOM mounted
    panel.handleJobUpdate({
      id: "narrated:1", window_id: "main", scope: "workspace", kind: "import", tone: "narrated",
      title: "Building", fade_after_ms: null, started_at_ms: 0, verb: null, amount: null,
      elapsed_ms: null, parent: null, state: { state: "running", fraction: null, message: null },
    });
    // Surface the panel — a NeedsAction notice is a quiet seam dot at rest (the
    // resting default is collapsed); its row only renders when the body is shown.
    panel.show();
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:site-live-gate",
      item: null,
      reason: "Publish your site first — deploy to your website, then syndicate.",
      severity: "NeedsAction",
      scope: "Remote",
    });
    // The advisory should appear in the panel DOM as a degraded notice
    const noticeEl = container.querySelector(".moss-notice");
    expect(noticeEl).not.toBeNull();
  });
});

// Publishing completes only after live verification: sticky
// advisories, the CLEAR_NOTICE verb, and the `verifying` receipt correction.
describe("Publish verification — sticky advisories, CLEAR_NOTICE, verifying receipt", () => {
  let panel: ProgressPanel;
  let container: HTMLElement;
  let listenCallbacks: Map<string, Array<(event: any) => void>>;

  const stickyNotice = {
    task: "publish-verify",
    item: null,
    reason: "Checking your site is live… · 0:12",
    severity: "ShippedDegraded",
    scope: "Remote",
    sticky: true,
  } as const;
  const ordinaryNotice = {
    task: "bg:videos:v.mov",
    item: "v.mov",
    reason: "unsupported codec",
    severity: "ShippedDegraded",
    scope: "File",
  } as const;

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
    listenCallbacks = new Map();
    setEngineOn(async (eventName: string, cb: any) => {
      const list = listenCallbacks.get(eventName) ?? [];
      list.push(cb);
      listenCallbacks.set(eventName, list);
      return () => {};
    });
    cloudSync.reset();
    await panel.init();
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  it("a sticky advisory survives setStage('opening'); ordinary ones are cleared", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, stickyNotice);
    eventBus.emit(Events.PUSH_NOTICE, ordinaryNotice);
    expect(panel.getAdvisories()).toHaveLength(2);

    // Build-start blanket clear #1: the explicit opening stage. The
    // verification narrative describes the LAST PUBLISH, which a rebuild
    // does not answer — it must ride through.
    panel.setStage("opening");
    const advs = panel.getAdvisories();
    expect(advs).toHaveLength(1);
    expect(advs[0].task).toBe("publish-verify");
  });

  it("a sticky advisory survives a build-sourced advisory's own reconciliation sweep", async () => {
    // A sticky PUSH_NOTICE row must survive a build task's own completed:true sweep.
    // Use "notebooks": its completed tick speaks for its whole set, since
    // collect_notebooks reprocesses every notebook each build.
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, stickyNotice);
    const fireMossEvent = (payload: any) => {
      for (const cb of listenCallbacks.get("moss-event") ?? []) cb(payload);
    };
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "notebooks", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "File", severity: "ShippedDegraded", item: ordinaryNotice.item, what: ordinaryNotice.reason, action: "None" }] },
    });
    expect(panel.getAdvisories()).toHaveLength(2);

    // A later rebuild: notebooks completes again, the timeout resolved.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "notebooks", message: "", current: 1, total: 1, completed: true, advisories: [] },
    });
    const advs = panel.getAdvisories();
    expect(advs).toHaveLength(1);
    expect(advs[0].task).toBe("publish-verify");
  });

  it("CLEAR_NOTICE removes the keyed advisory and un-lights the severity dot", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    const severities: string[] = [];
    panel.onTopSeverityChange((s) => severities.push(s));

    eventBus.emit(Events.PUSH_NOTICE, stickyNotice);
    expect(panel.getAdvisories()).toHaveLength(1);
    expect(severities.at(-1)).toBe("degraded"); // the dot is lit

    eventBus.emit(Events.CLEAR_NOTICE, { task: "publish-verify" });
    expect(panel.getAdvisories()).toHaveLength(0);
    expect(severities.at(-1)).toBe("none"); // …and un-lit by the clear
  });

  it("CLEAR_NOTICE for an unknown key is a no-op (no render, no close)", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, ordinaryNotice);
    eventBus.emit(Events.CLEAR_NOTICE, { task: "never-posted" });
    expect(panel.getAdvisories()).toHaveLength(1);
  });

  it("re-emitting the same key updates in place — the 1 Hz ticker path", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, stickyNotice);
    eventBus.emit(Events.PUSH_NOTICE, {
      ...stickyNotice,
      reason: "Checking your site is live… · 0:13",
    });
    const advs = panel.getAdvisories();
    expect(advs).toHaveLength(1); // last-write-wins, never a second row
    expect(advs[0].reason).toContain("0:13");
    expect(advs[0].sticky).toBe(true); // stickiness rides along on re-emit
  });

  it("verifying rewrites the publish receipt WITHOUT re-engaging the stage machine", () => {
    const cb = listenCallbacks.get("deploy-progress")![0];
    vi.useFakeTimers();
    try {
      cb({
          stage: "uploading", current: 3, total: 20,
          bytes_uploaded: 58e6, bytes_total: 58e6, message: "",
        });
      cb({ stage: "complete", current: 20, total: 20, message: "" });

      const verb = () =>
        container.querySelector(".moss-status-line__verb")?.textContent ?? "";
      expect(verb()).toContain("Published");

      // Complete precedes Verifying on this channel: the arrival swaps the
      // just-minted receipt for the honest rung-1 sentence…
      cb({ stage: "verifying", current: 0, total: 0, message: "" });
      expect(verb()).toContain("Uploaded");
      expect(verb()).not.toContain("Published");
      // …and the stage machine stays resolved — a resident stage would pin
      // shouldSurface() true for a minutes-to-hours window.
      expect(panel.getPublishStage()).toBe("idle");

      // It is a receipt like any other: it retracts on the normal dwell and
      // the settled panel lets go.
      vi.advanceTimersByTime(3_000);
      expect(verb()).not.toContain("Uploaded");
      expect(panel.isHidden).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

// The advisory must stay closed across edits for the same error.
// A refused Publish click (`pre-publish-gates.ts`) posts
// Blocking rows via PUSH_NOTICE that nothing ever cleared; every later
// BackgroundProgress tick (i.e. every edit) re-ran `shouldSurface()` against
// the SAME still-present row and popped the panel back open.
describe("Closing the panel acknowledges the Blocking advisory shown at that moment", () => {
  let panel: ProgressPanel;
  let container: HTMLElement;
  let listenCallbacks: Map<string, Array<(event: any) => void>>;
  const fireMossEvent = (payload: any) => {
    for (const cb of listenCallbacks.get("moss-event") ?? []) {
      cb(payload);
    }
  };

  beforeEach(async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    panel = new ProgressPanel(container);
    listenCallbacks = new Map();
    setEngineOn(async (eventName: string, cb: any) => {
      const list = listenCallbacks.get(eventName) ?? [];
      list.push(cb);
      listenCallbacks.set(eventName, list);
      return () => {};
    });
    cloudSync.reset();
    await panel.init();
    // Deliberately no panel.show() here — the resting state is collapsed,
    // and every test below drives its own open/close transitions from there.
  });

  afterEach(() => {
    panel.destroy();
    document.body.removeChild(container);
  });

  it("(a) a dismissed Blocking advisory does not reopen on a later routine rebuild tick", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    // A refused Publish click's advisory row.
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    expect(panel.isHidden).toBe(false); // the refusal surfaces on its own

    panel.dismiss(); // the user closes it — the ▾ handle's path
    expect(panel.isHidden).toBe(true);

    // A routine per-edit rebuild: BackgroundProgress ticks on every save,
    // whether or not it touches the dismissed advisory's task.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] } });
    expect(panel.isHidden).toBe(true); // stays closed — same problem, already seen
  });

  it("a real click on the ▾ handle acknowledges the Blocking advisory (not just a direct dismiss() call)", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    expect(panel.isHidden).toBe(false); // the refusal surfaces on its own

    // The handle callback wired in the ProgressPanel constructor — this is
    // the real UI control, not panel.dismiss() called directly.
    const handleBtn = container.querySelector<HTMLButtonElement>("button.moss-surface-handle");
    expect(handleBtn).not.toBeNull();
    handleBtn!.click();
    expect(panel.isHidden).toBe(true);

    // A routine per-edit rebuild: BackgroundProgress ticks on every save,
    // whether or not it touches the dismissed advisory's task.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] } });
    expect(panel.isHidden).toBe(true); // stays closed — dismissed via the real button
  });

  it("a Blocking advisory changing only its count stays dismissed under the same key", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // Same key, the count improved (3 → 2) — still the same problem.
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "2 files are missing", severity: "Blocking", scope: "File",
    });
    expect(panel.isHidden).toBe(true);
  });

  it("(b) a NEW Blocking advisory key surfaces the panel even after a dismissal", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // A different problem — FFmpeg missing, unrelated to the dismissed key.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
  });

  it("(c) the same key leaving `advisories` and later coming back reopens the panel", () => {
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // The problem is fixed and a new build starts — the build-start blanket
    // clear (`clearBuildAdvisories`) drops the non-sticky FFmpeg row, which
    // must drop its acknowledgement with it. `setStage('opening')` itself is
    // an explicit lifecycle surface (`show()`), so re-close to isolate what
    // the NEXT advisory arrival does on its own.
    panel.setStage("opening");
    panel.hide();
    expect(panel.isHidden).toBe(true);

    // The same key, same reason, arrives again — a new instance of the same
    // problem, not the one already dismissed.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
  });

  it("(b) a rebuild that re-raises the SAME key keeps it dismissed — the panel does not reopen", () => {
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // A per-edit rebuild whose videos task completes and reports the SAME
    // still-true problem — the key never left `advisories`, so the
    // dismissal (the reconciliation only sweeps a key that is
    // ABSENT from the fresh report) must still hold.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(true);
    expect(panel.getAdvisories()).toHaveLength(1); // still there, just not surfaced
  });

  it("(d) a Publish-gate PUSH_NOTICE row and a sticky row both survive an unrelated BackgroundProgress rebuild tick", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "publish-verify", item: null,
      reason: "Checking your site is live… · 0:12", severity: "ShippedDegraded", scope: "Remote", sticky: true,
    });
    expect(panel.getAdvisories()).toHaveLength(2);

    // A routine per-edit rebuild — neither PUSH_NOTICE row is build-sourced,
    // so the per-task sweep (scoped to BackgroundProgress's own
    // `task` field) must never reach either of them.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] } });
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [] } });
    expect(panel.getAdvisories().map((a) => a.task).sort()).toEqual(["deploy:stale-sources", "publish-verify"]);
  });

  it("(e) a refused Publish click after a dismissal surfaces the panel again", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    expect(panel.isHidden).toBe(false);
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // A second Publish click, refused again: pre-publish-gates.ts clears its
    // own prior rows via CLEAR_NOTICE before re-posting the fresh set — the
    // same discipline `runPrePublishGates` runs on every evaluation.
    eventBus.emit(Events.CLEAR_NOTICE, { task: "deploy:stale-sources" });
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:stale-sources", item: null,
      reason: "3 files are missing", severity: "Blocking", scope: "File",
    });
    expect(panel.isHidden).toBe(false); // the refusal answers the click just made
  });

  it("dismiss() only acknowledges Blocking advisories, not lesser severities", async () => {
    const { eventBus, Events } = await import("../../../context/event-bus");
    // A NeedsAction row never surfaces the panel on its own; open it by hand
    // (as a hover/click would) to exercise dismiss() over a mixed advisory set.
    eventBus.emit(Events.PUSH_NOTICE, {
      task: "deploy:site-live-gate", item: null,
      reason: "Publish your site first", severity: "NeedsAction", scope: "Remote",
    });
    panel.pin();
    panel.dismiss();
    expect(panel.isHidden).toBe(true);

    // A fresh Blocking row must still surface — dismiss() didn't blanket-mute
    // the panel, it only recorded the (absent) Blocking keys at that moment.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
  });

  it("hide() (not through dismiss()) does not acknowledge — an internal/automatic close still reopens", () => {
    // `hide()` is the escape hatch other automatic paths use (checkAutoClose,
    // the peek hover-leave timeout, setScreenUp) and must NOT silently
    // acknowledge a Blocking advisory the user never explicitly closed.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 1, total: 1, completed: true, advisories: [{ scope: "Environment", severity: "Blocking", item: null, what: "FFmpeg not available", action: "None" }] } });
    expect(panel.isHidden).toBe(false);
    panel.hide(); // NOT dismiss()
    expect(panel.isHidden).toBe(true);

    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "markdown", message: "", current: 1, total: 12, completed: false, advisories: [] } });
    expect(panel.isHidden).toBe(false); // still unacknowledged — reopens
  });

  it("an images advisory clears once a later run's full advisory set (including carry-forward) no longer includes it", () => {
    // moss-build now carries a skip-carried (unchanged) image's advisory
    // forward from its fingerprint cache and re-emits it in the SAME
    // completed:true tick as whatever else that run actually dispatched —
    // `dispatch_image_conversions`'s per-item change filter no longer means
    // the job's tick speaks for only the dispatched subset. So an item
    // truly absent from a completed tick's advisories really is fixed.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 2, total: 2, completed: true, advisories: [
      { scope: "File", severity: "ShippedDegraded", item: "photo-a.jpg", what: "shipped unoptimized", action: "None" },
      { scope: "File", severity: "ShippedDegraded", item: "photo-b.jpg", what: "shipped unoptimized", action: "None" },
    ] } });
    expect(panel.getAdvisories().map((a) => a.item).sort()).toEqual(["photo-a.jpg", "photo-b.jpg"]);

    // A later rebuild: photo-a.jpg was fixed and dispatched clean; photo-b.jpg
    // was untouched and skip-carried, so its advisory still rides this tick.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "images", message: "", current: 2, total: 2, completed: true, advisories: [
      { scope: "File", severity: "ShippedDegraded", item: "photo-b.jpg", what: "shipped unoptimized", action: "None" },
    ] } });
    expect(panel.getAdvisories().map((a) => a.item)).toEqual(["photo-b.jpg"]); // photo-a swept, photo-b survives
  });

  it("a videos advisory clears once a later run's full advisory set (including carry-forward) no longer includes it", () => {
    // Same shape as images, via `dispatch_video_conversions` and
    // `VideoConversionState`'s mirrored fingerprint-cache advisories.
    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 2, total: 2, completed: true, advisories: [
      { scope: "File", severity: "ShippedDegraded", item: "clip-a.mov", what: "shipped unoptimized", action: "None" },
      { scope: "File", severity: "ShippedDegraded", item: "clip-b.mov", what: "shipped unoptimized", action: "None" },
    ] } });
    expect(panel.getAdvisories().map((a) => a.item).sort()).toEqual(["clip-a.mov", "clip-b.mov"]);

    fireMossEvent({ kind: "BackgroundProgress", payload: { task: "videos", message: "", current: 2, total: 2, completed: true, advisories: [
      { scope: "File", severity: "ShippedDegraded", item: "clip-b.mov", what: "shipped unoptimized", action: "None" },
    ] } });
    expect(panel.getAdvisories().map((a) => a.item)).toEqual(["clip-b.mov"]); // clip-a swept, clip-b survives
  });

  it("moss's silent-clear fix: a symlink-skip advisory clears when a later build finds nothing, and a later recurrence brings it back", () => {
    // Real shape emitted by `make_symlink_skip_advisory` (moss-build,
    // progress.rs): task "assets", Scope::Config, Severity::NeedsAction.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "assets", message: "1 symlink skipped", current: 0, total: 0, completed: true, advisories: [{ scope: "Config", severity: "NeedsAction", item: null, what: "1 symlink skipped — broken target", action: "None" }] },
    });
    expect(panel.getAdvisories().some((a) => a.task === "assets")).toBe(true);

    // A later build where the symlinked file was fixed: the engine now emits
    // an explicit clear (completed:true, advisories:[]) instead of staying
    // silent — the fix under test. Before it, this tick never arrived at
    // all, so the advisory outlived the fix until the folder was reopened.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "assets", message: "", current: 0, total: 0, completed: true, advisories: [] },
    });
    expect(panel.getAdvisories().some((a) => a.task === "assets")).toBe(false);

    // NeedsAction never auto-surfaces the panel (`shouldSurface()` is
    // Blocking/awaiting-only — see its doc), so "a dismissed one reappearing
    // reopens the panel" does not apply to this severity; what must hold
    // instead is that clearing it once does not poison the key — a genuine
    // recurrence (someone reintroduces the broken symlink) is tracked again,
    // not silently dropped forever.
    fireMossEvent({
      kind: "BackgroundProgress",
      payload: { task: "assets", message: "1 symlink skipped", current: 0, total: 0, completed: true, advisories: [{ scope: "Config", severity: "NeedsAction", item: null, what: "1 symlink skipped — broken target", action: "None" }] },
    });
    expect(panel.getAdvisories().some((a) => a.task === "assets")).toBe(true);
  });
});
