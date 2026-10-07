/**
 * raster.ts — turns a fetched map SVG (the world document or one regional
 * tile) into a fixed-pixel `<canvas>` plus a small live rivers overlay.
 *
 * Both WebKit and Chromium were measured re-running the world's
 * relief/lighting filters on every repaint of a LIVE, in-document,
 * filtered `<svg>` — WebKit worst of all, going unresponsive for whole
 * seconds. A filtered SVG `<img>` is also not a reliable high-resolution
 * backing surface: native WebKit can rasterise its filters at the small
 * layout size before the ancestor camera transform enlarges it. Drawing a
 * decoded, pixel-sized SVG image into a canvas fixes the backing dimensions
 * before it enters that transform tree. Regional tiles use the same path,
 * so each surface is one bitmap too. `map.ts` and
 * `tiles.ts` both build their own raster this way; this module is the one
 * place that knows how to split a fetched document into the part worth
 * rasterising and the part that must stay live.
 */

const XML_NS = "http://www.w3.org/2000/svg" as const;

/**
 * Every `--moss-place-*` token svg.rs's non-river layers (water, coast,
 * relief, seafloor, ice, salt, lakes, reefs, built-up) read — the same set
 * the `:root`/`[data-theme="dark"]` place-map block in site.css defines,
 * from `--moss-place-water` through `--moss-place-reefs`. Globe and marker
 * tokens are left out on purpose: `emit_world_svg`/`emit_tile_svg` draw
 * neither (no globe inset, an always-empty marker group), so baking them
 * would spend bytes on something never painted.
 */
const BASE_TOKENS = [
  "--moss-place-water",
  "--moss-place-land",
  "--moss-place-land-mid",
  "--moss-place-land-high",
  "--moss-place-sea-deep",
  "--moss-place-coast",
  "--moss-place-coast-opacity",
  "--moss-place-shadow",
  "--moss-place-relief-edge-opacity",
  "--moss-place-light-warm",
  "--moss-place-light-cool",
  "--moss-place-light-warm-strength",
  "--moss-place-light-cool-strength",
  "--moss-place-lakes",
  "--moss-place-ice",
  "--moss-place-built-up",
  "--moss-place-salt",
  "--moss-place-reefs",
  "--moss-place-route",
];

/**
 * The current light/dark value of every `BASE_TOKENS` entry, as one inline
 * `style` string ready to splice onto a detached SVG's own root. A
 * standalone SVG resource (what an `<img>`'s `src` decodes) has no access
 * to the page's own stylesheet or cascade — without this, every fill and
 * stroke in it would fall back to its hardcoded (light-mode) default
 * regardless of the reader's actual theme, exactly the way an externally
 * loaded `<img>` was measured losing `--moss-place-river-scale` (the
 * reason rivers stay live DOM instead, see `splitMapSvg`). Read off the map
 * figure (`.moss-place-map`), not `document.documentElement`: site.css
 * declares the tokens on that class, so the root never carried them and the
 * runtime baked every layer with the SVG's hardcoded fallbacks — a site's own
 * palette override never reached the live map. The figure's computed style
 * also carries `data-theme`'s dark values through the cascade. Before any
 * figure exists the root is the fallback.
 */
export function capturePlaceMapTheme(): string {
  const computed = getComputedStyle(document.querySelector(".moss-place-map") ?? document.documentElement);
  return BASE_TOKENS.map((name) => [name, computed.getPropertyValue(name).trim()] as const)
    .filter(([, value]) => value !== "")
    .map(([name, value]) => `${name}:${value}`)
    .join(";");
}

/** Strip `<script>`/`<foreignObject>` and any `on*`/non-local `href` — the same defense-in-depth a fetched asset gets regardless of same-origin trust, ported from the runtime's previous single `parseMapSvg` sanitiser. */
function sanitize(doc: Document): void {
  doc.querySelectorAll("script, foreignObject").forEach((node) => node.remove());
  doc.querySelectorAll("*").forEach((node) => {
    for (const attribute of [...node.attributes]) {
      const local = attribute.value.startsWith("#");
      const dataImage = /^data:image\/(?:png|jpe?g|webp|svg\+xml);base64,[a-z\d+/=]+$/i.test(attribute.value);
      if (/^on/i.test(attribute.name) || (["href", "xlink:href"].includes(attribute.name) && !local && !dataImage)) {
        node.removeAttribute(attribute.name);
      }
    }
  });
}

