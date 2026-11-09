// The overlay slot: `#moss-preview-overlay` as a place screens are written
// into, and the blueprint grid running behind all of them.
//
// ## One grid per overlay, not one per screen
//
// `<canvas id="blueprint-canvas">` used to be part of the markup each screen
// emitted, so every content change destroyed it and `startBlueprintGrid` had
// to run again — which re-seeds the particle field from `Math.random()` and
// restarts the canvas at zero width. During a build the stage sentence changes
// on nearly every progress event, so that ran continuously and the background
// flickered instead of drifting. Worse, the two screen
// surfaces shared ONE cleanup handle through their `startGrid`/`stopGrid`
// dependencies, so a handoff between them either leaked the old rAF loop or
// stopped the new one.
//
// The canvas belongs to the overlay here and outlives every screen written
// into it: `renderScreen` keeps the node, and only `clear` takes it away.
// Neither screen surface has a grid dependency any more — there is nothing for
// them to get wrong.

import { count as diagCount } from "../../lib/diag";
import { startBlueprintGrid } from "@symbiosis-lab/moss-blueprint-grid";

/**
 * The blueprint canvas, created once per overlay and kept forever after.
 *
 * A sibling of the screen markup rather than part of it, which is the whole
 * point — see this file's header.
 */
function ensureBlueprintCanvas(container: HTMLElement): HTMLCanvasElement {
  const existing = container.querySelector<HTMLCanvasElement>("#blueprint-canvas");
  if (existing) return existing;
  const canvas = document.createElement("canvas");
  canvas.id = "blueprint-canvas";
  canvas.setAttribute("aria-hidden", "true");
  container.prepend(canvas);
  return canvas;
}

/** Write a screen into the overlay, keeping the grid that is already running
 *  behind it. The one way any surface puts markup on the overlay. */
export function renderScreen(container: HTMLElement, html: string): void {
  const canvas = ensureBlueprintCanvas(container);
  container.replaceChildren(canvas);
  canvas.insertAdjacentHTML("afterend", html);
}

/**
 * The overlay's grid, and the two edges that start and stop it.
 *
 * Held as an object rather than as two more fields on `PreviewManager` because
 * the handle and the two edges are one fact: a grid is running iff the overlay
 * is up. Splitting them is how the shared-handle bug happened the first time.
 */
export class OverlaySlot {
  private grid: (() => void) | null = null;

  /**
   * `onFrame` is called once per rendered frame. It exists so this app-side
   * module can count frames for diagnostics WITHOUT the grid package importing
   * the diag → Tauri-log chain — the package is also bundled into the
   * preview-only iframe bridge, which must stay free of it (the preview is
   * sacred).
   */
  constructor(private readonly onFrame: () => void = countBlueprintFrame) {}

  /**
   * The overlay, ready for a screen to be written into it: visible, carrying
   * the blueprint canvas, and with the grid running behind whatever comes.
   *
   * Every path that puts markup on the overlay goes through here, so the grid
   * has exactly one start — the canvas has to exist before `startBlueprintGrid`
   * looks for it, and a second start would leak the first loop forever.
   */
  raise(container: HTMLElement): HTMLElement {
    ensureBlueprintCanvas(container);
    container.classList.add("visible");
    this.grid ??= startBlueprintGrid(container, { onFrame: this.onFrame });
    return container;
  }

  /** Back to empty: grid stopped, canvas and markup gone, both markers down.
   *  The only place the grid is torn down. */
  clear(container: HTMLElement | null): void {
    this.grid?.();
    this.grid = null;
    if (!container) return;
    container.classList.remove("home-gate-active", "visible");
    container.replaceChildren();
  }
}

/**
 * [diag] energy: preview-overlay blueprint frames, the #1 energy suspect. This
 * grid runs for minutes at a time, including while the window is hidden.
 */
function countBlueprintFrame(): void {
  diagCount("blueprint.frames");
  if (document.hidden) diagCount("blueprint.frames.hidden");
}
