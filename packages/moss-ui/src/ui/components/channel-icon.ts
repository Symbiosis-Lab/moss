/**
 * ChannelIcon — a presentational primitive: glyph + optional corner badge.
 *
 * Owns: DOM structure, badge SVGs, corner positioning.
 * Does not own: click behavior, tooltips, domain semantics. Wrap in a
 * `<button>` with whatever click/tooltip behavior the caller needs.
 *
 * Callers translate their own state (plugin lifecycle, deploy state, …) into
 * the badge prop at the boundary; ChannelIcon stays oblivious to what the
 * states mean.
 */

/**
 * The disc's hue. Absent = success green, the default for every morph pose.
 * `attention` repaints it amber — the channel rail's "needs attention" lamp.
 * It is a tone rather than a kind
 * so the ring still MORPHS into it: a channel that fails goes ring → amber
 * dot on paint properties alone, with no node swap.
 */
export type ChannelIconTone = "attention";

export type ChannelIconBadge =
  | { kind: "spinner"; dim?: boolean } // dim:true → queued (opacity .45, pulse); false/absent → sending
  // circle and dot are ONE persistent element — a disc posed two ways via CSS
  // — so armed → published is the ring filling in, not a node swap.
  //
  // The install lifecycle (plus / minus / check / hidden / failed poses) lives
  // in `components/install-ring.ts`, on top of
  // `shared/icon-morph.ts`, which rewrites path data per frame — the only
  // technique that can bend a bar into an arc at all. Do not re-add a pose
  // here.
  | { kind: "circle"; tone?: ChannelIconTone } // outline ring — disc stroked, unfilled
  | { kind: "dot"; tone?: ChannelIconTone };   // filled disc — circle's fill morphs in

export interface ChannelIconProps {
  /** Inline SVG markup for the main glyph. If null/invalid, falls back to a letter. */
  iconSvg: string | null;
  /** Letter used when `iconSvg` is missing. */
  fallbackLetter?: string;
  /** Optional corner badge. Omit for no badge. */
  badge?: ChannelIconBadge;
}

export function createChannelIcon(props: ChannelIconProps): HTMLElement {
  const root = document.createElement("div");
  root.className = "channel-icon";

  root.appendChild(renderGlyph(props.iconSvg, props.fallbackLetter ?? "?"));

  if (props.badge) {
    root.appendChild(renderBadge(props.badge));
  }

  return root;
}

/**
 * Update an existing ChannelIcon's badge in place.
 * Pass `undefined` to remove it.
 */
export function updateChannelIconBadge(
  root: HTMLElement,
  badge: ChannelIconBadge | undefined,
): void {
  const existing = root.querySelector(".channel-icon-badge");
  // Morph family → keep the node and flip its pose in place. The whole point
  // of the morph vocabulary is that plus/minus/check/hidden are ONE element
  // whose strokes transition between states; replacing it would jump-cut.
  if (
    existing instanceof HTMLElement &&
    badge &&
    existing.classList.contains("channel-icon-badge--morph") &&
    isMorphKind(badge.kind)
  ) {
    applyMorphShape(existing, badge.kind, toneOf(badge));
    return;
  }
  // Same kind → keep the node. Replacing it restarts CSS animations from 0,
  // so a spinner updated on every progress tick never completes a rotation
  // (it reads as stuck). Badge markup is static per kind; only the queued
  // dim state can differ, so reconcile that in place.
  if (existing && badge && existing.classList.contains(`channel-icon-badge--${badge.kind}`)) {
    existing.classList.toggle("is-queued", badge.kind === "spinner" && badge.dim === true);
    return;
  }
  if (existing) existing.remove();
  if (badge) root.appendChild(renderBadge(badge));
}

function renderGlyph(iconSvg: string | null, fallbackLetter: string): HTMLElement {
  const wrapper = document.createElement("div");
  wrapper.className = "channel-icon-glyph";

  if (iconSvg && isValidSvg(iconSvg)) {
    // Strip fixed dimensions from the markup, not the live element: WebKit
    // logs `Invalid value for <svg> attribute width=""` when an SVG geometry
    // attribute is removed after parsing (the empty re-parse fails).
    wrapper.innerHTML = stripRootSvgDimensions(iconSvg);
    const svg = wrapper.querySelector("svg");
    if (svg) {
      svg.classList.add("channel-icon-svg");
    }
    return wrapper;
  }

  const fallback = document.createElement("span");
  fallback.className = "channel-icon-fallback";
  fallback.textContent = fallbackLetter.charAt(0).toUpperCase();
  wrapper.appendChild(fallback);
  return wrapper;
}

