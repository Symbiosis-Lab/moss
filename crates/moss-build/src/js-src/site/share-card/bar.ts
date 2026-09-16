/**
 * Share-card bottom bar — the v3 "colophon line".
 *
 * Split from share-card.ts (ratchet size budget). Owns title formatting
 * (《》 vs italic), bar layout/measure, the QR plate, and domain
 * extraction. share-card.ts re-exports the public pieces so external
 * imports stay stable.
 */

import { wrapText } from "./text";

export interface Palette {
  bg: string;
  text: string;
  faded: string;
  rule: string;
  meta: string;
  muted: string;
}

const QR_SIZE = 56;
const QR_PLATE = 64;
const QR_RESERVE = QR_SIZE + 16;
const TITLE_LINE_HEIGHT = 19;
const META_LINE_HEIGHT = 16;
const BAR_TOP_GAP = 12;
const BAR_BOTTOM_PAD = 28;

const HAN_RE = /[一-鿿㐀-䶿豈-﫿]/;
const KANA_HANGUL_RE = /[぀-ヿ가-힯]/;

/**
 * Attribution treatment for the article title. Chinese convention marks
 * work titles (篇名) with 书名号 《》 — not 「」, which is the quotation
 * register and already frames the short-quote mode. English titles take
 * italic instead. Pre-bracketed titles are left alone; kana/hangul-only
 * titles stay plain (neither mark reads right).
 */
export function formatCardTitle(title: string): { text: string; italic: boolean } {
  const trimmed = title.trim();
  if (!trimmed) return { text: "", italic: false };
  if (/^[《「『]/.test(trimmed)) return { text: trimmed, italic: false };
  if (HAN_RE.test(trimmed)) return { text: `《${trimmed}》`, italic: false };
  if (KANA_HANGUL_RE.test(trimmed)) return { text: trimmed, italic: false };
  return { text: trimmed, italic: true };
}

/** Trim so that text+tail fits maxWidth, ALWAYS appending the tail — the
 * caller signals that content was dropped. Codepoint-safe so a trailing
 * emoji or ext-B ideograph never splits into a lone surrogate. Exported: the
 * vertical card's reading-end meta columns truncate the same way. */
export function ellipsize(
  ctx: CanvasRenderingContext2D,
  text: string,
  tail: string,
  maxWidth: number
): string {
  let chars = [...text];
  while (
    chars.length > 5 &&
    ctx.measureText(chars.join("") + tail).width > maxWidth
  ) {
    chars = chars.slice(0, -1);
  }
  return chars.join("") + tail;
}

export interface BottomBarLayout {
  barHeight: number;
  titleLines: string[];
  titleFont: string;
  metaText: string;
}

/**
 * Measure the bottom bar for the actual content: wrap the formatted title
 * to at most 2 lines (ellipsized), compose the meta line, and derive the
 * bar height. Called during the measure pass so the canvas height reserves
 * exactly what the bar draws.
 */
export function layoutBottomBar(
  ctx: CanvasRenderingContext2D,
  title: string,
  author: string,
  serifFont: string,
  hasQr: boolean,
  maxInnerWidth: number
): BottomBarLayout {
  const { text, italic } = formatCardTitle(title);
  const titleFont = `${italic ? "italic " : ""}13px ${serifFont}`;
  const titleMaxWidth = maxInnerWidth - (hasQr ? QR_RESERVE : 0);

  ctx.font = titleFont;
  let titleLines: string[] = [];
  if (text) {
    const wrapped = wrapText(ctx, text, titleMaxWidth);
    titleLines = wrapped.slice(0, 2);
    if (wrapped.length > 2) {
      // A clipped 《…》 title keeps its closing bracket: "…》" not "…".
      const tail = text.endsWith("》") ? "…》" : "…";
      titleLines[1] = ellipsize(ctx, titleLines[1], tail, titleMaxWidth);
    }
  }

  const domain = extractDomain();
  ctx.font = `11px ${serifFont}`;
  const metaRaw = author ? `${author} · ${domain}` : domain;
  const metaText =
    ctx.measureText(metaRaw).width <= titleMaxWidth
      ? metaRaw
      : ellipsize(ctx, metaRaw, "…", titleMaxWidth);

  const titleBlock =
    titleLines.length * TITLE_LINE_HEIGHT + (titleLines.length ? 6 : 0);
  const contentHeight = Math.max(
    titleBlock + META_LINE_HEIGHT,
    hasQr ? QR_SIZE : 0
  );

  return {
    barHeight: BAR_TOP_GAP + contentHeight + BAR_BOTTOM_PAD,
    titleLines,
    titleFont,
    metaText,
  };
}

