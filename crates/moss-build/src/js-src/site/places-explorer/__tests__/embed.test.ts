/**
 * Tests for embed.ts's two halves: the host page's own lazy hydration
 * (`initPlaceEmbeds`) and the iframe content's own embed-mode switch
 * (`attachEmbedModeIfRequested`), plus the chip seam (`setEmbedScope`).
 * `setupImmersiveIframe`'s own wrapping/controls are covered by
 * immersive-mode.test.ts; here it is only asserted that embed.ts calls it
 * on the still-`src`-less iframe, before the one real navigation — never
 * after, which would re-navigate an already-loaded iframe a second time.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { attachEmbedModeIfRequested, initPlaceEmbeds, setEmbedScope } from "../embed";
import type { PlacesMapController } from "../map";

class FakeIntersectionObserver {
  static instances: FakeIntersectionObserver[] = [];
  callback: IntersectionObserverCallback;
  observe = vi.fn();
  disconnect = vi.fn();
  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback;
    FakeIntersectionObserver.instances.push(this);
  }
  trigger(target: Element): void {
    this.callback([{ target, isIntersecting: true } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
  }
}

function poster(hydrateUrl = "/places/?place=lisbon&embed=1"): HTMLElement {
  document.body.innerHTML = `<figure class="moss-place-map" data-moss-place-embed data-hydrate-url="${hydrateUrl}" role="img" aria-label="Map of Lisbon, city precision"><svg data-static-floor aria-hidden="true"></svg></figure>`;
  return document.querySelector<HTMLElement>("[data-moss-place-embed]")!;
}

beforeEach(() => {
  FakeIntersectionObserver.instances = [];
  vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
  vi.useFakeTimers();
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("initPlaceEmbeds — host page lazy hydration", () => {
  test("creates no iframe until the poster nears the viewport", () => {
    const el = poster();
    initPlaceEmbeds();
    expect(el.querySelector("iframe")).toBeNull();
    expect(FakeIntersectionObserver.instances).toHaveLength(1);
  });

  test("creates the iframe, pointed at the poster's own data-hydrate-url, once it nears the viewport", () => {
    const el = poster("/places/?place=lisbon&embed=1");
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe")!;
    expect(iframe).not.toBeNull();
    expect(iframe.getAttribute("src")).toBe("/places/?place=lisbon&embed=1");
  });

  test("the hydrated iframe carries no open-in-new-tab control — only the expand/collapse control", () => {
    const el = poster("/places/?place=lisbon&embed=1");
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe") as HTMLIFrameElement;
    expect(iframe.dataset.openUrl).toBeUndefined();
    expect(el.querySelector(".immersive-new-window-btn")).toBeNull();
    expect(el.querySelector(".immersive-fullscreen-btn")).not.toBeNull();
  });

  test("gives the hydrated iframe a title built from the poster's own data-embed-name, through this page's own locale copy", () => {
    document.documentElement.lang = "en";
    document.body.innerHTML =
      '<figure class="moss-place-map" data-moss-place-embed data-hydrate-url="/places/?place=places/kyoto&embed=1" data-embed-name="Kyoto"><svg data-static-floor></svg></figure>';
    const el = document.querySelector<HTMLElement>("[data-moss-place-embed]")!;
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe") as HTMLIFrameElement;
    expect(iframe.title).toBe("Map: Kyoto");
  });

  test("a poster with no data-embed-name leaves the iframe's title unset rather than a blank/broken one", () => {
    const el = poster();
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe") as HTMLIFrameElement;
    expect(iframe.title).toBe("");
  });

  test("on a slow/Save-Data connection, hydration waits for a tap instead of the viewport", () => {
    vi.stubGlobal("navigator", { connection: { saveData: true } });
    const el = poster();
    initPlaceEmbeds();
    expect(FakeIntersectionObserver.instances).toHaveLength(0);
    expect(el.querySelector("iframe")).toBeNull();
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(el.querySelector("iframe")).not.toBeNull();
  });

  test("cross-fades the iframe in only once it posts the ready message from the right source", () => {
    const el = poster();
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe") as HTMLIFrameElement;
    expect(iframe.classList.contains("moss-places-embed-frame--settled")).toBe(false);

    // A message from some other source must not settle it.
    window.dispatchEvent(new MessageEvent("message", { data: { type: "moss-places-embed-ready" }, origin: location.origin, source: window }));
    vi.advanceTimersByTime(50); // flush the rAF polyfill, if any is pending
    expect(iframe.classList.contains("moss-places-embed-frame--settled")).toBe(false);

    window.dispatchEvent(new MessageEvent("message", { data: { type: "moss-places-embed-ready" }, origin: location.origin, source: iframe.contentWindow }));
    vi.advanceTimersByTime(50);
    expect(iframe.classList.contains("moss-places-embed-frame--settled")).toBe(true);
    expect(el.getAttribute("data-moss-place-embed-ready")).toBe("ready");
  });

  test("hands the accessible description off to the live map once settled: the figure's own role/label (the no-JS image description) is dropped, and the static floor is made inert — never announced or focusable alongside the iframe's own (titled) content", () => {
    const el = poster();
    const staticFloor = el.querySelector("[data-static-floor]")!;
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    const iframe = el.querySelector("iframe") as HTMLIFrameElement;
    expect(el.getAttribute("role")).toBe("img");
    expect(el.hasAttribute("aria-label")).toBe(true);
    expect(staticFloor.hasAttribute("inert")).toBe(false);

    window.dispatchEvent(new MessageEvent("message", { data: { type: "moss-places-embed-ready" }, origin: location.origin, source: iframe.contentWindow }));
    vi.advanceTimersByTime(50);

    expect(el.hasAttribute("role")).toBe(false);
    expect(el.hasAttribute("aria-label")).toBe(false);
    expect(staticFloor.hasAttribute("inert")).toBe(true);
  });

  test("a ready message that never arrives removes the iframe and leaves the poster untouched", () => {
    const el = poster();
    const originalHtml = el.innerHTML;
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    expect(el.querySelector("iframe")).not.toBeNull();

    vi.advanceTimersByTime(8001);
    expect(el.querySelector("iframe")).toBeNull();
    expect(el.innerHTML).toBe(originalHtml);
  });

  test("attaches the iframe to the DOM before setting its src (WebKit double-navigates a src set while still detached)", () => {
    const descriptor = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "src")!;
    let connectedWhenSrcSet: boolean | null = null;
    Object.defineProperty(HTMLIFrameElement.prototype, "src", {
      configurable: true,
      get: descriptor.get,
      set(this: HTMLIFrameElement, value: string) {
        connectedWhenSrcSet = this.isConnected;
        descriptor.set!.call(this, value);
      },
    });
    try {
      const el = poster();
      initPlaceEmbeds();
      FakeIntersectionObserver.instances[0]!.trigger(el);
    } finally {
      Object.defineProperty(HTMLIFrameElement.prototype, "src", descriptor);
    }
    expect(connectedWhenSrcSet).toBe(true);
  });

  test("reaches its final DOM position (inside the immersive wrapper) before src is set, so the one real navigation happens only once — re-parenting an iframe that already navigated double-boots it in every engine", () => {
    const descriptor = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "src")!;
    let parentClassWhenSrcSet: string | null = null;
    Object.defineProperty(HTMLIFrameElement.prototype, "src", {
      configurable: true,
      get: descriptor.get,
      set(this: HTMLIFrameElement, value: string) {
        parentClassWhenSrcSet = this.parentElement?.className ?? null;
        descriptor.set!.call(this, value);
      },
    });
    try {
      const el = poster();
      initPlaceEmbeds();
      FakeIntersectionObserver.instances[0]!.trigger(el);
    } finally {
      Object.defineProperty(HTMLIFrameElement.prototype, "src", descriptor);
    }
    expect(parentClassWhenSrcSet).toContain("immersive-iframe-wrapper");
  });

  test("a ready message that never arrives removes the whole immersive wrapper, not just the bare iframe, leaving the poster exactly as it was", () => {
    const el = poster();
    const originalHtml = el.innerHTML;
    initPlaceEmbeds();
    FakeIntersectionObserver.instances[0]!.trigger(el);
    expect(el.querySelector(".immersive-iframe-wrapper")).not.toBeNull();

    vi.advanceTimersByTime(8001);
    expect(el.querySelector("iframe")).toBeNull();
    expect(el.querySelector(".immersive-iframe-wrapper")).toBeNull();
    expect(el.innerHTML).toBe(originalHtml);
  });

  test("a poster with no data-hydrate-url is left alone", () => {
    document.body.innerHTML = '<figure class="moss-place-map" data-moss-place-embed><svg></svg></figure>';
    initPlaceEmbeds();
    expect(FakeIntersectionObserver.instances).toHaveLength(0);
  });

  test("re-running initPlaceEmbeds on the same poster does not double-bind it", () => {
    const el = poster();
    initPlaceEmbeds();
    initPlaceEmbeds();
    expect(FakeIntersectionObserver.instances).toHaveLength(1);
    FakeIntersectionObserver.instances[0]!.trigger(el);
    expect(el.querySelectorAll("iframe")).toHaveLength(1);
  });
});

function fakeController(): PlacesMapController {
  return {
    setScope: vi.fn(),
    setCooperativeGestures: vi.fn(),
    refitScopeIfClipped: vi.fn(),
    setRowExclusion: vi.fn(),
    viewportEl: document.createElement("div"),
  };
}

describe("attachEmbedModeIfRequested — iframe content side", () => {
  afterEach(() => {
    history.replaceState(null, "", "/places/");
    document.documentElement.removeAttribute("data-moss-embed");
  });

  test("is a no-op when the page's own URL carries no embed=1", () => {
    history.replaceState(null, "", "/places/?place=lisbon");
    const controller = fakeController();
    const figure = document.createElement("figure");
    attachEmbedModeIfRequested(controller, figure);
    expect(controller.setCooperativeGestures).not.toHaveBeenCalled();
    expect(figure.hasAttribute("data-moss-places-embed-mode")).toBe(false);
    expect(document.documentElement.hasAttribute("data-moss-embed")).toBe(false);
  });

  test("a place-scoped embed enables cooperative gestures without touching scope (place already round-tripped at boot)", () => {
    history.replaceState(null, "", "/places/?place=lisbon&embed=1");
    const controller = fakeController();
    const figure = document.createElement("figure");
    attachEmbedModeIfRequested(controller, figure);
    expect(controller.setCooperativeGestures).toHaveBeenCalledWith(true);
    expect(figure.getAttribute("data-moss-places-embed-mode")).toBe("collapsed");
    expect(controller.setScope).not.toHaveBeenCalled();
    expect(controller.setRowExclusion).not.toHaveBeenCalled();
  });

  test("marks the document so this page's own chrome (header/nav/footer) hides itself — the embed is never meant to show it", () => {
    history.replaceState(null, "", "/places/?place=lisbon&embed=1");
    attachEmbedModeIfRequested(fakeController(), document.createElement("figure"));
    expect(document.documentElement.hasAttribute("data-moss-embed")).toBe(true);
  });

  test("an article-scoped embed (the locator) scopes to that work and excludes it from the row", () => {
    history.replaceState(null, "", "/places/?article=w1&embed=1");
    const controller = fakeController();
    attachEmbedModeIfRequested(controller, document.createElement("figure"));
    expect(controller.setScope).toHaveBeenCalledWith({ kind: "article", id: "w1" });
    expect(controller.setRowExclusion).toHaveBeenCalledWith("w1");
  });

  test("posts the ready message to the parent frame", () => {
    history.replaceState(null, "", "/places/?place=lisbon&embed=1");
    const postMessage = vi.spyOn(window.parent, "postMessage");
    attachEmbedModeIfRequested(fakeController(), document.createElement("figure"));
    expect(postMessage).toHaveBeenCalledWith({ type: "moss-places-embed-ready" }, location.origin);
  });

  test("an expand message switches gesture mode, the figure's own attribute, and re-fits the camera", () => {
    history.replaceState(null, "", "/places/?place=lisbon&embed=1");
    const controller = fakeController();
    const figure = document.createElement("figure");
    attachEmbedModeIfRequested(controller, figure);
    (controller.setCooperativeGestures as any).mockClear();

    window.dispatchEvent(new MessageEvent("message", { data: { type: "moss-places-embed-mode", expanded: true }, origin: location.origin, source: window.parent }));
    expect(figure.getAttribute("data-moss-places-embed-mode")).toBe("expanded");
    expect(controller.setCooperativeGestures).toHaveBeenCalledWith(false);
    expect(controller.refitScopeIfClipped).toHaveBeenCalled();

    window.dispatchEvent(new MessageEvent("message", { data: { type: "moss-places-embed-mode", expanded: false }, origin: location.origin, source: window.parent }));
    expect(figure.getAttribute("data-moss-places-embed-mode")).toBe("collapsed");
    expect(controller.setCooperativeGestures).toHaveBeenLastCalledWith(true);
  });

  test("a same-origin mode message from a window other than window.parent is ignored", () => {
    history.replaceState(null, "", "/places/?place=lisbon&embed=1");
    const controller = fakeController();
    const figure = document.createElement("figure");
    attachEmbedModeIfRequested(controller, figure);
    (controller.setCooperativeGestures as any).mockClear();

    // Same origin, but NOT window.parent (an iframe embedded inside this
    // embedded page itself, say) — origin alone must not be enough.
    const impostor = document.createElement("iframe");
    document.body.append(impostor);
    window.dispatchEvent(
      new MessageEvent("message", { data: { type: "moss-places-embed-mode", expanded: true }, origin: location.origin, source: impostor.contentWindow }),
    );
    impostor.remove();

    expect(figure.getAttribute("data-moss-places-embed-mode")).toBe("collapsed");
    expect(controller.setCooperativeGestures).not.toHaveBeenCalled();
  });
});

describe("setEmbedScope — the chip's own seam", () => {
  test("forwards to the controller's own setScope — clearing the row exclusion on a scope change is setScope's own job (map.test.ts), not this seam's", () => {
    const controller = fakeController();
    setEmbedScope(controller, { kind: "place", id: "lisbon" });
    expect(controller.setScope).toHaveBeenCalledWith({ kind: "place", id: "lisbon" });
  });
});