function renderBadge(badge: ChannelIconBadge): HTMLElement {
  const el = document.createElement("div");
  el.className = `channel-icon-badge channel-icon-badge--${badge.kind}`;

  switch (badge.kind) {
    case "spinner":
      el.innerHTML = spinnerSvg();
      // queued = dim/pulsing spinner; add class so CSS can reduce opacity
      if (badge.dim) el.classList.add("is-queued");
      break;
    case "circle":
    case "dot":
      el.classList.add("channel-icon-badge--morph");
      el.innerHTML = morphSvg();
      applyMorphShape(el, badge.kind, toneOf(badge));
      break;
  }

  return el;
}

function isValidSvg(content: string): boolean {
  const trimmed = content.trim();
  return trimmed.startsWith("<svg") || trimmed.includes("<svg");
}

/**
 * Remove width/height from the root `<svg>` tag so CSS sizes the glyph.
 * Nested shapes (`<rect width=…>`) keep theirs — only the first svg open-tag
 * is rewritten.
 */
function stripRootSvgDimensions(markup: string): string {
  return markup.replace(/<svg\b[^>]*>/i, (tag) =>
    tag.replace(/\s+(?:width|height)\s*=\s*("[^"]*"|'[^']*'|[^\s>/]+)/gi, ""),
  );
}

function spinnerSvg(): string {
  return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="channel-icon-spinner"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>`;
}

// --- The lamp badge (circle / dot) ------------------------------------------
//
// One skeleton: a disc plus two 8-unit stroke lines, each drawn CENTERED AT
// THE ORIGIN so a state is nothing but a CSS transform (translate → place,
// rotate → orient, scaleX → length). Poses and transitions live in the host's stylesheet
// under `.channel-icon-badge--morph`; every pose writes the same canonical
// `translate() rotate() scaleX()` list so transitions interpolate per
// function instead of falling back to matrix decomposition.
// circle/dot are pure disc poses (strokes retracted): circle keeps the disc
// unfilled with a stroked rim, dot fills it — so armed → published morphs as
// the ring filling in, on paint properties alone.
// Colors: the disc reads success green for circle/dot/plus/check, amber under
// the `attention` tone, and neutral gray for minus (the minus is removal, not
// failure — the plugin installer's red ✗ shares no skeleton and stays a
// separate badge kind). The strokes are inked by `--icon-morph-ink`, which is
// re-pointed per disc color so the glyph clears 3:1 on whichever disc it lands
// on.

type MorphKind = "circle" | "dot";
const MORPH_KINDS: readonly MorphKind[] = ["circle", "dot"];

function isMorphKind(kind: ChannelIconBadge["kind"]): kind is MorphKind {
  return (MORPH_KINDS as readonly string[]).includes(kind);
}

/** Only circle/dot carry a tone; every other kind reads as the default hue. */
function toneOf(badge: ChannelIconBadge): ChannelIconTone | undefined {
  return "tone" in badge ? badge.tone : undefined;
}

// The skeleton carries geometry only — no paint. Both the disc's fill and the
// strokes' color come from the host's stylesheet, because the ink has to follow the disc
// (see `--icon-morph-ink` there): a glyph hard-coded to white was 2.54:1 on
// the light success disc and 1.92:1 on the dark one, under the 3:1 WCAG
// 1.4.11 floor for non-text graphics.
function morphSvg(): string {
  return `<svg viewBox="0 0 24 24"><circle class="icon-morph-disc" cx="12" cy="12" r="10"/><line class="icon-morph-a" x1="-4" y1="0" x2="4" y2="0" stroke-width="2.5" stroke-linecap="round"/><line class="icon-morph-b" x1="-4" y1="0" x2="4" y2="0" stroke-width="2.5" stroke-linecap="round"/></svg>`;
}

/** Pose an existing lamp badge in place — the node is never replaced. */
function applyMorphShape(el: HTMLElement, kind: MorphKind, tone?: ChannelIconTone): void {
  for (const k of MORPH_KINDS) {
    el.classList.toggle(`channel-icon-badge--${k}`, k === kind);
  }
  // The tone is a class rather than a pose, so it survives every shape flip
  // and repaints the disc through the same `fill` transition the poses use.
  el.classList.toggle("channel-icon-badge--attention", tone === "attention");
  el.dataset.shape = kind;
}