export function drawBottomBar(
  ctx: CanvasRenderingContext2D,
  palette: Palette,
  w: number,
  h: number,
  padding: number,
  layout: BottomBarLayout,
  serifFont: string,
  qrImg: HTMLImageElement | null | undefined,
  isDark: boolean
): void {
  const ruleY = h - layout.barHeight;

  ctx.textBaseline = "top";
  ctx.globalAlpha = 1.0;

  // Rule line
  ctx.fillStyle = palette.rule;
  ctx.fillRect(padding, ruleY, w - padding * 2, 1);

  // Title (left)
  let y = ruleY + BAR_TOP_GAP;
  ctx.fillStyle = palette.meta;
  ctx.font = layout.titleFont;
  for (const line of layout.titleLines) {
    ctx.fillText(line, padding, y);
    y += TITLE_LINE_HEIGHT;
  }

  // Meta line: author · domain (or domain alone)
  if (layout.titleLines.length) y += 6;
  ctx.fillStyle = palette.muted;
  ctx.font = `11px ${serifFont}`;
  ctx.fillText(layout.metaText, padding, y);

  // QR (right). Dark cards get a paper plate under an ink-on-paper code —
  // an inverted (light-on-dark) QR fails in WeChat's scanner, the one that
  // matters most for this card's audience.
  if (qrImg) {
    const qrTop = ruleY + BAR_TOP_GAP;
    if (isDark) {
      const plateX = w - padding - QR_PLATE;
      const plateY = qrTop - (QR_PLATE - QR_SIZE) / 2;
      ctx.fillStyle = "#f5f0e6";
      const c = ctx as CanvasRenderingContext2D & {
        roundRect?: (x: number, y: number, w: number, h: number, r: number) => void;
      };
      if (typeof c.roundRect === "function" && typeof c.beginPath === "function") {
        c.beginPath();
        c.roundRect(plateX, plateY, QR_PLATE, QR_PLATE, 4);
        c.fill();
      } else {
        ctx.fillRect(plateX, plateY, QR_PLATE, QR_PLATE);
      }
      ctx.drawImage(
        qrImg,
        plateX + (QR_PLATE - QR_SIZE) / 2,
        plateY + (QR_PLATE - QR_SIZE) / 2,
        QR_SIZE,
        QR_SIZE
      );
    } else {
      ctx.drawImage(qrImg, w - padding - QR_SIZE, qrTop, QR_SIZE, QR_SIZE);
    }
  }
}

// ── Metadata helpers ────────────────────────────────────────────────────

export function extractDomain(): string {
  // Display strips a leading "www." — machine URLs bake the serving host
  // (www for CDN-fronted domains), the card shows the memorable apex.
  const display = (host: string) => host.replace(/^www\./, "");
  // 1. Try canonical link (set at build time)
  const canonical = document.querySelector<HTMLLinkElement>(
    'link[rel="canonical"]'
  );
  if (canonical?.href) {
    try {
      return display(new URL(canonical.href).hostname);
    } catch {}
  }
  // 2. Try og:url (set at build time)
  const ogUrl = document.querySelector<HTMLMetaElement>(
    'meta[property="og:url"]'
  );
  if (ogUrl?.content) {
    try {
      return display(new URL(ogUrl.content).hostname);
    } catch {}
  }
  // 3. Fall back to runtime hostname
  return display(window.location.hostname) || "liu-guo.com";
}

