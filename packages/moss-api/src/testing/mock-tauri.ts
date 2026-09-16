/**
 * Tauri IPC mocking utilities for testing moss plugins
 *
 * Provides in-memory implementations of Tauri IPC commands that plugins use
 * through moss-api. This enables integration testing without a running Tauri app.
 *
 * @example
 * ```typescript
 * import { setupMockTauri } from "@symbiosis-lab/moss-api/testing";
 *
 * describe("my plugin", () => {
 *   let ctx: MockTauriContext;
 *
 *   beforeEach(() => {
 *     ctx = setupMockTauri();
 *   });
 *
 *   afterEach(() => {
 *     ctx.cleanup();
 *   });
 *
 *   it("reads files", async () => {
 *     ctx.filesystem.setFile("/test/project/test.md", "# Hello");
 *     const content = await readFile("test.md");
 *     expect(content).toBe("# Hello");
 *   });
 * });
 * ```
 */

// Define minimal types for invoke args
interface InvokeArgs {
  [key: string]: unknown;
}

// ============================================================================
// Mock Filesystem
// ============================================================================

/**
 * A file stored in the mock filesystem
 */
export interface MockFile {
  content: string;
  createdAt: Date;
  modifiedAt: Date;
}

/**
 * In-memory filesystem for testing file operations
 */
export interface MockFilesystem {
  /** Internal file storage */
  files: Map<string, MockFile>;
  /** Get a file by full path */
  getFile(path: string): MockFile | undefined;
  /** Set a file's content (creates or updates) */
  setFile(path: string, content: string): void;
  /** Delete a file */
  deleteFile(path: string): boolean;
  /** List files matching an optional pattern */
  listFiles(pattern?: string): string[];
  /** Clear all files */
  clear(): void;
}

/**
 * Create a new mock filesystem instance
 */
export function createMockFilesystem(): MockFilesystem {
  const files = new Map<string, MockFile>();

  return {
    files,
    getFile(path: string) {
      return files.get(path);
    },
    setFile(path: string, content: string) {
      const now = new Date();
      const existing = files.get(path);
      files.set(path, {
        content,
        createdAt: existing?.createdAt ?? now,
        modifiedAt: now,
      });
    },
    deleteFile(path: string) {
      return files.delete(path);
    },
    listFiles(pattern?: string) {
      const allPaths = Array.from(files.keys());
      if (!pattern) return allPaths;
      // Simple glob matching
      const regex = new RegExp(
        "^" + pattern.replace(/\*/g, ".*").replace(/\?/g, ".") + "$"
      );
      return allPaths.filter((p) => regex.test(p));
    },
    clear() {
      files.clear();
    },
  };
}

// ============================================================================
// Download Tracker
// ============================================================================

/**
 * Tracks download activity for testing concurrency and completion
 */
export interface DownloadTracker {
  /** Number of currently active downloads */
  activeDownloads: number;
  /** Maximum concurrent downloads observed */
  maxConcurrent: number;
  /** URLs of completed downloads */
  completedDownloads: string[];
  /** Failed downloads with error messages */
  failedDownloads: Array<{ url: string; error: string }>;
  /** Mark a download as started */
  startDownload(url: string): void;
  /** Mark a download as ended */
  endDownload(url: string, success: boolean, error?: string): void;
  /** Reset all tracking state */
  reset(): void;
}

/**
 * Create a new download tracker instance
 */
export function createDownloadTracker(): DownloadTracker {
  let activeDownloads = 0;
  let maxConcurrent = 0;
  const completedDownloads: string[] = [];
  const failedDownloads: Array<{ url: string; error: string }> = [];

  return {
    get activeDownloads() {
      return activeDownloads;
    },
    get maxConcurrent() {
      return maxConcurrent;
    },
    get completedDownloads() {
      return completedDownloads;
    },
    get failedDownloads() {
      return failedDownloads;
    },
    startDownload(url: string) {
      activeDownloads++;
      if (activeDownloads > maxConcurrent) {
        maxConcurrent = activeDownloads;
      }
    },
    endDownload(url: string, success: boolean, error?: string) {
      activeDownloads--;
      if (success) {
        completedDownloads.push(url);
      } else {
        failedDownloads.push({ url, error: error || "Unknown error" });
      }
    },
    reset() {
      activeDownloads = 0;
      maxConcurrent = 0;
      completedDownloads.length = 0;
      failedDownloads.length = 0;
    },
  };
}

