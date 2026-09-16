import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  setMessageContext,
  sendMessage,
  reportProgress,
  reportError,
  startTask,
  type AdvisoryProposal,
} from "../messaging.js";

describe("Messaging Utilities", () => {
  const originalWindow = globalThis.window;
  let mockInvoke: ReturnType<typeof vi.fn>;
  let mockEmit: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockInvoke = vi.fn().mockResolvedValue(undefined);
    mockEmit = vi.fn().mockResolvedValue(undefined);
    (globalThis as unknown as { window: unknown }).window = {
      __TAURI__: {
        core: { invoke: mockInvoke },
        event: { emit: mockEmit, listen: vi.fn() },
      },
    };
    // Reset context
    setMessageContext("", "");
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
  });

  describe("sendMessage", () => {
    it("uses emit for log messages (fire-and-forget)", async () => {
      setMessageContext("test-plugin", "test-hook");
      await sendMessage({ type: "log", level: "log", message: "Hello" });

      // Log messages now use events, not commands
      expect(mockEmit).toHaveBeenCalledWith("plugin-message", {
        pluginName: "test-plugin",
        hookName: "test-hook",
        message: { type: "log", level: "log", message: "Hello" },
      });
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("uses invoke for complete messages (must be acknowledged)", async () => {
      setMessageContext("test-plugin", "test-hook");
      await sendMessage({ type: "complete", success: true });

      // Complete messages use commands for acknowledgment
      expect(mockInvoke).toHaveBeenCalledWith("plugin_message", {
        pluginName: "test-plugin",
        hookName: "test-hook",
        message: { type: "complete", success: true },
      });
      expect(mockEmit).not.toHaveBeenCalled();
    });

    it("silently fails when Tauri is unavailable", async () => {
      (globalThis as unknown as { window: unknown }).window = {};
      await expect(
        sendMessage({ type: "log", level: "log", message: "Hello" })
      ).resolves.toBeUndefined();
    });

    it("silently catches invoke errors for complete messages", async () => {
      mockInvoke.mockRejectedValue(new Error("Invoke failed"));
      setMessageContext("plugin", "hook");
      // Should not throw
      await expect(
        sendMessage({ type: "complete", success: false })
      ).resolves.toBeUndefined();
    });
  });

  describe("reportProgress", () => {
    it("sends progress message via event (fire-and-forget)", async () => {
      setMessageContext("plugin", "hook");
      await reportProgress("building", 50, 100, "Half done");

      // Progress messages use events to avoid IPC congestion
      expect(mockEmit).toHaveBeenCalledWith("plugin-message", {
        pluginName: "plugin",
        hookName: "hook",
        message: {
          type: "progress",
          phase: "building",
          current: 50,
          total: 100,
          message: "Half done",
        },
      });
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("sends progress message without optional message", async () => {
      setMessageContext("plugin", "hook");
      await reportProgress("scanning", 1, 10);

      expect(mockEmit).toHaveBeenCalledWith("plugin-message", {
        pluginName: "plugin",
        hookName: "hook",
        message: {
          type: "progress",
          phase: "scanning",
          current: 1,
          total: 10,
          message: undefined,
        },
      });
    });
  });

  describe("reportError", () => {
    it("sends error message via command (must be acknowledged)", async () => {
      setMessageContext("plugin", "hook");
      await reportError("Something failed", "deployment");

      // Error messages use commands for acknowledgment
      expect(mockInvoke).toHaveBeenCalledWith("plugin_message", {
        pluginName: "plugin",
        hookName: "hook",
        message: {
          type: "error",
          error: "Something failed",
          context: "deployment",
          fatal: false,
        },
      });
    });

    it("sends error message with fatal=true when specified", async () => {
      setMessageContext("plugin", "hook");
      await reportError("Fatal error", "critical", true);

      expect(mockInvoke).toHaveBeenCalledWith("plugin_message", {
        pluginName: "plugin",
        hookName: "hook",
        message: {
          type: "error",
          error: "Fatal error",
          context: "critical",
          fatal: true,
        },
      });
    });

    it("sends error message without context", async () => {
      setMessageContext("plugin", "hook");
      await reportError("Error without context");

      expect(mockInvoke).toHaveBeenCalledWith("plugin_message", {
        pluginName: "plugin",
        hookName: "hook",
        message: {
          type: "error",
          error: "Error without context",
          context: undefined,
          fatal: false,
        },
      });
    });
  });

  // ──────────────────────────────────────────────────────────────────────
  // startTask (ADR-015 Phase 2 — T8a)
  //
  // The lifecycle API is invoke-based end-to-end (every transition needs
  // an acknowledgment so the Rust-side store keeps in sync), so all
  // assertions inspect mockInvoke. Mock returns sequential task ids so
  // we can verify the started→subsequent-call id threading.
  // ──────────────────────────────────────────────────────────────────────

  describe("startTask", () => {
    beforeEach(() => {
      // Each invoke call resolves to id="42" (string — Rust u64 → specta
      // serializes as string to preserve precision); the handle captures
      // this id and threads it through subsequent transitions.
      mockInvoke.mockResolvedValue("42");
    });

    it("invokes report_plugin_task_lifecycle_command with started lifecycle", async () => {
      setMessageContext("matters", "process");
      const task = await startTask("Importing 42 articles", {
        hook: "import",
        trigger: "onboarding_flow",
        hasProgress: true,
        cancellable: true,
      });

      expect(task.id).toBe("42");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        {
          pluginName: "matters",
          hook: "import",
          trigger: "onboarding_flow",
          taskId: undefined,
          lifecycle: {
            type: "started",
            label: "Importing 42 articles",
            has_progress: true,
            cancellable: true,
          },
        }
      );
    });

    it("threads task id into subsequent progress / succeeded calls", async () => {
      setMessageContext("matters", "process");
      const task = await startTask("Importing", { hook: "import" });
      mockInvoke.mockClear();

      await task.progress(0.42, "halfway");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          taskId: "42",
          lifecycle: {
            type: "progress",
            fraction: 0.42,
            message: "halfway",
          },
        })
      );

      mockInvoke.mockClear();
      await task.succeeded("Imported 42 articles");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          taskId: "42",
          lifecycle: {
            type: "succeeded",
            receipt: "Imported 42 articles",
          },
        })
      );
    });

    it("awaiting() splices venue into directive for the Awaiting renderer", async () => {
      setMessageContext("matters", "process");
      const task = await startTask("Deploying", {
        hook: "deploy",
        trigger: "onboarding_flow",
      });
      mockInvoke.mockClear();

      await task.awaiting("verify DNS", "your registrar", "recheck:Recheck DNS");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          taskId: "42",
          lifecycle: {
            type: "awaiting",
            directive: "verify DNS in your registrar",
            escape: "recheck:Recheck DNS",
          },
        })
      );
    });

    it("awaiting() defaults escape to cancel", async () => {
      setMessageContext("matters", "process");
      const task = await startTask("Importing");
      mockInvoke.mockClear();
      await task.awaiting("sign in", "Matters");
      const lifecycle = (mockInvoke.mock.calls[0][1] as { lifecycle: { escape: string } })
        .lifecycle;
      expect(lifecycle.escape).toBe("cancel");
    });

    it("failed() defaults recoverable to false (toast surface)", async () => {
      setMessageContext("matters", "process");
      const task = await startTask("Importing");
      mockInvoke.mockClear();
      await task.failed("offline");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          lifecycle: { type: "failed", error: "offline", recoverable: false },
        })
      );
    });

    it("hook defaults to 'import' and trigger defaults to 'background'", async () => {
      setMessageContext("matters", "process");
      await startTask("Default routing");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          hook: "import",
          trigger: "background",
        })
      );
    });

    it("captures plugin name at startTask() time, not transition time", async () => {
      // Plugin author starts an import; later, setMessageContext rolls
      // to a different hook (e.g., enhance) before the import resolves.
      // The task's transitions should keep routing under the original
      // plugin context — captured at start.
      setMessageContext("matters", "process");
      const task = await startTask("Importing");
      setMessageContext("other-plugin", "syndicate"); // hook rolls
      mockInvoke.mockClear();
      await task.progress(0.5);
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          pluginName: "matters",
        })
      );
    });

    it("returns id='-1' when Tauri is unavailable (out-of-Tauri test paths)", async () => {
      (globalThis as unknown as { window: unknown }).window = {};
      const task = await startTask("Importing");
      expect(task.id).toBe("-1");
      // Subsequent calls do not throw even when Tauri is gone.
      await expect(task.progress(0.5)).resolves.toBeUndefined();
      await expect(task.succeeded("ok")).resolves.toBeUndefined();
    });

    it("out-of-Tauri context: TaskHandle methods are no-op (do not emit invokes)", async () => {
      // Architecture-review fix: when Tauri isn't available, the
      // sentinel id ("-1") returned by startTask() must NOT be
      // threaded into subsequent invokes. The handle methods
      // short-circuit before touching `invoke`, so a plugin
      // exercised in a unit-test or browser-preview environment
      // sees no spurious IPC attempts (which would otherwise be
      // rejected by the Rust router as "unknown task id").
      (globalThis as unknown as { window: unknown }).window = {};
      mockInvoke.mockClear();
      const task = await startTask("Importing", { hook: "import" });
      expect(task.id).toBe("-1");
      // `startTask` itself short-circuits via isTauriAvailable()
      // before reaching invoke, so no started call either.
      expect(mockInvoke).not.toHaveBeenCalled();

      // Every TaskHandle method completes without throwing AND
      // without emitting an invoke.
      await expect(task.progress(0.25, "quarter")).resolves.toBeUndefined();
      await expect(
        task.awaiting("verify DNS", "your registrar", "recheck:Recheck DNS")
      ).resolves.toBeUndefined();
      await expect(task.succeeded("done")).resolves.toBeUndefined();
      await expect(task.failed("offline", true)).resolves.toBeUndefined();
      await expect(task.cancelled()).resolves.toBeUndefined();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("end-to-end fake plugin: startTask().progress().succeeded()", async () => {
      // Approach A test from the dispatch plan: write a tiny fake plugin
      // that drives the API surface and inspect every emitted invoke.
      setMessageContext("fake-plugin", "import");
      const calls: Array<{ type: string; lifecycle: unknown }> = [];
      const originalInvoke = mockInvoke.getMockImplementation();
      mockInvoke.mockImplementation(async (cmd: string, args: { lifecycle: { type: string } }) => {
        calls.push({ type: cmd, lifecycle: args.lifecycle });
        if (args.lifecycle.type === "started") return "100";
        return "0";
      });

      const task = await startTask("Fake import", {
        hook: "import",
        trigger: "onboarding_flow",
      });
      await task.progress(0.0, "starting");
      await task.progress(0.5, "halfway");
      await task.progress(1.0, "done");
      await task.succeeded("Imported 0 articles");

      // 1 started + 3 progress + 1 succeeded = 5 invoke calls.
      expect(calls).toHaveLength(5);
      const types = calls.map((c) => (c.lifecycle as { type: string }).type);
      expect(types).toEqual([
        "started",
        "progress",
        "progress",
        "progress",
        "succeeded",
      ]);
      // The task id is consistent across transitions.
      expect(task.id).toBe("100");
      // Restore impl in case other tests run after.
      if (originalInvoke) mockInvoke.mockImplementation(originalInvoke);
    });
  });

  // ──────────────────────────────────────────────────────────────────────
  // advise() — the plugin advisory path (Step 3 Phase 5 Task 5.3)
  //
  // A plugin PROPOSES advisories on its task; moss holds the severity gavel
  // (R13) Rust-side. `advise()` accumulates proposals on the handle; the next
  // terminal call (`succeeded`/`failed`) flushes them into the lifecycle IPC
  // as `advisories: PluginAdvisory[]`. A run with no proposals keeps the
  // legacy bare `{ type: "succeeded", receipt }` wire shape (serde defaults
  // `advisories` to `[]`), so existing call sites are untouched.
  // ──────────────────────────────────────────────────────────────────────
  describe("advise()", () => {
    beforeEach(() => {
      mockInvoke.mockResolvedValue("7");
    });

    const authNeeded: AdvisoryProposal = {
      scope: "Account",
      severity: "NeedsAction",
      item: null,
      what: "Sign in to Matters to finish syndicating",
      action: {
        InApp: { op: "SignIn", args: null, label: "Sign in" },
      },
    };

    it("accumulated advisories flush into the succeeded lifecycle", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicating", { hook: "syndicate" });
      mockInvoke.mockClear();

      await task.advise(authNeeded);
      // advise() itself does NOT emit — it accumulates on the handle.
      expect(mockInvoke).not.toHaveBeenCalled();

      await task.succeeded("Syndicated · 3 posts");
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          taskId: "7",
          lifecycle: {
            type: "succeeded",
            receipt: "Syndicated · 3 posts",
            advisories: [authNeeded],
          },
        })
      );
    });

    it("accumulated advisories flush into the failed lifecycle", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicating", { hook: "syndicate" });
      mockInvoke.mockClear();

      await task.advise(authNeeded);
      await task.failed("auth expired", false);
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          lifecycle: {
            type: "failed",
            error: "auth expired",
            recoverable: false,
            advisories: [authNeeded],
          },
        })
      );
    });

    it("no advise() ⇒ legacy bare succeeded payload (wire-compatible)", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicating", { hook: "syndicate" });
      mockInvoke.mockClear();

      await task.succeeded("Syndicated · 3 posts");
      // No `advisories` key — serde defaults it Rust-side. This keeps the
      // pre-Phase-5 wire shape byte-identical for plugins that never advise.
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          lifecycle: { type: "succeeded", receipt: "Syndicated · 3 posts" },
        })
      );
    });

    it("multiple advise() calls accumulate in order", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicating", { hook: "syndicate" });
      mockInvoke.mockClear();

      const degraded: AdvisoryProposal = {
        scope: "Remote",
        severity: "ShippedDegraded",
        item: "post-2",
        what: "published without a cover image",
        action: "None",
      };
      await task.advise(authNeeded);
      await task.advise(degraded);
      await task.succeeded("Syndicated · 2 posts");
      const lifecycle = (mockInvoke.mock.calls[0][1] as {
        lifecycle: { advisories: AdvisoryProposal[] };
      }).lifecycle;
      expect(lifecycle.advisories).toEqual([authNeeded, degraded]);
    });

    it("out-of-Tauri context: advise() is a no-op and never throws", async () => {
      (globalThis as unknown as { window: unknown }).window = {};
      const task = await startTask("Syndicating", { hook: "syndicate" });
      expect(task.id).toBe("-1");
      await expect(task.advise(authNeeded)).resolves.toBeUndefined();
      await expect(task.succeeded("done")).resolves.toBeUndefined();
    });

    // ── descriptor-driven verb/amount (Step 3 Phase 5, §8 + R13) ──────────

    it("job ref flows into the started lifecycle for descriptor lookup", async () => {
      setMessageContext("matters", "syndicate");
      await startTask("Syndicate", { hook: "syndicate", job: "syndicate" });
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          lifecycle: expect.objectContaining({ type: "started", job: "syndicate" }),
        })
      );
    });

    it("no job ref ⇒ legacy started payload (no job key, wire-compatible)", async () => {
      setMessageContext("matters", "syndicate");
      await startTask("Syndicate", { hook: "syndicate" });
      const lifecycle = (mockInvoke.mock.calls[0][1] as { lifecycle: Record<string, unknown> })
        .lifecycle;
      expect(lifecycle).not.toHaveProperty("job");
    });

    it("succeeded(receipt, count) sends the amount for descriptor jobs", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicate", { hook: "syndicate", job: "syndicate" });
      mockInvoke.mockClear();

      // moss owns the receipt — the plugin reports the COUNT, not a string.
      await task.succeeded(undefined, 3);
      expect(mockInvoke).toHaveBeenCalledWith(
        "report_plugin_task_lifecycle_command",
        expect.objectContaining({
          lifecycle: { type: "succeeded", receipt: undefined, amount: 3 },
        })
      );
    });

    it("succeeded() without a count omits amount (serde defaults None)", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicate", { hook: "syndicate" });
      mockInvoke.mockClear();
      await task.succeeded("legacy receipt");
      const lifecycle = (mockInvoke.mock.calls[0][1] as { lifecycle: Record<string, unknown> })
        .lifecycle;
      expect(lifecycle).not.toHaveProperty("amount");
    });

    it("advise() after a terminal call is a no-op (handle is spent)", async () => {
      setMessageContext("matters", "syndicate");
      const task = await startTask("Syndicate", { hook: "syndicate" });
      await task.succeeded(undefined, 1);
      mockInvoke.mockClear();

      // A late advise() must NOT accumulate, and must NOT ride any further IPC.
      await task.advise(authNeeded);
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });
});
