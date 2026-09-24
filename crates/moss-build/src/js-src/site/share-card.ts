/**
 * Share Card v2 — generates a branded quote card image.
 *
 * Two modes:
 * - Short (a single line of <=40 chars): centered 「quote」 with larger type
 * - Long: the captured blocks, set as the page set them, with the context
 *   around the selection faded
 *
 * Supports light/dark palettes, optional cover image, and QR code.
 */

import { showToast } from "./toast";
import { langBucket, type Lang } from "./subscribe/i18n";

// ── Palette from CSS tokens + card overrides ─────────────────────────────

import {
  type Palette,
  formatCardTitle,
  layoutBottomBar,
  drawBottomBar,
  extractDomain,
} from "./share-card/bar";
import {
  type CardBlock,
  type LineSpan,
  type WrappedLine,
  wrapText,
  isShortQuote,
  quoteFontSize,
  wrapSegments,
  addCjkLatinSpacing,
  spaceSegments,
  drawJustifiedLine,
  drawJustifiedWrappedLine,
} from "./share-card/text";
import { type BlockLayout, layoutBlocks, drawBlocks } from "./share-card/blocks";
import { buildVerticalCardCanvas } from "./share-card/vertical";
import { drawCoverImage } from "./share-card/cover";

// Re-exports: external importers and tests reach these through share-card.
export {
  formatCardTitle,
  extractDomain,
  wrapText,
  wrapSegments,
  addCjkLatinSpacing,
  spaceSegments,
  drawJustifiedLine,
  drawJustifiedWrappedLine,
};
export type { LineSpan, WrappedLine };

/**
 * Build card palette from site CSS tokens, then apply card-specific
 * "printed paper" overrides.
 *
 * Shared with site (read from CSS custom properties):
 *   light — text, meta
 *   dark  — bg, meta
 *
 * Card overrides (intentionally different from site):
 *   both  — rule (warmer tint than site border)
 *   both  — muted lifted for AA contrast at the bar's 11px sizes
 *   light — bg (#f5f0e6, warm paper)
 *   dark  — text/faded (#d4ccb8, brighter for card contrast)
 */
export function buildPalette(isDark: boolean): Palette {
  const style = getComputedStyle(document.documentElement);
  const get = (name: string) => style.getPropertyValue(name).trim();

  const text = get("--moss-color-text") || (isDark ? "#c4bba8" : "#2c2825");
  const meta = get("--moss-color-text-secondary") || (isDark ? "#8a8070" : "#716d69");
  const bg = get("--moss-color-bg") || (isDark ? "#1c1914" : "#faf8f5");

  if (isDark) {
    return {
      bg,
      text: "#d4ccb8",   // Brighter than site for card contrast
      faded: "#d4ccb8",
      rule: "#2e2a22",   // Card-specific (site uses #332e25)
      meta,
      muted: "#9a9080", // Lifted from #6a6050 (~2.8:1) — AA at 11px on #1c1914
    };
  }

  return {
    bg: "#f5f0e6",        // Warm paper, not site bg
    text,
    faded: text,
    rule: "#e8e0d0",      // Card-specific (site uses #e6e2db)
    meta,
    muted: "#6e6963", // Lifted from the site token (~3.2:1) — AA at 11px on paper
  };
}

// ── Public API ────────────────────────────────────────────────────────────

export interface ShareOptions {
  blocks?: CardBlock[];
}