// ============================================================================
// URL Response Configuration
// ============================================================================

/**
 * Configuration for a mocked URL response
 */
export interface MockUrlResponse {
  /** HTTP status code */
  status: number;
  /** Whether the request was successful (2xx) */
  ok: boolean;
  /** Content-Type header */
  contentType?: string;
  /** Response body as base64 (for fetch_url) */
  bodyBase64?: string;
  /** Number of bytes written (for download_asset) */
  bytesWritten?: number;
  /** Actual file path where asset was saved */
  actualPath?: string;
  /** Artificial delay in milliseconds */
  delay?: number;
}

/**
 * URL response configuration for mocking HTTP requests
 */
export interface MockUrlConfig {
  /** Map of URL to response(s) */
  responses: Map<string, MockUrlResponse | MockUrlResponse[]>;
  /** Default response for unregistered URLs */
  defaultResponse: MockUrlResponse;
  /** Set response for a URL (can be single or array for retry testing) */
  setResponse(url: string, response: MockUrlResponse | MockUrlResponse[]): void;
  /** Get response for a URL (handles retry sequences) */
  getResponse(url: string): MockUrlResponse;
  /** Reset all URL configurations */
  reset(): void;
}

/**
 * Create a new URL config instance
 */
export function createMockUrlConfig(): MockUrlConfig {
  const responses = new Map<string, MockUrlResponse | MockUrlResponse[]>();
  const callCounts = new Map<string, number>();

  // Default: 1x1 red PNG
  const defaultResponse: MockUrlResponse = {
    status: 200,
    ok: true,
    contentType: "image/png",
    bodyBase64:
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    bytesWritten: 68,
    actualPath: "assets/image.png",
  };

  return {
    responses,
    defaultResponse,
    setResponse(url: string, response: MockUrlResponse | MockUrlResponse[]) {
      responses.set(url, response);
      callCounts.set(url, 0);
    },
    getResponse(url: string): MockUrlResponse {
      const config = responses.get(url);
      if (!config) return defaultResponse;

      if (Array.isArray(config)) {
        const count = callCounts.get(url) || 0;
        callCounts.set(url, count + 1);
        // Return the response at the current index, or the last one if exceeded
        return config[Math.min(count, config.length - 1)];
      }

      return config;
    },
    reset() {
      responses.clear();
      callCounts.clear();
    },
  };
}

// ============================================================================
// Binary Execution Tracker
// ============================================================================

/**
 * Result for a mocked binary execution
 */
export interface MockBinaryResult {
  success: boolean;
  exitCode: number;
  stdout: string;
  stderr: string;
}

/**
 * Configuration for mocking binary execution
 */
export interface MockBinaryConfig {
  /** Map of binary commands to results */
  results: Map<string, MockBinaryResult>;
  /** Default result for unregistered binaries */
  defaultResult: MockBinaryResult;
  /** Set result for a binary command (key format: "binaryPath args...") */
  setResult(key: string, result: MockBinaryResult): void;
  /** Get result for a binary command */
  getResult(binaryPath: string, args: string[]): MockBinaryResult;
  /** Reset all configurations */
  reset(): void;
}

/**
 * Create a new binary config instance
 */
export function createMockBinaryConfig(): MockBinaryConfig {
  const results = new Map<string, MockBinaryResult>();

  const defaultResult: MockBinaryResult = {
    success: true,
    exitCode: 0,
    stdout: "",
    stderr: "",
  };

  return {
    results,
    defaultResult,
    setResult(key: string, result: MockBinaryResult) {
      results.set(key, result);
    },
    getResult(binaryPath: string, args: string[]): MockBinaryResult {
      // Try exact match first
      const exactKey = `${binaryPath} ${args.join(" ")}`.trim();
      if (results.has(exactKey)) {
        return results.get(exactKey)!;
      }
      // Try binary name only
      if (results.has(binaryPath)) {
        return results.get(binaryPath)!;
      }
      return defaultResult;
    },
    reset() {
      results.clear();
    },
  };
}

