/**
 * Tests for card video hover/touch playback with thumbnail overlay.
 *
 * initCardVideos():
 * - Desktop (hover: hover): mouseenter arms a 150ms hover-intent dwell,
 *   then plays video + fades out thumbnail; mouseleave cancels the dwell
 *   (pass-through) or pauses the video
 * - Mobile (no hover): per-element long-press (200ms) to preview,
 *   release to stop. Quick taps navigate (no playback).
 * - Thumbnail overlay (.cover-thumb) toggles .is-playing class
 * - play() promise gates the fade — no black flash
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { initCardVideos } from "../card-video";

/** Create a card with video + thumbnail overlay */
function createCard(): {
  card: HTMLElement;
  video: HTMLVideoElement;
  thumb: HTMLImageElement;
} {
  const card = document.createElement("a");
  card.className = "moss-card";

  const cover = document.createElement("div");
  cover.className = "moss-card-cover";

  const video = document.createElement("video");
  video.src = "clip.mp4";
  video.muted = true;

  const thumb = document.createElement("img");
  thumb.src = "clip.thumb.jpg";
  thumb.className = "cover-thumb";

  cover.appendChild(video);
  cover.appendChild(thumb);
  card.appendChild(cover);
  document.body.appendChild(card);

  return { card, video, thumb };
}

/** Mock matchMedia to control hover detection */
function mockHover(hasHover: boolean) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === "(hover: hover)" ? hasHover : false,
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

describe("initCardVideos — desktop (hover device)", () => {
  const DWELL_MS = 150;

  /** Dispatch mouseenter and sit through the hover-intent dwell. */
  function hover(el: HTMLElement) {
    el.dispatchEvent(new Event("mouseenter"));
    vi.advanceTimersByTime(DWELL_MS);
  }

  beforeEach(() => {
    document.body.innerHTML = "";
    mockHover(true);
    vi.useFakeTimers();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.useRealTimers();
  });

  test("does nothing when no video cards exist", () => {
    initCardVideos();
    // No errors thrown
  });

  test("hover dwell elapses, play() is called, is-playing added on resolve", async () => {
    const { card, video, thumb } = createCard();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();
    card.dispatchEvent(new Event("mouseenter"));

    // Dwell not elapsed yet — hover is a glance, nothing plays
    expect(video.play).not.toHaveBeenCalled();
    vi.advanceTimersByTime(DWELL_MS);
    expect(video.play).toHaveBeenCalledTimes(1);

    // is-playing added only after promise resolves
    expect(thumb.classList.contains("is-playing")).toBe(false);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);
  });

  test("pointer pass-through (leave before dwell) never plays", () => {
    const { card, video } = createCard();
    video.play = vi.fn().mockReturnValue(Promise.resolve());

    initCardVideos();
    card.dispatchEvent(new Event("mouseenter"));
    vi.advanceTimersByTime(DWELL_MS - 50);
    card.dispatchEvent(new Event("mouseleave"));
    vi.advanceTimersByTime(500); // flush — cancelled timer must not fire

    expect(video.play).not.toHaveBeenCalled();
  });

  test("mouseleave after playing keeps video frame visible", async () => {
    const { card, video, thumb } = createCard();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();

    // Enter + dwell — video plays, thumbnail fades out
    hover(card);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);

    // Leave — video pauses but thumbnail stays hidden (frozen frame visible)
    card.dispatchEvent(new Event("mouseleave"));
    expect(thumb.classList.contains("is-playing")).toBe(true);
  });

  test("play() rejection does not add is-playing", async () => {
    const { card, video, thumb } = createCard();
    video.play = vi.fn().mockReturnValue(Promise.reject(new Error("blocked")));

    initCardVideos();
    hover(card);

    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(false);
  });

  test("folder cover video plays on hover over the cover itself", async () => {
    // Folder index covers use .moss-collection-cover (no parent card element)
    const row = document.createElement("div");
    row.className = "moss-collection-cover-row";
    const cover = document.createElement("div");
    cover.className = "moss-collection-cover";
    const video = document.createElement("video");
    video.src = "clip.mp4";
    video.muted = true;
    const thumb = document.createElement("img");
    thumb.src = "clip.thumb.jpg";
    thumb.className = "cover-thumb";
    cover.appendChild(video);
    cover.appendChild(thumb);
    row.appendChild(cover);
    document.body.appendChild(row);

    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();
    hover(cover);

    expect(video.play).toHaveBeenCalledTimes(1);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);

    // mouseleave pauses but keeps video frame visible
    cover.dispatchEvent(new Event("mouseleave"));
    expect(thumb.classList.contains("is-playing")).toBe(true);
  });

  test("fast hover-leave-hover still plays on re-enter", async () => {
    const { card, video, thumb } = createCard();
    // Use a deferred promise so play() stays pending during the fast leave
    let resolvePlay!: () => void;
    const slowPlay = new Promise<void>((r) => { resolvePlay = r; });
    video.play = vi.fn().mockReturnValue(slowPlay);

    initCardVideos();

    // Hover + dwell — play() called but promise still pending
    hover(card);
    expect(video.play).toHaveBeenCalledTimes(1);

    // Leave immediately while play() is still pending
    card.dispatchEvent(new Event("mouseleave"));

    // Now resolve the first play
    resolvePlay();
    await slowPlay;
    await vi.advanceTimersByTimeAsync(0);

    // Thumbnail should NOT have is-playing (we left before it resolved)
    expect(thumb.classList.contains("is-playing")).toBe(false);

    // Re-enter — play() should be called again and work
    const secondPlay = Promise.resolve();
    video.play = vi.fn().mockReturnValue(secondPlay);
    hover(card);
    expect(video.play).toHaveBeenCalledTimes(1);
    await secondPlay;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);
  });

  test("hovering card B stops card A", async () => {
    const a = createCard();
    const b = createCard();
    const playA = Promise.resolve();
    const playB = Promise.resolve();
    a.video.play = vi.fn().mockReturnValue(playA);
    b.video.play = vi.fn().mockReturnValue(playB);

    initCardVideos();

    // Hover card A
    hover(a.card);
    await playA;
    await vi.advanceTimersByTimeAsync(0);
    expect(a.thumb.classList.contains("is-playing")).toBe(true);

    // Hover card B (without leaving A explicitly)
    hover(b.card);
    await playB;
    await vi.advanceTimersByTimeAsync(0);

    // Card A should be stopped, card B should be playing
    expect(a.thumb.classList.contains("is-playing")).toBe(false);
    expect(b.thumb.classList.contains("is-playing")).toBe(true);
  });

  test("skips covers without thumbnail overlay", () => {
    const card = document.createElement("a");
    card.className = "moss-card";
    const cover = document.createElement("div");
    cover.className = "moss-card-cover";
    const video = document.createElement("video");
    cover.appendChild(video);
    card.appendChild(cover);
    document.body.appendChild(card);

    video.play = vi.fn().mockReturnValue(Promise.resolve());

    initCardVideos();
    hover(card);

    // play() should NOT be called since there's no thumb
    expect(video.play).not.toHaveBeenCalled();
  });
});

