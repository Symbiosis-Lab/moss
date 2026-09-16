import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { getInternalContext, hasContext } from "../context.js";

describe("Context Utilities", () => {
  const originalWindow = globalThis.window;
  let mockWindow: Record<string, unknown>;

  beforeEach(() => {
    mockWindow = {};
    (globalThis as unknown as { window: unknown }).window = mockWindow;
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
    vi.clearAllMocks();
  });

  describe("getInternalContext", () => {
    it("returns context when set", () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "test-plugin",
        project_path: "/test/project",
        moss_dir: "/test/project/.moss",
      };

      const ctx = getInternalContext();

      expect(ctx.plugin_name).toBe("test-plugin");
      expect(ctx.project_path).toBe("/test/project");
      expect(ctx.moss_dir).toBe("/test/project/.moss");
    });

    it("throws descriptive error when called outside hook", () => {
      expect(() => getInternalContext()).toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("throws when context is undefined", () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = undefined;

      expect(() => getInternalContext()).toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("includes helpful function names in error message", () => {
      expect(() => getInternalContext()).toThrow(
        /process\(\), generate\(\), deploy\(\), or syndicate\(\)/
      );
    });
  });

  describe("hasContext", () => {
    it("returns true when context is set", () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "test",
        project_path: "/project",
        moss_dir: "/project/.moss",
      };

      expect(hasContext()).toBe(true);
    });

    it("returns false when context is not set", () => {
      expect(hasContext()).toBe(false);
    });

    it("returns false when context is undefined", () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = undefined;

      expect(hasContext()).toBe(false);
    });
  });
});
