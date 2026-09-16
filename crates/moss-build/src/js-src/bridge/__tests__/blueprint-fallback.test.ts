/**
 * Tests for the preview/editor-only missing-image placeholder upgrade
 * (frontend/bridge/blueprint-fallback.ts).
 *
 * Unlike iframe-bridge.ts (an IIFE with load-bearing side effects), this module
 * exports a plain installer, so we import and drive it directly against a jsdom
 * DOM. startBlueprintGrid is mocked so we can assert it is called with the
 * canvas host and that its cleanup runs on teardown / pause. jsdom has no
 * IntersectionObserver/ResizeObserver/matchMedia, which lets us exercise both
 * the "no IntersectionObserver → run unconditionally" path and, with a stub, the
 * multi-instance pause/resume path.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const { startBlueprintGridMock, cleanupSpy } = vi.hoisted(() => {
  const cleanupSpy = vi.fn();
  const startBlueprintGridMock = vi.fn(() => cleanupSpy);
  return { startBlueprintGridMock, cleanupSpy };
});

vi.mock("../blueprint-grid", () => ({
  startBlueprintGrid: startBlueprintGridMock,
}));

import { installBlueprintFallback } from "../blueprint-fallback";

/** Controllable IntersectionObserver stub (jsdom ships none). */
class MockIntersectionObserver {
  static instances: MockIntersectionObserver[] = [];
  private cb: IntersectionObserverCallback;
  elements: Element[] = [];
  constructor(cb: IntersectionObserverCallback) {
    this.cb = cb;
    MockIntersectionObserver.instances.push(this);
  }
  observe(el: Element): void {
    this.elements.push(el);
  }
  unobserve(el: Element): void {
    this.elements = this.elements.filter((e) => e !== el);
  }
  disconnect(): void {
    this.elements = [];
  }
  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
  /** Test helper: deliver an intersection state for every observed element. */
  emit(isIntersecting: boolean): void {
    const entries = this.elements.map(
      (target) => ({ isIntersecting, target }) as IntersectionObserverEntry,
    );
    this.cb(entries, this as unknown as IntersectionObserver);
  }
}

function makeCover(): { cover: HTMLElement; img: HTMLImageElement } {
  // Mirror the shell's post-error state: an <img> tagged `.moss-img-fallback`
  // with the static blueprint SVG data-URI, inside a `.moss-card-cover` box.
  const cover = document.createElement("div");
  cover.className = "moss-card-cover";
  const img = document.createElement("img");
  img.className = "moss-img-fallback";
  img.src =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E";
  cover.appendChild(img);
  document.body.appendChild(cover);
  return { cover, img };
}

describe("blueprint-fallback: installBlueprintFallback", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    startBlueprintGridMock.mockClear();
    cleanupSpy.mockClear();
    MockIntersectionObserver.instances = [];
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("upgrades a .moss-img-fallback to a blueprint canvas and starts the grid", () => {
    const { cover, img } = makeCover();

    // jsdom has no IntersectionObserver → module runs the grid unconditionally.
    const stop = installBlueprintFallback();

    const container = cover.querySelector<HTMLElement>(".moss-blueprint-fallback");
    expect(container).not.toBeNull();
    const canvas = container!.querySelector<HTMLCanvasElement>(
      "canvas#blueprint-canvas",
    );
    expect(canvas).not.toBeNull();
    // Grid started against the canvas host.
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);
    expect(startBlueprintGridMock).toHaveBeenCalledWith(container);
    // The <img> stays in place (box-definer + heal-anchor) but hidden.
    expect(img.isConnected).toBe(true);
    expect(img.style.opacity).toBe("0");
    // Overlay is absolutely positioned and does not swallow clicks meant for
    // the card link (canvas is pointer-events:none).
    expect(container!.style.position).toBe("absolute");
    expect(canvas!.style.pointerEvents).toBe("none");

    stop();
  });

  it("tears down the grid and removes the overlay on teardown", () => {
    const { cover } = makeCover();
    const stop = installBlueprintFallback();

    expect(cleanupSpy).not.toHaveBeenCalled();
    stop();

    expect(cleanupSpy).toHaveBeenCalledTimes(1);
    expect(cover.querySelector(".moss-blueprint-fallback")).toBeNull();
  });

  it("respects prefers-reduced-motion by leaving the static SVG untouched", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((q: string) => ({
        matches: q.includes("prefers-reduced-motion"),
        media: q,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    );
    // matchMedia is read off `window`; stub it there too for the module.
    (window as unknown as { matchMedia: unknown }).matchMedia =
      globalThis.matchMedia;

    const { cover, img } = makeCover();
    const stop = installBlueprintFallback();

    // No upgrade: no canvas, grid never started, img untouched.
    expect(cover.querySelector(".moss-blueprint-fallback")).toBeNull();
    expect(startBlueprintGridMock).not.toHaveBeenCalled();
    expect(img.style.opacity).toBe("");

    stop();
  });

  it("pauses off-screen instances and resumes them (IntersectionObserver)", () => {
    vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);

    makeCover();
    const stop = installBlueprintFallback();

    const io = MockIntersectionObserver.instances[0];
    expect(io).toBeDefined();
    // Not started until the observer reports the cover on-screen.
    expect(startBlueprintGridMock).not.toHaveBeenCalled();

    io.emit(true); // on-screen → start
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

    io.emit(false); // off-screen → pause (grid torn down)
    expect(cleanupSpy).toHaveBeenCalledTimes(1);

    io.emit(true); // back on-screen → restart
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(2);

    stop();
  });

  it("tears down a grid whose host is removed by a morph (no rAF leak)", async () => {
    const { cover } = makeCover();
    const stop = installBlueprintFallback();
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

    // Simulate a morph/navigation clobbering the covers (idiomorph removes our
    // injected overlay + the img). The MutationObserver must reap the instance.
    cover.remove();
    // MutationObserver callbacks are delivered as a microtask.
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));

    expect(cleanupSpy).toHaveBeenCalledTimes(1);

    stop();
  });

  it("upgrades an <img> that gains .moss-img-fallback after install", async () => {
    // Image present but not yet failed at install time.
    const cover = document.createElement("div");
    cover.className = "moss-card-cover";
    const img = document.createElement("img");
    cover.appendChild(img);
    document.body.appendChild(cover);

    const stop = installBlueprintFallback();
    expect(startBlueprintGridMock).not.toHaveBeenCalled();

    // The shell's error handler tags it later.
    img.classList.add("moss-img-fallback");
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));

    expect(cover.querySelector("canvas#blueprint-canvas")).not.toBeNull();
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

    stop();
  });
});
