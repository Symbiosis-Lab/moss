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
 * The pause control is the server-rendered `.moss-hero-pause` checkbox: while
 * it is checked the advance stops and the current video pauses; unchecking
 * resumes. Under reduced motion nothing starts and the stylesheet hides it.
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
    videos[index].preload = "auto";
    // A rejected play() (autoplay blocked) leaves this slide's poster showing.
    void Promise.resolve(videos[index].play()).catch(() => {});
  };

  const advance = (from: number): void => {
    const next = (from + 1) % videos.length;
    videos[from].pause();
    videos[from].currentTime = 0;
    slides.forEach((slide, i) => slide.toggleAttribute("data-current", i === next));
    captions.forEach((caption) =>
      caption.toggleAttribute("data-current", Number(caption.dataset.slide) === next),
    );
    start(next);
  };

  const pause = hero.querySelector<HTMLInputElement>(".moss-hero-pause input");
  let paused = false;

  videos.forEach((video, index) => {
    video.addEventListener("ended", () => {
      if (current() !== index || paused) return;
      advance(index);
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
