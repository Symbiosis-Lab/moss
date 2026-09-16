/**
 * Testing utilities for moss plugins
 *
 * This module provides mock implementations of Tauri IPC commands,
 * enabling integration testing of plugins without a running Tauri app.
 *
 * @example
 * ```typescript
 * import { setupMockTauri, type MockTauriContext } from "@symbiosis-lab/moss-api/testing";
 * import { readFile, writeFile } from "@symbiosis-lab/moss-api";
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
 *   it("reads and writes files", async () => {
 *     // Seed a file at the mock project root (default: /test/project)
 *     ctx.filesystem.setFile("/test/project/input.md", "# Hello");
 *
 *     // moss-api file functions resolve paths against the project context
 *     const content = await readFile("input.md");
 *     await writeFile("output.md", content.toUpperCase());
 *
 *     // Verify
 *     expect(ctx.filesystem.getFile("/test/project/output.md")?.content).toBe("# HELLO");
 *   });
 * });
 * ```
 *
 * @packageDocumentation
 */

export {
  // Main setup function
  setupMockTauri,
  // Factory functions
  createMockFilesystem,
  createDownloadTracker,
  createMockUrlConfig,
  createMockBinaryConfig,
  createMockCookieStorage,
  createMockBrowserTracker,
  createMockDialogTracker,
  // Types
  type SetupMockTauriOptions,
  type MockTauriContext,
  type MockFilesystem,
  type MockFile,
  type DownloadTracker,
  type MockUrlConfig,
  type MockUrlResponse,
  type MockBinaryConfig,
  type MockBinaryResult,
  type MockCookieStorage,
  type MockBrowserTracker,
  type MockDialogTracker,
  type MockDialogResult,
} from "./mock-tauri.js";
