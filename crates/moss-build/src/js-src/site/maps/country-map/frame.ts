/** Framing: where the map looks when a country is chosen. */

export interface Box { x0: number; y0: number; x1: number; y1: number }
export type ViewBox = [x: number, y: number, width: number, height: number];

/** The most the map zooms in on a country, as a multiple of the whole-world view. */
export const MAX_ZOOM = 2.5;
const MOVE_MS = 400;
/** A country fills at most this share of the free width and height, so its neighbours stay in view. */
const FILL_WIDTH = 0.5;
const FILL_HEIGHT = 0.62;

/** A country's `data-box`: the four numbers the build measured, or null. */
export function parseBox(text: string | undefined): Box | null {
  const n = (text ?? "").split(" ").map(Number);
  return n.length === 4 && n.every(Number.isFinite) ? { x0: n[0], y0: n[1], x1: n[2], y1: n[3] } : null;
}

/**
 * The view that centres `box` in the left `free` share of the map (the rest is
 * under the panel), at most MAX_ZOOM times and never less than the world view,
 * so neighbours stay readable.
 */
export function framed(box: Box, base: ViewBox, free: number): ViewBox {
  const [, , width, height] = base;
  const k = Math.min(MAX_ZOOM, Math.max(1, Math.min((FILL_WIDTH * free * width) / (box.x1 - box.x0), (FILL_HEIGHT * height) / (box.y1 - box.y0))));
  const w = width / k;
  const h = height / k;
  return [(box.x0 + box.x1) / 2 - (w * free) / 2, (box.y0 + box.y1) / 2 - h / 2, w, h];
}

/** The `scrollLeft` that puts map x `cx` in the middle of a sideways box. */
export const scrollLeftFor = (cx: number, mapWidth: number, scrollWidth: number, viewport: number): number =>
  Math.max(0, (cx / mapWidth) * scrollWidth - viewport / 2);

export interface Camera {
  /** The whole-world view the map starts with. */
  base: ViewBox;
  to(target: ViewBox, animate: boolean): void;
}

/** Moves an svg's viewBox, easing over MOVE_MS or jumping. */
export function createCamera(svg: SVGSVGElement): Camera {
  const read = (): ViewBox => svg.getAttribute("viewBox")!.split(" ").map(Number) as ViewBox;
  const write = (v: ViewBox): void => svg.setAttribute("viewBox", v.map((n) => +n.toFixed(2)).join(" "));
  const base = read();
  let frame = 0;
  return {
    base,
    to(target, animate) {
      cancelAnimationFrame(frame);
      if (!animate) return write(target);
      const from = read();
      const start = performance.now();
      const step = (now: number): void => {
        const p = Math.min(1, (now - start) / MOVE_MS);
        const eased = 1 - (1 - p) ** 3;
        write(from.map((v, i) => v + (target[i] - v) * eased) as ViewBox);
        if (p < 1) frame = requestAnimationFrame(step);
      };
      frame = requestAnimationFrame(step);
    },
  };
}
