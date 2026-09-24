// asset-placeholder.ts — the ONE owner of "this media isn't here yet".
//
// An <img> or <video> that fails to load gets the blueprint grid instead of the
// browser's broken-image icon or an empty black box. That covers two situations
// moss cannot tell apart from a load error, and deliberately does not try to:
//
//   1. The reference is broken for good — deleted, renamed, typo'd. It never
//      enters the AssetRegistry, so nothing server-side can placeholder it.
//   2. The asset is still being produced — a background WebP encode or an
//      ffmpeg thumbnail that lands seconds to minutes after the page paints.
//
// The blueprint grid is the placeholder for BOTH. Case 1 simply never resolves;
// case 2 resolves when the bytes land. Collapsing them removes the thing that
// used to break: an error handler guessing "permanently broken" and acting on
// that guess irreversibly.
//
// WHY EVERY MUTATION HERE IS REVERSIBLE
// -------------------------------------
// This used to be a hand-written script inlined in shell.html that swapped the
// <img>'s src to the blueprint data-URI, dropped its `srcset`, and DELETED every
// <source> child of an enclosing <picture>. Nothing was saved. The preview's
// asset-ready swap (js-src/bridge/iframe-bridge.ts) finds elements by URL, so
// after that transform there was no longer anything to find: the <source>
// elements were gone and the <img>'s src was a data-URI. A pending image that
// errored once stayed a blueprint grid until the user pressed Cmd+R, no matter
// how many AssetReady/AssetsSettled events arrived. Diagnosed from ticket
// LOG-8D03-T0528-08-08 (876 images, ~9 minutes of background encoding).
//
// So: we still paint into `img.src`, because that is the only thing that
// suppresses the native broken-image icon in every engine (Chromium draws the
// icon over any CSS background; verified 2026-08-08 in chromium + webkit). But
// every attribute we touch is stashed first, and `restore()` puts all of it
// back. Neutralizing a <picture>'s <source> means moving its `srcset` ONTO the
// same element as `data-moss-ph-srcset` — source-set selection skips a <source>
// with no srcset, and the element stays in the DOM, so the undo is an attribute
// write rather than a re-parse.
//
// A <video> takes the same treatment through a different attribute: the grid
// goes into `poster` (a video cannot display an SVG data-URI as media) and the
// dead `src` comes off, both stashed. The class is still called
// `.moss-img-fallback` — it predates video, and renaming it would touch a dozen
// files to say the same thing.
//
// WHERE IT RUNS — LOCAL PREVIEW ONLY
// ----------------------------------
// The preview server injects this into <head> on every page it serves
// (`preview::iframe_bridge::inject_placeholder_into_head`). It goes in <head>,
// not before </body>, because it must be registered before any image load can
// fail — which also rules out an external script.
//
// A published site carries none of it. It used to: shell.html inlined the
// script into every page, so a broken reference showed a stranger a blueprint
// grid on a live site while the author's local preview looked identical, and
// nothing ever told the author. Rather than dress up the failure, moss removes
// the cause — a publish is refused while any media reference is broken. The
// placeholder is a working state, and the work is local.
//
// `restore()` is published to `window.__mossAssetPlaceholder` so the preview's
// iframe-bridge drives this implementation instead of carrying a second copy of
// the stash attribute names.

import {
  bustSrcset,
  normPath,
  srcsetMatches,
  stripBust,
} from "./asset-urls";

/** Blueprint-grid SVG, light theme. Blueprint blue `#143c82` on paper `#f4f1ec`. */
const LIGHT =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 40'%3E%3Cdefs%3E%3Cpattern id='g' width='10' height='10' patternUnits='userSpaceOnUse'%3E%3Cpath d='M10 0H0V10' fill='none' stroke='%23143c82' stroke-opacity='.18'/%3E%3C/pattern%3E%3C/defs%3E%3Crect width='40' height='40' fill='%23f4f1ec'/%3E%3Crect width='40' height='40' fill='url(%23g)'/%3E%3C/svg%3E";

