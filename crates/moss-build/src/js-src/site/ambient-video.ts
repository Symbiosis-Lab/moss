/**
 * ambient-video.ts — Accessibility wiring for `![[clip.mp4|loop]]` embeds.
 *
 * moss emits `<video data-loop autoplay muted loop playsinline ...>` for
 * the ambient branch. Two WCAG 2.2.2 obligations are handled here:
 *
 * 1. Reduced-motion guard (JS, always on).
 *    CSS `animation: none` alone cannot stop autoplay — the guard must run in
 *    JS. On DOMContentLoaded, for each `video[data-loop]`:
 *    if `prefers-reduced-motion: reduce` → removeAttribute("autoplay"), pause(),
 *    leave the poster visible, and mark the toggle as play-first (user opts in).
 *
 * 2. Chrome-free pause/play toggle (WCAG 2.2.2, all users).
 *    A minimal button wraps the video inside `.moss-ambient-video`. The toggle
 *    is keyboard-focusable, uses aria-label, and shows on hover/focus (CSS
 *    handles visibility; JS handles state).
 *
 * The wrapper `.moss-ambient-video` is injected by this module at init time so
 * the Rust synthesizer stays HTML-only and the JS owns the interactive layer.
 */

const PAUSE_LABEL = "Pause video";
const PLAY_LABEL = "Play video";
const PAUSE_ICON = "⏸";
const PLAY_ICON = "▶";

function wrapAndInit(video: HTMLVideoElement): void {
  // Wrap the video in .moss-ambient-video (the CSS hook for the toggle).
  // Guard: skip if already wrapped (idempotent on re-init).
  if (video.parentElement?.classList.contains("moss-ambient-video")) return;

  const wrapper = document.createElement("div");
  wrapper.className = "moss-ambient-video";
  video.parentNode?.insertBefore(wrapper, video);
  wrapper.appendChild(video);

  // Pause/play toggle button.
  const toggle = document.createElement("button");
  toggle.className = "moss-ambient-toggle";
  toggle.type = "button";
  toggle.setAttribute("aria-label", PAUSE_LABEL);
  toggle.textContent = PAUSE_ICON;
  wrapper.appendChild(toggle);

  // --- Reduced-motion guard ---
  const prefersReduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (prefersReduced) {
    // Remove autoplay before the browser acts on it (guard fires synchronously
    // during init, before the next paint). pause() is a no-op if the video
    // hasn't started yet — belt-and-suspenders.
    video.removeAttribute("autoplay");
    video.pause();
    // Switch toggle to play affordance so users can opt in.
    toggle.setAttribute("aria-label", PLAY_LABEL);
    toggle.textContent = PLAY_ICON;
    wrapper.setAttribute("data-paused", "");
  }

  // --- Toggle click ---
  toggle.addEventListener("click", () => {
    if (video.paused) {
      video.play().catch(() => {});
      toggle.setAttribute("aria-label", PAUSE_LABEL);
      toggle.textContent = PAUSE_ICON;
      wrapper.removeAttribute("data-paused");
    } else {
      video.pause();
      toggle.setAttribute("aria-label", PLAY_LABEL);
      toggle.textContent = PLAY_ICON;
      wrapper.setAttribute("data-paused", "");
    }
  });

  // Keep toggle label in sync with external play/pause events (e.g. browser
  // autoplay policy blocks the play and the browser fires a pause event).
  video.addEventListener("play", () => {
    toggle.setAttribute("aria-label", PAUSE_LABEL);
    toggle.textContent = PAUSE_ICON;
    wrapper.removeAttribute("data-paused");
  });
  video.addEventListener("pause", () => {
    toggle.setAttribute("aria-label", PLAY_LABEL);
    toggle.textContent = PLAY_ICON;
    wrapper.setAttribute("data-paused", "");
  });
}

/**
 * Initialise all ambient-loop videos in the document.
 *
 * Called once from theme.ts after DOMContentLoaded. Safe to call multiple
 * times (idempotent wrapper guard).
 */
export function initAmbientVideos(): void {
  initHeroSequences();
  const videos = document.querySelectorAll<HTMLVideoElement>("video[data-loop]");
  if (!videos.length) return;

  videos.forEach(wrapAndInit);
}

/**
 * Hero sequences: an all-video hero (`.moss-hero[data-sequence]`) plays its
 * slides one after another. Each video plays once. When it ends, the hero's
 * `data-active` moves to the next slide (wrapping) and that video starts; the
 * stylesheet shows the slide and caption whose `data-slide` matches it.
 *
 * The pause checkbox (`.moss-hero-pause input`) stops the advance while it is
 * checked: the current video pauses, and unchecking resumes it. Under reduced
 * motion nothing starts, and the stylesheet pins slide 0 and its caption.
 */
const heroSequenceWired = new WeakSet<HTMLElement>();

export function initHeroSequences(root: ParentNode = document): void {
  root
    .querySelectorAll<HTMLElement>(".moss-hero[data-sequence]")
    .forEach(wireHeroSequence);
}

function wireHeroSequence(hero: HTMLElement): void {
  if (heroSequenceWired.has(hero)) return;
  heroSequenceWired.add(hero);

  const videos = Array.from(
    hero.querySelectorAll<HTMLElement>(".moss-hero-slide[data-slide]"),
  )
    .sort((a, b) => Number(a.dataset.slide) - Number(b.dataset.slide))
    .map((slide) => slide.querySelector("video"))
    .filter((video): video is HTMLVideoElement => video !== null);
  if (videos.length < 2) return;

  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    for (const video of videos) {
      video.removeAttribute("autoplay");
      video.pause();
    }
    return;
  }

  const current = (): number => {
    const index = Number(hero.dataset.active ?? "0");
    return Number.isInteger(index) && index >= 0 && index < videos.length ? index : 0;
  };

  const start = (index: number): void => {
    videos[index].preload = "auto";
    // A rejected play() (autoplay blocked) leaves this slide's poster showing.
    void Promise.resolve(videos[index].play()).catch(() => {});
  };

  const advance = (from: number): void => {
    const next = (from + 1) % videos.length;
    videos[from].pause();
    videos[from].currentTime = 0;
    hero.dataset.active = String(next);
    start(next);
  };

  const pause = sequencePauseButton(hero);
  let paused = false;

  videos.forEach((video, index) => {
    video.addEventListener("ended", () => {
      if (current() !== index || paused) return;
      advance(index);
    });
  });

  pause.addEventListener("click", () => {
    paused = !paused;
    pause.setAttribute("aria-pressed", String(paused));
    pause.setAttribute("aria-label", paused ? PLAY_SEQUENCE_LABEL : PAUSE_SEQUENCE_LABEL);
    pause.textContent = paused ? PLAY_ICON : PAUSE_ICON;
    if (paused) {
      videos[current()].pause();
    } else {
      start(current());
    }
  });
}

const PAUSE_SEQUENCE_LABEL = "Pause the videos";
const PLAY_SEQUENCE_LABEL = "Play the videos";

/** The sequence's one pause control: a button the script adds, styled as `.moss-hero-pause`. */
function sequencePauseButton(hero: HTMLElement): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "moss-hero-pause";
  button.setAttribute("aria-label", PAUSE_SEQUENCE_LABEL);
  button.setAttribute("aria-pressed", "false");
  button.textContent = PAUSE_ICON;
  hero.appendChild(button);
  return button;
}
