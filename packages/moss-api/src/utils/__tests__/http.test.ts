import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { fetchUrl, downloadAsset } from "../http.js";

describe("HTTP Utilities", () => {
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

  describe("fetchUrl", () => {
    it("calls fetch_url with correct arguments", async () => {
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        body_base64: btoa("response body"),
        content_type: "text/plain",
      });

      await fetchUrl("https://example.com/api");

      expect(mockInvoke).toHaveBeenCalledWith("fetch_url", {
        url: "https://example.com/api",
        timeoutMs: 30000,
      });
    });

    it("uses custom timeout when provided", async () => {
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        body_base64: btoa(""),
        content_type: null,
      });

      await fetchUrl("https://example.com", { timeoutMs: 60000 });

      expect(mockInvoke).toHaveBeenCalledWith("fetch_url", {
        url: "https://example.com",
        timeoutMs: 60000,
      });
    });

    it("returns FetchResult with decoded body", async () => {
      const content = "Hello, World!";
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        body_base64: btoa(content),
        content_type: "text/plain",
      });

      const result = await fetchUrl("https://example.com");

      expect(result.status).toBe(200);
      expect(result.ok).toBe(true);
      expect(result.contentType).toBe("text/plain");
      expect(result.text()).toBe(content);
    });

    it("returns body as Uint8Array", async () => {
      const content = "test";
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        body_base64: btoa(content),
        content_type: null,
      });

      const result = await fetchUrl("https://example.com");

      expect(result.body).toBeInstanceOf(Uint8Array);
      expect(result.body.length).toBe(4);
    });

    it("handles HTTP errors correctly", async () => {
      mockInvoke.mockResolvedValue({
        status: 404,
        ok: false,
        body_base64: btoa("Not Found"),
        content_type: "text/plain",
      });

      const result = await fetchUrl("https://example.com/missing");

      expect(result.status).toBe(404);
      expect(result.ok).toBe(false);
    });

    it("propagates network errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Connection refused"));

      await expect(fetchUrl("https://unreachable.com")).rejects.toThrow(
        "Connection refused"
      );
    });

    it("does not require context (stateless)", async () => {
      // fetchUrl doesn't need context - it's a pure HTTP call
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        body_base64: btoa("response"),
        content_type: "text/plain",
      });

      const result = await fetchUrl("https://example.com");

      expect(result.ok).toBe(true);
    });
  });

  describe("downloadAsset", () => {
    it("downloads to project directory with auto-detected path", async () => {
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        content_type: "image/png",
        bytes_written: 1024,
        actual_path: "assets/image.png",
      });

      await downloadAsset("https://example.com/image.png", "assets");

      expect(mockInvoke).toHaveBeenCalledWith("download_asset", {
        url: "https://example.com/image.png",
        projectPath: "/my/project",
        targetDir: "assets",
        timeoutMs: 30000,
      });
    });

    it("uses custom timeout when provided", async () => {
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        content_type: null,
        bytes_written: 0,
        actual_path: "",
      });

      await downloadAsset("https://example.com", "downloads", {
        timeoutMs: 120000,
      });

      expect(mockInvoke).toHaveBeenCalledWith("download_asset", {
        url: "https://example.com",
        projectPath: "/my/project",
        targetDir: "downloads",
        timeoutMs: 120000,
      });
    });

    it("returns DownloadResult with correct fields", async () => {
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        content_type: "image/jpeg",
        bytes_written: 2048,
        actual_path: "assets/photo.jpg",
      });

      const result = await downloadAsset("https://example.com/photo", "assets");

      expect(result.status).toBe(200);
      expect(result.ok).toBe(true);
      expect(result.contentType).toBe("image/jpeg");
      expect(result.bytesWritten).toBe(2048);
      expect(result.actualPath).toBe("assets/photo.jpg");
    });

    it("handles download failures", async () => {
      mockInvoke.mockResolvedValue({
        status: 404,
        ok: false,
        content_type: null,
        bytes_written: 0,
        actual_path: "",
      });

      const result = await downloadAsset(
        "https://example.com/missing",
        "assets"
      );

      expect(result.ok).toBe(false);
      expect(result.status).toBe(404);
      expect(result.bytesWritten).toBe(0);
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(
        downloadAsset("https://example.com/image.png", "assets")
      ).rejects.toThrow(/must be called from within a plugin hook/);
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Timeout"));

      await expect(
        downloadAsset("https://slow.example.com", "assets")
      ).rejects.toThrow("Timeout");
    });
  });

  describe("context isolation", () => {
    it("uses project path from context for downloads", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "other-plugin",
        project_path: "/different/project",
        moss_dir: "/different/project/.moss",
      };
      mockInvoke.mockResolvedValue({
        status: 200,
        ok: true,
        content_type: "image/png",
        bytes_written: 1024,
        actual_path: "images/photo.png",
      });

      await downloadAsset("https://example.com/photo.png", "images");

      expect(mockInvoke).toHaveBeenCalledWith("download_asset", {
        url: "https://example.com/photo.png",
        projectPath: "/different/project",
        targetDir: "images",
        timeoutMs: 30000,
      });
    });
  });
});
