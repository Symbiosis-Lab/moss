/**
 * Tests for the preview/editor-only missing-image placeholder upgrade
 * (js-src/bridge/blueprint-fallback.ts).
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

vi.mock("@symbiosis-lab/moss-blueprint-grid", () => ({
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
  // with its empty stand-in data-URI, inside a `.moss-card-cover` box.
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
      "canvas",
    );
    expect(canvas).not.toBeNull();
    // Grid started against the canvas host.
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);
    expect(startBlueprintGridMock).toHaveBeenCalledWith(container, { canvas });
    // The <img> stays in place and visible: it defines the box and carries the
    // edge, and its stand-in is transparent, so there is nothing to hide.
    expect(img.isConnected).toBe(true);
    expect(img.style.opacity).toBe("");
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

  it("mounts the live grid under prefers-reduced-motion too; the grid itself holds still", () => {
    (window as unknown as { matchMedia: unknown }).matchMedia = vi.fn(
      (q: string) => ({ matches: q.includes("prefers-reduced-motion"), media: q }),
    );
    const { cover } = makeCover();
    const stop = installBlueprintFallback();
    delete (window as unknown as { matchMedia?: unknown }).matchMedia;

    // The grid package paints one still frame for reduced motion, so the
    // waiting picture looks the same, at rest, and never falls back to a
    // stretched static pattern.
    expect(cover.querySelector(".moss-blueprint-fallback canvas")).not.toBeNull();
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);
    stop();
  });

  it("upgrades a failed <video> the same way", () => {
    const box = document.createElement("div");
    const video = document.createElement("video");
    video.className = "moss-img-fallback";
    box.appendChild(video);
    document.body.appendChild(box);
    const stop = installBlueprintFallback();
    expect(box.querySelector(".moss-blueprint-fallback canvas")).not.toBeNull();
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);
    stop();
  });

  it("keeps the grid until the restored picture has loaded, so the box never shows empty", async () => {
    const { cover, img } = makeCover();
    const stop = installBlueprintFallback();
    Object.defineProperty(img, "complete", { value: false, configurable: true });
    img.classList.remove("moss-img-fallback");
    await new Promise((r) => setTimeout(r, 0));
    expect(cover.querySelector(".moss-blueprint-fallback")).not.toBeNull();
    Object.defineProperty(img, "complete", { value: true, configurable: true });
    img.dispatchEvent(new Event("load"));
    expect(cover.querySelector(".moss-blueprint-fallback")).toBeNull();
    expect(cleanupSpy).toHaveBeenCalledTimes(1);
    stop();
  });

  it("drops the grid at once when the picture is already there", async () => {
    const { cover, img } = makeCover();
    const stop = installBlueprintFallback();
    Object.defineProperty(img, "complete", { value: true, configurable: true });
    img.classList.remove("moss-img-fallback");
    await new Promise((r) => setTimeout(r, 0));
    expect(cover.querySelector(".moss-blueprint-fallback")).toBeNull();
    expect(cleanupSpy).toHaveBeenCalledTimes(1);
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

    expect(cover.querySelector("canvas")).not.toBeNull();
    expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

    stop();
  });

  describe("pending derived text", () => {
    function makePending(cls = "moss-derived moss-derived--pending"): HTMLElement {
      const block = document.createElement("div");
      block.className = cls;
      block.setAttribute("role", "status");
      block.innerHTML = '<span>Reading the text of this file…</span>';
      document.body.appendChild(block);
      return block;
    }

    it("mounts a canvas host inside a pending block and starts the grid", () => {
      const block = makePending();
      const stop = installBlueprintFallback();

      const host = block.querySelector<HTMLElement>(".moss-blueprint-fallback");
      expect(host).not.toBeNull();
      expect(host!.getAttribute("aria-hidden")).toBe("true");
      expect(host!.querySelector("canvas")).not.toBeNull();
      expect(startBlueprintGridMock).toHaveBeenCalledWith(host, { canvas: host!.querySelector("canvas") });
      expect(block.style.position).toBe("relative");
      // The status role and its hidden text survive.
      expect(block.getAttribute("role")).toBe("status");
      expect(block.querySelector("span")?.textContent).toBe("Reading the text of this file…");

      stop();
      expect(block.querySelector(".moss-blueprint-fallback")).toBeNull();
      expect(block.style.position).toBe("");
    });

    it("animates only while the block is on-screen", () => {
      vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
      makePending();
      const stop = installBlueprintFallback();
      const io = MockIntersectionObserver.instances[0];

      expect(startBlueprintGridMock).not.toHaveBeenCalled();
      io.emit(true);
      expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);
      io.emit(false);
      expect(cleanupSpy).toHaveBeenCalledTimes(1);

      stop();
    });

    it("tears the grid down when the block is removed", async () => {
      const block = makePending();
      const stop = installBlueprintFallback();
      expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

      block.remove();
      await new Promise((r) => setTimeout(r, 0));

      expect(cleanupSpy).toHaveBeenCalledTimes(1);
      stop();
    });

    it("upgrades a block inserted after install, and drops one that stops being pending", async () => {
      const stop = installBlueprintFallback();
      const block = makePending();
      await new Promise((r) => setTimeout(r, 0));
      expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

      block.className = "moss-derived";
      await new Promise((r) => setTimeout(r, 0));
      expect(cleanupSpy).toHaveBeenCalledTimes(1);
      expect(block.querySelector(".moss-blueprint-fallback")).toBeNull();

      stop();
    });

    it("re-mounts the grid when a morph of the same still-pending block removes the host", async () => {
      const block = makePending();
      const stop = installBlueprintFallback();
      expect(startBlueprintGridMock).toHaveBeenCalledTimes(1);

      // idiomorph keeps the block and prunes the host it does not know.
      block.querySelector(".moss-blueprint-fallback")!.remove();
      await new Promise((r) => setTimeout(r, 0));

      expect(block.querySelector(".moss-blueprint-fallback canvas")).not.toBeNull();
      expect(startBlueprintGridMock).toHaveBeenCalledTimes(2);
      stop();
    });

    it("leaves ready and skipped derived blocks alone", () => {
      const ready = makePending("moss-derived");
      const skipped = makePending("moss-derived moss-derived--skipped");
      const stop = installBlueprintFallback();

      expect(ready.querySelector(".moss-blueprint-fallback")).toBeNull();
      expect(skipped.querySelector(".moss-blueprint-fallback")).toBeNull();
      expect(startBlueprintGridMock).not.toHaveBeenCalled();
      stop();
    });
  });
});
