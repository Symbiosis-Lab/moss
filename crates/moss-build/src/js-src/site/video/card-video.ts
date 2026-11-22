/**
 * card-video.ts — Silent video playback on card hover and touch
 *
 * Uses a separate <img class="cover-thumb"> overlay (YouTube/Netflix pattern).
 * Thumbnail fades out only after play() resolves (frames rendering).
 * On desktop mouseleave, the video pauses but the last frame stays visible
 * (thumbnail remains hidden). The thumbnail only returns when another video
 * takes over or on mobile touch exit.
 *
 * Desktop: mouseenter arms a 150ms hover-intent dwell, then plays;
 *          mouseleave cancels the dwell or pauses (resumes on re-hover).
 *          "Hover is a glance" (design-vocabulary.md): the dwell means a
 *          pointer pass-through never strobes playback.
 * Mobile:  Long-press (200ms) on a cover to preview. Release to stop.
 *          Quick taps navigate normally (no playback triggered).
 *          Per-element listeners — no global hit-testing needed.
 *
 * In view: unless the visitor asks for reduced motion, a cover plays muted
 *          while half of it is on screen and pauses when it leaves, so touch
 *          and keyboard visitors see it move too. It plays on its own, apart
 *          from the one-at-a-time hover and long-press preview.
 *
 * Race-safe: pause() is deferred until the pending play() promise settles,
 * avoiding the browser's "play() was interrupted by pause()" AbortError.
 * Only one hover or long-press preview plays at a time (activeStop
 * singleton); in-view covers play beside it, at most MAX_IN_VIEW at once.
 */

/** Covers that play on their own at once; the rest wait for a free place the next time they scroll into view. */
const MAX_IN_VIEW = 3;

export function initCardVideos(): void {
  const covers = document.querySelectorAll<HTMLElement>(
    ".moss-card-cover, .moss-collection-cover"
  );
  if (!covers.length) return;

  const hasHover = window.matchMedia("(hover: hover)").matches;
  let playingInView = 0;
  let activeStop: ((showThumb: boolean) => void) | null = null;
  const autoplays =
    "IntersectionObserver" in window &&
    !window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  covers.forEach((cover) => {
    const v = cover.querySelector<HTMLVideoElement>("video");
    const thumb = cover.querySelector<HTMLImageElement>(".cover-thumb");
    if (!v || !thumb) return;

    const hoverTarget = cover.closest(".moss-card") as HTMLElement | null ?? cover;

    let wantsPlay = false;
    let hasPlayed = false;
    let inView = false;
    let playPromise: Promise<void> | null = null;

    const stop = (showThumb: boolean) => {
      wantsPlay = false;
      if (inView) {
        // Still on screen: the preview ends, the cover keeps playing.
        if (activeStop === stop) activeStop = null;
        return;
      }
      if (showThumb) thumb.classList.remove("is-playing");
      if (activeStop === stop) activeStop = null;
      const p = playPromise;
      if (p) {
        p.then(() => { if (!wantsPlay) v.pause(); }).catch(() => {});
      } else {
        v.pause();
      }
    };

    const play = () => {
      if (v.preload !== "auto") v.preload = "auto";
      if (activeStop && activeStop !== stop) activeStop(true);
      wantsPlay = true;
      activeStop = stop;
      playPromise = v.play();
      playPromise
        .then(() => {
          hasPlayed = true;
          if (wantsPlay) thumb.classList.add("is-playing");
        })
        .catch(() => {});
    };

    if (autoplays) {
      v.muted = true;
      v.playsInline = true;
      new IntersectionObserver((entries) => {
        const visible = entries[entries.length - 1].isIntersecting;
        if (visible === inView) return;
        inView = visible;
        if (visible) {
          if (playingInView >= MAX_IN_VIEW) { inView = false; return; }
          playingInView++;
          playPromise = v.play();
          playPromise
            .then(() => {
              hasPlayed = true;
              if (inView) thumb.classList.add("is-playing");
            })
            .catch(() => {});
        } else {
          playingInView--;
          if (wantsPlay) return;
          const p = playPromise;
          if (p) p.then(() => { if (!inView && !wantsPlay) v.pause(); }).catch(() => {});
          else v.pause();
        }
      }, { threshold: 0.5 }).observe(cover);
    }

    if (hasHover) {
      // Hover-intent dwell (design-vocabulary.md "Hover is a glance"):
      // same threshold as chip-bar.ts's HOVER_INTENT_MS.
      const HOVER_INTENT_MS = 150;
      let hoverTimer: ReturnType<typeof setTimeout> | null = null;

      hoverTarget.addEventListener("mouseenter", () => {
        hoverTimer = setTimeout(() => {
          hoverTimer = null;
          play();
        }, HOVER_INTENT_MS);
      });
      hoverTarget.addEventListener("mouseleave", () => {
        if (hoverTimer) {
          // Pass-through: dwell never completed, nothing started.
          clearTimeout(hoverTimer);
          hoverTimer = null;
          return;
        }
        stop(!hasPlayed);
      });
    } else {
      // Mobile: long-press to preview, release to stop, tap to navigate
      const LONG_PRESS_MS = 200;
      let timer: ReturnType<typeof setTimeout> | null = null;
      let previewing = false;

      hoverTarget.addEventListener("touchstart", () => {
        timer = setTimeout(() => {
          timer = null;
          previewing = true;
          play();
        }, LONG_PRESS_MS);
      }, { passive: true });

      hoverTarget.addEventListener("touchend", (e) => {
        if (timer) { clearTimeout(timer); timer = null; }
        if (previewing) {
          previewing = false;
          stop(true);
          e.preventDefault();
        }
      });

      hoverTarget.addEventListener("touchcancel", () => {
        if (timer) { clearTimeout(timer); timer = null; }
        if (previewing) { previewing = false; stop(true); }
      });
    }
  });
}
