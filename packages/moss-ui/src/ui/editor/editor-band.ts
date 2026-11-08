// packages/moss-ui/src/ui/editor/editor-band.ts
//
// The strip along the bottom of the editor pane that holds the doc-state
// cluster (undo · redo · save badge) on the left and the insert-bar pill in the
// middle. One element, so the browser divides the row instead of us.
//
// WHY THIS EXISTS
//
// Two independent `position: absolute` siblings (one pinned left, one centred
// with `translateX(-50%)`) cannot see each other, so every question about the
// row ends up answered in arithmetic — two numbers describing one quantity,
// computed in different places and disagreeing.
//
// A three-column grid answers all of it declaratively:
//
//   minmax(auto, 1fr)   auto     minmax(0, 1fr)
//   └─ the cluster      └─ pill   └─ counterweight, empty
//
// The two `1fr` tracks split whatever the pill does not need, so the pill is
// centred for free. When the pane is too narrow for an equal split, the left
// track pins at the cluster's own `min-width` and the leftover goes to the right —
// so the pill SLIDES right of the cluster, for free, rather than overlapping
// it. And because the left track's growth limit is a share rather than the
// cluster's natural width, the cluster is the one that yields: its save badge
// ellipsizes, which is a degradation the user can see, instead of the pill
// silently scissoring buttons off inside its `overflow: hidden`.
//
// What is left for JavaScript is the one thing CSS genuinely cannot do here:
// the pill's width is set in pixels so it can be TRANSITIONED between pages
// (a `width` transition does not run between two `auto` values).

/** The pane the band lives in; `position: relative` in editor.css. */
const PANE_ID = 'moss-editor-cm';

/**
 * The band, created on first use and re-created if the pane was emptied under
 * it. Every surface mount does `#moss-editor-cm.innerHTML = ''` — the
 * documented contract for dismissing the cold-start skeleton — so "it was here
 * a moment ago" is not something either occupant may assume.
 *
 * `host` names the pane explicitly, for a caller that already knows which one
 * it is mounting into (the cluster can be re-pointed at a new pane). Without
 * it, the editor's own pane — falling back to `document.body` when that is
 * absent (tests, the email composer), which keeps both occupants mountable
 * without a null check each.
 */
export function editorBand(host?: HTMLElement | null): HTMLElement {
  const pane = host ?? document.getElementById(PANE_ID) ?? document.body;
  const existing = pane.querySelector(':scope > .editor-band');
  if (existing) return existing as HTMLElement;
  const band = document.createElement('div');
  band.className = 'editor-band';
  pane.appendChild(band);
  return band;
}
