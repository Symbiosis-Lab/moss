/**
 * Whether a feature holds the preview screen, drawing into the preview
 * overlay while the open folder keeps building behind it.
 *
 * While held, the folder's status, rebuild and unshowable-screen traffic must
 * not re-navigate or hide that overlay: `PreviewManager` checks `active` at the
 * entry points that paint it (`renderStatus`, `reconcileUnshowable`) and in
 * `hideOverlay`, which every `loadPreview` reaches. `#moss-preview-iframe` is
 * never what the holder shows, so there is nothing to restore when the hold
 * ends. A host starter picker holds it while its frame is active.
 */

import type { OverlaySlot } from "./overlay-slot";

export interface HeldOverlay {
  container: HTMLElement | null;
  slot: OverlaySlot;
}

export class ScreenHold {
  active = false;

  /** `overlay` reads the preview overlay and its slot lazily: both are created
   *  after `PreviewManager`'s constructor runs. The holder renders into that
   *  same overlay rather than a second `OverlaySlot` — a second grid canvas on
   *  one container leaks a rAF loop the first never stops. */
  constructor(readonly overlay: () => HeldOverlay) {}
}