/** Blueprint-grid SVG, dark theme. */
const DARK =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 40'%3E%3Cdefs%3E%3Cpattern id='g' width='10' height='10' patternUnits='userSpaceOnUse'%3E%3Cpath d='M10 0H0V10' fill='none' stroke='%235a9bff' stroke-opacity='.22'/%3E%3C/pattern%3E%3C/defs%3E%3Crect width='40' height='40' fill='%23252017'/%3E%3Crect width='40' height='40' fill='url(%23g)'/%3E%3C/svg%3E";

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

/** The <source> children of an enclosing <picture>, or [] when there is none. */
function pictureSources(img: HTMLImageElement): HTMLSourceElement[] {
  const parent = img.parentElement;
  if (!parent || parent.tagName !== "PICTURE") return [];
  return Array.from(parent.querySelectorAll("source"));
}

/**
 * Apply the blueprint placeholder to a failed <img> or <video>, stashing
 * everything needed to undo it. Idempotent: a second error on the same element
 * (the data-URI cannot fail, but a restore-then-fail cycle can) is a no-op while
 * the mark is set.
 */
export function place(
  el: HTMLImageElement | HTMLVideoElement,
  doc: Document = document,
): void {
  if (el.hasAttribute(MARK)) return;

  if (el instanceof HTMLVideoElement || el.tagName === "VIDEO") {
    placeVideo(el as HTMLVideoElement, doc);
    return;
  }
  const img = el as HTMLImageElement;
  img.setAttribute(MARK, "1");

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
  const dark = doc.documentElement.getAttribute("data-theme") === "dark";
  img.src = dark ? DARK : LIGHT;
}

/**
 * A <video> whose source failed: paint the grid as its `poster` and take the
 * dead `src` off the element.
 *
 * Poster rather than `src`, because a <video> cannot display an SVG data-URI as
 * media — but it does display its poster for as long as no frame has decoded,
 * which for a source that will never load is forever. Dropping `src` is what
 * stops the engine re-requesting a URL it already failed on, and it is the same
 * reversible move `place` makes on an <img>'s `srcset`.
 */
function placeVideo(video: HTMLVideoElement, doc: Document): void {
  video.setAttribute(MARK, "1");
  video.setAttribute(STASH_SRC, video.getAttribute("src") ?? "");
  video.setAttribute(STASH_POSTER, video.getAttribute("poster") ?? "");
  video.removeAttribute("src");

  video.classList.add("moss-img-fallback");
  const dark = doc.documentElement.getAttribute("data-theme") === "dark";
  video.setAttribute("poster", dark ? DARK : LIGHT);
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
  const bust = "?_t=" + Date.now();

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
      source.setAttribute("srcset", bustSrcset(ss, bust));
      source.removeAttribute(STASH_SRCSET);
    }
    if (stashedOwn !== null) {
      img.setAttribute("srcset", bustSrcset(stashedOwn, bust));
      img.removeAttribute(STASH_SRCSET);
    }

    img.classList.remove("moss-img-fallback");
    img.removeAttribute(MARK);
    img.removeAttribute(STASH_SRC);
    if (stashedSrc !== "") img.src = stashedSrc + bust;

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
    video.setAttribute("src", stashedSrc + bust);
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
    // missing one still blocks the publish (`missing_media::refuse_publish`).
    if (target instanceof HTMLImageElement || target instanceof HTMLVideoElement) {
      place(target, doc);
    }
  };
  doc.addEventListener("error", onError, true);

  // The bridge calls through this handle rather than bundling a second copy of
  // the stash attribute names — two spellings of `data-moss-ph-srcset` would
  // fail silently and look exactly like the bug this replaced.
  (win as unknown as Record<string, unknown>).__mossAssetPlaceholder = {
    restore: (absPath: string) => restore(absPath, doc),
  };

  return () => {
    doc.removeEventListener("error", onError, true);
    delete (win as unknown as Record<string, unknown>).__mossAssetPlaceholder;
  };
}

install();
