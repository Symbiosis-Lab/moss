//#region src/index.d.ts
interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
}
interface BlueprintGridConfig {
  particleCount: number;
  speed: number;
  repulsion: number;
  attraction: number;
  dampening: number;
  gridSpacing: number;
  sigmaPerp: number;
  sigmaParallel: number;
  segmentLength: number;
  minLineWidth: number;
  maxLineWidth: number;
}
/**
 * Pixel pitch of the grid — horizontal lines are stroked at y = k × this.
 *
 * Exported so a page can align its own marks to a real grid line. Two
 * constants would drift, and the failure would be silent: a mark floating one
 * pixel off a line still looks deliberate.
 */
declare const GRID_SPACING = 40;
declare function createParticles(count: number, speed: number): Particle[];
declare function updatePhysics(particles: Particle[], mouseX: number, mouseY: number, config: BlueprintGridConfig): void;
interface BlueprintGridOptions {
  /** The canvas to draw on; by default the container's `#blueprint-canvas`. */
  canvas?: HTMLCanvasElement;
  /** Where pointer and touch movement is read; by default the container. */
  pointerTarget?: EventTarget;
  /** The line colour, any CSS colour, read at every paint. Empty or absent:
   *  blueprint blue, lighter when the root element has `data-theme="dark"`. */
  color?: () => string | null | undefined;
  /** Peak opacity at a crossing; 0.25 by default. */
  maxAlpha?: number;
  /** Called once per painted frame, for a caller that counts frames. */
  onFrame?: () => void;
}
/** The viewport position an event from `POINTER_EVENTS` reports, if any. */
declare function pointerPoint(e: Event): {
  clientX: number;
  clientY: number;
} | null;
declare function startBlueprintGrid(container: HTMLElement, options?: BlueprintGridOptions): () => void;
//#endregion
export { BlueprintGridConfig, BlueprintGridOptions, GRID_SPACING, Particle, createParticles, pointerPoint, startBlueprintGrid, updatePhysics };