// ============================================================================
// Cookie Storage
// ============================================================================

/**
 * Mock cookie storage for plugin authentication testing
 */
export interface MockCookieStorage {
  /** Map of pluginName:projectPath to cookies */
  cookies: Map<string, Array<{ name: string; value: string; domain?: string; path?: string }>>;
  /** Get cookies for a plugin/project */
  getCookies(
    pluginName: string,
    projectPath: string
  ): Array<{ name: string; value: string; domain?: string; path?: string }>;
  /** Set cookies for a plugin/project */
  setCookies(
    pluginName: string,
    projectPath: string,
    cookies: Array<{ name: string; value: string; domain?: string; path?: string }>
  ): void;
  /** Clear all cookies */
  clear(): void;
}

/**
 * Create a new cookie storage instance
 */
export function createMockCookieStorage(): MockCookieStorage {
  const cookies = new Map<
    string,
    Array<{ name: string; value: string; domain?: string; path?: string }>
  >();

  return {
    cookies,
    getCookies(pluginName: string, projectPath: string) {
      const key = `${pluginName}:${projectPath}`;
      return cookies.get(key) || [];
    },
    setCookies(
      pluginName: string,
      projectPath: string,
      newCookies: Array<{ name: string; value: string; domain?: string; path?: string }>
    ) {
      const key = `${pluginName}:${projectPath}`;
      cookies.set(key, newCookies);
    },
    clear() {
      cookies.clear();
    },
  };
}

// ============================================================================
// Browser Tracker
// ============================================================================

/**
 * Tracks browser open/close calls for testing
 */
export interface MockBrowserTracker {
  /** URLs that were opened in action panel */
  openedUrls: string[];
  /** URLs that were opened in system browser */
  systemBrowserUrls: string[];
  /** HTML content passed to openBrowserWithHtml */
  htmlContent: string[];
  /** Number of times closeBrowser was called */
  closeCount: number;
  /** Whether browser is currently open */
  isOpen: boolean;
  /** Reset tracking state */
  reset(): void;
}

/**
 * Create a new browser tracker instance
 */
export function createMockBrowserTracker(): MockBrowserTracker {
  const openedUrls: string[] = [];
  const systemBrowserUrls: string[] = [];
  const htmlContent: string[] = [];
  let closeCount = 0;
  let isOpen = false;

  return {
    get openedUrls() {
      return openedUrls;
    },
    get systemBrowserUrls() {
      return systemBrowserUrls;
    },
    get htmlContent() {
      return htmlContent;
    },
    get closeCount() {
      return closeCount;
    },
    set closeCount(value: number) {
      closeCount = value;
    },
    get isOpen() {
      return isOpen;
    },
    set isOpen(value: boolean) {
      isOpen = value;
    },
    reset() {
      openedUrls.length = 0;
      systemBrowserUrls.length = 0;
      htmlContent.length = 0;
      closeCount = 0;
      isOpen = false;
    },
  };
}

// ============================================================================
// Dialog Tracker
// ============================================================================

/**
 * Dialog result types matching the moss backend
 */
export interface MockDialogResult {
  type: "submitted" | "cancelled";
  value?: unknown;
}

/**
 * Tracks dialog interactions for testing
 */
export interface MockDialogTracker {
  /** Dialogs that were shown (with their URLs and titles) */
  shownDialogs: Array<{ url: string; title: string; width: number; height: number }>;
  /** Configure the next dialog result (for automatic response) */
  nextResult: MockDialogResult | null;
  /** Submitted results by dialog ID */
  submittedResults: Map<string, MockDialogResult>;
  /** Set the result for the next dialog shown */
  setNextResult(result: MockDialogResult): void;
  /** Simulate user submitting a dialog result */
  submitResult(dialogId: string, value: unknown): void;
  /** Simulate user cancelling a dialog */
  cancelDialog(dialogId: string): void;
  /** Reset tracking state */
  reset(): void;
}

/**
 * Create a new dialog tracker instance
 */
