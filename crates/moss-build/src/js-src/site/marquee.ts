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
 *
 * Hover and focus pauses end when the pointer or focus leaves, and a reader's
 * reduced-motion setting is not on the page, so the script also draws one
 * button before the first strip that moves. It pauses and resumes every strip
 * on the page (`data-marquee-paused` on the root) and remembers the choice for
 * the other pages. The script draws it, not the build, because without the
 * script nothing moves and a button would do nothing.
 */
import { langBucket, type Lang } from "./subscribe/i18n";
import { readStored, writeStored } from "./safe-storage";

const DEFAULT_SPEED = 60;
const STORE_KEY = "moss-marquee-paused";
const PAUSED_ATTR = "data-marquee-paused";

/** A Rust test (`marquee_labels_match_the_site_strings`) holds these to
 * `i18n/strings.rs`, the site's own copy. */
const LABELS: Record<Lang, { pause: string; play: string }> = {
  en: { pause: "Pause motion", play: "Play motion" },
  de: { pause: "Bewegung anhalten", play: "Bewegung abspielen" },
  "zh-hans": { pause: "暂停动态", play: "播放动态" },
  "zh-hant": { pause: "暫停動態", play: "播放動態" },
};

const root = () => document.documentElement;

function speedOf(strip: HTMLElement): number {
  const speed = parseFloat(getComputedStyle(strip).getPropertyValue("--moss-marquee-speed"));
  return speed > 0 ? speed : DEFAULT_SPEED;
}

function fit(strip: HTMLElement, set: HTMLElement): void {
  const width = set.getBoundingClientRect().width;
  strip.toggleAttribute("data-marquee-still", !(width > 0 && width >= strip.clientWidth));
  if (width > 0) strip.style.setProperty("--moss-marquee-duration", `${width / speedOf(strip)}s`);
  syncControl();
}

let control: HTMLButtonElement | null = null;

function label(): void {
  if (!control) return;
  const words = LABELS[langBucket(root().lang)];
  control.textContent = root().hasAttribute(PAUSED_ATTR) ? words.play : words.pause;
}

/** Draws the button before the first strip that moves, hides it when none does. */
function syncControl(): void {
  const moving = document.querySelector<HTMLElement>("[data-motion='marquee'][data-marquee-ready]:not([data-marquee-still])");
  if (!moving) {
    if (control) control.hidden = true;
    return;
  }
  if (!control || !control.isConnected) {
    control = document.createElement("button");
    control.type = "button";
    control.className = "moss-marquee-control";
    control.addEventListener("click", () => {
      const paused = !root().hasAttribute(PAUSED_ATTR);
      root().toggleAttribute(PAUSED_ATTR, paused);
      writeStored(STORE_KEY, paused ? "1" : "0");
      label();
    });
  }
  control.hidden = false;
  label();
  // A listing strip sits inside a card's link, where a button may not go.
  const anchor = moving.closest<HTMLElement>(".moss-cards, .moss-gallery") ?? moving;
  if (control.nextElementSibling !== anchor) anchor.before(control);
}

const watched = new WeakSet<HTMLElement>();

export function initMarquees(scope: ParentNode = document): void {
  if (readStored(STORE_KEY) === "1") root().setAttribute(PAUSED_ATTR, "");
  scope.querySelectorAll<HTMLElement>("[data-motion='marquee']").forEach((strip, index) => {
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
