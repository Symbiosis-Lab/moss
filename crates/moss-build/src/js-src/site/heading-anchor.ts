/**
 * heading-anchor.ts — clipboard + toast for section permalink anchors.
 *
 * The anchor is a real <a href="#slug"> (emitted by render_heading), so
 * native navigation, middle-click, and keyboard activation all work even
 * if this script never loads. This script ADDS: copy the absolute
 * permalink to the clipboard on click, with a transient toast. It never
 * preventDefaults — native hash navigation + scroll still happen.
 */
import { showToast } from "./toast";

export {};

function onAnchorClick(event: MouseEvent): void {
  const target = event.target as Element | null;
  const anchor = target?.closest<HTMLAnchorElement>("a.moss-heading-anchor");
  if (!anchor) return;

  // Do NOT preventDefault — let the browser set the hash and scroll.
  const slug = anchor.getAttribute("href") ?? "";
  const absolute = location.origin + location.pathname + slug;

  // navigator.clipboard is unavailable in insecure contexts — degrade
  // gracefully (navigation already happened).
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(absolute).then(
      () => showToast(),
      () => {
        /* copy failed — navigation still succeeded, no toast */
      },
    );
  }
}

document.addEventListener("click", onAnchorClick);