export interface MapSvgSplit {
  /**
   * Every layer but rivers, with the page's current theme tokens inlined
   * on its root (see `capturePlaceMapTheme`).
   * Never mutated by `rasterize` — it clones before resizing, so the same
   * split can be rasterised more than once (the world layer re-bakes at a
   * sharper size on settle; see `map.ts`).
   */
  base: SVGSVGElement;
  /**
   * The extracted, unfiltered `[data-map-layer="rivers"]` group, wrapped
   * in its own small live `<svg>` sized/viewBoxed to match `base` — `null`
   * when the source carried no rivers (an empty tile cell, or a group with
   * no paths). Stays real, inserted DOM rather than part of the raster, so
   * `--moss-place-river-scale`/`--river-w` keep working through the normal
   * page cascade exactly as they did before this module existed; an
   * externally-referenced raster resource cannot read either.
   */
  rivers: SVGSVGElement | null;
  /**
   * `base`, pre-serialised once here rather than by `rasterize` on every
   * call. The world layer re-bakes at a sharper size on settle (`map.ts`),
   * and cloning + `XMLSerializer`-walking the full DOM tree on each of
   * those was measured blocking the main thread for over a second in
   * WebKit specifically — long enough to delay a reader's very next click
   * past this module's own latency budget. `rasterize` instead patches
   * just this string's own root `width`/`height`.
   */
  baseMarkup: string;
  /** The source's own canvas size in SVG user units (svg.rs's `canvas_width`/`canvas_height`) — what `rasterize`'s caller scales up from, and what a caller sizing the wrapper that will hold the raster uses directly. */
  width: number;
  height: number;
}

/**
 * Parse a fetched map SVG (the world document or one regional tile),
 * sanitise it, pull its rivers group out into a separate live overlay, and
 * leave every other layer as drawn. `null` on anything that fails
 * to parse as a real `<svg>` — the same "leave the static floor alone"
 * contract `tiles.ts`'s old `parseMapSvg` carried.
 */
export function splitMapSvg(markup: string): MapSvgSplit | null {
  const parsed = new DOMParser().parseFromString(markup, "image/svg+xml");
  const svg = parsed.documentElement;
  if (svg.localName !== "svg" || parsed.querySelector("parsererror")) return null;
  sanitize(parsed);

  const width = Number(svg.getAttribute("width")) || 0;
  const height = Number(svg.getAttribute("height")) || 0;
  const viewBox = svg.getAttribute("viewBox") ?? `0 0 ${width} ${height}`;

  const riverGroup = svg.querySelector('[data-map-layer="rivers"]');
  let rivers: SVGSVGElement | null = null;
  if (riverGroup && riverGroup.childElementCount > 0) {
    riverGroup.querySelectorAll<SVGElement>("path[stroke-width]").forEach((path) => {
      const strokeWidth = path.getAttribute("stroke-width");
      if (strokeWidth) path.style.setProperty("--river-w", strokeWidth);
    });
    const overlay = parsed.createElementNS(XML_NS, "svg");
    overlay.setAttribute("viewBox", viewBox);
    overlay.setAttribute("focusable", "false");
    overlay.setAttribute("aria-hidden", "true");
    overlay.append(riverGroup);
    rivers = document.importNode(overlay, true) as SVGSVGElement;
  } else {
    riverGroup?.remove();
  }

  svg.removeAttribute("role");
  svg.removeAttribute("aria-label");
  const existingStyle = svg.getAttribute("style") ?? "";
  const theme = capturePlaceMapTheme();
  if (theme) svg.setAttribute("style", existingStyle ? `${existingStyle};${theme}` : theme);

  // `Document.documentElement` is typed as `HTMLElement` even for an XML
  // document (a known imprecision in the DOM lib, not a real possibility
  // here — `svg.localName !== "svg"` already returned above otherwise), so
  // the cast needs the `unknown` detour TypeScript can't bridge directly.
  const base = document.importNode(svg, true) as unknown as SVGSVGElement;
  return { base, baseMarkup: new XMLSerializer().serializeToString(svg), rivers, width, height };
}

