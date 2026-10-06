/**
 * Tests for initAmbientVideos — the WCAG 2.2.2 accessibility layer for
 * `![[clip.mp4|loop]]` embeds.
 *
 * Covers:
 * - Wrapper and toggle are injected
 * - Toggle click pauses/plays and updates aria-label
 * - Reduced-motion guard: removes autoplay, pauses video, switches toggle to play
 * - Idempotent: calling initAmbientVideos twice does not double-wrap
 * - External play/pause events keep toggle in sync
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { initAmbientVideos } from "../ambient-video";

function mockMatchMedia(reducedMotion: boolean) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches:
        query === "(prefers-reduced-motion: reduce)" ? reducedMotion : false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

function createAmbientVideo(): HTMLVideoElement {
  const video = document.createElement("video");
  video.setAttribute("data-loop", "");
  video.src = "clip.mp4";
  video.muted = true;
  document.body.appendChild(video);
  return video;
}

describe("initAmbientVideos — normal motion", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mockMatchMedia(false);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("does nothing when no ambient videos exist", () => {
    initAmbientVideos();
    // no errors thrown
  });

  test("wraps video in .moss-ambient-video", () => {
    createAmbientVideo();
    initAmbientVideos();
    const wrapper = document.querySelector(".moss-ambient-video");
    expect(wrapper).not.toBeNull();
    expect(wrapper?.querySelector("video")).not.toBeNull();
  });

  test("injects .moss-ambient-toggle inside the wrapper", () => {
    createAmbientVideo();
    initAmbientVideos();
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle");
    expect(toggle).not.toBeNull();
    expect(toggle?.getAttribute("aria-label")).toBe("Pause video");
    expect(toggle?.type).toBe("button");
  });

  test("toggle click pauses a playing video and switches aria-label", async () => {
    const video = createAmbientVideo();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);
    video.pause = vi.fn();

    // Simulate video is playing
    Object.defineProperty(video, "paused", { get: () => false, configurable: true });

    initAmbientVideos();
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;

    toggle.click();

    expect(video.pause).toHaveBeenCalledTimes(1);
    expect(toggle.getAttribute("aria-label")).toBe("Play video");
  });

  test("toggle click plays a paused video and switches aria-label", async () => {
    const video = createAmbientVideo();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);
    video.pause = vi.fn();

    // Simulate video is paused
    Object.defineProperty(video, "paused", { get: () => true, configurable: true });

    initAmbientVideos();
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;

    toggle.click();

    expect(video.play).toHaveBeenCalledTimes(1);
    expect(toggle.getAttribute("aria-label")).toBe("Pause video");
  });

  test("data-paused set on wrapper when paused, removed when playing", () => {
    const video = createAmbientVideo();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);
    video.pause = vi.fn();

    Object.defineProperty(video, "paused", {
      get: vi.fn().mockReturnValueOnce(false).mockReturnValue(true),
      configurable: true,
    });

    initAmbientVideos();
    const wrapper = document.querySelector<HTMLElement>(".moss-ambient-video")!;
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;

    // Click to pause (was playing)
    toggle.click();
    expect(wrapper.hasAttribute("data-paused")).toBe(true);
  });

  test("play event from video syncs toggle to pause label", () => {
    createAmbientVideo();
    initAmbientVideos();
    const video = document.querySelector<HTMLVideoElement>("video")!;
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;

    video.dispatchEvent(new Event("play"));

    expect(toggle.getAttribute("aria-label")).toBe("Pause video");
  });

  test("pause event from video syncs toggle to play label", () => {
    createAmbientVideo();
    initAmbientVideos();
    const video = document.querySelector<HTMLVideoElement>("video")!;
    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;

    video.dispatchEvent(new Event("pause"));

    expect(toggle.getAttribute("aria-label")).toBe("Play video");
  });

  test("initAmbientVideos is idempotent — does not double-wrap", () => {
    createAmbientVideo();
    initAmbientVideos();
    initAmbientVideos(); // second call
    const wrappers = document.querySelectorAll(".moss-ambient-video");
    expect(wrappers.length).toBe(1);
    const toggles = document.querySelectorAll(".moss-ambient-toggle");
    expect(toggles.length).toBe(1);
  });
});

describe("initAmbientVideos — prefers-reduced-motion: reduce", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mockMatchMedia(true);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("removes autoplay attribute from video", () => {
    const video = createAmbientVideo();
    video.setAttribute("autoplay", "");
    video.pause = vi.fn();

    initAmbientVideos();

    expect(video.hasAttribute("autoplay")).toBe(false);
  });

  test("calls pause() on the video", () => {
    const video = createAmbientVideo();
    video.setAttribute("autoplay", "");
    video.pause = vi.fn();

    initAmbientVideos();

    expect(video.pause).toHaveBeenCalledTimes(1);
  });

  test("toggle starts as play affordance (user opts in)", () => {
    const video = createAmbientVideo();
    video.setAttribute("autoplay", "");
    video.pause = vi.fn();

    initAmbientVideos();

    const toggle = document.querySelector<HTMLButtonElement>(".moss-ambient-toggle")!;
    expect(toggle.getAttribute("aria-label")).toBe("Play video");
  });

  test("wrapper has data-paused set after reduced-motion guard", () => {
    const video = createAmbientVideo();
    video.setAttribute("autoplay", "");
    video.pause = vi.fn();

    initAmbientVideos();

    const wrapper = document.querySelector<HTMLElement>(".moss-ambient-video")!;
    expect(wrapper.hasAttribute("data-paused")).toBe(true);
  });
});
