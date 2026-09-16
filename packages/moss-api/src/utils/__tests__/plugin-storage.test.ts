import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  readPluginFile,
  writePluginFile,
  pluginFileExists,
} from "../plugin-storage.js";

describe("Plugin Storage Utilities", () => {
  const originalWindow = globalThis.window;
  let mockInvoke: ReturnType<typeof vi.fn>;
  let mockWindow: Record<string, unknown>;

  beforeEach(() => {
    mockInvoke = vi.fn();
    mockWindow = {
      __TAURI__: {
        core: { invoke: mockInvoke },
      },
      __MOSS_INTERNAL_CONTEXT__: {
        plugin_name: "matters",
        project_path: "/my/project",
        moss_dir: "/my/project/.moss",
      },
    };
    (globalThis as unknown as { window: unknown }).window = mockWindow;
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
    vi.clearAllMocks();
  });

  describe("readPluginFile", () => {
    it("reads from plugin directory with auto-detected identity", async () => {
      mockInvoke.mockResolvedValue('{"userName":"alice"}');

      const content = await readPluginFile("config.json");

      expect(mockInvoke).toHaveBeenCalledWith("read_plugin_file", {
        pluginName: "matters",
        projectPath: "/my/project",
        relativePath: "config.json",
      });
      expect(content).toBe('{"userName":"alice"}');
    });

    it("handles nested paths", async () => {
      mockInvoke.mockResolvedValue('{"cached":"data"}');

      await readPluginFile("cache/articles.json");

      expect(mockInvoke).toHaveBeenCalledWith("read_plugin_file", {
        pluginName: "matters",
        projectPath: "/my/project",
        relativePath: "cache/articles.json",
      });
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(readPluginFile("config.json")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("File not found"));

      await expect(readPluginFile("missing.json")).rejects.toThrow(
        "File not found"
      );
    });
  });

  describe("writePluginFile", () => {
    it("writes to plugin directory with auto-detected identity", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await writePluginFile("config.json", '{"userName":"bob"}');

      expect(mockInvoke).toHaveBeenCalledWith("write_plugin_file", {
        pluginName: "matters",
        projectPath: "/my/project",
        relativePath: "config.json",
        content: '{"userName":"bob"}',
      });
    });

    it("handles nested paths (creates directories)", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await writePluginFile("cache/articles.json", '[]');

      expect(mockInvoke).toHaveBeenCalledWith("write_plugin_file", {
        pluginName: "matters",
        projectPath: "/my/project",
        relativePath: "cache/articles.json",
        content: "[]",
      });
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(writePluginFile("config.json", "{}")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Permission denied"));

      await expect(writePluginFile("file.json", "{}")).rejects.toThrow(
        "Permission denied"
      );
    });
  });

  describe("pluginFileExists", () => {
    it("returns true when file exists", async () => {
      mockInvoke.mockResolvedValue(true);

      const exists = await pluginFileExists("config.json");

      expect(mockInvoke).toHaveBeenCalledWith("plugin_file_exists", {
        pluginName: "matters",
        projectPath: "/my/project",
        relativePath: "config.json",
      });
      expect(exists).toBe(true);
    });

    it("returns false when file does not exist", async () => {
      mockInvoke.mockResolvedValue(false);

      const exists = await pluginFileExists("missing.json");

      expect(exists).toBe(false);
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(pluginFileExists("config.json")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Unexpected error"));

      await expect(pluginFileExists("file.json")).rejects.toThrow(
        "Unexpected error"
      );
    });
  });

  describe("plugin isolation", () => {
    it("uses correct plugin name for github plugin", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "github",
        project_path: "/other/project",
        moss_dir: "/other/project/.moss",
      };
      mockInvoke.mockResolvedValue("{}");

      await readPluginFile("config.json");

      expect(mockInvoke).toHaveBeenCalledWith("read_plugin_file", {
        pluginName: "github",
        projectPath: "/other/project",
        relativePath: "config.json",
      });
    });
  });
});
