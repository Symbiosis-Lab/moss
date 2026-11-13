/**
 * Country map: choose a country and a panel shows its row. Without this
 * script the map is a picture and the list below it (an embedded map) is
 * still the content, every row an anchor.
 *
 * A tap or click on a country, a click on a list row, a search result and the
 * address `#<country-slug>` all run one `choose`: the country is outlined, the
 * map frames it, the panel opens and the address takes the hash. Esc, the
 * close button or a click on empty sea clears it. Disputed land is inert. The
 * build emits what the panel shows as one <template> per rated row and one for
 * a country with no row; this script only clones the right one.
 */
import { createCamera, framed, parseBox, type ViewBox } from "./frame";
import { mountHover, mountLegend } from "./hover";
import { reducedMotion, media, WIDE } from "../media";
import { mountSearch } from "./search";
import { stampToday } from "./stamp";
import { levelName } from "./shared";
import { tintIn } from "./tint";
import { mountZoom } from "./zoom";

/** The panel sits over the map's right side; the map keeps clear of it by this much (px). */
const PANEL_GAP = 24;
/** However wide the panel, a country is framed in at least this share of the map's width. */
const MIN_FREE = 0.35;

/** The listeners of each live figure, so a morph that replaces a figure drops the old one's. */
const live = new Map<HTMLElement, AbortController>();

