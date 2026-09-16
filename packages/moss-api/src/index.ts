/**
 * moss Plugin SDK
 *
 * Shared types and utilities for moss plugins.
 *
 * @example
 * ```typescript
 * import type { DeployContext, HookResult } from "@symbiosis-lab/moss-api";
 * import { reportProgress } from "@symbiosis-lab/moss-api";
 *
 * const MyPlugin = {
 *   async on_deploy(context: DeployContext): Promise<HookResult> {
 *     await reportProgress("deploying", 0, 100, "Initializing");
 *     // ... deployment logic
 *     return { success: true, message: "Deployed successfully" };
 *   }
 * };
 * ```
 *
 * @packageDocumentation
 */

// Types
export * from "./types/index.js";

// Utilities
export * from "./utils/index.js";
