import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { readFile, writeFile, listFiles, fileExists } from "../filesystem.js";

describe("Filesystem Utilities", () => {
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
        plugin_name: "test-plugin",
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

  describe("readFile", () => {
    it("reads project file with auto-detected path", async () => {
      mockInvoke.mockResolvedValue("# Hello World");

      const content = await readFile("README.md");

      expect(mockInvoke).toHaveBeenCalledWith("read_project_file", {
        projectPath: "/path/to/project",
        relativePath: "README.md",
      });
      expect(content).toBe("# Hello World");
    });

    it("handles nested paths", async () => {
      mockInvoke.mockResolvedValue("file contents");

      await readFile("src/index.ts");

      expect(mockInvoke).toHaveBeenCalledWith("read_project_file", {
        projectPath: "/path/to/project",
        relativePath: "src/index.ts",
      });
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(readFile("file.md")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("File not found"));

      await expect(readFile("missing.txt")).rejects.toThrow("File not found");
    });
  });

  describe("writeFile", () => {
    it("writes to project file with auto-detected path", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await writeFile("output/file.txt", "content");

      expect(mockInvoke).toHaveBeenCalledWith("write_project_file", {
        projectPath: "/path/to/project",
        relativePath: "output/file.txt",
        data: "content",
      });
    });

    it("handles nested paths", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await writeFile("deep/nested/path/file.md", "# Title");

      expect(mockInvoke).toHaveBeenCalledWith("write_project_file", {
        projectPath: "/path/to/project",
        relativePath: "deep/nested/path/file.md",
        data: "# Title",
      });
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(writeFile("file.txt", "data")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Permission denied"));

      await expect(writeFile("protected.txt", "data")).rejects.toThrow(
        "Permission denied"
      );
    });
  });

  describe("listFiles", () => {
    it("lists all project files with auto-detected path", async () => {
      const files = ["src/index.ts", "package.json", "README.md"];
      mockInvoke.mockResolvedValue(files);

      const result = await listFiles();

      expect(mockInvoke).toHaveBeenCalledWith("list_project_files", {
        projectPath: "/path/to/project",
      });
      expect(result).toEqual(files);
    });

    it("returns empty array when no files", async () => {
      mockInvoke.mockResolvedValue([]);

      const result = await listFiles();

      expect(result).toEqual([]);
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(listFiles()).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });

    it("propagates errors from Tauri", async () => {
      mockInvoke.mockRejectedValue(new Error("Directory not found"));

      await expect(listFiles()).rejects.toThrow("Directory not found");
    });
  });

  describe("fileExists", () => {
    it("returns true when file can be read", async () => {
      mockInvoke.mockResolvedValue("file contents");

      const result = await fileExists("existing.txt");

      expect(result).toBe(true);
    });

    it("returns false when readFile throws", async () => {
      mockInvoke.mockRejectedValue(new Error("File not found"));

      const result = await fileExists("missing.txt");

      expect(result).toBe(false);
    });

    it("calls read_project_file to check existence", async () => {
      mockInvoke.mockResolvedValue("");

      await fileExists("file.txt");

      expect(mockInvoke).toHaveBeenCalledWith("read_project_file", {
        projectPath: "/path/to/project",
        relativePath: "file.txt",
      });
    });

    it("throws when called outside hook context", async () => {
      delete mockWindow.__MOSS_INTERNAL_CONTEXT__;

      await expect(fileExists("file.txt")).rejects.toThrow(
        /must be called from within a plugin hook/
      );
    });
  });

  describe("context isolation", () => {
    it("uses project path from context", async () => {
      mockWindow.__MOSS_INTERNAL_CONTEXT__ = {
        plugin_name: "other-plugin",
        project_path: "/different/project",
        moss_dir: "/different/project/.moss",
      };
      mockInvoke.mockResolvedValue("content");

      await readFile("file.md");

      expect(mockInvoke).toHaveBeenCalledWith("read_project_file", {
        projectPath: "/different/project",
        relativePath: "file.md",
      });
    });
  });
});
