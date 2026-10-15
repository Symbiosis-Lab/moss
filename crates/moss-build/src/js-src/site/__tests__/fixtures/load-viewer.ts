import { vi } from "vitest";

// The viewer binds to the document when it loads, so a second load in the same
// jsdom would stack a second set of listeners on the first. Record what each
// load binds and take it down before the next.
const bound: { type: string; fn: EventListenerOrEventListenerObject; opts?: unknown }[] = [];
const observers: MutationObserver[] = [];
let patched = false;

function patch(): void {
  if (patched) return;
  patched = true;
  // jsdom does not implement media playback.
  window.HTMLMediaElement.prototype.pause = () => {};
  const add = document.addEventListener.bind(document);
  document.addEventListener = ((type: string, fn: EventListenerOrEventListenerObject, opts?: unknown) => {
    bound.push({ type, fn, opts });
    add(type, fn, opts as AddEventListenerOptions);
  }) as typeof document.addEventListener;
  const Real = window.MutationObserver;
  window.MutationObserver = class extends Real {
    constructor(cb: MutationCallback) {
      super(cb);
      observers.push(this);
    }
  };
}

export function unloadViewer(): void {
  for (const { type, fn, opts } of bound.splice(0)) document.removeEventListener(type, fn, opts as EventListenerOptions);
  for (const o of observers.splice(0)) o.disconnect();
  document.body.style.overflow = "";
}

/** Put `html` in the body and run the shipped source of the viewer script against it. */
export async function loadViewer(html: string): Promise<void> {
  patch();
  unloadViewer();
  document.body.innerHTML = html;
  vi.resetModules();
  await import("../../fullscreen");
}