export function createMockDialogTracker(): MockDialogTracker {
  const shownDialogs: Array<{ url: string; title: string; width: number; height: number }> = [];
  const submittedResults = new Map<string, MockDialogResult>();
  let nextResult: MockDialogResult | null = null;

  return {
    get shownDialogs() {
      return shownDialogs;
    },
    get nextResult() {
      return nextResult;
    },
    set nextResult(value: MockDialogResult | null) {
      nextResult = value;
    },
    get submittedResults() {
      return submittedResults;
    },
    setNextResult(result: MockDialogResult) {
      nextResult = result;
    },
    submitResult(dialogId: string, value: unknown) {
      submittedResults.set(dialogId, { type: "submitted", value });
    },
    cancelDialog(dialogId: string) {
      submittedResults.set(dialogId, { type: "cancelled" });
    },
    reset() {
      shownDialogs.length = 0;
      submittedResults.clear();
      nextResult = null;
    },
  };
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Extract filename from URL (mimics Rust backend behavior)
 */
function extractFilenameFromUrl(url: string): string {
  try {
    const urlObj = new URL(url);
    const pathname = urlObj.pathname;
    const segments = pathname.split("/").filter((s) => s.length > 0);

    // Try to find UUID in path
    for (const segment of segments) {
      if (
        /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(
          segment
        )
      ) {
        return `${segment}.png`; // Default to PNG for mock
      }
    }

    // Fallback to last segment or hash
    const lastSegment = segments[segments.length - 1] || "image";
    return lastSegment.includes(".") ? lastSegment : `${lastSegment}.png`;
  } catch {
    return "image.png";
  }
}

// ============================================================================
// Main Setup Function
// ============================================================================

/**
 * Options for setting up mock Tauri environment
 */
export interface SetupMockTauriOptions {
  /** Plugin name for internal context (default: "test-plugin") */
  pluginName?: string;
  /** Project path for internal context (default: "/test/project") */
  projectPath?: string;
}

/**
 * Context returned by setupMockTauri with all mock utilities
 */
export interface MockTauriContext {
  /** In-memory filesystem */
  filesystem: MockFilesystem;
  /** Download tracking for concurrency tests */
  downloadTracker: DownloadTracker;
  /** URL response configuration */
  urlConfig: MockUrlConfig;
  /** Binary execution configuration */
  binaryConfig: MockBinaryConfig;
  /** Cookie storage */
  cookieStorage: MockCookieStorage;
  /** Browser open/close tracking */
  browserTracker: MockBrowserTracker;
  /** Dialog interaction tracking */
  dialogTracker: MockDialogTracker;
  /** The project path used for internal context */
  projectPath: string;
  /** The plugin name used for internal context */
  pluginName: string;
  /** Cleanup function - must be called after tests */
  cleanup: () => void;
}

/**
 * Set up mock Tauri IPC for testing
 *
 * This sets up `window.__TAURI__.core.invoke` to intercept all IPC calls
 * and route them to in-memory implementations. It also sets up
 * `__MOSS_INTERNAL_CONTEXT__` for the context-aware APIs.
 *
 * @param options - Optional configuration for project path and plugin name
 * @returns Context with mock utilities and cleanup function
 *
 * @example
 * ```typescript
 * const ctx = setupMockTauri({ projectPath: "/my/project", pluginName: "my-plugin" });
 *
 * // Set up test data
 * ctx.filesystem.setFile("/my/project/article.md", "# Test");
 * ctx.urlConfig.setResponse("https://example.com/image.png", {
 *   status: 200,
 *   ok: true,
 *   contentType: "image/png",
 *   bytesWritten: 1024,
 * });
 *
 * // Run your plugin code...
 *
 * // Verify results
 * expect(ctx.downloadTracker.completedDownloads).toHaveLength(1);
 *
 * // Cleanup
 * ctx.cleanup();
 * ```
 */
export function setupMockTauri(options?: SetupMockTauriOptions): MockTauriContext {
  const projectPath = options?.projectPath ?? "/test/project";
  const pluginName = options?.pluginName ?? "test-plugin";

  const filesystem = createMockFilesystem();
  const downloadTracker = createDownloadTracker();
  const urlConfig = createMockUrlConfig();
  const binaryConfig = createMockBinaryConfig();
  const cookieStorage = createMockCookieStorage();
  const browserTracker = createMockBrowserTracker();
  const dialogTracker = createMockDialogTracker();

  // ── Mock event system (listen / emit) ──────────────────────────────────
  type EventHandler = (event: { payload: unknown }) => void;
  const eventListeners = new Map<string, Set<EventHandler>>();

  /** Register a listener; returns an unlisten function */
  const listen = async (
    event: string,
    handler: EventHandler
  ): Promise<() => void> => {
    if (!eventListeners.has(event)) {
      eventListeners.set(event, new Set());
    }
    eventListeners.get(event)!.add(handler);
    return () => {
      eventListeners.get(event)?.delete(handler);
    };
  };

  /** Emit an event to all registered listeners */
  const emit = async (event: string, payload?: unknown): Promise<void> => {
    const handlers = eventListeners.get(event);
    if (handlers) {
      for (const handler of handlers) {
        handler({ payload });
      }
    }
  };

  /** Internal helper: emit to listeners synchronously (used by invoke handlers) */
  const emitToListeners = (event: string, payload: unknown): void => {
    const handlers = eventListeners.get(event);
    if (handlers) {
      for (const handler of handlers) {
        handler({ payload });
      }
    }
  };

  // Create invoke handler
  const invoke = async (cmd: string, args?: InvokeArgs): Promise<unknown> => {
    const payload = args as Record<string, unknown> | undefined;

    switch (cmd) {
      // ======================================================================
      // Filesystem Operations
      // ======================================================================
      case "read_project_file": {
        const projectPath = payload?.projectPath as string;
        const relativePath = payload?.relativePath as string;
        const fullPath = `${projectPath}/${relativePath}`;
        const file = filesystem.getFile(fullPath);
        if (file) {
          return file.content;
        }
        throw new Error(`File not found: ${fullPath}`);
      }

      case "write_project_file": {
        const projectPath = payload?.projectPath as string;
        const relativePath = payload?.relativePath as string;
        const content = payload?.data as string; // Note: moss-api uses 'data' not 'content'
        const fullPath = `${projectPath}/${relativePath}`;
        filesystem.setFile(fullPath, content);
        return null;
      }

      case "list_project_files": {
        const projectPath = payload?.projectPath as string;
        // Return all file paths relative to the project path
        const allPaths = filesystem.listFiles();
        return allPaths
          .filter((p) => p.startsWith(projectPath + "/"))
          .map((p) => p.substring(projectPath.length + 1));
      }

      case "list_project_tree": {
        // Return files with is_home annotations (mirrors Rust annotate_home_files)
        const allPaths = filesystem.listFiles();
        const prefix = projectPath + "/";
        const relPaths = allPaths
          .filter((p) => p.startsWith(prefix))
          .map((p) => p.substring(prefix.length));

        // Group by parent folder
        const folderFiles = new Map<string, string[]>();
        for (const p of relPaths) {
          const lastSlash = p.lastIndexOf("/");
          const parent = lastSlash >= 0 ? p.substring(0, lastSlash) : "";
          const filename = lastSlash >= 0 ? p.substring(lastSlash + 1) : p;
          if (!folderFiles.has(parent)) folderFiles.set(parent, []);
          folderFiles.get(parent)!.push(filename);
        }

        // Detect home file per folder (simplified version of Rust logic)
        const INDEX_STEMS = ["index", "readme", "_index", "main"];
        const homePaths = new Set<string>();
        for (const [folder, files] of folderFiles) {
          let winner: string | null = null;
          // Priority 1: index stems
          for (const stem of INDEX_STEMS) {
            const found = files.find(
              (f) => f.toLowerCase().replace(/\.[^.]+$/, "") === stem
            );
            if (found) { winner = found; break; }
          }
          // Priority 2: self-named folder note
          if (!winner) {
            const folderName = folder.includes("/")
              ? folder.substring(folder.lastIndexOf("/") + 1)
              : folder;
            if (folderName) {
              const found = files.find(
                (f) => f.toLowerCase().replace(/\.[^.]+$/, "") === folderName.toLowerCase()
              );
              if (found) winner = found;
            }
          }
          if (winner) {
            homePaths.add(folder ? `${folder}/${winner}` : winner);
          }
        }

        return relPaths.map((p) => ({
          path: p,
          is_home: homePaths.has(p),
        }));
      }

      case "project_file_exists": {
        const projectPath = payload?.projectPath as string;
        const relativePath = payload?.relativePath as string;
        const fullPath = `${projectPath}/${relativePath}`;
        return filesystem.getFile(fullPath) !== undefined;
      }

      // ======================================================================
      // Plugin Storage Operations
      // ======================================================================
      case "read_plugin_file": {
        const pn = payload?.pluginName as string;
        const pp = payload?.projectPath as string;
        const rp = payload?.relativePath as string;
        const fullPath = `${pp}/.moss/plugins/${pn}/${rp}`;
        const file = filesystem.getFile(fullPath);
        if (file) {
          return file.content;
        }
        throw new Error(`Plugin file not found: ${fullPath}`);
      }

      case "write_plugin_file": {
        const pn = payload?.pluginName as string;
        const pp = payload?.projectPath as string;
        const rp = payload?.relativePath as string;
        const content = payload?.content as string;
        const fullPath = `${pp}/.moss/plugins/${pn}/${rp}`;
        filesystem.setFile(fullPath, content);
        return null;
      }

      case "list_plugin_files": {
        const pn = payload?.pluginName as string;
        const pp = payload?.projectPath as string;
        const prefix = `${pp}/.moss/plugins/${pn}/`;
        const allPaths = filesystem.listFiles();
        return allPaths
          .filter((p) => p.startsWith(prefix))
          .map((p) => p.substring(prefix.length));
      }

      case "plugin_file_exists": {
        const pn = payload?.pluginName as string;
        const pp = payload?.projectPath as string;
        const rp = payload?.relativePath as string;
        const fullPath = `${pp}/.moss/plugins/${pn}/${rp}`;
        return filesystem.getFile(fullPath) !== undefined;
      }

      // ======================================================================
      // HTTP Operations
      // ======================================================================
      case "html_to_markdown": {
        // Mock of moss's htmd converter. Tests don't assert on converted body
        // content, so return the input HTML (mirrors the Rust Err-fallback) to
        // give callers a defined string instead of an unhandled-IPC undefined.
        return (payload?.html as string) ?? "";
      }

      case "fetch_url": {
        const url = payload?.url as string;
        const response = urlConfig.getResponse(url);

        if (response.delay) {
          return new Promise((resolve) =>
            setTimeout(
              () =>
                resolve({
                  status: response.status,
                  ok: response.ok,
                  body_base64: response.bodyBase64 || "",
                  content_type: response.contentType || null,
                }),
              response.delay
            )
          );
        }

        return {
          status: response.status,
          ok: response.ok,
          body_base64: response.bodyBase64 || "",
          content_type: response.contentType || null,
        };
      }

      case "http_post": {
        // HTTP POST - similar to fetch_url but for POST requests
        const url = payload?.url as string;
        const response = urlConfig.getResponse(url);

        if (response.delay) {
          return new Promise((resolve) =>
            setTimeout(
              () =>
                resolve({
                  status: response.status,
                  ok: response.ok,
                  body_base64: response.bodyBase64 || "",
                  content_type: response.contentType || null,
                }),
              response.delay
            )
          );
        }

        return {
          status: response.status,
          ok: response.ok,
          body_base64: response.bodyBase64 || "",
          content_type: response.contentType || null,
        };
      }

      case "download_asset": {
        const url = payload?.url as string;
        const targetDir = payload?.targetDir as string;
        const response = urlConfig.getResponse(url);

        downloadTracker.startDownload(url);

        // status 0 simulates a network error/timeout - throw an error
        if (response.status === 0) {
          downloadTracker.endDownload(url, false, "Network error");
          throw new Error("Network timeout");
        }

        // Generate actual_path based on URL or use configured value
        const actualPath =
          response.actualPath || `${targetDir}/${extractFilenameFromUrl(url)}`;

        const result = {
          status: response.status,
          ok: response.ok,
          content_type: response.contentType || null,
          bytes_written: response.bytesWritten || 0,
          actual_path: actualPath,
        };

        if (response.delay) {
          return new Promise((resolve) =>
            setTimeout(() => {
              downloadTracker.endDownload(url, response.ok);
              resolve(result);
            }, response.delay)
          );
        }

        downloadTracker.endDownload(url, response.ok);
        return result;
      }

      // ======================================================================
      // Cookie Operations
      // ======================================================================
      case "get_plugin_cookie": {
        const pluginName = payload?.pluginName as string;
        const projectPath = payload?.projectPath as string;
        return cookieStorage.getCookies(pluginName, projectPath);
      }

      case "set_plugin_cookie": {
        const pluginName = payload?.pluginName as string;
        const projectPath = payload?.projectPath as string;
        const cookies = payload?.cookies as Array<{
          name: string;
          value: string;
          domain?: string;
          path?: string;
        }>;
        cookieStorage.setCookies(pluginName, projectPath, cookies);
        return null;
      }

      // ======================================================================
      // Browser Operations
      // ======================================================================
      case "set_action_panel_html": {
        const html = payload?.html as string;
        browserTracker.htmlContent.push(html);
        (browserTracker as { isOpen: boolean }).isOpen = true;
        return null;
      }

      case "open_action_panel": {
        const url = payload?.url as string;
        browserTracker.openedUrls.push(url);
        (browserTracker as { isOpen: boolean }).isOpen = true;
        return null;
      }

      case "close_action_panel": {
        (browserTracker as { closeCount: number }).closeCount++;
        (browserTracker as { isOpen: boolean }).isOpen = false;
        return null;
      }

      case "open_system_browser": {
        const url = payload?.url as string;
        browserTracker.systemBrowserUrls.push(url);
        return null;
      }

      // ======================================================================
      // Binary Execution
      // ======================================================================
      case "execute_binary": {
        const binaryPath = payload?.binaryPath as string;
        const binaryArgs = payload?.args as string[];
        const streamId = payload?.streamId as string | undefined;
        const result = binaryConfig.getResult(binaryPath, binaryArgs);

        // When streamId is provided, emit binary-output events for each stderr line
        if (streamId && result.stderr) {
          const lines = result.stderr.split("\n").filter((l) => l.length > 0);
          for (const line of lines) {
            emitToListeners("binary-output", { streamId, line });
          }
        }

        return {
          success: result.success,
          exit_code: result.exitCode,
          stdout: result.stdout,
          stderr: result.stderr,
        };
      }

      // ======================================================================
      // Messaging (silent no-op)
      // ======================================================================
      case "plugin_message": {
        // Silently accept plugin messages (logs, progress, errors, etc.)
        return null;
      }

      default:
        console.warn(`Unhandled IPC command: ${cmd}`);
        return null;
    }
  };

  // Set up window.__TAURI__ directly (moss-api checks for this)
  interface MockTauriWindow {
    __TAURI__?: {
      core?: { invoke: typeof invoke };
      event?: { listen: typeof listen; emit: typeof emit };
    };
    __MOSS_INTERNAL_CONTEXT__?: {
      plugin_name: string;
      project_path: string;
      moss_dir: string;
    };
  }

  const w = globalThis as unknown as { window?: MockTauriWindow };

  // Ensure window exists (for Node.js environments like happy-dom)
  if (typeof w.window === "undefined") {
    (globalThis as unknown as { window: object }).window = {};
  }

  const win = (globalThis as unknown as { window: MockTauriWindow }).window;

  win.__TAURI__ = {
    core: { invoke },
    event: { listen, emit },
  };

  // Set up internal context for context-aware APIs
  win.__MOSS_INTERNAL_CONTEXT__ = {
    plugin_name: pluginName,
    project_path: projectPath,
    moss_dir: `${projectPath}/.moss`,
  };

  return {
    filesystem,
    downloadTracker,
    urlConfig,
    binaryConfig,
    cookieStorage,
    browserTracker,
    dialogTracker,
    projectPath,
    pluginName,
    cleanup: () => {
      // Clear the mock Tauri interface and internal context
      delete win.__TAURI__;
      delete win.__MOSS_INTERNAL_CONTEXT__;
      filesystem.clear();
      downloadTracker.reset();
      urlConfig.reset();
      binaryConfig.reset();
      cookieStorage.clear();
      browserTracker.reset();
      dialogTracker.reset();
      eventListeners.clear();
    },
  };
}