function sanitizeFilename(title: string): string {
  return title
    .replace(/[\/\\:*?"<>|]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 60) || "quote";
}

export async function shareAsImage(
  text: string,
  title: string,
  author: string,
  shareOpts: ShareOptions = {}
): Promise<void> {
  // Concurrently, not one after the other: each load has its own 1500 ms
  // budget, and awaiting them in sequence spent the first before the second
  // began. On a slow connection that made the QR miss a deadline the cover
  // had already used up, so a card lost both or neither.
  const [coverImg, qrImg] = await Promise.all([loadCoverImage(), loadQrImage()]);
  const canvas = buildCardCanvas(text, title, author, {
    blocks: shareOpts.blocks,
    coverImg,
    qrImg,
  });
  const blob = await canvasToBlob(canvas);
  const safeTitle = sanitizeFilename(title);

  // Show lightbox immediately (user can preview + retry/download). The preview
  // reuses the blob rather than calling `toDataURL()`, which would encode the
  // same card to PNG a second time — on a phone, on a tall card, that is the
  // most expensive thing this function does, and it was done twice for one
  // image.
  showShareLightbox(blob, safeTitle);

  // Fire share action simultaneously
  const file = new File([blob], `${safeTitle}.png`, { type: "image/png" });
  if (navigator.canShare?.({ files: [file] })) {
    try {
      await navigator.share({ files: [file] });
      return;
    } catch {
      // User cancelled — lightbox stays open for retry
    }
  }

  // Clipboard fallback (lightbox stays open)
  try {
    await navigator.clipboard.write([
      new ClipboardItem({ "image/png": blob }),
    ]);
    showToast();
  } catch {
    // Neither worked — lightbox is already showing for download
  }
}

export interface CardOptions {
  blocks?: CardBlock[];
  coverImg?: HTMLImageElement | null;
  qrImg?: HTMLImageElement | null;
}

export function buildCardCanvas(
  text: string,
  title: string,
  author: string,
  options: CardOptions = {}
): HTMLCanvasElement {
  const { blocks, coverImg, qrImg } = options;
  const isDark = detectDarkMode();
  const palette = buildPalette(isDark);
  const serifFont = getComputedStyle(document.documentElement)
    .getPropertyValue("--moss-font-heading").trim() || "serif";

  // A vertical page gets a vertical card. Dispatched here rather than threaded
  // through the passes below: the two cards share only the palette, the
  // wrapper and the bar.
  if (document.body.dataset.typesetting === "vertical") {
    // The vertical card's meta column carries a site/author line; most moss
    // pages never set `meta[name="author"]`, so the vertical card alone
    // falls back to the site's own title — `og:site_name` —
    // rather than dropping the line. Scoped to this branch only: the
    // horizontal card's byte-identity regression test pins its output to a
    // caller that never sets an author either.
    const cardAuthor =
      author ||
      document.querySelector('meta[property="og:site_name"]')?.getAttribute("content") ||
      "";
    return buildVerticalCardCanvas(text, title, cardAuthor, {
      palette, serifFont, isDark, coverImg, qrImg, blocks,
    });
  }

  const scale = 2;
  const w = 375;
  const padding = 36;
  const maxWidth = w - padding * 2;
  const isShort = isShortQuote(text);

  // Determine if we use block-aware rendering (long mode + blocks provided)
  const useBlocks = !isShort && blocks && blocks.length > 0;

  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    canvas.width = w * scale;
    canvas.height = 300 * scale;
    return canvas;
  }

  // ── Measure pass: calculate total height ──────────────────────────────

  const bodyFontSize = quoteFontSize(isShort);
  const lineHeight = 1.85;
  const lineStep = bodyFontSize * lineHeight;
  const paragraphGap = lineStep * 0.6;

  ctx.font = `${bodyFontSize}px ${serifFont}`;

  // Cover strip height
  const coverHeight = coverImg ? 140 : 0;

  // Body text measurement
  let bodyHeight = 0;

  // Block metrics shared by the measure and draw passes, so the height
  // reserved for a block is by construction the height it draws in.
  const blockMetrics = { bodyFontSize, serifFont, maxWidth, paragraphGap };
  let blockRows: BlockLayout[] = [];
  let legacyBodyLines: string[] = [];

  if (useBlocks) {
    const laid = layoutBlocks(ctx, blocks, blockMetrics);
    blockRows = laid.rows;
    bodyHeight += laid.height;
    ctx.font = `${bodyFontSize}px ${serifFont}`;
  } else {
    // No blocks: short mode, or a capture that found nothing to structure.
    const spacedText = addCjkLatinSpacing(text);
    const displayText = isShort ? `\u300C${spacedText}\u300D` : spacedText;
    legacyBodyLines = wrapText(ctx, displayText, maxWidth);
    bodyHeight += legacyBodyLines.length * lineStep;
  }

  // Bottom bar: measured for the actual title (1–2 lines), so a one-line
  // title doesn't reserve a phantom second line and bottom padding isn't
  // counted twice (the old formula left a ~60pt dead band above the rule).
  const barLayout = layoutBottomBar(ctx, title, author, serifFont, !!qrImg, maxWidth);

  // Short mode extra vertical centering padding
  const topTextPadding = isShort ? 48 : padding;

  const h = Math.max(
    300,
    coverHeight + topTextPadding + bodyHeight + 28 + barLayout.barHeight
  );

  canvas.width = w * scale;
  canvas.height = h * scale;
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;

  ctx.scale(scale, scale);

  // ── Draw background ───────────────────────────────────────────────────

  ctx.fillStyle = palette.bg;
  ctx.fillRect(0, 0, w, h);

  // ── Draw cover image ──────────────────────────────────────────────────

  if (coverImg) {
    drawCoverImage(ctx, coverImg, w, coverHeight);
  }

  // ── Draw body text ────────────────────────────────────────────────────

  ctx.textBaseline = "top";
  let y = coverHeight + topTextPadding;

  if (isShort) {
    // Short mode: centered quote with larger type
    ctx.fillStyle = palette.text;
    ctx.font = `${bodyFontSize}px ${serifFont}`;
    ctx.globalAlpha = 1.0;

    const bodyLines = legacyBodyLines;
    // Center vertically in available space
    const availableSpace = h - coverHeight - barLayout.barHeight - topTextPadding;
    const textBlockHeight = bodyLines.length * lineStep;
    y = coverHeight + topTextPadding + (availableSpace - textBlockHeight) / 2;

    for (const line of bodyLines) {
      // Center horizontally
      const lineWidth = ctx.measureText(line).width;
      const x = (w - lineWidth) / 2;
      ctx.fillText(line, x, y);
      y += lineStep;
    }
  } else if (useBlocks) {
    y = drawBlocks(ctx, blockRows, palette, padding, y, blockMetrics);
    ctx.font = `${bodyFontSize}px ${serifFont}`;
  } else {
    // No blocks — draw the quote as plain wrapped prose. Capture always
    // produces blocks for a real selection, so this is the safety net, not a
    // second layout engine: the "context before / context after" pass that
    // used to live here could not run once blocks existed, and its twin in
    // `captureContext` recomputed the same thing on every mouseup.
    ctx.fillStyle = palette.text;
    ctx.globalAlpha = 1.0;
    ctx.font = `${bodyFontSize}px ${serifFont}`;
    for (let i = 0; i < legacyBodyLines.length; i++) {
      drawJustifiedLine(
        ctx, legacyBodyLines[i], padding, y, maxWidth,
        i === legacyBodyLines.length - 1
      );
      y += lineStep;
    }
  }

  // ── Draw bottom bar ───────────────────────────────────────────────────

  drawBottomBar(ctx, palette, w, h, padding, barLayout, serifFont, qrImg, isDark);

  return canvas;
}

// ── Theme detection ─────────────────────────────────────────────────────

function detectDarkMode(): boolean {
  return document.documentElement.getAttribute("data-theme") === "dark";
}

// ── Cover image loading ─────────────────────────────────────────────────

/**
 * The page's cover picture, as the build named it — not as this script can
 * guess it.
 *
 * moss writes `data-share-cover="<url>"` on `<article class="container">`
 * whenever the page has an author-chosen cover: its `:::hero` image, else its
 * `cover:` frontmatter. No attribute means no cover, and the card draws none.
 * The value is a same-origin URL for an image the page already loads, so it
 * is registered in the AssetRegistry (ADR-013) and survives the
 * `crossOrigin = "anonymous"` fetch below.
 *
 * One writer, one reader, and nothing in between to rot. The previous version
 * of this function looked for `.article-cover img` — a class moss has never
 * emitted in any version — so every article silently lost its cover strip and
 * every test stayed green, because the tests built the element themselves.
 *
 * The og:image meta tag is NOT a cover source and must not become one again:
 * on a page without `cover:` it holds the auto-generated 1200×630 title card
 * (tofu for CJK titles), and it is an absolute URL, so a www/bare-apex
 * mismatch turns the crossOrigin fetch into a CORS failure.
 */
export function findCoverSource(): string | null {
  const raw = document
    .querySelector("article.container")
    ?.getAttribute("data-share-cover");
  if (!raw) return null;
  return new URL(raw, location.href).href;
}

/**
 * The cover as the page already has it, or `null` if the page is not showing it.
 *
 * `data-share-cover` names the *source* file, but the page renders it through a
 * `<picture>` and every current browser takes the WebP variant — so loading the
 * attribute URL fetched a file the reader had never downloaded (on one site, the
 * 174 KB original beside the 78 KB `w800.webp` already in cache) and raced it
 * against a 1500 ms deadline. Cold, over a phone connection, that lost, and the
 * card came out with no cover band at all.
 *
 * Matching on the `<img>` whose `src` is the attribute and reading its
 * `currentSrc` gives the variant the browser actually chose, already decoded.
 * Same-origin, so the canvas is not tainted and `toBlob` still works —
 * `findCoverSource`'s contract is that the value is a same-origin URL.
 */
function coverAlreadyOnThePage(src: string): HTMLImageElement | null {
  for (const img of document.querySelectorAll("img")) {
    if (img.src === src) return img;
  }
  return null;
}

async function loadCoverImage(): Promise<HTMLImageElement | null> {
  const src = findCoverSource();
  if (!src) return null;

  // A `<picture>`'s `<img>` node can report a `naturalWidth`/`naturalHeight`
  // that no longer matches its own `currentSrc` — the browser re-picks a
  // `srcset` candidate on a later layout pass and the live element's decoded
  // bitmap can lag one step behind the property that names it. `drawImage`
  // on that stale node then paints only the fraction of the destination its
  // old (differently-cropped) bitmap covers, leaving the rest whatever was
  // already on the canvas — reproduced against a real vault page, not
  // theoretical. A fresh `Image()` for the same URL sidesteps it: same
  // origin, already in the browser cache, so this costs no real network
  // round trip even though it goes through the same load path as a cold one.
  const onPage = coverAlreadyOnThePage(src);
  // Still in flight (or lazy and not yet reached): load the variant it chose,
  // never the source file, so this at worst joins a request already made.
  const fetchSrc = onPage?.currentSrc || src;

  return new Promise<HTMLImageElement | null>((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    // 1500ms: a real cover photo rarely beats the old 500ms cold — the
    // card silently varied with network state.
    const timeout = setTimeout(() => resolve(null), 1500);
    img.onload = () => {
      clearTimeout(timeout);
      resolve(img);
    };
    img.onerror = () => {
      clearTimeout(timeout);
      resolve(null);
    };
    img.src = fetchSrc;
  });
}

// ── QR image loading ────────────────────────────────────────────────────

/**
 * The page's QR code, as the build named it — not as this script can guess it.
 *
 * moss writes `data-share-qr="<url>"` on `<article class="container">` for every
 * page that has one, exactly as it writes `data-share-cover`. No attribute means
 * no QR code, and the card draws none: an unpublished site has no real URL to
 * encode, and a draft is not public.
 *
 * This function used to be `qrKeyForPathname`, which rebuilt the filename from
 * `location.pathname` and had to stay byte-identical to `qr_key_for_url_path` in
 * `src-tauri/src/build/media/qr.rs`. A mismatch 404s in silence and a missing QR
 * looks exactly like a site that never had one, so both halves shipped broken to
 * real sites at once — one serving fourteen codes for the wrong page, another
 * missing every folder note's code. The reader derives nothing now, so there is
 * no arithmetic left to disagree about.
 */
export function findQrSource(): string | null {
  const raw = document
    .querySelector("article.container")
    ?.getAttribute("data-share-qr");
  if (!raw) return null;
  return new URL(raw, location.href).href;
}

async function loadQrImage(): Promise<HTMLImageElement | null> {
  const isDark = detectDarkMode();
  const palette = buildPalette(isDark);

  const src = findQrSource();
  if (!src) return null;

  try {
    // Same 1500ms budget as the cover, and for the same reason — but here it
    // has teeth: a server that accepts the connection and never answers leaves
    // a bare `fetch` pending forever, and the whole Share action is awaiting it.
    // The reader taps the button and nothing happens, with no error to catch
    // and no toast to show. A card with an empty corner is the right failure.
    const resp = await fetch(src, { signal: AbortSignal.timeout(1500) });
    if (!resp.ok) return null;
    let svgText = await resp.text();

    // Remove background rect (the card supplies the background: paper bg in
    // light mode, a paper plate in dark mode) and recolor the modules to
    // full ink. Never a light-on-dark code — see drawBottomBar's plate.
    svgText = svgText.replace(/<rect[^>]*fill="#ffffff"[^>]*\/?>/, "");
    const moduleColor = isDark ? "#2c2825" : palette.text;
    svgText = svgText.replace(/fill="#000000"/g, `fill="${moduleColor}"`);

    const blob = new Blob([svgText], { type: "image/svg+xml" });
    const blobUrl = URL.createObjectURL(blob);

    return new Promise<HTMLImageElement | null>((resolve) => {
      const img = new Image();
      img.onload = () => {
        URL.revokeObjectURL(blobUrl);
        resolve(img);
      };
      img.onerror = () => {
        URL.revokeObjectURL(blobUrl);
        resolve(null);
      };
      img.src = blobUrl;
    });
  } catch {
    return null;
  }
}

// ── Canvas to Blob ──────────────────────────────────────────────────────

function canvasToBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve) => {
    canvas.toBlob((blob) => resolve(blob!), "image/png");
  });
}

