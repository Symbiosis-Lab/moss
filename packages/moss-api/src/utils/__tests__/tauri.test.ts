import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { getTauriCore, isTauriAvailable } from "../tauri.js";

describe("Tauri Utilities", () => {
  const originalWindow = globalThis.window;

  beforeEach(() => {
    // Reset window mock before each test
    (globalThis as unknown as { window: unknown }).window = {};
  });

  afterEach(() => {
    (globalThis as unknown as { window: unknown }).window = originalWindow;
  });

  describe("isTauriAvailable", () => {
    it("returns false when window.__TAURI__ is undefined", () => {
      expect(isTauriAvailable()).toBe(false);
    });

    it("returns false when window.__TAURI__.core is undefined", () => {
      (window as unknown as { __TAURI__: object }).__TAURI__ = {};
      expect(isTauriAvailable()).toBe(false);
    });

    it("returns true when window.__TAURI__.core exists", () => {
      (window as unknown as { __TAURI__: { core: { invoke: unknown } } }).__TAURI__ = {
        core: { invoke: vi.fn() },
      };
      expect(isTauriAvailable()).toBe(true);
    });
  });

  describe("getTauriCore", () => {
    it("throws error when Tauri is not available", () => {
      expect(() => getTauriCore()).toThrow("Tauri core not available");
    });

    it("throws error when __TAURI__ exists but core is undefined", () => {
      (window as unknown as { __TAURI__: object }).__TAURI__ = {};
      expect(() => getTauriCore()).toThrow("Tauri core not available");
    });

    it("returns TauriCore when available", () => {
      const mockInvoke = vi.fn();
      (window as unknown as { __TAURI__: { core: { invoke: unknown } } }).__TAURI__ = {
        core: { invoke: mockInvoke },
      };
      const core = getTauriCore();
      expect(core.invoke).toBe(mockInvoke);
    });

    it("TauriCore.invoke can be called with command and args", async () => {
      const mockInvoke = vi.fn().mockResolvedValue("result");
      (window as unknown as { __TAURI__: { core: { invoke: unknown } } }).__TAURI__ = {
        core: { invoke: mockInvoke },
      };

      const core = getTauriCore();
      const result = await core.invoke("test_command", { arg1: "value1" });

      expect(mockInvoke).toHaveBeenCalledWith("test_command", { arg1: "value1" });
      expect(result).toBe("result");
    });
  });
});
