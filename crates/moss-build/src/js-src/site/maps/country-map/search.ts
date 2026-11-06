/** The search field: an ARIA combobox over countries and cities that chooses like a tap. */
import { rank, type Hit, type Row } from "./match";
import { levelName } from "./shared";

interface SearchParts {
  figure: HTMLElement;
  /** The no-script link this replaces. */
  link: HTMLAnchorElement;
  countries: Map<string, SVGPathElement>;
  /** The level names in order, from the legend. */
  levels: string[];
  /** Drops the document listener when the figure leaves the page. */
  signal: AbortSignal;
  /** `returnFocusTo` is set when the pick came from the keyboard. */
  choose(slug: string, returnFocusTo?: HTMLElement): void;
}

/** What the combobox shows is derived from this and the field's text. */
interface View {
  hits: Hit[];
  active: number;
  open: boolean;
}

let counter = 0;

export function mountSearch({ figure, link, countries, levels, signal, choose }: SearchParts): void {
  const id = `moss-country-results-${++counter}`;
  const label = link.textContent || "";
  const notRated = figure.dataset.notRated || "";
  const words = link.dataset;
  const fill = (template: string | undefined, values: Record<string, string>): string => (template || "").replace(/\{(\w+)\}/g, (_, k) => values[k] ?? "");

  const root = document.createElement("div");
  root.className = "moss-country-search";
  root.innerHTML = `<input class="moss-input--boxed" type="text" role="combobox" aria-autocomplete="list" aria-expanded="false" autocomplete="off" spellcheck="false"><ul role="listbox" hidden></ul><p class="moss-country-search-none" hidden></p><p class="moss-country-search-status" role="status"></p>`;
  const input = root.querySelector("input")!;
  const list = root.querySelector("ul")!;
  const none = root.querySelector<HTMLElement>(".moss-country-search-none")!;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  list.id = id;
  input.placeholder = label;
  input.setAttribute("aria-label", label);
  input.setAttribute("aria-controls", id);
  list.setAttribute("aria-label", label);
  link.replaceWith(root);

  let rows: Row[] = [...countries].map(([slug, path]) => [slug, path.dataset.name || slug, [], []]);
  let view: View = { hits: [], active: -1, open: false };
  let built: Hit[] | null = null;

  const render = (): void => {
    const query = input.value.trim();
    const showing = view.open && query !== "";
    const listing = showing && view.hits.length > 0;
    const empty = showing && !listing;
    if (!listing) {
      built = null;
      list.replaceChildren();
    } else if (built !== view.hits) {
      built = view.hits;
      list.replaceChildren(...view.hits.map(row));
    }
    list.hidden = !listing;
    input.setAttribute("aria-expanded", String(listing));
    [...list.children].forEach((li, i) => li.setAttribute("aria-selected", String(i === view.active)));
    if (listing && view.active >= 0) input.setAttribute("aria-activedescendant", list.children[view.active].id);
    else input.removeAttribute("aria-activedescendant");
    none.hidden = !empty;
    if (empty) {
      const see = document.createElement("a");
      see.href = "#moss-country-rows";
      see.textContent = words.seeList || "";
      none.replaceChildren(`${fill(words.none, { q: query })} `, see);
    }
    status.textContent = listing ? fill(view.hits.length === 1 ? words.countOne : words.countOther, { n: String(view.hits.length) }) : empty ? fill(words.none, { q: query }) : "";
  };

  const row = (hit: Hit, i: number): HTMLElement => {
    const path = countries.get(hit.slug);
    const level = path?.dataset.level;
    const li = document.createElement("li");
    li.id = `${id}-${i}`;
    li.setAttribute("role", "option");
    li.dataset.i = String(i);
    li.innerHTML = `<span class="moss-country-swatch" aria-hidden="true"></span><span class="moss-country-search-name"></span><span class="moss-country-search-level"></span>`;
    if (level) li.firstElementChild!.setAttribute("data-level", level);
    li.children[1].textContent = hit.text;
    li.children[2].textContent = levelName(path, levels, notRated);
    return li;
  };

  const update = (): void => {
    const hits = rank(rows, input.value);
    view = { hits, active: hits.length ? 0 : -1, open: true };
    render();
  };

  const pick = (i: number, fromKeyboard: boolean): void => {
    const hit = view.hits[i];
    if (!hit) return;
    input.value = countries.get(hit.slug)?.dataset.name || hit.text;
    view = { hits: [], active: -1, open: false };
    render();
    if (fromKeyboard) choose(hit.slug, input);
    else {
      input.blur();
      choose(hit.slug);
    }
  };

  let loading = false;
  input.addEventListener("focus", () => {
    if (loading || !figure.dataset.search) return;
    loading = true;
    fetch(figure.dataset.search)
      .then((r) => r.json())
      .then((index: { c: Row[] }) => {
        rows = index.c.filter((r) => countries.has(r[0]));
        if (view.open) update();
      })
      .catch(() => {
        loading = false;
      });
  });
  input.addEventListener("input", update);
  input.addEventListener("keydown", (e) => {
    const listing = view.open && view.hits.length > 0;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!listing) return update();
      view = { ...view, active: (view.active + (e.key === "ArrowDown" ? 1 : view.hits.length - 1)) % view.hits.length };
      render();
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick(view.active, true);
    } else if (e.key === "Escape" && view.open && input.value.trim()) {
      e.stopPropagation();
      view = { ...view, open: false };
      render();
    }
  });
  list.addEventListener("mousedown", (e) => e.preventDefault());
  list.addEventListener("click", (e) => {
    const li = (e.target as Element).closest<HTMLElement>("li");
    if (li) pick(+li.dataset.i!, false);
  });
  list.addEventListener("mousemove", (e) => {
    const i = +((e.target as Element).closest<HTMLElement>("li")?.dataset.i ?? view.active);
    if (i !== view.active) {
      view = { ...view, active: i };
      render();
    }
  });
  document.addEventListener(
    "click",
    (e) => {
      if (view.open && !root.contains(e.target as Node)) {
        view = { ...view, open: false };
        render();
      }
    },
    { signal },
  );
}
