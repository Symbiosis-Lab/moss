// asset-placeholder.ts — the ONE owner of "this media isn't here yet".
//
// An <img> or <video> that fails to load waits on the live grid instead of the
// browser's broken-image icon or an empty black box. That covers two situations
// moss cannot tell apart from a load error, and deliberately does not try to:
//
//   1. The reference is broken for good — deleted, renamed, typo'd. It never
//      enters the AssetRegistry, so nothing server-side can placeholder it.
//   2. The asset is still being produced — a background WebP encode or an
//      ffmpeg thumbnail that lands seconds to minutes after the page paints.
//
// The waiting look is the placeholder for BOTH. Case 1 simply never resolves;
// case 2 resolves when the bytes land. Collapsing them means no error handler
// guesses "permanently broken" and acts on that guess irreversibly.
//
// WHY EVERY MUTATION HERE IS REVERSIBLE
// -------------------------------------
// The preview's asset-ready swap (js-src/bridge/iframe-bridge.ts) finds
// elements by URL, so a transform that discards the original `src`/`srcset`/
// <source> elements leaves a pending image that errored once stuck as a
// blueprint grid, however many AssetReady/AssetsSettled events arrive.
//
// So: we write an empty stand-in into `img.src`, because that is the only thing
// that suppresses the native broken-image icon in every engine (Chromium draws
// the icon over any CSS background). The grid itself is the bridge's live
// overlay (./blueprint-fallback.ts); nothing here paints one. But every attribute we touch is stashed first, and
// `restore()` puts all of it back. Neutralizing a <picture>'s <source> means moving its `srcset` ONTO the
// same element as `data-moss-ph-srcset` — source-set selection skips a <source>
// with no srcset, and the element stays in the DOM, so the undo is an attribute
// write rather than a re-parse.
//
// A <video> takes the same treatment through a different attribute: the stand-in
// goes into `poster` (a video cannot display an SVG data-URI as media) and the
// dead `src` comes off, both stashed. The class is called `.moss-img-fallback`
// for both.
//
// WHERE IT RUNS — LOCAL PREVIEW ONLY
// ----------------------------------
// The preview server injects this into <head> on every page it serves
// (`preview::iframe_bridge::inject_placeholder_into_head`). It goes in <head>,
// not before </body>, because it must be registered before any image load can
// fail — which also rules out an external script.
//
// A published site carries none of it: a publish is refused while any file
// reference is broken, so the placeholder is a local working state only.
//
// `restore()` is published to `window.__mossAssetPlaceholder` so the preview's
// iframe-bridge drives this implementation instead of carrying a second copy of
// the stash attribute names.

import {
  bustSrcset,
  normPath,
  srcsetUrls,
  srcsetMatches,
  stripBust,
  withBust,
} from "./asset-urls";

/**
 * An empty SVG in the element's own shape. It paints nothing: the live grid the
 * preview bridge lays over it and the edge in the injected style are the whole
 * look. It still has to be a real image, because writing it into `src` is the
 * only thing that keeps an engine from drawing its broken-image icon, and its
 * width and height keep the element's box when no stylesheet sizes it.
 */
function standIn(el: Element): string {
  const w = Number(el.getAttribute("width")) || 400;
  const h = Number(el.getAttribute("height")) || 300;
  return `data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 ${w} ${h}' width='${w}' height='${h}'/%3E`;
}

/** Set on a placeheld element — the idempotence guard and the restore selector. */
const MARK = "data-moss-ph";
/** Stashed original `src` of the <img> or <video>. */
const STASH_SRC = "data-moss-ph-src";
/**
 * Stashed original `poster` of a <video>. moss usually sets one (the generated
 * thumbnail), and it may be a perfectly good frame of a video whose bytes are
 * still converting — so the blueprint replaces it only for as long as the
 * placeholder is up.
 */
const STASH_POSTER = "data-moss-ph-poster";
/**
 * Stashed original `srcset`. Set on the <img> for a bare `<img srcset>` ladder,
 * and on each `<source>` of an enclosing `<picture>`. Same attribute name on
 * both because the undo is the same operation on both.
 */
const STASH_SRCSET = "data-moss-ph-srcset";
/** URL the browser chose when this image failed; absent once retrying is pointless. */
const FAILED_URL = "data-moss-ph-failed-url";
/** Path the last restore swapped in. The same path failing again is final. */
const RESTORED = "data-moss-ph-restored";

/** An empty src resolves to the page itself, which answers 200 and is no picture. */
function isPage(url: string, doc: Document): boolean {
  try { return new URL(url, doc.baseURI).href.split("#")[0] === doc.URL.split("#")[0]; } catch { return false; }
}

/** The <source> children of an enclosing <picture>, or [] when there is none. */
function pictureSources(img: HTMLImageElement): HTMLSourceElement[] {
  const parent = img.parentElement;
  if (!parent || parent.tagName !== "PICTURE") return [];
  return Array.from(parent.querySelectorAll("source"));
}

