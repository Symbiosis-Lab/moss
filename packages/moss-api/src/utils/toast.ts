/**
 * Toast notification utilities for plugins
 *
 * Allows plugins to display toast notifications in the main moss UI
 * through Tauri's event system.
 *
 * ## Design Principles
 *
 * 1. **Plugin Full Control**: Plugins specify exactly what appears in toast
 * 2. **Minimal Assumptions**: moss just renders what plugin says
 * 3. **Direct Path**: Plugin → showToast() → Frontend renders
 * 4. **Separation of Concerns**: Toast (UX) is separate from HookResult (flow control)
 */

import { emitEvent } from "./events.js";

// =============================================================================
// Types
// =============================================================================

/**
 * Toast variant determines the visual style
 * @category Toast
 */
export type ToastVariant = "success" | "error" | "info" | "warning";

/**
 * Action button in a toast
 * @category Toast
 */
export interface ToastAction {
  /** Button text, e.g., "View site" */
  label: string;
  /** URL to open in system browser when clicked */
  url: string;
}

/**
 * Options for showing a toast notification
 *
 * @example
 * ```typescript
 * // Simple success toast
 * showToast({ message: "Saved!" });
 *
 * // Toast with action
 * showToast({
 *   message: "Deployed!",
 *   variant: "success",
 *   actions: [{ label: "View site", url: "https://..." }],
 *   duration: 8000
 * });
 *
 * // Persistent progress toast
 * showToast({
 *   message: "Deploying...",
 *   variant: "info",
 *   id: "deploy-progress",
 *   persistent: true,
 *   dismissible: false
 * });
 * ```
 * @category Toast
 */
export interface ToastOptions {
  /** The message to display (required) */
  message: string;

  /** Visual style hint (default: "info") */
  variant?: ToastVariant;

  /** Action buttons (opens URL in system browser) */
  actions?: ToastAction[];

  /** Duration in ms before auto-dismiss (bounded by frontend to 2000-30000) */
  duration?: number;

  /** If true, toast stays until user dismisses or plugin calls dismissToast() */
  persistent?: boolean;

  /** If false, hide the X close button (default: true) */
  dismissible?: boolean;

  /** Unique ID for update-in-place pattern (used with updateToast/dismissToast) */
  id?: string;
}

/**
 * @deprecated Use ToastVariant instead
 * @category Toast
 */
export type ToastType = ToastVariant;

// =============================================================================
// Event Names
// =============================================================================

/**
 * Event for showing a new toast
 * @category Toast
 */
export const TOAST_EVENT = "show-toast";

/**
 * Event for updating an existing toast by ID
 * @category Toast
 */
export const TOAST_UPDATE_EVENT = "show-toast-update";

/**
 * Event for dismissing a toast by ID
 * @category Toast
 */
export const TOAST_DISMISS_EVENT = "show-toast-dismiss";

// =============================================================================
// Functions
// =============================================================================

/**
 * Show a toast notification in the main moss UI
 *
 * @param options - Toast options or simple message string
 *
 * @example
 * ```typescript
 * // Object form (recommended)
 * await showToast({
 *   message: "Deployed!",
 *   variant: "success",
 *   actions: [{ label: "View site", url: "https://..." }],
 *   duration: 8000
 * });
 *
 * // Simple form (for quick messages)
 * await showToast("Processing...");
 * ```
 * @category Toast
 */
export async function showToast(options: ToastOptions | string): Promise<void> {
  const opts: ToastOptions = typeof options === "string"
    ? { message: options }
    : options;

  await emitEvent(TOAST_EVENT, opts);
}


/**
 * Dismiss a toast by ID
 *
 * @param id - The toast ID to dismiss
 *
 * @example
 * ```typescript
 * // Show a toast
 * await showToast({
 *   message: "Processing...",
 *   id: "process-toast",
 *   persistent: true
 * });
 *
 * // Later, dismiss it
 * await dismissToast("process-toast");
 * ```
 * @category Toast
 */
export async function dismissToast(id: string): Promise<void> {
  await emitEvent(TOAST_DISMISS_EVENT, { id });
}