export interface Raster {
  el: HTMLCanvasElement;
  /** Drop the owned backing store once `el` is no longer shown. */
  release(): void;
}

/**
 * The opening `<svg ...>` tag's own `width="…" height="…"` pair, as
 * `svg.rs`'s `open_svg()` always emits them (immediately after any
 * `xmlns`, before `viewBox`) — anchored to the START of the string and
 * stopping at the tag's own `>` (`[^>]` cannot cross into it), so this can
 * never match anything past the root element even if some descendant
 * happens to carry its own `width`/`height` attributes.
 */
const ROOT_SIZE_PATTERN = /^(<svg\b[^>]*?)\swidth="[^"]*"\s+height="[^"]*"/;

/** The string-patch half of `rasterize`, pulled out so a test can check it directly without needing the `Blob`/`decode()` round trip jsdom can't do. */
export function withRootSize(markup: string, pixelWidth: number, pixelHeight: number): string {
  const width = Math.max(1, Math.round(pixelWidth));
  const height = Math.max(1, Math.round(pixelHeight));
  return markup.replace(ROOT_SIZE_PATTERN, (_match, prefix: string) => `${prefix} width="${width}" height="${height}"`);
}

/**
 * Patch `baseMarkup`'s own root `width`/`height` to `pixelWidth`x
 * `pixelHeight` — never its `viewBox`, so content geometry is unchanged —
 * then draw the decoded source into a canvas whose backing dimensions are
 * exactly those requested pixels. The SVG image stays detached from the
 * transformed map tree; this matters in WebKit, where a filtered SVG image
 * can be cached at its small CSS box before a camera transform enlarges it.
 * A string patch rather
 * than `cloneNode` + `XMLSerializer` on every call — see `MapSvgSplit.
 * baseMarkup`'s own doc for the cost that was measured costing.
 */
export async function rasterize(baseMarkup: string, pixelWidth: number, pixelHeight: number): Promise<Raster> {
  const sized = withRootSize(baseMarkup, pixelWidth, pixelHeight);
  const url = URL.createObjectURL(new Blob([sized], { type: "image/svg+xml" }));
  const source = new Image();
  source.decoding = "async";
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(pixelWidth));
  canvas.height = Math.max(1, Math.round(pixelHeight));
  canvas.setAttribute("aria-hidden", "true");
  try {
    source.src = url;
    await source.decode();
    const context = canvas.getContext("2d");
    if (!context) throw new Error("2D canvas is unavailable");
    context.drawImage(source, 0, 0, canvas.width, canvas.height);
  } finally {
    URL.revokeObjectURL(url);
  }
  return {
    el: canvas,
    release: () => {
      canvas.width = 0;
      canvas.height = 0;
    },
  };
}

/** The element a caller actually shows: a decoded raster, or (see `rasterizeOrFallback`) the live SVG itself. */
export interface Surface {
  el: HTMLCanvasElement | SVGSVGElement;
  release(): void;
}

/**
 * `rasterize`, falling back to showing `fallback` itself — live,
 * unrasterised — on a browser lacking the `Blob`/`decode()` support every
 * real target engine (Chromium, WebKit, Firefox) has. The same "never show
 * nothing" posture the rest of this runtime takes on a failed fetch: a map
 * that repaints slowly is still a map, which an absent one is not. In
 * practice this path is only ever exercised by a test environment with no
 * real rendering (jsdom implements neither API), which is also why it
 * costs nothing to keep — nothing here is reachable on the browsers this
 * module exists to speed up.
 */
export async function rasterizeOrFallback(baseMarkup: string, fallback: SVGSVGElement, pixelWidth: number, pixelHeight: number): Promise<Surface> {
  try {
    return await rasterize(baseMarkup, pixelWidth, pixelHeight);
  } catch {
    return { el: fallback, release: () => {} };
  }
}
