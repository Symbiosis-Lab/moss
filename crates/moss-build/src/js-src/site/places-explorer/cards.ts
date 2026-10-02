/**
 * cards.ts — the horizontal row of works currently in scope and in view,
 * floating along the bottom of the map.
 *
 * Reuses the site's own card primitives (`moss-card`, `moss-card-row`,
 * `moss-card-body`, `moss-card-title`, `moss-card-meta`, `moss-card-cover`,
 * `moss-card-description` — all already in the component contract) rather
 * than a parallel set of explorer-only classes: this IS a `moss-card`, just
 * one that can expand a detail region (`moss-places-card-detail`) in place.
 */
import type { Place, Work } from "./types";
import type { PlacesStrings } from "./strings";

/** `work.date`, or `""` so an undated work always sorts after every dated one — never mistaken for the oldest. */
function dateKey(work: Work): string {
  return work.date ?? "";
}

/** Stable row order: date descending, then title. */
export function byDateDescThenTitle(a: Work, b: Work): number {
  const ad = dateKey(a);
  const bd = dateKey(b);
  if (ad !== bd) {
    if (ad === "") return 1;
    if (bd === "") return -1;
    return ad > bd ? -1 : 1;
  }
  return a.title.localeCompare(b.title);
}

/**
 * The works the row shows: in view, further narrowed to `scopedIds` when a
 * ring has one open, plus the selected work even if it just scrolled out of
 * view (collapsing its own expanded card would be a jarring side-effect of
 * an unrelated pan) — the same three rules `renderResults()` applied in the
 * source prototype this ports, cut down to the `all`/`place` scopes this
 * landing supports (an `article` scope is a selection here, not handled by
 * this filter at all).
 */
export function worksForRow(works: Work[], visibleIds: ReadonlySet<string>, scopedIds: ReadonlySet<string> | null, selectedId: string | null): Work[] {
  let records = works.filter((work) => visibleIds.has(work.id));
  if (scopedIds) records = records.filter((work) => scopedIds.has(work.id));
  // The selected work survives falling out of VIEW (an unrelated pan), so
  // its own expanded card never collapses as a side effect — but a ring's
  // scope is a hard restriction: it stays enforced even against whatever
  // was selected before the ring opened, matching "scopes the row to its
  // members" exactly rather than quietly readmitting one outsider.
  if (selectedId && !scopedIds && !records.some((work) => work.id === selectedId)) {
    const selected = works.find((work) => work.id === selectedId);
    if (selected) records.push(selected);
  }
  return records.sort(byDateDescThenTitle);
}

export interface CardCallbacks {
  /** Toggle a work's own selection; `null` clears it. */
  selectWork(id: string | null): void;
}

/** One card's own persistent DOM, reused across renders by `work.id` — see `CardRow`'s own doc on why. */
interface CardEntry {
  card: HTMLElement;
  select: HTMLButtonElement;
  detail: HTMLElement;
}

export class CardRow {
  private readonly container: HTMLElement;
  private readonly callbacks: CardCallbacks;
  private readonly strings: PlacesStrings;
  private placesById = new Map<string, Place>();
  // Persistent DOM, reused across `render()` calls when a work's own card
  // already exists — the same reasoning `MarkerLayer`'s own
  // `markerEntries`/`ringEntry` are built on: `container.replaceChildren()`
  // every render could replace a card's `.moss-places-card-select` button
  // between a real `pointerdown` and `pointerup` on it (any settle, not
  // only this row's own re-renders — a resize or a tile arriving re-renders
  // the whole explorer), leaving the browser's click synthesis nothing to
  // fire `click` on. A work's own title/byline/cover/description/companions
  // never change between renders, so only the "is this the selected card"
  // state below is ever updated on a reused entry.
  private cardEntries = new Map<string, CardEntry>();

  constructor(container: HTMLElement, callbacks: CardCallbacks, strings: PlacesStrings) {
    this.container = container;
    this.callbacks = callbacks;
    this.strings = strings;
  }

  render(works: Work[], places: Place[], selectedId: string | null): void {
    this.placesById = new Map(places.map((place) => [place.id, place]));
    const seenIds = new Set<string>();
    for (const work of works) {
      seenIds.add(work.id);
      this.renderCard(work, work.id === selectedId);
    }
    for (const [id, entry] of this.cardEntries) {
      if (!seenIds.has(id)) {
        entry.card.remove();
        this.cardEntries.delete(id);
      }
    }
  }

