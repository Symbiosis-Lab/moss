import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { executeBinary } from "../binary.js";

describe("Binary Execution Utilities", () => {
  const originalWindow = globalThis.window;
  let mockInvoke: ReturnType<typeof vi.fn>;
  let mockListen: ReturnType<typeof vi.fn>;
  let mockUnlisten: ReturnType<typeof vi.fn>;
  let mockWindow: Record<string, unknown>;

  beforeEach(() => {
    mockInvoke = vi.fn();
    mockUnlisten = vi.fn();
    mockListen = vi.fn().mockResolvedValue(mockUnlisten);
    mockWindow = {
      __TAURI__: {
        core: { invoke: mockInvoke },
        event: { listen: mockListen, emit: vi.fn() },
      },
      __MOSS_INTERNAL_CONTEXT__: {
        plugin_name: "github",
        project_path: "/path/to/project",
        moss_dir: "/path/to/project/.moss",
      },
    };
    (globalThis as unknown as { window: unknown }).window = mockWindow;
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
    vi.clearAllMocks();
  });

  describe("executeBinary", () => {
    it("uses project root as working directory from context", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "output",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["status"],
      });

      expect(mockInvoke).toHaveBeenCalledWith("execute_binary", {
        binaryPath: "git",
        args: ["status"],
        workingDir: "/path/to/project",
        timeoutMs: 60000,
        env: undefined,
      });
    });

    it("uses custom timeout when provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "npm",
        args: ["install"],
        timeoutMs: 120000,
      });

      expect(mockInvoke).toHaveBeenCalledWith("execute_binary", {
        binaryPath: "npm",
        args: ["install"],
        workingDir: "/path/to/project",
        timeoutMs: 120000,
        env: undefined,
      });
    });

    it("passes environment variables when provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "node",
        args: ["script.js"],
        env: { NODE_ENV: "production", DEBUG: "true" },
      });

      expect(mockInvoke).toHaveBeenCalledWith("execute_binary", {
        binaryPath: "node",
        args: ["script.js"],
        workingDir: "/path/to/project",
        timeoutMs: 60000,
        env: { NODE_ENV: "production", DEBUG: "true" },
      });
    });

    it("returns ExecuteResult with mapped field names", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "Hello from stdout",
        stderr: "Warning from stderr",
      });

      const result = await executeBinary({
        binaryPath: "echo",
        args: ["Hello"],
      });

      expect(result.success).toBe(true);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toBe("Hello from stdout");
      expect(result.stderr).toBe("Warning from stderr");
    });

    it("handles failed command execution", async () => {
      mockInvoke.mockResolvedValue({
        success: false,
        exit_code: 1,
        stdout: "",
        stderr: "Command failed",
      });

      const result = await executeBinary({
        binaryPath: "false",
        args: [],
      });

      expect(result.success).toBe(false);
      expect(result.exitCode).toBe(1);
      expect(result.stderr).toBe("Command failed");
    });

    it("handles non-zero exit codes", async () => {
      mockInvoke.mockResolvedValue({
        success: false,
        exit_code: 127,
        stdout: "",
        stderr: "command not found",
      });

      const result = await executeBinary({
        binaryPath: "nonexistent",
        args: [],
      });

      expect(result.success).toBe(false);
      expect(result.exitCode).toBe(127);
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(
        executeBinary({
          binaryPath: "git",
          args: ["status"],
        })
      ).rejects.toThrow(/must be called from within a plugin hook/);
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Binary not found"));

      await expect(
        executeBinary({
          binaryPath: "missing",
          args: [],
        })
      ).rejects.toThrow("Binary not found");
    });

    it("handles git commands correctly", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "main",
        stderr: "",
      });

      const result = await executeBinary({
        binaryPath: "git",
        args: ["branch", "--show-current"],
      });

      expect(result.success).toBe(true);
      expect(result.stdout).toBe("main");
    });
  });

  describe("context isolation", () => {
    it("uses project path from context for different projects", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "other-plugin",
        project_path: "/different/project",
        moss_dir: "/different/project/.moss",
      };
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "ls",
        args: ["-la"],
      });

      expect(mockInvoke).toHaveBeenCalledWith("execute_binary", {
        binaryPath: "ls",
        args: ["-la"],
        workingDir: "/different/project",
        timeoutMs: 60000,
        env: undefined,
      });
    });
  });

  describe("onStderr streaming", () => {
    it("passes streamId to invoke when onStderr is provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "some output",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["push", "origin", "main"],
        onStderr: () => {},
      });

      // Should have been called with a streamId string
      const invokeArgs = mockInvoke.mock.calls[0][1];
      expect(invokeArgs.streamId).toBeDefined();
      expect(typeof invokeArgs.streamId).toBe("string");
      // streamId should be a UUID
      expect(invokeArgs.streamId).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
      );
    });

    it("does NOT pass streamId when onStderr is not provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["status"],
      });

      const invokeArgs = mockInvoke.mock.calls[0][1];
      expect(invokeArgs.streamId).toBeUndefined();
    });

    it("sets up event listener for binary-output when onStderr is provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["push"],
        onStderr: () => {},
      });

      expect(mockListen).toHaveBeenCalledWith(
        "binary-output",
        expect.any(Function)
      );
    });

    it("does NOT set up event listener when onStderr is not provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["status"],
      });

      expect(mockListen).not.toHaveBeenCalled();
    });

    it("calls onStderr for matching streamId events", async () => {
      const stderrLines: string[] = [];
      const onStderr = (line: string) => stderrLines.push(line);

      // Capture the listener callback when listen is called
      let capturedCallback: ((event: { payload: unknown }) => void) | null = null;
      mockListen.mockImplementation(
        async (
          _event: string,
          handler: (event: { payload: unknown }) => void
        ) => {
          capturedCallback = handler;
          return mockUnlisten;
        }
      );

      // Make invoke resolve asynchronously so we can fire events before it completes
      let resolveInvoke!: (value: unknown) => void;
      mockInvoke.mockReturnValue(
        new Promise((resolve) => {
          resolveInvoke = resolve;
        })
      );

      const resultPromise = executeBinary({
        binaryPath: "git",
        args: ["push"],
        onStderr,
      });

      // Wait for both listen AND invoke to be called
      await vi.waitFor(() => {
        expect(capturedCallback).not.toBeNull();
        expect(mockInvoke).toHaveBeenCalled();
      });

      // Get the streamId that was passed to invoke
      const invokeArgs = mockInvoke.mock.calls[0][1];
      const streamId = invokeArgs.streamId;

      // Simulate matching events
      capturedCallback!({ payload: { streamId, line: "Enumerating objects: 5" } });
      capturedCallback!({ payload: { streamId, line: "Writing objects: 100%" } });

      expect(stderrLines).toEqual([
        "Enumerating objects: 5",
        "Writing objects: 100%",
      ]);

      // Complete the invoke
      resolveInvoke({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "Enumerating objects: 5\nWriting objects: 100%",
      });

      await resultPromise;
    });

    it("ignores events with non-matching streamId", async () => {
      const stderrLines: string[] = [];
      const onStderr = (line: string) => stderrLines.push(line);

      let capturedCallback: ((event: { payload: unknown }) => void) | null = null;
      mockListen.mockImplementation(
        async (
          _event: string,
          handler: (event: { payload: unknown }) => void
        ) => {
          capturedCallback = handler;
          return mockUnlisten;
        }
      );

      let resolveInvoke!: (value: unknown) => void;
      mockInvoke.mockReturnValue(
        new Promise((resolve) => {
          resolveInvoke = resolve;
        })
      );

      const resultPromise = executeBinary({
        binaryPath: "git",
        args: ["push"],
        onStderr,
      });

      // Wait for both listen AND invoke to be called
      await vi.waitFor(() => {
        expect(capturedCallback).not.toBeNull();
        expect(mockInvoke).toHaveBeenCalled();
      });

      // Fire event with a different streamId
      capturedCallback!({
        payload: { streamId: "different-stream-id", line: "should be ignored" },
      });

      expect(stderrLines).toEqual([]);

      resolveInvoke({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await resultPromise;
    });

    it("cleans up event listener after command completes", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "",
        stderr: "",
      });

      await executeBinary({
        binaryPath: "git",
        args: ["push"],
        onStderr: () => {},
      });

      expect(mockUnlisten).toHaveBeenCalled();
    });

    it("cleans up event listener even when command fails", async () => {
      mockInvoke.mockRejectedValue(new Error("push failed"));

      await expect(
        executeBinary({
          binaryPath: "git",
          args: ["push"],
          onStderr: () => {},
        })
      ).rejects.toThrow("push failed");

      expect(mockUnlisten).toHaveBeenCalled();
    });

    it("returns correct result when onStderr is provided", async () => {
      mockInvoke.mockResolvedValue({
        success: true,
        exit_code: 0,
        stdout: "remote output",
        stderr: "progress lines",
      });

      const result = await executeBinary({
        binaryPath: "git",
        args: ["push"],
        onStderr: () => {},
      });

      expect(result.success).toBe(true);
      expect(result.exitCode).toBe(0);
      expect(result.stdout).toBe("remote output");
      expect(result.stderr).toBe("progress lines");
    });
  });
});