/**
 * Put the stand-in on a failed <img> or <video>, stashing
 * everything needed to undo it. Idempotent: a second error on the same element
 * (the data-URI cannot fail, but a restore-then-fail cycle can) is a no-op while
 * the mark is set.
 */
export function place(el: HTMLImageElement | HTMLVideoElement): void {
  if (el.hasAttribute(MARK)) return;

  if (el instanceof HTMLVideoElement || el.tagName === "VIDEO") {
    placeVideo(el as HTMLVideoElement);
    return;
  }
  const img = el as HTMLImageElement;
  const chosen = img.currentSrc ||
    pictureSources(img).flatMap((source) => srcsetUrls(source.getAttribute("srcset") ?? ""))[0] ||
    img.src;
  img.setAttribute(MARK, "1");
  if (!isPage(chosen, img.ownerDocument) && img.getAttribute(RESTORED) !== normPath(stripBust(chosen))) {
    img.setAttribute(FAILED_URL, chosen);
  }

  // Stash before touching anything.
  img.setAttribute(STASH_SRC, img.getAttribute("src") ?? "");
  const ownSrcset = img.getAttribute("srcset");
  if (ownSrcset !== null) {
    img.setAttribute(STASH_SRCSET, ownSrcset);
    img.removeAttribute("srcset");
  }
  // Neutralize each <source> by MOVING its srcset onto itself under the stash
  // name. Source-set selection skips a <source> without srcset, so the
  // data-URI below wins — and the element survives for the undo.
  for (const source of pictureSources(img)) {
    const srcset = source.getAttribute("srcset");
    if (srcset === null) continue;
    source.setAttribute(STASH_SRCSET, srcset);
    source.removeAttribute("srcset");
  }

  img.classList.add("moss-img-fallback");
  img.src = standIn(img);
}

/**
 * A <video> whose source failed: the stand-in becomes its `poster`, and the
 * dead `src` comes off the element.
 *
 * Poster rather than `src`, because a <video> cannot display an SVG data-URI as
 * media — but it does display its poster for as long as no frame has decoded,
 * which for a source that will never load is forever. Dropping `src` is what
 * stops the engine re-requesting a URL it already failed on, and it is the same
 * reversible move `place` makes on an <img>'s `srcset`.
 */
function placeVideo(video: HTMLVideoElement): void {
  video.setAttribute(MARK, "1");
  video.setAttribute(STASH_SRC, video.getAttribute("src") ?? "");
  video.setAttribute(STASH_POSTER, video.getAttribute("poster") ?? "");
  video.removeAttribute("src");

  video.classList.add("moss-img-fallback");
  video.setAttribute("poster", standIn(video));
}

/**
 * Undo the placeholder on every <img> or <video> whose stashed URLs reference
 * `absPath`, and return how many were restored.
 *
 * `absPath` is an absolute, decoded pathname (`/assets/photo.w800.webp`) — the
 * shape `normPath` produces and the shape iframe-bridge builds from an
 * AssetReady/AssetsSettled path.
 *
 * Restores with a cache-bust: the original fetch failed, and a negatively
 * cached 404 would otherwise fail again on the same URL. When the re-fetch
 * succeeds the <img> fires `load`, which is what tears down the preview's
 * animated blueprint overlay (js-src/bridge/blueprint-fallback.ts).
 */
export function restore(absPath: string, doc: Document = document): number {
  let restored = 0;
  const stamp = Date.now();

  doc.querySelectorAll<HTMLImageElement>(`img[${MARK}]`).forEach((img) => {
    const stashedSrc = stripBust(img.getAttribute(STASH_SRC) ?? "");
    const stashedOwn = img.getAttribute(STASH_SRCSET);
    const sources = pictureSources(img);

    const hit =
      (stashedSrc !== "" && normPath(stashedSrc) === absPath) ||
      (stashedOwn !== null && srcsetMatches(stashedOwn, absPath)) ||
      sources.some((s) => {
        const ss = s.getAttribute(STASH_SRCSET);
        return ss !== null && srcsetMatches(ss, absPath);
      });
    if (!hit) return;

    // Put the ladder back first, so source-set selection sees the full set the
    // moment the <img>'s own src changes below.
    for (const source of sources) {
      const ss = source.getAttribute(STASH_SRCSET);
      if (ss === null) continue;
      source.setAttribute("srcset", bustSrcset(ss, stamp));
      source.removeAttribute(STASH_SRCSET);
    }
    if (stashedOwn !== null) {
      img.setAttribute("srcset", bustSrcset(stashedOwn, stamp));
      img.removeAttribute(STASH_SRCSET);
    }

    img.classList.remove("moss-img-fallback");
    img.removeAttribute(MARK);
    img.removeAttribute(STASH_SRC);
    img.removeAttribute(FAILED_URL);
    img.setAttribute(RESTORED, absPath);
    if (stashedSrc !== "") img.src = withBust(stashedSrc, stamp);

    restored++;
  });

  doc.querySelectorAll<HTMLVideoElement>(`video[${MARK}]`).forEach((video) => {
    const stashedSrc = stripBust(video.getAttribute(STASH_SRC) ?? "");
    if (stashedSrc === "" || normPath(stashedSrc) !== absPath) return;

    const stashedPoster = video.getAttribute(STASH_POSTER) ?? "";
    if (stashedPoster === "") video.removeAttribute("poster");
    else video.setAttribute("poster", stashedPoster);

    video.classList.remove("moss-img-fallback");
    video.removeAttribute(MARK);
    video.removeAttribute(STASH_SRC);
    video.removeAttribute(STASH_POSTER);
    video.setAttribute("src", withBust(stashedSrc, stamp));
    // <video> caches its own media state — a new `src` alone does not start a
    // fetch. `load()` is what makes the arriving bytes actually play.
    video.load();

    restored++;
  });

  return restored;
}

