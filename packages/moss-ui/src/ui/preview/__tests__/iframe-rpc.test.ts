import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { IframeRPC } from "../iframe-rpc";

describe("IframeRPC", () => {
  let iframe: HTMLIFrameElement;
  let rpc: IframeRPC;
  let postMessageSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    // Create mock iframe with contentWindow
    iframe = document.createElement("iframe");
    postMessageSpy = vi.fn();
    Object.defineProperty(iframe, "contentWindow", {
      value: { postMessage: postMessageSpy },
      writable: true,
    });
    document.body.appendChild(iframe);
    rpc = new IframeRPC(iframe);
  });

  afterEach(() => {
    rpc.destroy();
    document.body.removeChild(iframe);
    vi.clearAllMocks();
  });

  describe("constructor", () => {
    test("stores iframe reference", () => {
      expect(rpc).toBeDefined();
    });

    test("adds message event listener", () => {
      const addEventListenerSpy = vi.spyOn(window, "addEventListener");
      const newRpc = new IframeRPC(iframe);
      expect(addEventListenerSpy).toHaveBeenCalledWith(
        "message",
        expect.any(Function)
      );
      newRpc.destroy();
    });
  });

  describe("destroy", () => {
    test("removes message event listener", () => {
      const removeEventListenerSpy = vi.spyOn(window, "removeEventListener");
      rpc.destroy();
      expect(removeEventListenerSpy).toHaveBeenCalledWith(
        "message",
        expect.any(Function)
      );
    });
  });

  describe("call", () => {
    test("sends postMessage with correct format", () => {
      rpc.call("history.back");

      expect(postMessageSpy).toHaveBeenCalledWith(
        {
          type: "moss-rpc-call",
          id: "1",
          method: "history.back",
          args: [],
        },
        "*"
      );
    });

    test("increments call ID for each call", () => {
      rpc.call("method1");
      rpc.call("method2");

      expect(postMessageSpy).toHaveBeenNthCalledWith(
        1,
        expect.objectContaining({ id: "1" }),
        "*"
      );
      expect(postMessageSpy).toHaveBeenNthCalledWith(
        2,
        expect.objectContaining({ id: "2" }),
        "*"
      );
    });

    test("passes arguments correctly", () => {
      rpc.call("someMethod", "arg1", 42, true);

      expect(postMessageSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          args: ["arg1", 42, true],
        }),
        "*"
      );
    });

    test("resolves when response received", async () => {
      const promise = rpc.call<string>("location.href");

      // Simulate response from iframe
      window.dispatchEvent(
        new MessageEvent("message", {
          data: {
            type: "moss-rpc-result",
            id: "1",
            result: "http://localhost:8080/",
          },
        })
      );

      await expect(promise).resolves.toBe("http://localhost:8080/");
    });

    test("rejects when error response received", async () => {
      const promise = rpc.call("invalid.method");

      // Simulate error response
      window.dispatchEvent(
        new MessageEvent("message", {
          data: {
            type: "moss-rpc-result",
            id: "1",
            error: "Cannot access invalid",
          },
        })
      );

      await expect(promise).rejects.toThrow("Cannot access invalid");
    });

    test("rejects on timeout", async () => {
      vi.useFakeTimers();

      const promise = rpc.call("slow.method");

      // Fast-forward past timeout
      vi.advanceTimersByTime(5001);

      await expect(promise).rejects.toThrow("RPC timeout: slow.method");

      vi.useRealTimers();
    });

    test("ignores messages with wrong type", async () => {
      const promise = rpc.call<string>("test");

      // Send irrelevant message
      window.dispatchEvent(
        new MessageEvent("message", {
          data: { type: "other-message", id: "1", result: "ignored" },
        })
      );

      // Send correct response
      window.dispatchEvent(
        new MessageEvent("message", {
          data: { type: "moss-rpc-result", id: "1", result: "correct" },
        })
      );

      await expect(promise).resolves.toBe("correct");
    });

    test("ignores messages with wrong id", async () => {
      vi.useFakeTimers();

      const promise = rpc.call<string>("test");

      // Send message with wrong id
      window.dispatchEvent(
        new MessageEvent("message", {
          data: { type: "moss-rpc-result", id: "999", result: "wrong" },
        })
      );

      // Should still be pending, advance to timeout
      vi.advanceTimersByTime(5001);

      await expect(promise).rejects.toThrow("RPC timeout");

      vi.useRealTimers();
    });
  });

  describe("convenience methods", () => {
    test("historyBack calls history.back", () => {
      rpc.historyBack();
      expect(postMessageSpy).toHaveBeenCalledWith(
        expect.objectContaining({ method: "history.back" }),
        "*"
      );
    });

    test("historyForward calls history.forward", () => {
      rpc.historyForward();
      expect(postMessageSpy).toHaveBeenCalledWith(
        expect.objectContaining({ method: "history.forward" }),
        "*"
      );
    });

    test("getHistoryLength calls history.length", () => {
      rpc.getHistoryLength();
      expect(postMessageSpy).toHaveBeenCalledWith(
        expect.objectContaining({ method: "history.length" }),
        "*"
      );
    });

    test("getLocationHref calls location.href", () => {
      rpc.getLocationHref();
      expect(postMessageSpy).toHaveBeenCalledWith(
        expect.objectContaining({ method: "location.href" }),
        "*"
      );
    });
  });
});