describe("initCardVideos — mobile (no hover) — per-element touch", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mockHover(false);
    vi.useFakeTimers();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test("attaches touchstart listener on card element, not window", () => {
    const { card, video } = createCard();
    video.play = vi.fn().mockReturnValue(Promise.resolve());
    const cardSpy = vi.spyOn(card, "addEventListener");

    initCardVideos();

    const touchStartCall = cardSpy.mock.calls.find((c) => c[0] === "touchstart");
    expect(touchStartCall).toBeDefined();
  });

  test("tap on cover does not play video (navigates normally)", () => {
    const { card, video } = createCard();
    video.play = vi.fn().mockReturnValue(Promise.resolve());

    initCardVideos();

    // Quick tap: touchstart then touchend within 200ms
    card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));
    vi.advanceTimersByTime(100); // only 100ms — under threshold
    card.dispatchEvent(new TouchEvent("touchend", { bubbles: true, cancelable: true }));

    // Timer hasn't fired yet, so play should NOT be called
    vi.advanceTimersByTime(200); // flush any remaining timers
    expect(video.play).not.toHaveBeenCalled();
  });

  test("long-press (>200ms) on cover plays video", async () => {
    const { card, video, thumb } = createCard();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();

    card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));

    // Not yet — timer hasn't fired
    expect(video.play).not.toHaveBeenCalled();

    // Advance past threshold
    vi.advanceTimersByTime(200);

    expect(video.play).toHaveBeenCalledTimes(1);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);
  });

  test("release after long-press stops video and suppresses click", async () => {
    const { card, video, thumb } = createCard();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();

    // Long-press
    card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));
    vi.advanceTimersByTime(200);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);

    // Release — touchend with preventDefault check
    const touchEnd = new TouchEvent("touchend", { bubbles: true, cancelable: true });
    const preventSpy = vi.spyOn(touchEnd, "preventDefault");
    card.dispatchEvent(touchEnd);

    expect(thumb.classList.contains("is-playing")).toBe(false);
    expect(preventSpy).toHaveBeenCalled();
  });

  test("touchcancel stops video", async () => {
    const { card, video, thumb } = createCard();
    const playPromise = Promise.resolve();
    video.play = vi.fn().mockReturnValue(playPromise);

    initCardVideos();

    // Long-press to start
    card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));
    vi.advanceTimersByTime(200);
    await playPromise;
    await vi.advanceTimersByTimeAsync(0);
    expect(thumb.classList.contains("is-playing")).toBe(true);

    // touchcancel
    card.dispatchEvent(new TouchEvent("touchcancel", { bubbles: true }));
    expect(thumb.classList.contains("is-playing")).toBe(false);
  });

  test("long-press on B stops A (only one plays at a time)", async () => {
    const a = createCard();
    const b = createCard();
    const playA = Promise.resolve();
    const playB = Promise.resolve();
    a.video.play = vi.fn().mockReturnValue(playA);
    b.video.play = vi.fn().mockReturnValue(playB);

    initCardVideos();

    // Long-press card A
    a.card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));
    vi.advanceTimersByTime(200);
    await playA;
    await vi.advanceTimersByTimeAsync(0);
    expect(a.thumb.classList.contains("is-playing")).toBe(true);

    // Long-press card B
    b.card.dispatchEvent(new TouchEvent("touchstart", { bubbles: true }));
    vi.advanceTimersByTime(200);
    await playB;
    await vi.advanceTimersByTimeAsync(0);

    // A stopped, B playing
    expect(a.thumb.classList.contains("is-playing")).toBe(false);
    expect(b.thumb.classList.contains("is-playing")).toBe(true);
  });

  test("does not attach mouseenter/mouseleave on mobile", () => {
    const { card, video } = createCard();
    video.play = vi.fn().mockReturnValue(Promise.resolve());
    const addSpy = vi.spyOn(card, "addEventListener");

    initCardVideos();

    const eventTypes = addSpy.mock.calls.map((c) => c[0]);
    expect(eventTypes).not.toContain("mouseenter");
    expect(eventTypes).not.toContain("mouseleave");
  });
});
