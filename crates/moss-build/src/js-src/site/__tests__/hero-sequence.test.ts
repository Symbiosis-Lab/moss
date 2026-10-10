/**
 * Tests for the all-video hero sequence driver in ambient-video.ts.
 *
 * Covers:
 * - `ended` on the active slide moves data-current to the next slide and starts it
 * - the last slide wraps to the first
 * - `ended` on a slide that is not active does nothing
 * - the pause checkbox stops the advance; unchecking resumes the current slide
 * - reduced motion removes autoplay and starts nothing
 * - a rejected play() is swallowed and does not advance
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { initHeroSequences } from "../hero-sequence";

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

function buildSequence(count: number) {
  const slides = Array.from(
    { length: count },
    (_, i) =>
      `<div class="moss-hero-slide" data-slide="${i}"${i === 0 ? " data-current" : ""}><video autoplay muted playsinline preload="metadata"></video></div>`,
  ).join("");
  document.body.innerHTML =
    `<section class="moss-hero" data-sequence>${slides}` +
    `<label class="moss-hero-pause"><input type="checkbox" aria-label="Pause"><span></span></label></section>`;
  const hero = document.querySelector<HTMLElement>(".moss-hero")!;
  const videos = Array.from(hero.querySelectorAll<HTMLVideoElement>("video"));
  for (const video of videos) {
    video.play = vi.fn().mockResolvedValue(undefined);
    video.pause = vi.fn();
  }
  return { hero, videos };
}

function pauseControl(hero: HTMLElement): HTMLInputElement {
  return hero.querySelector<HTMLInputElement>(".moss-hero-pause input")!;
}

/** Tick the pause checkbox the way a click does: toggle, then fire `change`. */
function togglePause(hero: HTMLElement) {
  const input = pauseControl(hero);
  input.checked = !input.checked;
  input.dispatchEvent(new Event("change"));
}

/** Which slide carries the `data-current` marker, and that its caption agrees. */
function currentSlide(hero: HTMLElement): number {
  const slides = Array.from(hero.querySelectorAll(".moss-hero-slide"));
  const marked = slides.filter((slide) => slide.hasAttribute("data-current"));
  expect(marked).toHaveLength(1);
  return slides.indexOf(marked[0]);
}

function ended(video: HTMLVideoElement) {
  video.dispatchEvent(new Event("ended"));
}

describe("initHeroSequences — normal motion", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mockMatchMedia(false);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("ended on the active slide advances data-current and starts the next video", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    ended(videos[0]);
    expect(currentSlide(hero)).toBe(1);
    expect(videos[1].play).toHaveBeenCalled();
    expect(videos[0].pause).toHaveBeenCalled();
  });

  test("the last slide wraps back to the first", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    ended(videos[0]);
    ended(videos[1]);
    expect(currentSlide(hero)).toBe(2);
    ended(videos[2]);
    expect(currentSlide(hero)).toBe(0);
    expect(videos[0].play).toHaveBeenCalled();
  });

  test("ended on a slide that is not active does nothing", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    ended(videos[2]);
    expect(currentSlide(hero)).toBe(0);
    expect(videos[0].pause).not.toHaveBeenCalled();
  });

  test("the sequence's one pause checkbox stops the advance and resumes the current slide", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    expect(hero.querySelectorAll(".moss-hero-pause").length).toBe(1);
    togglePause(hero);
    expect(pauseControl(hero).checked).toBe(true);
    expect(videos[0].pause).toHaveBeenCalled();
    ended(videos[0]);
    expect(currentSlide(hero)).toBe(0);
    togglePause(hero);
    expect(pauseControl(hero).checked).toBe(false);
    expect(videos[0].play).toHaveBeenCalled();
    ended(videos[0]);
    expect(currentSlide(hero)).toBe(1);
  });

  test("a rejected play() is swallowed and the sequence stops on that slide", async () => {
    const { hero, videos } = buildSequence(2);
    videos[1].play = vi.fn().mockRejectedValue(new Error("autoplay blocked"));
    initHeroSequences();
    ended(videos[0]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(currentSlide(hero)).toBe(1);
    expect(videos[1].play).toHaveBeenCalled();
  });
});

describe("initHeroSequences — reduced motion", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    mockMatchMedia(true);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("removes autoplay and starts no video", () => {
    const { videos } = buildSequence(3);
    initHeroSequences();
    for (const video of videos) {
      expect(video.hasAttribute("autoplay")).toBe(false);
      expect(video.play).not.toHaveBeenCalled();
    }
  });
});

describe("the clip indicator", () => {
  beforeEach(() => {
    mockMatchMedia(false);
  });
  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  const segments = (hero: HTMLElement) =>
    Array.from(hero.querySelectorAll<HTMLButtonElement>(".moss-hero-indicator-segment"));

  test("one labelled button per clip, the first aria-current", () => {
    const { hero } = buildSequence(3);
    initHeroSequences();
    const segs = segments(hero);
    expect(segs).toHaveLength(3);
    expect(segs[1].getAttribute("aria-label")).toBe("Clip 2 of 3");
    expect(segs[0].getAttribute("aria-current")).toBe("true");
    expect(segs[1].hasAttribute("aria-current")).toBe(false);
  });

  test("a click jumps to that clip, plays it and moves the marker and aria-current", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    segments(hero)[2].click();
    expect(currentSlide(hero)).toBe(2);
    expect(videos[2].play).toHaveBeenCalled();
    expect(videos[0].pause).toHaveBeenCalled();
    expect(segments(hero)[2].getAttribute("aria-current")).toBe("true");
    expect(segments(hero)[0].hasAttribute("aria-current")).toBe(false);
  });

  test("while paused a jump shows the chosen clip without playing it", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    togglePause(hero);
    segments(hero)[1].click();
    expect(currentSlide(hero)).toBe(1);
    expect(videos[1].play).not.toHaveBeenCalled();
    expect(pauseControl(hero).checked).toBe(true);
  });

  test("the natural advance moves aria-current too", () => {
    const { hero, videos } = buildSequence(3);
    initHeroSequences();
    ended(videos[0]);
    expect(segments(hero)[1].getAttribute("aria-current")).toBe("true");
  });

  test("the playing segment's progress follows the video's time", () => {
    const { hero, videos } = buildSequence(2);
    initHeroSequences();
    Object.defineProperty(videos[0], "duration", { value: 10 });
    videos[0].currentTime = 4;
    videos[0].dispatchEvent(new Event("timeupdate"));
    expect(segments(hero)[0].style.getPropertyValue("--moss-hero-progress")).toBe("0.4");
  });

  test("reduced motion adds no indicator", () => {
    mockMatchMedia(true);
    const { hero } = buildSequence(3);
    initHeroSequences();
    expect(segments(hero)).toHaveLength(0);
  });
});
