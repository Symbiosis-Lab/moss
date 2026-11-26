// Preview-click disambiguation.
//
// Single click = reveal (editor scrolls + flashes, focus stays in the preview).
// Double click = jump (editor focused, caret at the mapped position).
//
// Disambiguation is by CLICK COUNT (`event.detail`), not a delay timer, and
// that choice is deliberate: a 250–300ms timer would delay every single-click
// highlight perceptibly, and the vocabulary makes suppression unnecessary —
// the single action is now a strict prefix of the double (reveal ⊂ jump). So:
//
//   click  detail=1 → 'click'   (the reveal fires with zero latency; on a
//                                double it is simply superseded by the jump
//                                ~200ms later — the natural escalation, same
//                                as single-select → double-open in a file
//                                manager)
//   click  detail≥2 → null      (second click of a double: the reveal already
//                                fired once; dblclick owns the gesture)
//   dblclick        → 'dblclick'
//
// Pure and DOM-free so the state machine is directly unit-testable.

export type ClickInteraction = 'click' | 'dblclick' | null;

export function classifyClickGesture(
  eventType: 'click' | 'dblclick',
  detail: number,
): ClickInteraction {
  if (eventType === 'dblclick') return 'dblclick';
  return detail > 1 ? null : 'click';
}

// A click on a gallery image's link belongs to the full-screen viewer, which
// fullscreen.js opens from the same click; the bridge must not navigate to the
// image file first.
export function shouldLeaveClickToPage(target: Element): boolean {
  return target.closest(".moss-gallery-link") !== null;
}

// Whether following `anchor` to `target` loads a new document, the only kind of
// navigation that ever reports back to the shell. A fragment on the current
// page only scrolls (and a click on the fragment already in the URL fires no
// hashchange), and a download commits no document.
export function loadsDocument(anchor: Element, target: URL, current: URL): boolean {
  if (anchor.hasAttribute("download")) return false;
  const samePage = target.origin === current.origin && target.pathname === current.pathname && target.search === current.search;
  return !(samePage && target.hash !== "");
}

// A picture the site's viewer script has taken as its own, which it marks
// `data-viewer`. In the editor preview a click or Enter on one belongs to the
// editor's select gesture, so the guard defaults it away in the capture phase,
// before the site script, which skips any event already prevented. Gallery
// links are the exception above: they keep opening the viewer.
export function installViewerGuard(doc: Document): void {
  const claim = (e: Event): void => {
    if (e.target instanceof Element && e.target.closest("[data-viewer]")) e.preventDefault();
  };
  doc.addEventListener("click", claim, true);
  doc.addEventListener("keydown", (e) => (e.key === "Enter" || e.key === " ") && claim(e), true);
}
