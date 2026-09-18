//#region src/testing/mock-tauri.d.ts
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
/**
 * A file stored in the mock filesystem
 */
interface MockFile {
  content: string;
  createdAt: Date;
  modifiedAt: Date;
}
/**
 * In-memory filesystem for testing file operations
 */
interface MockFilesystem {
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
declare function createMockFilesystem(): MockFilesystem;
/**
 * Tracks download activity for testing concurrency and completion
 */
interface DownloadTracker {
  /** Number of currently active downloads */
  activeDownloads: number;
  /** Maximum concurrent downloads observed */
  maxConcurrent: number;
  /** URLs of completed downloads */
  completedDownloads: string[];
  /** Failed downloads with error messages */
  failedDownloads: Array<{
    url: string;
    error: string;
  }>;
  /** Mark a download as started */
  startDownload(url: string): void;
  /** Mark a download as ended */
  endDownload(url: string, success: boolean, error?: string): void;
  /** Reset all tracking state */
  reset(): void;
}
declare function createDownloadTracker(): DownloadTracker;
/**
 * Configuration for a mocked URL response
 */
interface MockUrlResponse {
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
interface MockUrlConfig {
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
declare function createMockUrlConfig(): MockUrlConfig;
/**
 * Result for a mocked binary execution
 */
interface MockBinaryResult {
  success: boolean;
  exitCode: number;
  stdout: string;
  stderr: string;
}
/**
 * Configuration for mocking binary execution
 */
interface MockBinaryConfig {
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
declare function createMockBinaryConfig(): MockBinaryConfig;
/**
 * Mock cookie storage for plugin authentication testing
 */
interface MockCookieStorage {
  /** Map of pluginName:projectPath to cookies */
  cookies: Map<string, Array<{
    name: string;
    value: string;
    domain?: string;
    path?: string;
  }>>;
  /** Get cookies for a plugin/project */
  getCookies(pluginName: string, projectPath: string): Array<{
    name: string;
    value: string;
    domain?: string;
    path?: string;
  }>;
  /** Set cookies for a plugin/project */
  setCookies(pluginName: string, projectPath: string, cookies: Array<{
    name: string;
    value: string;
    domain?: string;
    path?: string;
  }>): void;
  /** Remove every cookie stored for one plugin/project */
  clearCookies(pluginName: string, projectPath: string): void;
  /** Clear all cookies */
  clear(): void;
}
declare function createMockCookieStorage(): MockCookieStorage;
/**
 * Mock secret storage.
 *
 * Scoped by plugin, as the host scopes it — a mock keyed only by the secret's
 * name would let a test pass that the host would refuse.
 *
 * `seed` stands in for the user answering moss's modal; a plugin's own
 * `setSecret` writes the same store. `rejectSecret` re-asks, so the mock answers
 * for the user — it cancels unless a test called `answerNextPrompt`, because
 * cancel is the case plugins forget to handle.
 */
interface MockSecretStorage {
  /** Store a secret as if the user had answered moss's credential modal. */
  seed(pluginName: string, key: string, value: string): void;
  /** What `getSecret` would return: the stored value, or `null`. */
  get(pluginName: string, key: string): string | null;
  /** Did the plugin tell moss to forget this secret? */
  wasRejected(pluginName: string, key: string): boolean;
  /** What the next re-prompt answers. Consumed by one `reject`, then cancel again. */
  answerNextPrompt(value: string | null): void;
  /** Forget it, re-ask, and answer — what `rejectSecret` does end to end. */
  reject(pluginName: string, key: string): string | null;
  clear(): void;
}
declare function createMockSecretStorage(): MockSecretStorage;
/**
 * Tracks browser open/close calls for testing
 */
interface MockBrowserTracker {
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
declare function createMockBrowserTracker(): MockBrowserTracker;
/**
 * Dialog result types matching the moss backend
 */
interface MockDialogResult {
  type: "submitted" | "cancelled";
  value?: unknown;
}
/**
 * Tracks dialog interactions for testing
 */
interface MockDialogTracker {
  /** Dialogs that were shown (with their URLs and titles) */
  shownDialogs: Array<{
    url: string;
    title: string;
    width: number;
    height: number;
  }>;
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
declare function createMockDialogTracker(): MockDialogTracker;
/**
 * Options for setting up mock Tauri environment
 */
interface SetupMockTauriOptions {
  /** Plugin name for internal context (default: "test-plugin") */
  pluginName?: string;
  /** Project path for internal context (default: "/test/project") */
  projectPath?: string;
}
/**
 * Context returned by setupMockTauri with all mock utilities
 */
interface MockTauriContext {
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
  /** Secret storage */
  secretStorage: MockSecretStorage;
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
declare function setupMockTauri(options?: SetupMockTauriOptions): MockTauriContext;
//#endregion
//#region src/testing/mock-moss-fence.d.ts
/**
 * Mock-side mirror of the `.moss/` plugin sandbox fence.
 *
 * Mirrors the real guard: `PluginPath::sandboxed` in
 * open/crates/moss-build/src/vault/fs.rs, and its one documented exception,
 * `PluginPath::shared_social_data` (docs/reference/social-data-standard.md),
 * which is bound to the calling plugin's own `<plugin_id>.json` file — not
 * the whole shared directory. Before this mirror existed, mock-tauri.ts's
 * `write_project_file` / `read_project_file` handlers never enforced the
 * `.moss/` fence at all — so a plugin's own test suite kept passing for two
 * months after the real command started refusing the Matters plugin's
 * `.moss/data/social/matters.json` write, because "green" here proved
 * nothing about the real guard. Kept in its own module rather than inline in
 * mock-tauri.ts so that file's size doesn't grow for a self-contained policy
 * mirror.
 *
 * The example cases this rule must get right — a plugin's own file, a
 * sibling's, first-party `review.json`, the legacy directory, traversal —
 * are not hand-duplicated here: both this module's own test and the Rust
 * `PluginPath::shared_social_data` test read the same fixture table,
 * `open/fixtures/social-data-fence-cases.json`, so the two policies cannot
 * silently drift the way the mock and the real guard once did.
 */
/** Throws the same message the real Rust guard returns, unless the path is
 * the calling plugin's own shared-social-data file. */
declare function enforceMossFence(pluginId: string | null | undefined, relativePath: string): void;
//#endregion
export { type DownloadTracker, type MockBinaryConfig, type MockBinaryResult, type MockBrowserTracker, type MockCookieStorage, type MockDialogResult, type MockDialogTracker, type MockFile, type MockFilesystem, type MockSecretStorage, type MockTauriContext, type MockUrlConfig, type MockUrlResponse, type SetupMockTauriOptions, createDownloadTracker, createMockBinaryConfig, createMockBrowserTracker, createMockCookieStorage, createMockDialogTracker, createMockFilesystem, createMockSecretStorage, createMockUrlConfig, enforceMossFence, setupMockTauri };