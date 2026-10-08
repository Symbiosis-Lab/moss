/**
 * The message protocol of capture/responder.ts: who may ask, what comes back,
 * and that a failed capture is an answer rather than silence or a throw. The
 * pixels themselves are checked in a real browser (playwright/bridge-capture).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { installCaptureResponder } from "../capture/responder";

const bitmap = { close() {} } as unknown as ImageBitmap;
const canvas = { width: 320, height: 200 } as HTMLCanvasElement;

function ask(source: MessageEventSource | null, data: unknown = { type: "moss-capture-request", id: 7 }) {
  window.dispatchEvent(new MessageEvent("message", { data, source }));
}
/** Let the capture promise chain settle. */
const settle = () => new Promise((r) => setTimeout(r, 0));

describe("capture responder", () => {
  let post: ReturnType<typeof vi.spyOn>;
  const capture = vi.fn();

  beforeEach(() => {
    // jsdom has no createImageBitmap; at top level window.parent is window.
    vi.stubGlobal("createImageBitmap", vi.fn(async () => bitmap));
    post = vi.spyOn(window.parent, "postMessage").mockImplementation(() => {});
    capture.mockReset().mockResolvedValue(canvas);
    installCaptureResponder(window, capture);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    post.mockRestore();
  });

  it("answers the parent with the bitmap, transferred, and the size", async () => {
    ask(window.parent);
    await settle();
    expect(post).toHaveBeenCalledTimes(1);
    const [msg, target, transfer] = post.mock.calls[0] as unknown[];
    expect(msg).toEqual({ type: "moss-capture-response", id: 7, ok: true, width: 320, height: 200, bitmap });
    expect(target).toBe("*");
    expect(transfer).toEqual([bitmap]);
  });

  it("ignores a request that did not come from the parent window", async () => {
    const other = document.createElement("iframe");
    document.body.appendChild(other);
    ask(other.contentWindow);
    ask(null);
    await settle();
    expect(capture).not.toHaveBeenCalled();
    expect(post).not.toHaveBeenCalled();
    other.remove();
  });

  it("does no work for other messages", async () => {
    ask(window.parent, { type: "moss-iframe-scroll-to", scrollTop: 0 });
    ask(window.parent, "capture");
    await settle();
    expect(capture).not.toHaveBeenCalled();
  });

  it("reports a failed capture as ok:false with the reason, under the same id", async () => {
    capture.mockRejectedValue(new Error("scene raster timed out"));
    ask(window.parent);
    await settle();
    expect(post.mock.calls[0][0]).toEqual({ type: "moss-capture-response", id: 7, ok: false, error: "scene raster timed out" });
  });

  it("reports a capture that throws synchronously instead of throwing into the page", async () => {
    capture.mockImplementation(() => { throw new Error("boom"); });
    expect(() => ask(window.parent)).not.toThrow();
    await settle();
    expect(post.mock.calls[0][0]).toMatchObject({ ok: false, error: "boom", id: 7 });
  });
});
