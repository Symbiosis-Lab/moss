/**
 * hls-player.ts — the hls.js half of adaptive playback, loaded on demand.
 *
 * Split from `hls-boot.ts` because this file *is* hls.js: bundling it eagerly
 * would put 183 kb brotli on every page carrying a video, including the pages
 * read on the 118 kbps link this whole feature exists for, where a browser
 * with native HLS needs none of it. `hls-boot.ts` decides; this arrives only
 * if the answer was yes.
 *
 * That figure is the full build, and it is not a choice: the light build is
 * roughly a third the size but sets `USE_ALT_AUDIO = false`, so it cannot
 * resolve the `EXT-X-MEDIA` audio group every rung names and would play the
 * ladder silently. Anyone shrinking this is proposing to drop the audio.
 *
 * **No `startLevel`, and no ABR tuning.** hls.js's defaults already do the
 * right thing here: `startLevel` unset means auto, and auto with
 * `testBandwidth: true` fetches one fragment of the *lowest* rung purely to
 * measure the link before choosing. Pinning `startLevel` is the single change
 * that breaks that — `testBandwidth` applies only when the start level is auto,
 * so setting it disables the probe and starts everyone on the first variant in
 * the playlist regardless of their connection.
 */

import Hls from "hls.js";

/**
 * Play `url` in `video` through hls.js.
 *
 * Fatal errors are given hls.js's own two recoveries — a network error retries
 * the load, a media error resets the decoder — and only a third failure gives
 * up. When it does, the progressive MP4 goes back on the element: this ladder
 * exists to make a slow connection watchable, and a player that has stopped
 * serves that worse than a large file the viewer can leave running.
 */
export function attach(video: HTMLVideoElement, url: string): void {
  if (!Hls.isSupported()) {
    fallBackToMp4(video);
    return;
  }
  const hls = new Hls();
  // The budget is counted, not assumed. Without it a fatal NETWORK_ERROR calls
  // startLoad() forever, and on a link that has actually died the viewer sits
  // on an empty player: the MP4 fallback below is unreachable in exactly the
  // case it was written for.
  let recoveries = 0;
  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (!data.fatal) return;
    if (recoveries++ >= 2) {
      hls.destroy();
      fallBackToMp4(video);
      return;
    }
    if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
      hls.startLoad();
    } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
      hls.recoverMediaError();
    } else {
      hls.destroy();
      fallBackToMp4(video);
    }
  });
  hls.attachMedia(video);
  hls.loadSource(url);
}

function fallBackToMp4(video: HTMLVideoElement): void {
  const mp4 = video.dataset.mossMp4;
  if (!mp4) return;
  video.src = mp4;
  video.load();
}
