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
 * **No `startLevel`.** Left unset, hls.js 1.7 runs no bandwidth probe: the
 * first level comes from the default estimate, which it raises to the first
 * variant's bitrate (capped at 5 Mbps) when the manifest loads, and it adapts
 * from real fragments after that. Pinning a level would start every viewer on
 * it whatever their connection. (The one-fragment `testBandwidth` probe runs
 * only for an explicit `startLevel: -1`.)
 *
 * That start is a guess, and a short clip is over before the guess is
 * corrected: a clip of a few seconds is buffered whole at the guessed rung and
 * loops from the buffer, so nothing is left to fetch at a better one. Three
 * small rules cover it: a video that is not playing does not load; a player
 * that starts after another has buffered real media starts from that
 * page-wide estimate; and a short clip that finished buffering below what the
 * link now supports is refetched once at that rung.
 */

import Hls from "hls.js";
import type { Fragment } from "hls.js";

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
  const deferred = defersLoad(video);
  // Through the constructor: hls.js raises an estimate set later to the first
  // variant's bitrate when the manifest loads, but leaves a configured one.
  const hls = new Hls({
    ...(pageEstimate !== null ? { abrEwmaDefaultEstimate: pageEstimate } : {}),
    ...(deferred ? { autoStartLoad: false } : {}),
  });
  const stopWaiting = deferred ? startWhenWanted(video, hls) : () => {};
  const sharpen = shortClipSharpener(hls, video);
  hls.on(Hls.Events.FRAG_BUFFERED, (_event, data) => {
    if (!isMediaFragment(data.frag)) return;
    rememberEstimate(hls);
    sharpen(data.frag);
  });
  // The budget is counted, not assumed. Without it a fatal NETWORK_ERROR calls
  // startLoad() forever, and on a link that has actually died the viewer sits
  // on an empty player: the MP4 fallback below is unreachable in exactly the
  // case it was written for.
  let recoveries = 0;
  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (!data.fatal) return;
    if (recoveries++ >= 2) {
      stopWaiting();
      hls.destroy();
      fallBackToMp4(video);
      return;
    }
    if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
      hls.startLoad();
    } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
      hls.recoverMediaError();
    } else {
      stopWaiting();
      hls.destroy();
      fallBackToMp4(video);
    }
  });
  hls.attachMedia(video);
  hls.loadSource(url);
}

// ── A clip that is not playing does not load ────────────────────────

/**
 * True for a video that asked not to load (`preload="none"`) and is not being
 * played by autoplay. hls.js ignores `preload`, so without this every slide of
 * a hero, and every article video, downloads segments at page load.
 */
export function defersLoad(video: HTMLVideoElement): boolean {
  return video.preload === "none" && !video.autoplay && video.paused;
}

/**
 * Start `hls`'s segment loading the first time the video is asked to play, or
 * when its `preload` is raised to `auto`. The master playlist is already
 * fetched (`autoStartLoad: false` only holds back what follows: the level
 * playlist, the init segment, the fragments). Returns a function that drops
 * both triggers, for when the player is destroyed first.
 */
function startWhenWanted(video: HTMLVideoElement, hls: Hls): () => void {
  let done = false;
  const stop = (): void => {
    done = true;
    observer.disconnect();
    video.removeEventListener("play", start);
  };
  const start = (): void => {
    if (done) return;
    stop();
    // Another player may have measured the link since this one was created.
    // The manifest is parsed by now, so this is not overwritten.
    if (pageEstimate !== null) hls.bandwidthEstimate = pageEstimate;
    hls.startLoad();
  };
  const observer = new MutationObserver(() => {
    if (video.preload === "auto") start();
  });
  observer.observe(video, { attributes: true, attributeFilter: ["preload"] });
  video.addEventListener("play", start);
  return stop;
}

// ── Later players start from what earlier ones learned ───────────────

/**
 * Bits per second measured on a real media fragment of any player on this
 * page. Two known limits, accepted: a fragment answered from a cache records
 * an inflated rate, and a link that slows later leaves a stale value until
 * the next fragment. Either way the next player starts one fragment too high
 * and hls.js steps down.
 */
let pageEstimate: number | null = null;

/** Real media only: not an init segment, not alternate audio. */
function isMediaFragment(frag: Fragment): boolean {
  return frag.type === "main" && frag.sn !== "initSegment";
}

/** Keep `hls`'s estimate for the next player. Called only once a real fragment is buffered. */
export function rememberEstimate(hls: Hls): void {
  const estimate = hls.bandwidthEstimate;
  if (Number.isFinite(estimate) && estimate > 0) pageEstimate = estimate;
}

// ── A short clip buffered below what the link supports is refetched ──

/**
 * The level to refetch at, or `null` to leave things alone. Only a clip that is
 * whole (its last fragment is buffered) or short enough to be buffered in one
 * go qualifies, and only upward: `best` is hls.js's own pick for the current
 * estimate, and `floor` is the highest level already played or asked for, so a
 * slow link keeps its low rung and no step is taken twice.
 */
export function upgradeTarget(s: {
  floor: number;
  best: number;
  duration: number;
  whole: boolean;
  bufferTarget: number;
}): number | null {
  if (!(s.duration > 0)) return null;
  if (!s.whole && s.duration > s.bufferTarget) return null;
  return s.best > s.floor ? s.best : null;
}

/**
 * Returns the handler to call with each real fragment as it is buffered: if
 * the clip is short or whole and sits below the level the estimate now
 * supports, switch to that level at once. `currentLevel` (not
 * `nextLevel`/`loadLevel`) because only it flushes the buffer, and the buffered
 * picture is exactly what must be replaced; it also pins the level manually,
 * where hls.js's own abandon rules are off. So the pin is released
 * (`loadLevel = -1`, no second flush) as soon as the clip is whole again at
 * that level (its last fragment buffered, or end of stream), and on any error.
 */
function shortClipSharpener(hls: Hls, video: HTMLVideoElement): (frag: Fragment) => void {
  let pinned: number | null = null;
  let lastTarget = -1;
  const release = (): void => {
    if (pinned === null) return;
    pinned = null;
    hls.loadLevel = -1;
  };
  hls.on(Hls.Events.BUFFER_EOS, release);
  hls.on(Hls.Events.ERROR, release);
  return (frag) => {
    const whole = frag.sn === hls.levels[frag.level]?.details?.endSN;
    if (pinned !== null) {
      if (frag.level === pinned && whole) release();
      return;
    }
    const target = upgradeTarget({
      floor: Math.max(frag.level, lastTarget),
      best: hls.nextAutoLevel,
      duration: video.duration,
      whole,
      bufferTarget: hls.config.maxBufferLength,
    });
    if (target === null) return;
    pinned = lastTarget = target;
    hls.currentLevel = target;
  };
}

function fallBackToMp4(video: HTMLVideoElement): void {
  const mp4 = video.dataset.mossMp4;
  if (!mp4) return;
  video.src = mp4;
  video.load();
}
