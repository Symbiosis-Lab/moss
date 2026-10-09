/**
 * section-event.ts — how the island's scrollspy talks to what follows it.
 *
 * The island is the only thing that works out which section is current. Each
 * frame it dispatches this event on itself with the answer; the contents ruler
 * and the section name on the button listen, and any per-frame reading of their
 * own (the ruler's fade) rides the same event, so a scroll costs one pass of
 * layout reads however many surfaces there are.
 */

export const SECTION_SYNC_EVENT = "moss-section-sync";

export interface SectionSyncDetail {
  /** The current section's position; `-1` before the first. */
  index: number;
}

/** Mark `rows` for `index`: the current one, and the ones already read. */
export function markRows(rows: Iterable<Element>, index: number): void {
  let i = 0;
  for (const row of rows) {
    if (i === index) row.setAttribute("aria-current", "true");
    else row.removeAttribute("aria-current");
    row.toggleAttribute("data-read", i < index);
    i++;
  }
}
