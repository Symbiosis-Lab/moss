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
 * Race-safe: pause() is deferred until the pending play() promise settles,
 * avoiding the browser's "play() was interrupted by pause()" AbortError.
 * Only one cover video plays at a time (activeStop singleton).
 */

export function initCardVideos(): void {
  const covers = document.querySelectorAll<HTMLElement>(
    ".moss-card-cover, .moss-collection-cover"
  );
  if (!covers.length) return;

  const hasHover = window.matchMedia("(hover: hover)").matches;
  let activeStop: ((showThumb: boolean) => void) | null = null;

  covers.forEach((cover) => {
    const v = cover.querySelector<HTMLVideoElement>("video");
    const thumb = cover.querySelector<HTMLImageElement>(".cover-thumb");
    if (!v || !thumb) return;

    const hoverTarget = cover.closest(".moss-card") as HTMLElement | null ?? cover;

    let wantsPlay = false;
    let hasPlayed = false;
    let playPromise: Promise<void> | null = null;

    const stop = (showThumb: boolean) => {
      wantsPlay = false;
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
