/**
 * Hook result types
 *
 * ## Architecture: Single Completion Path
 *
 * Plugins complete by returning a HookResult from their hook function.
 * The runtime handles sending the completion message to Rust.
 *
 * - **Completion** = `success` (flow control for moss)
 * - **Outcome UX** = `toast` (data describing what happened; moss renders it)
 *
 * Both travel in the same return value. A hook never raises its own outcome
 * toast imperatively — moss owns every status surface (see moss's
 * docs/reference/plugin-architecture-boundary.md), and an imperative toast
 * raised mid-hook cannot be reconciled with the surfaces moss is already
 * showing for the same operation.
 */

import type { DeploymentInfo } from "./context.js";

/**
 * Outcome notification described by a hook result.
 *
 * This is data about what happened, not a rendering instruction: moss owns
 * every status surface (per its plugin-architecture boundary) and maps
 * `outcome` to its own toast severity, timing, and suppression rules — e.g.
 * a surface that already shows the outcome (the first-publish wizard) can
 * swallow it entirely.
 *
 * @category Hooks
 */
export interface HookToast {
  /** What happened: "success" | "info" | "error" */
  outcome: "success" | "info" | "error";
  /** Short display text (e.g., "Live on Tor", "No changes to deploy") */
  title: string;
  /** Optional clickable URL (e.g., the deployed site URL) */
  url?: string | null;
}

/**
 * Standard result returned from hook execution
 *
 * ## Design Principles
 *
 * 1. **Single completion path**: Return value only, no explicit reporting
 * 2. **Flow control only**: `success` tells moss whether to continue
 * 3. **Outcome UX is data, not calls**: describe the outcome in `toast`;
 *    moss decides how (and whether) to present it. A hook must succeed with
 *    no UI attached at all — CLI and headless hosts run the same hooks.
 *
 * ## Usage Pattern
 *
 * ```typescript
 * async function deploy(context): Promise<HookResult> {
 *   // Do work...
 *
 *   // Return result; `toast` describes the outcome for moss to render
 *   return {
 *     success: true,
 *     deployment: {...},
 *     toast: { outcome: "success", title: "Deployed!", url },
 *   };
 * }
 * ```
 * @category Hooks
 */
export interface HookResult {
  /** Whether the operation succeeded */
  success: boolean;
  /** Detailed message for logs/debugging */
  message?: string;
  /** Outcome notification for moss to present (moss controls rendering) */
  toast?: HookToast | null;
  /** Deployment info (populated by deploy hooks) */
  deployment?: DeploymentInfo;
}
