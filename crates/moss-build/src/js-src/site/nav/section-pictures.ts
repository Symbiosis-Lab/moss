/**
 * section-pictures.ts — the small picture that stands for a section in a
 * contents list that has asked for pictures (`contents: open`).
 *
 * A section's picture is the first `<img>` between its heading and the next
 * heading of the same or a higher level, so a picture inside a subsection
 * belongs to the section that contains it. The picture is read off the image
 * already on the page rather than a new asset: of the widths moss generated
 * for that image (its responsive ladder, in the `srcset` of the `<picture>`'s
 * source or of the `<img>`), the narrowest is used. A list row is a few dozen
 * pixels wide, so the full-size file the page's own `src` names would be many
 * times the bytes for nothing. An image with no ladder (SVG, a small raster)
 * has one file, which is used.
 */

/** Whether the page asked for its contents to stay open, with pictures. */
export function contentsOpen(): boolean {
  return document.body.dataset.contents === "open";
}

/** The narrowest candidate of a `srcset`, or null when there is none. */
function narrowest(srcset: string | null | undefined): string | null {
  let best: { url: string; w: number } | null = null;
  for (const candidate of (srcset ?? "").split(",")) {
    const [url, descriptor] = candidate.trim().split(/\s+/);
    if (!url) continue;
    const w = descriptor?.endsWith("w") ? parseInt(descriptor, 10) : Infinity;
    if (!best || w < best.w) best = { url, w };
  }
  return best?.url ?? null;
}

/** The file to show for an image already on the page. */
export function thumbnailUrl(img: HTMLImageElement): string | null {
  const source = img.closest("picture")?.querySelector("source[srcset]");
  return narrowest(source?.getAttribute("srcset")) ?? narrowest(img.getAttribute("srcset")) ?? img.getAttribute("src");
}

const level = (heading: Element): number => Number(heading.tagName[1]);

/** For each section heading, the thumbnail of its first image, or null. */
export function sectionPictures(headings: HTMLElement[]): Array<string | null> {
  const main = headings[0]?.closest("main") ?? document.body;
  const all = [...main.querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6")];
  const images = [...main.querySelectorAll<HTMLImageElement>("img")];
  return headings.map((heading) => {
    const end = all.find(
      (h) => h !== heading && level(h) <= level(heading) && heading.compareDocumentPosition(h) & Node.DOCUMENT_POSITION_FOLLOWING,
    );
    const img = images.find(
      (i) =>
        heading.compareDocumentPosition(i) & Node.DOCUMENT_POSITION_FOLLOWING &&
        (!end || end.compareDocumentPosition(i) & Node.DOCUMENT_POSITION_PRECEDING),
    );
    return img ? thumbnailUrl(img) : null;
  });
}

/** A thumbnail `<img>`; when its file will not load it removes itself, so the
 *  row falls back to its dash. */
export function pictureImg(url: string, onGone: () => void): HTMLImageElement {
  const img = document.createElement("img");
  img.alt = "";
  img.loading = "lazy";
  img.decoding = "async";
  img.src = url;
  img.addEventListener("error", () => {
    img.remove();
    onGone();
  });
  return img;
}
