import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { getPlatformInfo, clearPlatformCache } from "../platform.js";

describe("Platform Detection Utilities", () => {
  const originalWindow = globalThis.window;
  let mockInvoke: ReturnType<typeof vi.fn>;
  let mockWindow: Record<string, unknown>;

  beforeEach(() => {
    clearPlatformCache();
    mockInvoke = vi.fn();
    mockWindow = {
      __TAURI__: {
        core: { invoke: mockInvoke },
      },
      __MOSS_INTERNAL_CONTEXT__: {
        plugin_name: "test-plugin",
        project_path: "/path/to/project",
        moss_dir: "/path/to/project/.moss",
      },
    };
    (globalThis as unknown as { window: unknown }).window = mockWindow;
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
    clearPlatformCache();
    vi.clearAllMocks();
  });

  describe("getPlatformInfo", () => {
    describe("macOS detection", () => {
      it("detects darwin-arm64 (Apple Silicon)", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Darwin\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "arm64\n",
            stderr: "",
          });

        const platform = await getPlatformInfo();

        expect(platform.os).toBe("darwin");
        expect(platform.arch).toBe("arm64");
        expect(platform.platformKey).toBe("darwin-arm64");
      });

      it("detects darwin-x64 (Intel Mac)", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Darwin\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "x86_64\n",
            stderr: "",
          });

        const platform = await getPlatformInfo();

        expect(platform.os).toBe("darwin");
        expect(platform.arch).toBe("x64");
        expect(platform.platformKey).toBe("darwin-x64");
      });
    });

    describe("Linux detection", () => {
      it("detects linux-x64", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Linux\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "x86_64\n",
            stderr: "",
          });

        const platform = await getPlatformInfo();

        expect(platform.os).toBe("linux");
        expect(platform.arch).toBe("x64");
        expect(platform.platformKey).toBe("linux-x64");
      });

      it("detects aarch64 as arm64 (unsupported for Hugo but detected correctly)", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Linux\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "aarch64\n",
            stderr: "",
          });

        // linux-arm64 is not a supported platform (Hugo doesn't provide ARM Linux binaries)
        await expect(getPlatformInfo()).rejects.toThrow(/Unsupported platform: linux-arm64/);
      });
    });

    describe("Windows detection", () => {
      it("detects windows-x64", async () => {
        // uname fails on Windows
        mockInvoke
          .mockRejectedValueOnce(new Error("command not found"))
          // cmd /c ver succeeds
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Microsoft Windows [Version 10.0.22631.4460]\n",
            stderr: "",
          })
          // Check architecture
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "AMD64\n",
            stderr: "",
          });

        const platform = await getPlatformInfo();

        expect(platform.os).toBe("windows");
        expect(platform.arch).toBe("x64");
        expect(platform.platformKey).toBe("windows-x64");
      });

      it("detects windows-arm64 (unsupported for Hugo but detected correctly)", async () => {
        mockInvoke
          .mockRejectedValueOnce(new Error("command not found"))
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Microsoft Windows [Version 10.0.22631.4460]\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "ARM64\n",
            stderr: "",
          });

        // windows-arm64 is not a supported platform (Hugo doesn't provide ARM Windows binaries)
        await expect(getPlatformInfo()).rejects.toThrow(/Unsupported platform: windows-arm64/);
      });
    });

    describe("caching", () => {
      it("caches platform info after first call", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Darwin\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "arm64\n",
            stderr: "",
          });

        const first = await getPlatformInfo();
        const second = await getPlatformInfo();

        expect(first).toBe(second);
        // Should only call uname twice (once for OS, once for arch) in total
        expect(mockInvoke).toHaveBeenCalledTimes(2);
      });

      it("clearPlatformCache forces re-detection", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Darwin\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "arm64\n",
            stderr: "",
          });

        await getPlatformInfo();
        clearPlatformCache();

        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Linux\n",
            stderr: "",
          })
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "x86_64\n",
            stderr: "",
          });

        const platform = await getPlatformInfo();

        expect(platform.os).toBe("linux");
        expect(platform.arch).toBe("x64");
      });
    });

    describe("error handling", () => {
      it("throws when OS cannot be detected", async () => {
        mockInvoke
          .mockRejectedValueOnce(new Error("command not found"))
          .mockRejectedValueOnce(new Error("command not found"));

        await expect(getPlatformInfo()).rejects.toThrow(
          /Unable to detect operating system/
        );
      });

      it("defaults to x64 when arch detection fails", async () => {
        mockInvoke
          .mockResolvedValueOnce({
            success: true,
            exit_code: 0,
            stdout: "Darwin\n",
            stderr: "",
          })
          .mockRejectedValueOnce(new Error("uname failed"));

        const platform = await getPlatformInfo();

        expect(platform.arch).toBe("x64");
      });
    });

    describe("architecture mapping", () => {
      const testCases = [
        { input: "x86_64", expected: "x64" },
        { input: "amd64", expected: "x64" },
        { input: "arm64", expected: "arm64" },
        { input: "aarch64", expected: "arm64" },
        { input: "armv8", expected: "arm64" },
      ];

      for (const { input, expected } of testCases) {
        it(`maps ${input} to ${expected}`, async () => {
          mockInvoke
            .mockResolvedValueOnce({
              success: true,
              exit_code: 0,
              stdout: "Darwin\n",
              stderr: "",
            })
            .mockResolvedValueOnce({
              success: true,
              exit_code: 0,
              stdout: `${input}\n`,
              stderr: "",
            });

          clearPlatformCache();
          const platform = await getPlatformInfo();

          expect(platform.arch).toBe(expected);
        });
      }
    });
  });
});
