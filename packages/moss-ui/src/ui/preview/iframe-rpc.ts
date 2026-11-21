// iframe RPC Client - Cross-origin communication with preview iframe
//
// This module provides a promise-based RPC client for communicating with
// the preview iframe via postMessage. The iframe runs an injected bridge
// script (iframe-bridge.js) that handles RPC calls dynamically.

/** How a request to update the page in place ended; `error` says why the caller must reload instead. */
export type MorphResult = { applied: true; title?: string } | { applied: false; error: string };

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
  private morphId = 0;
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

  /**
   * Ask the page to update itself in place, keeping scroll, focus and undo.
   * Settles on the bridge's answer to this request, or after 4 s without one:
   * its fetch, parse and morph of a large page fit well inside that.
   */
  morph(): Promise<MorphResult> {
    const target = this.iframe.contentWindow;
    if (!target) return Promise.resolve({ applied: false, error: "no page" });
    const gen = ++this.morphId;
    return new Promise((resolve) => {
      const done = (result: MorphResult) => {
        window.removeEventListener("message", onMessage);
        clearTimeout(timer);
        resolve(result);
      };
      const onMessage = (e: MessageEvent) => {
        const type = e.data?.type;
        if ((type !== "moss-morph-applied" && type !== "moss-morph-failed") || e.source !== target || e.data?.gen !== gen) return;
        if (type === "moss-morph-applied") return done({ applied: true, title: e.data?.title });
        // `changedScripts` names the scripts behind a `script-changed` verdict.
        const why = (e.data?.changedScripts as string[] | undefined)?.join(", ");
        done({ applied: false, error: `${e.data?.error ?? "unknown"}${why ? ` [${why}]` : ""}` });
      };
      window.addEventListener("message", onMessage);
      const timer = setTimeout(() => done({ applied: false, error: "timed out" }), 4000);
      target.postMessage({ type: "moss-morph", gen }, "*");
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
