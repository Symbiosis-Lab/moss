/** First view: the levels fade in, lowest first, once, when a third of the map is in view. */
import { reducedMotion } from "../media";

export function tintIn(figure: HTMLElement): void {
  if (reducedMotion() || !("IntersectionObserver" in window)) return;
  figure.classList.add("is-arming");
  const watch = new IntersectionObserver(
    (entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      figure.classList.add("is-tinted");
      watch.disconnect();
    },
    { threshold: 0.33 },
  );
  watch.observe(figure.querySelector("svg")!);
}