const COPY: Record<Lang, { alt: string; share: string; download: string }> = {
  en: {
    alt: "Share card",
    share: "Share",
    download: "Download",
  },
  "zh-hans": {
    alt: "分享卡片",
    share: "分享",
    download: "下载",
  },
  "zh-hant": {
    alt: "分享卡片",
    share: "分享",
    download: "下載",
  },
};

// ── Share lightbox ───────────────────────────────────────────────────────

/**
 * Dismisses whatever lightbox is open, if any. One card is on screen at a time:
 * tapping Share twice used to stack a second overlay on the first, so dismissing
 * revealed another identical card underneath, and every earlier overlay kept its
 * `keydown` listener and its object URL alive behind it.
 */
let dismissOpenLightbox: (() => void) | null = null;

function showShareLightbox(blob: Blob, filename: string): void {
  dismissOpenLightbox?.();

  const copy = COPY[langBucket(document.documentElement.lang)];

  const overlay = document.createElement("div");
  overlay.className = "share-card-overlay";

  const previewUrl = URL.createObjectURL(blob);
  const img = document.createElement("img");
  img.src = previewUrl;
  img.alt = copy.alt;

  const btnBar = document.createElement("div");
  btnBar.className = "pill-bar";

  const shareBtn = document.createElement("button");
  shareBtn.className = "pill-bar-btn";
  shareBtn.textContent = copy.share;
  shareBtn.type = "button";

  const downloadBtn = document.createElement("button");
  downloadBtn.className = "pill-bar-btn";
  downloadBtn.textContent = copy.download;
  downloadBtn.type = "button";

  btnBar.append(shareBtn, downloadBtn);

  const inner = document.createElement("div");
  inner.className = "share-lightbox-inner";
  inner.append(img, btnBar);
  overlay.appendChild(inner);

  // Dismiss
  const dismiss = () => {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
    URL.revokeObjectURL(previewUrl);
    if (dismissOpenLightbox === dismiss) dismissOpenLightbox = null;
  };
  dismissOpenLightbox = dismiss;
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") dismiss();
  };
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) dismiss();
  });
  document.addEventListener("keydown", onKey);

  // Share button (retry — lightbox stays open for further actions)
  shareBtn.addEventListener("click", async () => {
    const file = new File([blob], `${filename}.png`, { type: "image/png" });
    if (navigator.canShare?.({ files: [file] })) {
      try {
        await navigator.share({ files: [file] });
        return;
      } catch {}
    }
    try {
      await navigator.clipboard.write([
        new ClipboardItem({ "image/png": blob }),
      ]);
      showToast();
    } catch {}
  });

  // Download button
  downloadBtn.addEventListener("click", () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${filename}.png`;
    a.click();
    URL.revokeObjectURL(a.href);
    dismiss();
  });

  document.body.appendChild(overlay);
}
