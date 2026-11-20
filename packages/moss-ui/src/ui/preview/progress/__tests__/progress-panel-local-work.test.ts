import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PanelTaskWire, TaskState } from "@symbiosis-lab/moss-ui";
import { ProgressPanel } from "../progress-panel";
import { eventBus, Events, type PushNoticeEvent } from "../../../context/event-bus";
import { host } from "../../../host";

const running: TaskState = { state: "running", fraction: null, message: "请登录" };
const awaiting: TaskState = { state: "awaiting", directive: "请登录", action: "None" };
const succeeded: TaskState = { state: "succeeded", receipt: "Signed in", advisories: [] };
const failed: TaskState = { state: "failed", advisory: {
  scope: "Account", severity: "Blocking", item: null, what: "Sign-in failed", action: "None",
} };
const cancelled: TaskState = { state: "cancelled" };
const lifecycle = [running, awaiting, running, succeeded, failed, cancelled];
function wire(overrides: Partial<PanelTaskWire> = {}): PanelTaskWire {
  return {
    id: "local", window_id: "main", scope: "action_panel", kind: "import", tone: "awaiting",
    title: "Sign in", fade_after_ms: null, started_at_ms: 0, verb: null,
    amount: null, elapsed_ms: null, parent: null, state: awaiting, ...overrides,
  };
}

