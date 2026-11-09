/**
 * Tests for the swipe arrow indicator — the shell-chrome UI layer over the
 * swipe tracker. Verifies the visible contract: arrows appear and track
 * progress, commit triggers navigation exactly once, cancel retracts, and
 * the whole feature degrades to inert when the native phase stream is
 * unavailable (non-macOS).
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

import { SwipeIndicator, type SwipePhasePayload } from "../swipe/swipe-indicator";

describe("SwipeIndicator", () => {
  let container: HTMLElement;
  let iframe: HTMLIFrameElement;
  let goBack: ReturnType<typeof vi.fn>;
  let goForward: ReturnType<typeof vi.fn>;
  let emit: ((e: SwipePhasePayload) => void) | null;
  let indicator: SwipeIndicator;

  function makeIndicator(subscribeOk = true): SwipeIndicator {
    return new SwipeIndicator({
      container,
      iframe,
      navigate: {
        canGoBack: () => true,
        canGoForward: () => true,
        goBack,
        goForward,
      },
      subscribe: async (onEvent) => {
        if (!subscribeOk) return false;
        emit = onEvent;
        return true;
      },
    });
  }

  beforeEach(() => {
    document.body.innerHTML = `<div class="moss-preview-container"></div>`;
    container = document.querySelector(".moss-preview-container")!;
    Object.defineProperty(container, "clientWidth", { value: 1000, configurable: true });
    iframe = document.createElement("iframe");
    container.appendChild(iframe);
    goBack = vi.fn().mockResolvedValue(undefined);
    goForward = vi.fn().mockResolvedValue(undefined);
    emit = null;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  function candidate(direction: string, ceded = false): void {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-swipe-wheel", direction, ceded },
        source: iframe.contentWindow,
      })
    );
  }

  function phase(
    phase: string,
    deltaX = 0,
    t = 0,
    momentumPhase = "none"
  ): void {
    emit!({
      delta_x: deltaX,
      delta_y: 0,
      phase,
      momentum_phase: momentumPhase,
      location_x: 0,
      location_y: 0,
      timestamp_ms: t,
      swipe_tracking_enabled: true,
      direction_inverted_from_device: true,
    });
  }

  async function dragBack(indicatorRef: SwipeIndicator, count = 10): Promise<number> {
    phase("began", 0, 1000);
    candidate("back");
    let t = 1008;
    for (let i = 0; i < count; i++) {
      phase("changed", 30, t);
      t += 8;
    }
    return t;
  }

  it("becomes active when the native stream subscribes", async () => {
    indicator = makeIndicator();
    await indicator.start();
    expect(indicator.nativeActive).toBe(true);
    expect(emit).not.toBeNull();
  });

  it("stays inert (fallback mode) when the stream is unavailable", async () => {
    indicator = makeIndicator(false);
    await indicator.start();
    expect(indicator.nativeActive).toBe(false);
    expect(container.querySelector(".moss-swipe-arrow")).toBeNull();
  });

  it("shows the back arrow with progress during a back drag", async () => {
    indicator = makeIndicator();
    await indicator.start();
    await dragBack(indicator);
    const arrow = container.querySelector(".moss-swipe-arrow--back") as HTMLElement;
    expect(arrow).not.toBeNull();
    expect(arrow.classList.contains("is-visible")).toBe(true);
    // 300px on a 1000px container is past min(187.5/1000, .5) → armed
    expect(arrow.classList.contains("is-armed")).toBe(true);
  });

  it("commits on release past threshold: navigates exactly once", async () => {
    indicator = makeIndicator();
    await indicator.start();
    const t = await dragBack(indicator);
    phase("ended", 0, t);
    expect(goBack).toHaveBeenCalledTimes(1);
    expect(goForward).not.toHaveBeenCalled();
    // Momentum tail must not navigate again.
    phase("none", 40, t + 20, "began");
    phase("none", 40, t + 40, "changed");
    phase("none", 40, t + 60, "ended");
    expect(goBack).toHaveBeenCalledTimes(1);
  });

  it("retracts without navigating when released short of threshold", async () => {
    indicator = makeIndicator();
    await indicator.start();
    phase("began", 0, 1000);
    candidate("back");
    // 100px slow → cancel
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 10, t);
      t += 200;
    }
    phase("ended", 0, t + 200);
    expect(goBack).not.toHaveBeenCalled();
    const arrow = container.querySelector(".moss-swipe-arrow--back") as HTMLElement;
    expect(arrow.classList.contains("is-visible")).toBe(false);
  });

  it("ignores candidates from a non-iframe source", async () => {
    indicator = makeIndicator();
    await indicator.start();
    phase("began", 0, 1000);
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "moss-swipe-wheel", direction: "back", ceded: false },
      })
    );
    let t = 1008;
    for (let i = 0; i < 10; i++) {
      phase("changed", 30, t);
      t += 8;
    }
    phase("ended", 0, t);
    expect(goBack).not.toHaveBeenCalled();
  });
});
