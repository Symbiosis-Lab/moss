// How tall is a live-preview block row? — the single owner of that answer.
//
// CM6 asks a block WidgetType for `estimatedHeight` to lay out the document
// before the widget's real DOM is measured, then swaps in the measurement when
// the block scrolls into view. Every pixel of divergence is a scroll jump: the
// document height changes under the user with no input — the "CM6 layout
// shifted with no user action" churn.
//
// Every block widget that owns a row gets its estimate from here: the picture
// and media-card blocks drawn below an embed's source line
// (cm-block-embed-widgets.ts). They used to each do their
// own arithmetic, or none — which is how the media widget ended up returning
// one flat 180px for BOTH the audio and the video player (audio really
// renders 62px), and how the row's `padding-block` got added to the measured
// root but forgotten in two of three estimates. (A table was a third such
// widget; it is styled source now, not a block widget, and carries no estimate
// here.)
//
// The invariant this module exists to hold: **every exported function returns
// the height of the widget ROOT** — the box CM6 measures — with that root's own
// vertical padding already folded in. Callers add nothing.
//
// The height functions are pure + separate from the CM6 integration (project
// convention) so they are testable without a live EditorView. The `refresh*` /
// `sample*` exports are the impure ones: they read live layout to keep the pure
// functions fed with real numbers, and they carry one legality rule, stated
// once here and referenced from each definition:
//
//   LAYOUT READS ARE LEGAL ONLY in a widget `toDOM` or a CM6 `requestMeasure`
//   READ phase. NEVER in a ViewPlugin `update()`, which throws "Reading the
//   editor layout isn't allowed during an update".

/** Column-width fallback when the real editor column hasn't been measured yet
 *  (matches the historical `600 - 56` text-column guess). */
export const FALLBACK_COLUMN_WIDTH = 544;

/** Total vertical padding on `.cm-lp-image-row` — the root CM6 measures for the
 *  image and media blocks (`padding-block: 4px` top + bottom, see
 *  cm-block-embeds.css). Folded into every height below rather than left to
 *  callers. Keep in step with the CSS. */
export const ROW_PADDING = 8;

// ── Chrome heights ──────────────────────────────────────────────────────────
//
// These describe boxes whose height comes from the user agent or from font
// metrics, not from a number in our CSS, so there is nothing to derive them
// from statically:
//
//  - audio: the native control bar. Fixed height, no intrinsic media size
//    involved — 54px in Chromium at every wrapper width from 1px to 2000px.
//  - video: see the aspect-ratio cache below.
//  - card: the placeholder card, one line of icon + label + name plus its own
//    padding. ~44px, and it CANNOT rewrap
//    at any width — the name is `white-space: nowrap` + ellipsis and every
//    sibling is `flex-shrink: 0`. Its height tracks `.cm-scroller`'s
//    line-height, so it moves with the user's font settings.
//
// The defaults are Chromium's at default font settings; WKWebView (macOS) and
// WebKitGTK (Linux) draw a different control bar, and any font-size change
// moves the rest. `sampleBlockChromeHeights` corrects them all from live
// layout, so these numbers are a starting point, not a promise — but they are
// also what the FIRST layout of a document uses, before anything has rendered
// to sample, so they need to be right.
let audioPlayerHeight = 54;
let mediaCardHeight = 44;

/** Smallest height worth believing from a live sample. A real control bar,
 *  card or table row is a whole line of text tall; anything below this is a
 *  detached, `display:none` or mid-teardown element, and latching it would
 *  collapse every off-viewport estimate for the rest of the session. */
const MIN_PLAUSIBLE_CHROME = 16;

// ── Video aspect ratios ─────────────────────────────────────────────────────
//
// A `<video preload="metadata">` has NO intrinsic size until metadata arrives,
// so CSS gives the replaced element its default 300x150 box (CSS Images 3,
// default sizing) and the row measures ROW_PADDING + 150. That is correct for a
// few hundred milliseconds — and then metadata lands, the element takes its
// real aspect ratio, and the row can become anything: measured at a 300px
// player, 16:9 → 176.75, 4:3 → 233, 1:1 → 308, 9:16 portrait → 541.33. A flat
// 150 is off by -383px on that last one, three times the audio bug this module
// was written for and in the growing direction.
//
// The backend can't help at estimate time: asset_resolver.rs probes dimensions
// for `image/*` only. But the DOM can — a video that has ever been on screen
// reports `videoWidth`/`videoHeight`, so we record the ratio per source and
// every LATER layout of that source is exact. First sight of a given video is
// still 150; closing that needs backend probing.
const videoAspect = new Map<string, number>();