describe("ProgressPanel local work boundary", () => {
  let container: HTMLElement;
  let panel: ProgressPanel;
  beforeEach(async () => {
    vi.useFakeTimers();
    container = document.createElement("div");
    document.body.append(container);
    panel = new ProgressPanel(container);
    vi.spyOn(host.engine, "on").mockResolvedValue(() => {});
    await panel.init();
  });
  afterEach(() => {
    panel.destroy();
    container.remove();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  function expectQuiet() {
    expect(panel.getActiveTasks()).toEqual([]);
    expect(panel.getNarratedTasks()).toEqual([]);
    expect(panel.getAdvisories()).toEqual([]);
    const vm = panel.buildViewModel();
    expect(vm.status.kind).toBe("none");
    expect(vm.topSeverity).toBe("none");
    expect(vm.notices).toEqual([]);
    expect(panel.isHidden).toBe(true);
  }

  it.each([
    { scope: "action_panel", kind: "import" },
    { scope: "action_panel", kind: "deploy" },
    { scope: "workspace", kind: "process" },
    { scope: "workspace", kind: "import" },
    { scope: "workspace", kind: "syndicate" },
    { scope: "preview", kind: "syndicate" },
  ] as const)("job filter keeps $scope/$kind lifecycle out of status and dot", (identity) => {
    for (const state of lifecycle) {
      for (const tone of ["awaiting", "narrated", "ambient"] as const) {
        panel.handleJobUpdate(wire({ ...identity, state, tone }));
        vi.advanceTimersByTime(300);
        expectQuiet();
      }
    }
  });

  it.each(["awaiting", "narrated", "preview"])("job filter removes stale %s state without disturbing a concurrent build", (route) => {
    panel.setStage("compiling");
    const build = panel.buildViewModel().status;
    panel.handleJobUpdate(wire({ scope: route === "preview" ? "preview" : "workspace",
      kind: "deploy", tone: route === "awaiting" ? "awaiting" : "narrated",
      state: route === "narrated" ? running : awaiting }));
    expect(panel.getActiveTasks().length + panel.getNarratedTasks().length).toBeGreaterThan(0);
    panel.handleJobUpdate(wire());
    expect(panel.getActiveTasks()).toEqual([]);
    expect(panel.getNarratedTasks()).toEqual([]);
    expect(panel.getAdvisories()).toEqual([]);
    expect(panel.buildViewModel().topSeverity).toBe("none");
    expect(panel.buildViewModel().status).toEqual(build);
    panel.setStage("ready");
    expect(panel.isHidden).toBe(true);
  });

  it.each(["login", "syndicate", "process"])("plugin filter keeps %s progress and completion quiet", (hook_name) => {
    const progress = { plugin_name: "Mirror", hook_name, phase: "running", current: 1,
      total: 2, message: "请登录", completed: false };
    panel.handlePluginProgress(progress);
    vi.advanceTimersByTime(1_000);
    expectQuiet();
    panel.handlePluginProgress({ ...progress, completed: true });
    expectQuiet();
  });

  it("plugin filter removes stale login rows and cancels pending paint while deploy continues", () => {
    const id = "plugin:Mirror:login";
    const stale = { id, label: "Sign in", progress: null, source: "plugin" as const };
    panel.upsertTask(stale);
    // Simulate a first paint queued before the operation was excluded.
    const timer = setTimeout(() => panel.upsertTask(stale), 250);
    (panel as any).deferredUpserts.set(id, { timer, task: stale });
    const progress = { plugin_name: "Site", hook_name: "deploy", phase: "running", current: 1,
      total: 2, message: "Uploading site", completed: false };
    panel.handlePluginProgress(progress);
    panel.handlePluginProgress({ ...progress, plugin_name: "Mirror", hook_name: "login" });
    vi.advanceTimersByTime(1_000);
    expect(panel.getActiveTasks().map(t => t.id)).toEqual(["plugin:Site:deploy"]);
    expect(panel.buildViewModel().status.text).toBe("Site");
    panel.handlePluginProgress({ ...progress, completed: true });
    expectQuiet();
  });

  const notice: PushNoticeEvent = { task: "panel:signin", item: null, reason: "请登录",
    severity: "NeedsAction", scope: "Account", taskScope: "action_panel" };
  it.each([
    notice,
    { ...notice, task: "plugin:Mirror:login", taskScope: undefined },
    { ...notice, task: "plugin:Mirror:syndicate", taskScope: undefined },
    { ...notice, task: "deploy:email-draft", taskScope: undefined },
    { ...notice, task: "deploy:email-no-subscribers", taskScope: undefined },
    { ...notice, task: "deploy:site-live-gate", taskScope: undefined },
  ])("notice filter keeps $task out of status and dot", (entry) => {
    for (const severity of ["Blocking", "NeedsAction", "ShippedDegraded"] as const) {
      eventBus.emit(Events.PUSH_NOTICE, { ...entry, severity, sticky: true });
      expectQuiet();
    }
  });

  it("notice filter clears a stale replacement while preserving site failure", () => {
    eventBus.emit(Events.PUSH_NOTICE, { ...notice, taskScope: undefined });
    expect(panel.buildViewModel().topSeverity).toBe("degraded");
    eventBus.emit(Events.PUSH_NOTICE, { task: "deploy:failed", item: null,
      reason: "Upload failed", severity: "Blocking", scope: "Remote" });
    eventBus.emit(Events.PUSH_NOTICE, notice);
    expect(panel.getAdvisories().map(n => n.task)).toEqual(["deploy:failed"]);
    expect(panel.buildViewModel().topSeverity).toBe("broken");
  });

  it.each(["preview", "workspace"] as const)("keeps site email verification and DNS waits in %s", (scope) => {
    for (const op of ["SignIn", "RecheckDns"] as const) {
      panel.handleJobUpdate(wire({ scope, kind: "deploy", state: { state: "awaiting",
        directive: "Verify site", action: { InApp: { op, args: { reason: "verify_email" }, label: "Continue" } } } }));
      expect(panel.getActiveTasks()).toHaveLength(1);
      expect(panel.buildViewModel().topSeverity).toBe("degraded");
      panel.handleJobUpdate(wire({ scope, kind: "deploy", state: cancelled }));
    }
  });

  it("keeps site verification notices even with Account scope and a SignIn action", () => {
    eventBus.emit(Events.PUSH_NOTICE, { ...notice, task: "publish-verify", taskScope: "preview",
      action: { InApp: { op: "SignIn", args: null, label: "Open Settings" } } });
    expect(panel.getAdvisories()).toHaveLength(1);
    expect(panel.buildViewModel().topSeverity).toBe("degraded");
  });

  it("coarse channel-only publish events create no deploying status or published receipt", () => {
    for (const status of ["pending", "in_progress", "success", "error"] as const) {
      eventBus.emit(Events.DEPLOY_PROGRESS, { isDeploying: status === "pending" || status === "in_progress", status });
      expectQuiet();
    }
  });
});
