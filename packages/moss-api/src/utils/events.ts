/**
 * Event utilities for plugin communication
 *
 * Provides a way for plugins and dialog windows to communicate
 * via Tauri's event system.
 */

/**
 * Tauri event listener interface
 */
interface TauriEventListen {
  (event: string, handler: (event: { payload: unknown }) => void): Promise<() => void>;
}

/**
 * Tauri event emit interface
 */
interface TauriEventEmit {
  (event: string, payload?: unknown): Promise<void>;
}

interface TauriEventWindow {
  __TAURI__?: {
    event?: {
      listen: TauriEventListen;
      emit: TauriEventEmit;
    };
  };
}

/**
 * Get Tauri event API
 * @internal
 */
function getTauriEvent(): { listen: TauriEventListen; emit: TauriEventEmit } {
  const w = window as unknown as TauriEventWindow;
  if (!w.__TAURI__?.event) {
    throw new Error("Tauri event API not available");
  }
  return w.__TAURI__.event;
}

/**
 * Check if Tauri event API is available
 * @category Events
 */
export function isEventApiAvailable(): boolean {
  const w = window as unknown as TauriEventWindow;
  return !!w.__TAURI__?.event;
}

/**
 * Emit an event to other parts of the application
 *
 * @param event - Event name (e.g., "repo-created", "dialog-result")
 * @param payload - Data to send with the event
 *
 * @example
 * ```typescript
 * // From dialog:
 * await emitEvent("repo-name-validated", { name: "my-repo", available: true });
 *
 * // From plugin:
 * await emitEvent("deployment-started", { url: "https://github.com/..." });
 * ```
 * @category Events
 */
export async function emitEvent(event: string, payload?: unknown): Promise<void> {
  await getTauriEvent().emit(event, payload);
}

/**
 * Listen for events from other parts of the application
 *
 * @param event - Event name to listen for
 * @param handler - Function to call when event is received
 * @returns Cleanup function to stop listening
 *
 * @example
 * ```typescript
 * const unlisten = await onEvent<{ name: string; available: boolean }>(
 *   "repo-name-validated",
 *   (data) => {
 *     console.log(`Repo ${data.name} is ${data.available ? "available" : "taken"}`);
 *   }
 * );
 *
 * // Later, to stop listening:
 * unlisten();
 * ```
 * @category Events
 */
export async function onEvent<T>(
  event: string,
  handler: (payload: T) => void
): Promise<() => void> {
  const unlisten = await getTauriEvent().listen(event, (e) => {
    handler(e.payload as T);
  });
  return unlisten;
}

