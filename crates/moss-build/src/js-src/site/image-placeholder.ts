/**
 * image-placeholder.ts — removes the blurred placeholder under a content image.
 *
 * moss paints each image with an inline low-quality background-image until
 * the real file arrives. Nothing removed it, so behind a transparent PNG or in
 * the letterbox of an object-fit: contain image it stayed visible as a smear.
 * An image that fails keeps it, since then the placeholder is all there is.
 */

function clearPlaceholder(img: HTMLImageElement): void {
  img.style.removeProperty("background-image");
  img.style.removeProperty("background-size");
}

/**
 * Clears the placeholder of every image that has already loaded. Run at
 * startup and after a preview morph, which can restore the inline style on an
 * image that is complete, so no `load` event will follow.
 *
 * An image that is complete with naturalWidth 0 is skipped: a broken image
 * looks the same from here, and the scan cannot tell it from a loaded SVG
 * without an intrinsic size. Such an SVG keeps its placeholder when the scan
 * is the first to see it. The load listener below has no such gap.
 */
export function clearLoadedPlaceholders(root: ParentNode = document): void {
  root
    .querySelectorAll<HTMLImageElement>('img[style*="background-image"]')
    .forEach((img) => {
      if (img.complete && img.naturalWidth > 0) clearPlaceholder(img);
    });
}

/** Wires the load listener and the scan. Call once per page bundle. */
export function initImagePlaceholders(): void {
  // Capture phase: `load` does not bubble, but it reaches a capturing listener
  // on document, so one listener covers every image, including ones added
  // later. It clears on any `load` regardless of naturalWidth, because an SVG
  // with no intrinsic size loads with naturalWidth 0 and should still lose its
  // placeholder. A failed image fires `error`, never `load`, so it keeps it.
  document.addEventListener(
    "load",
    (event: Event) => {
      const img = event.target;
      if (img instanceof HTMLImageElement && img.style.backgroundImage !== "") {
        clearPlaceholder(img);
      }
    },
    true,
  );
  document.addEventListener("DOMContentLoaded", () => clearLoadedPlaceholders());
  document.addEventListener("moss-morph-patched", () => clearLoadedPlaceholders());
}
