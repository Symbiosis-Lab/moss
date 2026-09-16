import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { getPluginCookie, setPluginCookie, clearPluginCookies } from "../cookies.js";

describe("Cookie Utilities", () => {
  const originalWindow = globalThis.window;
  let mockInvoke: ReturnType<typeof vi.fn>;
  let mockWindow: Record<string, unknown>;

  beforeEach(() => {
    mockInvoke = vi.fn();
    mockWindow = {
      __TAURI__: {
        core: { invoke: mockInvoke },
      },
    };
    (globalThis as unknown as { window: unknown }).window = mockWindow;
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
    vi.clearAllMocks();
  });

  describe("getPluginCookie", () => {
    it("reads plugin identity from runtime context", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "matters",
        project_path: "/test/project",
        moss_dir: "/test/project/.moss",
      };
      mockInvoke.mockResolvedValue([]);

      await getPluginCookie();

      expect(mockInvoke).toHaveBeenCalledWith("get_plugin_cookie", {
        pluginName: "matters",
        projectPath: "/test/project",
      });
    });

    it("returns null when called outside hook context", async () => {
      // No context set - should return null instead of throwing
      const result = await getPluginCookie();
      expect(result).toBeNull();
    });

    it("returns null (not empty array) to distinguish from 'no cookies found'", async () => {
      // No context set
      const result = await getPluginCookie();
      // null means "can't check cookies" (no context)
      // [] means "checked but found nothing" (has context)
      expect(result).toBeNull();
      expect(result).not.toEqual([]);
    });

    it("returns array of cookies from backend", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "matters",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      const cookies = [
        { name: "session", value: "abc123" },
        { name: "token", value: "xyz789", domain: "matters.town" },
      ];
      mockInvoke.mockResolvedValue(cookies);

      const result = await getPluginCookie();

      expect(result).toEqual(cookies);
    });

    it("returns empty array when no cookies", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "new-plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockResolvedValue([]);

      const result = await getPluginCookie();

      expect(result).toEqual([]);
    });

    it("propagates errors from Tauri", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockRejectedValue(new Error("Failed to read cookies"));

      await expect(getPluginCookie()).rejects.toThrow("Failed to read cookies");
    });
  });

  describe("setPluginCookie", () => {
    it("reads plugin identity from runtime context", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "matters",
        project_path: "/test/project",
        moss_dir: "/test/project/.moss",
      };
      mockInvoke.mockResolvedValue(undefined);

      const cookies = [{ name: "auth", value: "token123" }];
      await setPluginCookie(cookies);

      expect(mockInvoke).toHaveBeenCalledWith("set_plugin_cookie", {
        pluginName: "matters",
        projectPath: "/test/project",
        cookies,
      });
    });

    it("throws descriptive error when called outside hook", async () => {
      // No context set
      await expect(
        setPluginCookie([{ name: "a", value: "b" }])
      ).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("handles cookies with all fields", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockResolvedValue(undefined);

      const cookies = [
        {
          name: "session",
          value: "abc123",
          domain: "example.com",
          path: "/api",
        },
      ];
      await setPluginCookie(cookies);

      expect(mockInvoke).toHaveBeenCalledWith("set_plugin_cookie", {
        pluginName: "plugin",
        projectPath: "/project",
        cookies,
      });
    });

    it("handles multiple cookies", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockResolvedValue(undefined);

      const cookies = [
        { name: "cookie1", value: "value1" },
        { name: "cookie2", value: "value2" },
        { name: "cookie3", value: "value3" },
      ];
      await setPluginCookie(cookies);

      expect(mockInvoke).toHaveBeenCalledWith("set_plugin_cookie", {
        pluginName: "plugin",
        projectPath: "/project",
        cookies,
      });
    });

    it("handles empty cookie array", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockResolvedValue(undefined);

      await setPluginCookie([]);

      expect(mockInvoke).toHaveBeenCalledWith("set_plugin_cookie", {
        pluginName: "plugin",
        projectPath: "/project",
        cookies: [],
      });
    });

    it("propagates errors from Tauri", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockRejectedValue(new Error("Failed to save cookies"));

      await expect(
        setPluginCookie([{ name: "a", value: "b" }])
      ).rejects.toThrow("Failed to save cookies");
    });
  });

  describe("clearPluginCookies", () => {
    it("invokes clear_plugin_cookies with the plugin name and project path", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "matters",
        project_path: "/test/project",
        moss_dir: "/test/project/.moss",
      };
      mockInvoke.mockResolvedValue(undefined);

      await clearPluginCookies();

      expect(mockInvoke).toHaveBeenCalledWith("clear_plugin_cookies", {
        pluginName: "matters",
        projectPath: "/test/project",
      });
    });

    it("throws descriptive error when called outside hook", async () => {
      // No context set
      await expect(clearPluginCookies()).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "plugin",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };
      mockInvoke.mockRejectedValue(new Error("Failed to clear cookies"));

      await expect(clearPluginCookies()).rejects.toThrow("Failed to clear cookies");
    });
  });
});
