/**
 * The header's second level: a `.moss-nav-group` is a link plus a disclosure
 * button that shows a plain list of links (`.moss-nav-sub`). Tab walks the links,
 * Enter/Space on the button toggles, Escape closes and returns focus to the
 * button, a click elsewhere closes. Never on hover.
 *
 * The buttons are emitted `hidden`, so a reader without script never meets a
 * dead control; they are revealed here.
 */

import { clampLeft, maxSurfaceWidth } from "../viewport";

const isOpen = (group: Element): boolean => group.hasAttribute("data-open");

/** Keep the open panel inside the visible band on both edges. site.css reads the two properties for the popover only. */
function placePanel(panel: HTMLElement): void {
  panel.style.setProperty("--nav-sub-max", `${maxSurfaceWidth()}px`);
  panel.style.removeProperty("--nav-sub-shift");
  const { left, width } = panel.getBoundingClientRect();
  const shift = clampLeft(left, width) - left;
  if (shift) panel.style.setProperty("--nav-sub-shift", `${shift}px`);
}

function setOpen(group: HTMLElement, open: boolean, returnFocus = false): void {
  const button = group.querySelector<HTMLElement>(".moss-nav-sub-toggle");
  const panel = group.querySelector<HTMLElement>(".moss-nav-sub");
  if (!button || !panel) return;
  group.toggleAttribute("data-open", open);
  button.setAttribute("aria-expanded", String(open));
  panel.hidden = !open;
  if (open) placePanel(panel);
  else if (returnFocus) button.focus();
}

const groups = (): HTMLElement[] => [...document.querySelectorAll<HTMLElement>(".moss-nav-group")];

let wired = false;

/** Listeners go on only once the page has a group, so other sites pay nothing. */
function wire(): void {
  if (wired || !document.querySelector(".moss-nav-group")) return;
  wired = true;

  document.addEventListener("click", (event: MouseEvent) => {
    const target = event.target as Element | null;
    const within = target?.closest?.(".moss-nav-group") as HTMLElement | null;
    for (const group of groups()) if (group !== within && isOpen(group)) setOpen(group, false);
    if (within && target?.closest(".moss-nav-sub-toggle")) setOpen(within, !isOpen(within));
  });

  document.addEventListener("keydown", (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    const group = groups().find(isOpen);
    if (!group) return;
    event.preventDefault();
    setOpen(group, false, true);
  });

  // Tabbing out of the group closes it; a null `relatedTarget` (a click on
  // nothing focusable) is left to the click handler.
  document.addEventListener("focusout", (event: FocusEvent) => {
    const group = (event.target as Element | null)?.closest?.(".moss-nav-group") as HTMLElement | null;
    const next = event.relatedTarget as Node | null;
    if (group && next && isOpen(group) && !group.contains(next)) setOpen(group, false);
  });

  window.addEventListener("resize", () => {
    for (const group of groups()) {
      const panel = group.querySelector<HTMLElement>(".moss-nav-sub");
      if (panel && isOpen(group)) placePanel(panel);
    }
  });
}

function sync(): void {
  wire();
  for (const button of document.querySelectorAll<HTMLElement>(".moss-nav-sub-toggle")) button.hidden = false;
  // A preview morph can reset the attributes; the group's own flag is the truth.
  for (const group of groups()) setOpen(group, isOpen(group));
}

sync();
// A morph can add the first group to a page that had none.
document.addEventListener("moss-morph-patched", sync);
