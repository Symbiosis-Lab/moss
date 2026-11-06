/** A country-map figure as the build writes it, with invented countries. */
const HEAD = (name: string) =>
  `<div class="moss-country-panel-head"><h3 class="moss-country-panel-name">${name}</h3><button class="moss-country-panel-close" type="button">x</button></div>`;

export function page(extra = ""): void {
  Element.prototype.scrollIntoView = () => {};
  Element.prototype.scrollTo = () => {};
  document.body.innerHTML = `<figure data-moss-country-map data-not-rated="Not rated" data-search="/_moss/map.x/country-search.json" data-zoom-group="Map controls" data-zoom-in="Zoom in" data-zoom-out="Zoom out" data-zoom-reset="Show the whole map" data-zoom-hint="Use + and − to zoom.">
    <a class="moss-country-search" href="#moss-country-rows" data-count-one="{n} result" data-count-other="{n} results" data-none="Nothing matches “{q}”." data-see-list="See the list.">Search a country or city</a>
    <div class="moss-country-stage"><svg viewBox="0 0 1000 435"><path class="moss-country-graticule" id="sea"/><g>
      <path class="moss-country-disputed" id="disputed"/>
      <path class="moss-country" id="japan" data-slug="japan" data-name="Japan" data-level="1" data-box="800 150 840 200"/>
      <path class="moss-country" id="mexico" data-slug="mexico" data-name="Mexico" data-level="2" data-box="200 180 260 230"/>
      <path class="moss-country" id="chad" data-slug="chad" data-name="Chad" data-box="480 200 520 250"/>
    </g></svg><div class="moss-country-panel" hidden></div></div>
    <ul class="moss-country-legend"><li><span data-level="1"></span>Low</li><li><span data-level="2"></span>High</li></ul>
    <div class="moss-country-list" id="moss-country-rows"><a id="row" href="#mexico">Mexico</a></div>
    <template data-slug="japan">${HEAD("Japan")}<p class="moss-country-panel-level">Low</p><p>Take normal precautions.</p><p><a href="/ask/?to=Japan">Ask us</a></p></template>
    <template data-slug="mexico">${HEAD("Mexico")}<p class="moss-country-panel-level">High</p><dl><dt>Note</dt><dd>Night travel.</dd></dl></template>
    <template data-not-rated>${HEAD("")}<p class="moss-country-panel-level">Not rated</p><p>Not assessed.</p><p><a data-link="/ask/?to={country}">Ask us</a></p></template>
    ${extra}
  </figure>`;
}

export const INDEX = {
  c: [
    ["japan", "Japan", ["Nippon", "JPN"], ["Tokyo", "Osaka"]],
    ["mexico", "Mexico", ["United Mexican States", "MEX"], ["Mexico City"]],
    ["chad", "Chad", ["Republic of Chad"], ["N'Djamena"]],
  ],
};

/** Install a stub `matchMedia` answering from the given queries (others are false). */
export function media(answers: Record<string, boolean>): void {
  window.matchMedia = ((query: string) => ({ matches: answers[query] ?? false, media: query, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
}

export { WIDE } from "../../media";
export const HOVER = "(hover: hover)";
export const REDUCED = "(prefers-reduced-motion: reduce)";

export const panel = () => document.querySelector<HTMLElement>(".moss-country-panel")!;
export const click = (id: string) => document.getElementById(id)!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
export const $ = <T extends Element = HTMLElement>(selector: string) => document.querySelector<T>(selector)!;

/** Load the script fresh against the current page. */
export async function boot(): Promise<void> {
  await import("../index");
  document.dispatchEvent(new Event("moss-morph-patched"));
}
