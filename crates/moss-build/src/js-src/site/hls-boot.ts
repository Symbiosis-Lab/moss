/**
 * hls-boot.ts — decide, per page, whether an HLS ladder needs a JS player.
 *
 * moss emits `<video><source src="…/master.m3u8"><source src="….mp4"></video>`.
 * A browser with native HLS plays the first source; one without it falls
 * through to the progressive MP4 on its own, with no JS at all. This file
 * exists for the third case: a browser that has no native HLS but *can* run
 * hls.js, which would otherwise sit on the 2000k MP4 — exactly the viewer the
 * ladder was built for.
 *
 * **Capability, not browser name.** The gap is not "Firefox": desktop Chrome
 * and Edge only shipped native HLS in v142 (Dec 2025), and Android WebViews
 * vary. The question that decides it is whether Media Source Extensions exist,
 * which is what `Hls.isSupported()` itself asks — so this file asks it directly
 * and the player is fetched only where it will actually be used.
 *
 * **MSE is checked before `canPlayType`**, deliberately. `canPlayType(
 * 'application/vnd.apple.mpegurl')` answers `"maybe"` on some Chromium
 * versions that then fail to play, so trusting it first would leave those
 * viewers stalled with a player available and unused.
 *
 * **Cost.** The chunk is ~185 kb over the wire. A browser with native HLS pays
 * none of it, but one that takes this path pays it before the first frame.
 * It is hls.js's full build, not the *light* one: light sets `USE_ALT_AUDIO =
 * false` and cannot resolve an `EXT-X-MEDIA` group, so it would play the
 * ladder's two audio rendition groups silently.
 */

const HLS_TYPE = "application/vnd.apple.mpegurl";

/** The master playlist a `<video>` offers, or `null` if it has no ladder. */
function ladderUrl(video: HTMLVideoElement): string | null {
  const source = video.querySelector<HTMLSourceElement>(`source[type="${HLS_TYPE}"]`);
  return source?.getAttribute("src") ?? null;
}

/**
 * The same question `Hls.isSupported()` answers, without loading hls.js to ask
 * it. Kept in step with the library's own check: MSE plus the fragmented-MP4
 * codecs the ladder is encoded in.
 */
function canRunHlsJs(): boolean {
  const mse = window.MediaSource ?? (window as { ManagedMediaSource?: typeof MediaSource })
    .ManagedMediaSource;
  return (
    typeof mse === "function" &&
    typeof mse.isTypeSupported === "function" &&
    mse.isTypeSupported('video/mp4; codecs="avc1.42E01E,mp4a.40.2"')
  );
}

/**
 * Hand one video to the lazy player chunk.
 *
 * The `<source>` children are removed first. Left in place, the browser has
 * already committed to one of them — the MP4, on every browser that reaches
 * here — and hls.js attaching afterwards would mean the viewer paying for both.
 */
function upgrade(video: HTMLVideoElement, url: string, chunk: string): void {
  video.querySelectorAll("source").forEach((s) => s.remove());
  video.removeAttribute("src");
  video.load();
  void import(/* @vite-ignore */ new URL(chunk, location.href).href)
    .then((mod: typeof import("./hls-player")) => mod.attach(video, url))
    .catch(() => {
      // The chunk did not arrive. Put the progressive MP4 back rather than
      // leaving an empty player: a big video that plays beats none at all.
      const mp4 = video.dataset.mossMp4;
      if (mp4) {
        video.src = mp4;
        video.load();
      }
    });
}

function init(): void {
  const chunk = document.querySelector<HTMLScriptElement>("script[data-hls]")?.dataset.hls;
  if (!chunk || !canRunHlsJs()) return;
  document.querySelectorAll<HTMLVideoElement>("video").forEach((video) => {
    const url = ladderUrl(video);
    if (!url) return;
    // Remembered before the sources are removed, so the failure path above has
    // something to fall back to.
    const mp4 = video.querySelector<HTMLSourceElement>('source[type="video/mp4"]');
    if (mp4) video.dataset.mossMp4 = mp4.getAttribute("src") ?? "";
    upgrade(video, url, chunk);
  });
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