/**
 * Register the capture-phase `error` listener (`error` does not bubble) and
 * publish `restore` for the preview bridge. Returns a teardown for tests.
 */
export function install(doc: Document = document, win: Window = window): () => void {
  const onError = (e: Event): void => {
    const target = e.target;
    // <audio> is deliberately absent: there is no surface to placehold, and a
    // missing one still blocks the publish (`refuse_publish`).
    if (target instanceof HTMLImageElement || target instanceof HTMLVideoElement) {
      place(target);
      arm();
    }
  };
  doc.addEventListener("error", onError, true);

  // A browser never asks again for an <img> that failed, but in the preview a
  // failure is usually a file on its way: a 404 for a picture asked for before
  // the build promised it, a 503 for an original still downloading. So every
  // placeheld picture on screen is asked again each second, four at a time,
  // taking turns, until it answers. A 503 or a timeout rests that picture for
  // three ticks, so slow cloud reads cannot hold every connection the engine has.
  // A picture rests until a tick number; an unanswered probe rests until it answers.
  const restUntil = new WeakMap<HTMLImageElement, number>();
  let tick = 0;
  const active = new Set<AbortController>();
  let disposed = false;
  let cursor = 0;
  // The clock runs only while a picture still waits on a visible page.
  const waiting = `img[${MARK}][${FAILED_URL}]`;
  let clock: number | null = null;
  const arm = (): void => {
    if (clock === null && !disposed && doc.visibilityState !== "hidden" && doc.querySelector(waiting)) {
      clock = win.setInterval(retry, 1000);
    }
  };
  const disarm = (): void => {
    if (clock !== null) win.clearInterval(clock);
    clock = null;
  };
  const onVisibility = (): void => (doc.visibilityState === "hidden" ? disarm() : arm());
  doc.addEventListener("visibilitychange", onVisibility);
  const retry = (): void => {
    if (disposed || typeof win.fetch !== "function") return;
    if (doc.visibilityState === "hidden" || !doc.querySelector(waiting)) { disarm(); return; }
    tick++;
    const images = Array.from(doc.querySelectorAll<HTMLImageElement>(waiting)).filter((img) => {
      if (!img.isConnected || (restUntil.get(img) ?? 0) > tick) return false;
      const box = img.getBoundingClientRect();
      // Inclusive: a box that collapsed to 0 × 0 at the page's edge is still on screen.
      return box.bottom >= 0 && box.right >= 0 && box.top <= win.innerHeight && box.left <= win.innerWidth;
    });
    for (let n = 0; n < images.length && active.size < 4; n++, cursor++) {
      const img = images[cursor % images.length];
      const failedUrl = img.getAttribute(FAILED_URL)!;
      restUntil.set(img, Infinity);
      const abort = new AbortController();
      active.add(abort);
      const timeout = win.setTimeout(() => abort.abort(), 4000);
      void win.fetch(failedUrl, { method: "HEAD", cache: "no-store", signal: abort.signal })
        .then((response) => {
          if (response.status === 503) restUntil.set(img, tick + 4);
          if (disposed || !response.ok || !img.isConnected || img.getAttribute(FAILED_URL) !== failedUrl) return;
          restore(normPath(failedUrl), doc);
        })
        .catch(() => { restUntil.set(img, tick + 4); })
        .finally(() => {
          win.clearTimeout(timeout);
          active.delete(abort);
          if (restUntil.get(img) === Infinity) restUntil.delete(img);
        });
    }
  };

  // The bridge calls through this handle rather than bundling a second copy of
  // the stash attribute names — two spellings of `data-moss-ph-srcset` would
  // fail silently and look exactly like the bug this replaced.
  (win as unknown as Record<string, unknown>).__mossAssetPlaceholder = {
    restore: (absPath: string) => restore(absPath, doc),
  };

  return () => {
    disposed = true;
    for (const abort of active) abort.abort();
    active.clear();
    doc.removeEventListener("error", onError, true);
    disarm();
    doc.removeEventListener("visibilitychange", onVisibility);
    delete (win as unknown as Record<string, unknown>).__mossAssetPlaceholder;
  };
}

install();
