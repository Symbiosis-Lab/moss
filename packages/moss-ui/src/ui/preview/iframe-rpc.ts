// iframe RPC Client - Cross-origin communication with preview iframe
//
// This module provides a promise-based RPC client for communicating with
// the preview iframe via postMessage. The iframe runs an injected bridge
// script (iframe-bridge.js) that handles RPC calls dynamically.

type PendingCall = {
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
};

/**
 * RPC client for cross-origin iframe communication
 *
 * Sends RPC calls to the iframe's bridge script and handles responses.
 * All calls are promise-based with timeout handling.
 */
export class IframeRPC {
  private iframe: HTMLIFrameElement;
  private pending = new Map<string, PendingCall>();
  private callId = 0;
  private boundHandleMessage: (event: MessageEvent) => void;

  constructor(iframe: HTMLIFrameElement) {
    this.iframe = iframe;
    this.boundHandleMessage = this.handleMessage.bind(this);
    window.addEventListener("message", this.boundHandleMessage);
  }

  /**
   * Clean up event listener when no longer needed
   */
  destroy(): void {
    window.removeEventListener("message", this.boundHandleMessage);
    this.pending.clear();
  }

  private handleMessage(event: MessageEvent): void {
    if (event.data?.type !== "moss-rpc-result") return;

    const { id, result, error } = event.data;
    const pending = this.pending.get(id);

    if (pending) {
      this.pending.delete(id);
      if (error) {
        pending.reject(new Error(error));
      } else {
        pending.resolve(result);
      }
    }
  }

  /**
   * Make an RPC call to the iframe
   *
   * @param method - The method name to call (e.g., 'history.back')
   * @param args - Optional arguments to pass to the method
   * @returns Promise that resolves with the result or rejects on error/timeout
   */
  call<T = unknown>(method: string, ...args: unknown[]): Promise<T> {
    return new Promise((resolve, reject) => {
      const id = String(++this.callId);
      this.pending.set(id, {
        resolve: resolve as (result: unknown) => void,
        reject,
      });

      this.iframe.contentWindow?.postMessage(
        {
          type: "moss-rpc-call",
          id,
          method,
          args,
        },
        "*"
      );

      // Timeout after 5 seconds
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`RPC timeout: ${method}`));
        }
      }, 5000);
    });
  }

  // ============================================================
  // Convenience methods for common operations
  // ============================================================

  /**
   * Navigate back in iframe history
   */
  historyBack(): Promise<boolean> {
    return this.call<boolean>("history.back");
  }

  /**
   * Navigate forward in iframe history
   */
  historyForward(): Promise<boolean> {
    return this.call<boolean>("history.forward");
  }

  /**
   * Get the browser's history.length value
   */
  getHistoryLength(): Promise<number> {
    return this.call<number>("history.length");
  }

  /**
   * Get the current location.href from the iframe
   */
  getLocationHref(): Promise<string> {
    return this.call<string>("location.href");
  }
}
