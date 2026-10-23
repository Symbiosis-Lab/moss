/**
 * section-name.ts — the current section's name on the island's sections button.
 *
 * Where the contents ruler is not on screen the button is the only contents
 * control, and an icon alone says nothing about where the reader is. The name
 * sits before the glyph in a box of FIXED width, set in site.css, which ends a
 * long name with an ellipsis: the breadcrumb fold measures its neighbours
 * once and trusts them, and a button
 * that changes width as the section changes would make that measurement stale
 * on every scroll. Before the first section the box is empty but keeps its
 * width, for the same reason.
 *
 * The button's accessible name is its visible text plus a visually hidden
 * prefix, never an `aria-label`: a label would replace the content, and then
 * what is spoken could differ from what is shown (WCAG 2.5.3, Label in Name).
 */

import { SECTION_SYNC_EVENT, type SectionSyncDetail } from "./section-event";

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

  const onSync = (event: Event): void => {
    const { index } = (event as CustomEvent<SectionSyncDetail>).detail;
    const text = index >= 0 ? titles[index] : "";
    if (name.textContent !== text) name.textContent = text;
  };
  island.addEventListener(SECTION_SYNC_EVENT, onSync);

  return () => {
    island.removeEventListener(SECTION_SYNC_EVENT, onSync);
    prefix.remove();
    name.remove();
    button.setAttribute("aria-label", label);
  };
}
