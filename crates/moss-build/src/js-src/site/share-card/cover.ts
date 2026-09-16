/**
 * The cover picture as an object-fit: cover fill of a `(0, 0, w, h)` rect —
 * the strip across the top of the horizontal card, the band down the left of
 * the vertical one. Its own module so both cards import it without either
 * importing the other.
 */

export function drawCoverImage(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  cardWidth: number,
  stripHeight: number
): void {
  // object-fit: cover math
  const imgRatio = img.naturalWidth / img.naturalHeight;
  const stripRatio = cardWidth / stripHeight;

  let sx = 0,
    sy = 0,
    sw = img.naturalWidth,
    sh = img.naturalHeight;

  if (imgRatio > stripRatio) {
    // Image wider than strip: crop sides
    sw = img.naturalHeight * stripRatio;
    sx = (img.naturalWidth - sw) / 2;
  } else {
    // Image taller than strip: crop top/bottom
    sh = img.naturalWidth / stripRatio;
    sy = (img.naturalHeight - sh) / 2;
  }

  ctx.drawImage(img, sx, sy, sw, sh, 0, 0, cardWidth, stripHeight);
}
