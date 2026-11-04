/**
 * Country map: choose a country and a panel shows its row. Without this
 * script the map is a picture and the list below it (an embedded map) is
 * still the content, every row an anchor.
 *
 * A tap or click on a country, the address `#<country-slug>` and a click on a
 * list row all open the same panel; Esc, the close button or a click on empty
 * sea clears it. Disputed land is inert. The build emits what the panel shows
 * as one <template> per rated row and one for a country with no row; this
 * script only clones the right one.
 */

function setup(figure: HTMLElement): void {
  if (figure.dataset.countryReady) return;
  figure.dataset.countryReady = "1";
  const panel = figure.querySelector<HTMLElement>(".moss-country-panel");
  const svg = figure.querySelector("svg");
  if (!panel || !svg) return;
  const countries = new Map<string, SVGPathElement>();
  figure.querySelectorAll<SVGPathElement>(".moss-country").forEach((p) => countries.set(p.dataset.slug || "", p));
  let chosen: string | null = null;

  const clear = (): void => {
    if (chosen === null) return;
    countries.get(chosen)?.classList.remove("is-chosen");
    chosen = null;
    panel.hidden = true;
    panel.replaceChildren();
    if (countries.has(location.hash.slice(1))) history.replaceState(null, "", location.pathname + location.search);
  };

  const choose = (slug: string): void => {
    const path = countries.get(slug);
    if (!path) return;
    if (chosen !== null) countries.get(chosen)?.classList.remove("is-chosen");
    chosen = slug;
    path.classList.add("is-chosen");
    path.parentNode?.appendChild(path);
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
    panel.hidden = false;
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
  figure.addEventListener("click", (event) => {
    const link = (event.target as Element).closest<HTMLAnchorElement>(".moss-country-list a");
    if (link) choose((link.getAttribute("href") || "").slice(1));
  });
  window.addEventListener("hashchange", () => {
    const slug = location.hash.slice(1);
    if (countries.has(slug)) choose(slug);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") clear();
  });
  if (countries.has(location.hash.slice(1))) choose(location.hash.slice(1));
}

function init(): void {
  document.querySelectorAll<HTMLElement>("[data-moss-country-map]").forEach(setup);
}

document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", init) : init();
document.addEventListener("moss-morph-patched", init);