/** Record a video's real aspect ratio, keyed by source URL. Called from the
 *  widget's own `loadedmetadata` handler — reads media metadata, not layout, so
 *  the legality rule above does not apply. */
export function recordVideoAspect(src: string | undefined, w: number, h: number): void {
  if (src && w > 0 && h > 0) videoAspect.set(src, w / h);
}

/** The block shapes MediaEmbedBlockWidget can render. */
export type MediaRowShape = 'audio' | 'video' | 'card';

// [perf] Last-measured editor text-column width. `estimatedHeight` runs for
// off-viewport blocks with NO view access, so it reads this instead of a fixed
// guess that diverges from the real column and swings the doc height (no-drag
// CM6 shift).
let cachedColumnWidth = 0;

/** The live editor text column, or the fallback before anything is measured. */
export function editorColumnWidth(): number {
  return cachedColumnWidth || FALLBACK_COLUMN_WIDTH;
}

/** Refresh the column-width cache from LIVE layout. IMPURE — see the legality
 *  rule at the top of this file. A stale value during a resize makes CM6
 *  re-anchor the scroll every frame → a 6-10s editor freeze on image-heavy
 *  docs. Keep the >120 guard. */
export function refreshEditorColumnWidth(dom: HTMLElement | null | undefined): void {
  if (!dom) return;
  // Measure a REAL line's resolved paddingLeft, never the custom property:
  // --moss-editor-line-inset is a `max()` (see .moss-editor in editor.css), and
  // getPropertyValue hands back the unresolved string, so parseFloat yields NaN
  // and this silently fell back to 16 — a full-width column would be estimated
  // at offsetWidth - 32 instead of the real capped measure.
  const line = dom.querySelector<HTMLElement>('.cm-line');
  const inset = parseFloat(line ? getComputedStyle(line).paddingLeft : '') || 16;
  const colW = dom.offsetWidth - 2 * inset;
  if (colW > 120) cachedColumnWidth = colW;
}

/** `offsetHeight` of the first match, or 0. Deliberately NOT
 *  `getBoundingClientRect().height`: that returns the TRANSFORMED box, so an
 *  ancestor `scale()` would latch a shrunken chrome height that still clears
 *  the plausibility floor. `offsetHeight` is layout-space and immune. */
function sample(root: HTMLElement, selector: string): number {
  return (root.querySelector(selector) as HTMLElement | null)?.offsetHeight ?? 0;
}

/**
 * Correct every chrome height from LIVE layout. IMPURE — see the legality rule
 * at the top of this file.
 *
 * A widget already on screen tells us what this engine and this font size
 * really produce, so the estimates for widgets still below the fold track the
 * user's actual rendering rather than the constants above.
 */
export function sampleBlockChromeHeights(contentDOM: HTMLElement | null | undefined): void {
  if (!contentDOM) return;
  const audio = sample(contentDOM, '.cm-media-embed-player > audio');
  if (audio > MIN_PLAUSIBLE_CHROME) audioPlayerHeight = audio;
  const card = sample(contentDOM, '.cm-media-embed-card');
  if (card > MIN_PLAUSIBLE_CHROME) mediaCardHeight = card;
}

/** Options for the shapes whose height is not a constant. */
export interface MediaRowContext {
  /** The resolved `moss-source://` source — the key the video ratio is cached under. */
  src?: string;
  /** The `|NN%` width token, if the author set one. */
  widthToken?: string;
  /** The live editor text column, for the shapes that scale with it. */
  columnWidth?: number;
}

/**
 * Height of the `.cm-lp-image-row` root for a media embed of the given shape.
 * Includes ROW_PADDING.
 *
 * `audio` and `card` are constants (both self-calibrating, see above). `video`
 * needs the context: once its aspect ratio is known it scales with the column
 * exactly the way an image does, and unlike an image it DOES upscale past its
 * intrinsic width (`.cm-media-embed-video { width: 100% }`).
 */