function setup(figure: HTMLElement): void {
  if (figure.dataset.countryReady) return;
  figure.dataset.countryReady = "1";
  stampToday(figure);
  const panel = figure.querySelector<HTMLElement>(".moss-country-panel");
  const svg = figure.querySelector("svg");
  const stage = figure.querySelector<HTMLElement>(".moss-country-stage");
  if (!panel || !svg || !stage) return;
  const controller = new AbortController();
  live.set(figure, controller);
  const { signal } = controller;
  const countries = new Map<string, SVGPathElement>();
  figure.querySelectorAll<SVGPathElement>(".moss-country").forEach((p) => countries.set(p.dataset.slug || "", p));
  const levels = [...figure.querySelectorAll(".moss-country-legend li")].map((li) => li.textContent || "");
  const notRated = figure.dataset.notRated || "";
  let sync = (): void => {};
  const camera = createCamera(svg, () => sync());

  /** The one thing the panel, the outline and the address derive from. */
  let chosen: string | null = null;
  /** Where focus goes back to when the country is cleared; set only by a keyboard open. */
  let returnFocusTo: HTMLElement | null = null;
  /** The view the reader had before the first country was chosen, which closing the panel restores. */
  let before: ViewBox | null = null;
  /** The reader moved or zoomed the map since the last country was framed; closing then leaves the view alone. */
  let touched = false;

  /** Share of the map's width not under the panel. */
  const freeSpace = (): number => {
    const s = stage.getBoundingClientRect();
    if (!s.width || panel.hidden) return 1;
    return Math.max(MIN_FREE, (panel.getBoundingClientRect().left - s.left - PANEL_GAP) / s.width);
  };

  const frame = (animate: boolean): void => {
    const instant = !animate || reducedMotion();
    const box = chosen ? parseBox(countries.get(chosen)?.dataset.box) : null;
    if (!chosen) {
      if (before && !touched) camera.to(before, !instant);
      before = null;
    } else if (box) {
      // Under 56rem the panel is a sheet over the page, not beside the map.
      camera.to(framed(box, camera.whole, media(WIDE) ? freeSpace() : 1), !instant);
      touched = false;
    }
  };

  const fillPanel = (slug: string, path: SVGPathElement): void => {
    const rated = figure.querySelector<HTMLTemplateElement>(`template[data-slug="${slug}"]`);
    const template = rated || figure.querySelector<HTMLTemplateElement>("template[data-not-rated]");
    if (!template) return;
    panel.replaceChildren(template.content.cloneNode(true));
    if (!rated) {
      const name = path.dataset.name || slug;
      panel.querySelector(".moss-country-panel-name")!.textContent = name;
      const link = panel.querySelector<HTMLAnchorElement>("a[data-link]");
      if (link) link.href = (link.dataset.link || "").split("{country}").join(encodeURIComponent(name));
    }
  };

  /** Draw the outline, the panel and the address from `chosen`. */
  const render = (animate: boolean): void => {
    countries.forEach((path, slug) => path.classList.toggle("is-chosen", slug === chosen));
    const path = chosen ? countries.get(chosen) : undefined;
    if (chosen && path) {
      path.parentNode?.appendChild(path);
      fillPanel(chosen, path);
    } else panel.replaceChildren();
    panel.hidden = !chosen;
    if (chosen && location.hash.slice(1) !== chosen) history.replaceState(null, "", `#${chosen}`);
    else if (!chosen && countries.has(location.hash.slice(1))) history.replaceState(null, "", location.pathname + location.search);
    frame(animate);
  };

  const clear = (): void => {
    if (chosen === null) return;
    chosen = null;
    render(true);
    const back = returnFocusTo;
    returnFocusTo = null;
    if (back?.isConnected) back.focus();
  };

  /** `from` is the element a keyboard open came from; the panel's heading takes focus and Esc returns to `from`. */
  const choose = (slug: string, { animate = true, from }: { animate?: boolean; from?: HTMLElement } = {}): void => {
    if (!countries.has(slug)) return;
    if (chosen === null) before = camera.goal();
    chosen = slug;
    returnFocusTo = from ?? null;
    render(animate);
    if (from) {
      const heading = panel.querySelector<HTMLElement>(".moss-country-panel-name");
      heading?.setAttribute("tabindex", "-1");
      heading?.focus();
    }
  };

  panel.addEventListener("click", (event) => {
    if ((event.target as Element).closest(".moss-country-panel-close")) clear();
  });
  svg.addEventListener("click", (event) => {
    const target = event.target as Element;
    const path = target.closest<SVGPathElement>(".moss-country");
    if (path) choose(path.dataset.slug || "");
    else if (!target.closest(".moss-country-disputed")) clear();
  });
  // A row of this map's list, inside the figure or apart from it where `style:levels`
  // put it. Listening on the document keeps a list a preview morph replaced working.
  const key = figure.dataset.mossCountryMap;
  document.addEventListener("click", (event: MouseEvent) => {
    const list = (event.target as Element).closest<HTMLElement>(".moss-country-list");
    if (!list || !(figure.contains(list) || (key && list.dataset.mossCountryList === key))) return;
    const link = (event.target as Element).closest<HTMLAnchorElement>("a");
    const slug = (link?.getAttribute("href") || "").slice(1);
    if (!link || !countries.has(slug)) return;
    event.preventDefault();
    choose(slug, { from: event.detail === 0 ? link : undefined });
    stage.scrollIntoView({ block: "nearest", behavior: reducedMotion() ? "auto" : "smooth" });
  }, { signal });
  window.addEventListener("hashchange", () => choose(location.hash.slice(1)), { signal });
  document.addEventListener("keydown", (event) => event.key === "Escape" && clear(), { signal });

  mountHover({ stage, svg, panel, levelOf: (path) => levelName(path, levels, notRated) });
  mountLegend(figure);
  const link = figure.querySelector<HTMLAnchorElement>("a.moss-country-search");
  if (link) mountSearch({ figure, link, countries, levels, signal, choose: (slug, from) => choose(slug, { from }) });
  const { dataset } = figure;
  sync = mountZoom({
    stage,
    svg,
    camera,
    labels: { group: dataset.zoomGroup || "", zoomIn: dataset.zoomIn || "", zoomOut: dataset.zoomOut || "", reset: dataset.zoomReset || "", hint: dataset.zoomHint || "" },
    onUserMove: () => (touched = true),
  });
  const fit = (): void => {
    const box = svg.getBoundingClientRect();
    const world = camera.world[2] / camera.world[3];
    const aspect = box.width && box.height ? box.width / box.height : world;
    camera.fit(Math.abs(aspect / world - 1) < 0.01 ? world : aspect);
  };
  fit();
  if (typeof ResizeObserver === "function") {
    const observer = new ResizeObserver(fit);
    observer.observe(svg);
    signal.addEventListener("abort", () => observer.disconnect());
  }
  tintIn(figure);
  choose(location.hash.slice(1), { animate: false });
}

function init(): void {
  live.forEach((controller, figure) => {
    if (figure.isConnected) return;
    controller.abort();
    live.delete(figure);
  });
  document.querySelectorAll<HTMLElement>("[data-moss-country-map]").forEach(setup);
}

document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", init) : init();
document.addEventListener("moss-morph-patched", init);
