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
  /** Setup verdict (populated by the optional `check_setup` hook) */
  setup?: SetupVerdict | LegacySetupVerdict;
}

/**
 * One field of a {@link SetupForm} — the same vocabulary a manifest's
 * `settings[]` declares, minus `when` (in a stepped flow, the steps are the
 * conditionality). moss draws it; the plugin supplies no pixels.
 *
 * @category Hooks
 */
export interface SetupField {
  /** The key the submitted value arrives under in `SetupContext.values` */
  key: string;
  type: "string" | "number" | "boolean" | "secret";
  label?: string;
  /** One sentence under the field */
  description?: string;
  /** A suggestion moss pre-fills — a claimed-name candidate, say */
  default?: unknown;
  /** Present on a `string` field, it closes the value set: moss draws a select */
  options?: { value: string; label: string; description?: string }[];
  placeholder?: string;
  help_url?: string;
  /** Checked per keystroke; requires `pattern_message` */
  pattern?: string;
  pattern_message?: string;
}

/**
 * A {@link SetupBlocker}'s form. A zero-field form is a button, and the
 * blocker's `message` is what the person reads before pressing it.
 *
 * @category Hooks
 */
export interface SetupForm {
  fields: SetupField[];
  /** Submit button label */
  submit: string;
}

/**
 * One reason the contribution is not ready, and what to do about it.
 *
 * `id` is the routing verb: when the user submits the form, `check_setup`
 * runs again with this id as `SetupContext.action`. Ids beginning `moss:`
 * are reserved for moss's own blockers (a failed manifest `need`) and are
 * never dispatched to your hook.
 *
 * @category Hooks
 */
export interface SetupBlocker {
  id: string;
  /** What the person reads. Omit only when a form says it all. */
  message?: string;
  /** Present when there is something to submit */
  form?: SetupForm;
  /**
   * Per-field errors keyed by field `key`. Returning the SAME blocker again
   * with these marks the verdict a re-ask: moss re-shows the form with the
   * errors and does NOT persist the submitted values.
   */
  field_errors?: Record<string, string>;
}

/**
 * The `check_setup` verdict: `ready`, or `blocked` with blockers. There is no
 * third verdict — a hook that cannot tell answers `blocked` with a message
 * that says so and a zero-field retry form.
 *
 * On any answering verdict that is not a `field_errors` re-ask, moss persists
 * each submitted value whose key matches a declared setting: a `secret` into
 * the keystore, anything else into your config. Values your flow derived
 * (rather than the user typed) belong in your own state file.
 *
 * @category Hooks
 */
export type SetupVerdict =
  | { status: "ready" }
  | { status: "blocked"; blockers: SetupBlocker[] };

/**
 * @deprecated The pre-contract verdict shape. moss still accepts it — each
 * need's action becomes a blocker whose zero-field form carries the action's
 * label, and `consent` folds into the message — but new code answers with
 * {@link SetupVerdict}.
 *
 * @category Hooks
 */
export interface LegacySetupVerdict {
  ready: boolean;
  needs?: {
    id: string;
    message: string;
    actions?: { id: string; label: string; consent?: string }[];
  }[];
}