  private renderCard(work: Work, expanded: boolean): void {
    let entry = this.cardEntries.get(work.id);
    if (!entry) {
      entry = this.buildCard(work);
      this.cardEntries.set(work.id, entry);
    }
    // Always re-appended (never only on creation): `works` arrives sorted
    // (date descending, then title) on every render, and `append` on a
    // node already in the document MOVES it rather than duplicating it, so
    // this keeps the row's visible order matching that sort cheaply.
    this.container.append(entry.card);
    entry.card.setAttribute("aria-current", String(expanded));
    entry.select.setAttribute("aria-pressed", String(expanded));
    entry.select.setAttribute("aria-expanded", String(expanded));
    if (expanded) entry.detail.removeAttribute("inert");
    else entry.detail.setAttribute("inert", "");
  }

  /** Scrolls the selected card fully into the row's own viewport, if it isn't already. */
  revealSelected(selectedId: string | null): void {
    if (!selectedId) return;
    const card = this.container.querySelector<HTMLElement>(`[data-work-id="${CSS.escape(selectedId)}"]`);
    card?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: prefersReducedMotion() ? "auto" : "smooth" });
  }

  private buildCard(work: Work): CardEntry {
    const card = document.createElement("article");
    card.className = "moss-card";
    card.dataset.workId = work.id;
    card.setAttribute("role", "listitem");

    const select = document.createElement("button");
    select.type = "button";
    select.className = "moss-places-card-select";
    // The visible title inside is clamped to two lines while collapsed
    // (places-explorer.css) — this is the one place a reader who can't see
    // the rest (or a screen reader, which reads the clamped DOM text same
    // as any other) is told the whole thing without having to open the
    // card first. Always set, not only while collapsed: it stays accurate
    // once expanded too, and the title is then also visible in full
    // (unclamped, same element), so there's nothing to keep in sync.
    select.setAttribute("aria-label", work.title || this.strings.untitled);
    select.addEventListener("click", () => this.callbacks.selectWork(work.id));

    const row = document.createElement("div");
    row.className = "moss-card-row";
    const body = document.createElement("div");
    body.className = "moss-card-body";
    const title = document.createElement("h3");
    title.className = "moss-card-title";
    title.textContent = work.title || this.strings.untitled;
    body.append(title);
    // One compact meta line — date and/or the first byline entry, each
    // clamped to a single line by CSS — never the full byline: a work with
    // several linked contributors used to print its whole credit line here
    // with nothing to stop it wrapping, which is what made an ordinary
    // card hundreds of px tall. The full byline moves to the expanded
    // detail below, alongside everything else collapsed hides.
    const metaText = [work.date, work.byline[0]].filter(Boolean).join(" · ");
    if (metaText) {
      const meta = document.createElement("span");
      meta.className = "moss-card-meta";
      meta.textContent = metaText;
      body.append(meta);
    }
    row.append(body);

    if (work.cover) {
      const frame = document.createElement("div");
      frame.className = "moss-card-cover";
      const img = document.createElement("img");
      img.src = work.cover;
      img.alt = "";
      img.loading = "lazy";
      frame.append(img);
      row.append(frame);
    }
    select.append(row);
    card.append(select);
    const detail = this.buildDetail(work);
    card.append(detail);
    return { card, select, detail };
  }

  private buildDetail(work: Work): HTMLElement {
    const detail = document.createElement("div");
    detail.className = "moss-places-card-detail";

    if (work.description) {
      const description = document.createElement("p");
      description.className = "moss-card-description";
      description.textContent = work.description;
      detail.append(description);
    }

    // The full byline — every contributor, not just the collapsed meta
    // line's first — only here, where expanding is itself the reader's
    // request to see more.
    if (work.byline.length) {
      const byline = document.createElement("p");
      byline.className = "moss-card-meta";
      byline.textContent = work.byline.join(" · ");
      detail.append(byline);
    }

    const placeNames = work.places.map((id) => this.placesById.get(id)?.name).filter((name): name is string => Boolean(name));
    if (placeNames.length) {
      const places = document.createElement("p");
      places.textContent = placeNames.join(" · ");
      detail.append(places);
    }

    if (work.companions.length) {
      const companions = document.createElement("ul");
      for (const companion of work.companions) {
        const item = document.createElement("li");
        const link = document.createElement("a");
        link.href = companion.url;
        link.textContent = companion.title;
        item.append(link);
        companions.append(item);
      }
      detail.append(companions);
    }

    const read = document.createElement("a");
    read.className = "moss-places-card-read";
    read.href = work.url;
    read.textContent = this.strings.readArticle;
    detail.append(read);
    return detail;
  }
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}
