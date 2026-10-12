/**
 * hero-sequence.ts — an all-video hero (`.moss-hero[data-sequence]`) plays its
 * slides one after another. Each video plays once. When it ends, the next
 * slide (wrapping) becomes current and its video starts.
 *
 * The one piece of state is the `data-current` marker on the current slide and
 * its caption: the renderer emits it on slide 0, the stylesheet shows only the
 * marked slide and caption, and this script moves it. Without the script (or
 * under reduced motion) slide 0 stays current.
 *
 * The clip indicator (`.moss-hero-indicator`, one button per clip) is added by
 * the script; a click jumps to that clip through the same `show` the natural
 * advance uses, and its aria-current and progress read the marker.
 *
 * The pause control is the server-rendered `.moss-hero-pause` checkbox: while
 * it is checked the advance stops and the current video pauses; unchecking
 * resumes. Under reduced motion nothing starts and the stylesheet hides it.
 */
/** Seconds before a clip ends at which the next one starts loading. */
const WARM_SECONDS = 3;

const heroSequenceWired = new WeakSet<HTMLElement>();

export function initHeroSequences(root: ParentNode = document): void {
  root
    .querySelectorAll<HTMLElement>(".moss-hero[data-sequence]")
    .forEach(wireHeroSequence);
}

function wireHeroSequence(hero: HTMLElement): void {
  if (heroSequenceWired.has(hero)) return;
  heroSequenceWired.add(hero);

  const slides = Array.from(
    hero.querySelectorAll<HTMLElement>(".moss-hero-slide[data-slide]"),
  ).sort((a, b) => Number(a.dataset.slide) - Number(b.dataset.slide));
  const videos = slides
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

  const captions = Array.from(
    hero.querySelectorAll<HTMLElement>(".moss-hero-caption[data-slide]"),
  );

  const current = (): number => {
    const index = slides.findIndex((slide) => slide.hasAttribute("data-current"));
    return index >= 0 && index < videos.length ? index : 0;
  };

  const start = (index: number): void => {
    // A rejected play() (autoplay blocked) leaves this slide's poster showing.
    void Promise.resolve(videos[index].play()).catch(() => {});
  };

  const pause = hero.querySelector<HTMLInputElement>(".moss-hero-pause input");
  let paused = false;
  const indicator = sequenceIndicator(hero, videos.length, current);

  // Moves the sequence from clip `from` to clip `to`: the natural advance is
  // `from + 1`, a click on an indicator segment is any other `to`. While paused
  // the chosen clip waits on its first frame instead of playing.
  const show = (from: number, to: number): void => {
    videos[from].pause();
    videos[from].currentTime = 0;
    slides.forEach((slide, i) => slide.toggleAttribute("data-current", i === to));
    captions.forEach((caption) =>
      caption.toggleAttribute("data-current", Number(caption.dataset.slide) === to),
    );
    indicator.sync();
    if (paused) {
      videos[to].currentTime = 0;
    } else {
      start(to);
    }
  };
  const advance = (from: number): void => show(from, (from + 1) % videos.length);

  indicator.segments.forEach((segment, index) => {
    segment.addEventListener("click", () => show(current(), index));
  });

  videos.forEach((video, index) => {
    video.addEventListener("ended", () => {
      if (current() !== index || paused) return;
      advance(index);
    });
    video.addEventListener("timeupdate", () => {
      if (current() !== index) return;
      indicator.progress(index, video.currentTime / video.duration);
      // A waiting clip does not load (`preload="none"`), so the next one is
      // warmed shortly before its turn rather than showing a still while it
      // starts. Raising `preload` is the one load signal; playing it is not.
      const next = videos[(index + 1) % videos.length];
      if (!paused && video.duration - video.currentTime <= WARM_SECONDS && next.preload !== "auto") {
        next.preload = "auto";
      }
    });
  });

  pause?.addEventListener("change", () => {
    paused = pause.checked;
    if (paused) {
      videos[current()].pause();
    } else {
      start(current());
    }
  });
}

/**
 * One real `<button>` per clip, inside the captions container so the row stacks
 * directly above the caption (the container is created if the hero has no
 * captions). The thin bar is drawn by CSS; the button is the target. Added by
 * the script, so a page without it shows nothing.
 */
function sequenceIndicator(hero: HTMLElement, count: number, current: () => number) {
  let host = hero.querySelector<HTMLElement>(".moss-hero-captions");
  if (!host) {
    host = document.createElement("div");
    host.className = "moss-hero-captions";
    hero.appendChild(host);
  }
  const group = document.createElement("div");
  group.className = "moss-hero-indicator";
  group.setAttribute("aria-live", "off"); // inside the captions' polite region
  const segments: HTMLButtonElement[] = [];
  for (let i = 0; i < count; i++) {
    const caption = hero.querySelector(`.moss-hero-caption[data-slide="${i}"]`)?.textContent?.trim();
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-hero-indicator-segment";
    button.setAttribute("aria-label", `Clip ${i + 1} of ${count}${caption ? `: ${caption}` : ""}`);
    group.appendChild(button);
    segments.push(button);
  }
  host.insertBefore(group, host.firstChild);
  const sync = (): void => {
    const active = current();
    segments.forEach((segment, i) => {
      if (i === active) segment.setAttribute("aria-current", "true");
      else segment.removeAttribute("aria-current");
      segment.style.setProperty("--moss-hero-progress", "0");
    });
  };
  sync();
  return {
    segments,
    sync,
    progress(index: number, fraction: number): void {
      const value = Number.isFinite(fraction) ? Math.min(1, Math.max(0, fraction)) : 0;
      segments[index].style.setProperty("--moss-hero-progress", String(value));
    },
  };
}
