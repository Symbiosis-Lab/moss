/**
 * section-name.ts — the current section's name on the island's sections button.
 *
 * Where the contents ruler is not on screen the button is the only contents
 * control, and an icon alone says nothing about where the reader is. The name
 * sits before the glyph in a box of FIXED width (set in site.css, read back
 * here from the laid-out box): the
 * breadcrumb fold measures its neighbours once and trusts them, and a button
 * that changes width as the section changes would make that measurement stale
 * on every scroll. Before the first section the box is empty but keeps its
 * width, for the same reason.
 *
 * The button's accessible name is its visible text plus a visually hidden
 * prefix, never an `aria-label`: a label would replace the content, and then
 * what is spoken could differ from what is shown (WCAG 2.5.3, Label in Name).
 */

import { SECTION_SYNC_EVENT, type SectionSyncDetail } from "./section-event";
import { canvasMeasure, truncateMiddle } from "./middle-truncate";

export interface SectionNameOptions {
  button: HTMLButtonElement;
  island: HTMLElement;
  titles: string[];
}

/** Mount the name. Returns a disposer that restores the button as emitted. */
export function mountSectionName({ button, island, titles }: SectionNameOptions): () => void {
  const label = button.getAttribute("aria-label") ?? "";
  button.removeAttribute("aria-label");

  const prefix = document.createElement("span");
  prefix.className = "visually-hidden";
  prefix.textContent = `${label} `;
  const name = document.createElement("span");
  name.className = "moss-nav-island-section-name";
  button.prepend(prefix, name);

  let index = -1;
  function render(): void {
    const measure = canvasMeasure(name);
    const width = name.clientWidth;
    const title = index >= 0 ? titles[index] : "";
    name.textContent = measure && width > 0 ? truncateMiddle(title, width, measure) : title;
  }

  const onSync = (event: Event): void => {
    const next = (event as CustomEvent<SectionSyncDetail>).detail.index;
    if (next === index) return;
    index = next;
    render();
  };
  island.addEventListener(SECTION_SYNC_EVENT, onSync);
  // The face the width is measured in may arrive after the first render.
  document.fonts?.ready.then(render);
  // The box has a width only while it is shown: it appears when the window
  // grows past the point where the name was dropped.
  const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(render);
  ro?.observe(name);

  return () => {
    island.removeEventListener(SECTION_SYNC_EVENT, onSync);
    ro?.disconnect();
    prefix.remove();
    name.remove();
    button.setAttribute("aria-label", label);
  };
}
