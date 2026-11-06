/**
 * marquee.ts — a picture strip that slides sideways in a loop
 * (`motion:marquee`, `:::gallery marquee`).
 *
 * The build draws the strip's pictures twice in one track, the second set
 * hidden from assistive technology. The stylesheet translates the track by
 * minus one set's width, so the loop is seamless; this script supplies the one
 * thing CSS cannot know, how long that takes. A strip's duration is its set's
 * width over `--moss-marquee-speed` (pixels per second), so a long strip and a
 * short one move at the same pace. It also marks the strip `data-marquee-ready`
 * (the stylesheet draws the loop only then, so without the script the strip is
 * the ordinary still one) and gives every other strip in the page the opposite
 * direction. A strip whose one set is narrower than the strip's own box cannot
 * be filled by two copies (the loop would show a gap and jump), so it is marked
 * `data-marquee-still` and stays put; that is re-decided whenever either box
 * resizes.
 */
const DEFAULT_SPEED = 60;

function speedOf(strip: HTMLElement): number {
  const speed = parseFloat(getComputedStyle(strip).getPropertyValue("--moss-marquee-speed"));
  return speed > 0 ? speed : DEFAULT_SPEED;
}

function fit(strip: HTMLElement, set: HTMLElement): void {
  const width = set.getBoundingClientRect().width;
  strip.toggleAttribute("data-marquee-still", !(width > 0 && width >= strip.clientWidth));
  if (width > 0) strip.style.setProperty("--moss-marquee-duration", `${width / speedOf(strip)}s`);
}

const watched = new WeakSet<HTMLElement>();

export function initMarquees(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-motion='marquee']").forEach((strip, index) => {
    strip.toggleAttribute("data-marquee-reverse", index % 2 === 1);
    const set = strip.querySelector<HTMLElement>(".moss-marquee-set");
    if (!set) return;
    strip.setAttribute("data-marquee-ready", "");
    fit(strip, set);
    if (watched.has(strip)) return;
    watched.add(strip);
    // The set's width changes as its pictures load, the strip's with the viewport.
    const observer = new ResizeObserver(() => fit(strip, set));
    observer.observe(set);
    observer.observe(strip);
  });
}

const run = () => initMarquees();
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", run);
} else {
  run();
}
document.addEventListener("moss-morph-patched", run);
