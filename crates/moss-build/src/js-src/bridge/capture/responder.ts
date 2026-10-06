/**
 * capture/responder.ts - answers a hosting window's request for a bitmap of
 * this page.
 *
 * Parent -> guest: `{ type: "moss-capture-request", id }`.
 * Guest -> parent: `{ type: "moss-capture-response", id, ok: true, width,
 * height, bitmap }` with `bitmap` a transferred ImageBitmap, or
 * `{ type: "moss-capture-response", id, ok: false, error }`.
 *
 * Only the parent window is answered, and no work happens until it asks.
 */

import { capturePage } from "./page";

export type CaptureFn = () => Promise<HTMLCanvasElement>;

export function installCaptureResponder(win: Window = window, capture: CaptureFn = capturePage): void {
  win.addEventListener("message", (e: MessageEvent) => {
    if (e.source !== win.parent || e.data?.type !== "moss-capture-request") return;
    const id = e.data.id;
    const reply = (msg: Record<string, unknown>, transfer: Transferable[] = []) => {
      try {
        win.parent.postMessage({ type: "moss-capture-response", id, ...msg }, "*", transfer);
      } catch {
        /* the parent went away; nothing to tell */
      }
    };
    Promise.resolve()
      .then(capture)
      .then((canvas) => createImageBitmap(canvas).then((bitmap) => {
        reply({ ok: true, width: canvas.width, height: canvas.height, bitmap }, [bitmap]);
      }))
      .catch((err: unknown) => {
        reply({ ok: false, error: err instanceof Error ? err.message : String(err) });
      });
  });
}