export function mediaRowHeight(shape: MediaRowShape, ctx: MediaRowContext = {}): number {
  if (shape === 'audio') return ROW_PADDING + audioPlayerHeight;
  if (shape !== 'video') return ROW_PADDING + mediaCardHeight;

  const ratio = ctx.src ? videoAspect.get(ctx.src) : undefined;
  // Never seen this video play: it is still the 300x150 default replaced box.
  if (!ratio) return ROW_PADDING + 150;
  const column = ctx.columnWidth && ctx.columnWidth > 0 ? ctx.columnWidth : FALLBACK_COLUMN_WIDTH;
  return ROW_PADDING + Math.round((column * widthFraction(ctx.widthToken)) / ratio);
}

/** The `|NN%` token as a fraction of the column; 1 when absent or unparseable
 *  (both widgets fall back to a full-width block in that case). */
function widthFraction(widthToken: string | undefined): number {
  if (!widthToken) return 1;
  const pct = /^(\d+(?:\.\d+)?)%$/.exec(widthToken.trim());
  return pct ? parseFloat(pct[1]) / 100 : 1;
}

/**
 * How tall a body image may be in the editor, in px.
 *
 * A phone photo is about three times taller than it is wide, so at the column
 * width it fills the pane and the paragraph after it is off screen — you scroll
 * past your own picture to reach your next sentence. While you are editing, a
 * picture is a landmark; the text around it is the work.
 *
 * This deliberately breaks size parity with the published page: the editor caps
 * body image height. Do not remove that cap on parity grounds alone; the
 * trade-off is the part to argue with.
 */
export const IMAGE_HEIGHT_CAP = 400;

/**
 * The width a block image actually renders at — the ONE owner of that answer,
 * so `toDOM` and `imageRowHeight` cannot drift apart. A divergence here is a
 * scroll jump: CM6 lays the document out from the estimate and then swaps in
 * the measurement.
 *
 * Three rules, in order:
 *  - `|NN%` width token → that fraction of the column;
 *  - otherwise → never upscaled past the intrinsic px;
 *  - either way → narrow enough that the height lands within IMAGE_HEIGHT_CAP.
 *
 * The cap is applied to the WIDTH rather than as a `max-height` in CSS on
 * purpose. Capping the height of a box whose width is already fixed either
 * crops the picture (the wrapper sets `overflow: hidden`) or letterboxes it
 * inside empty bars. Narrowing instead keeps the whole picture, at the right
 * shape, in a box that is exactly as big as what is in it.
 */
export function imageDisplayWidth(
  intrinsicWidth: number,
  intrinsicHeight: number,
  widthToken: string | undefined,
  columnWidth: number,
): number {
  const column = columnWidth > 0 ? columnWidth : FALLBACK_COLUMN_WIDTH;
  const requested = widthToken
    ? column * widthFraction(widthToken)
    : Math.min(column, intrinsicWidth);
  return Math.min(requested, imageCapWidth(intrinsicWidth, intrinsicHeight));
}

/**
 * The width at which this image is exactly `IMAGE_HEIGHT_CAP` tall.
 *
 * Depends on the picture alone, never on the column, which is what lets
 * `toDOM` hand it straight to `max-width` in px: a `|NN%` box stays a
 * percentage and keeps following a divider drag, while this clamps it. A
 * column-derived px width would go stale the moment the pane was resized,
 * because `toDOM` does not re-run for a geometry change.
 */
export function imageCapWidth(intrinsicWidth: number, intrinsicHeight: number): number {
  return IMAGE_HEIGHT_CAP * (intrinsicWidth / intrinsicHeight);
}

/**
 * Height of the `.cm-lp-image-row` root for a block image, mirroring `toDOM`'s
 * width policy via the shared `imageDisplayWidth`. Includes ROW_PADDING.
 * Unknown intrinsic dims → a fixed fallback.
 *
 * @param intrinsicWidth  source image width in px (undefined → unknown)
 * @param intrinsicHeight source image height in px (undefined → unknown)
 * @param widthToken      the `width` token (e.g. "50%") or undefined
 * @param columnWidth     the real editor text-column width in px
 */
export function imageRowHeight(
  intrinsicWidth: number | undefined,
  intrinsicHeight: number | undefined,
  widthToken: string | undefined,
  columnWidth: number,
): number {
  if (!intrinsicWidth || !intrinsicHeight) return ROW_PADDING + 200;
  const renderedWidth = imageDisplayWidth(intrinsicWidth, intrinsicHeight, widthToken, columnWidth);
  return ROW_PADDING + Math.round((intrinsicHeight / intrinsicWidth) * renderedWidth);
}
