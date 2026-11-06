/** Framing: where the map looks when a country is chosen. */

export interface Box { x0: number; y0: number; x1: number; y1: number }
export type ViewBox = [x: number, y: number, width: number, height: number];

/** The most the map zooms in on a chosen country, as a multiple of the whole view. */
const FRAME_ZOOM = 2.5;
/** The deepest the reader can zoom, as a multiple of the whole view. */
export const MAX_ZOOM = 8;
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
  const k = Math.min(FRAME_ZOOM, Math.max(1, Math.min((FILL_WIDTH * free * width) / (box.x1 - box.x0), (FILL_HEIGHT * height) / (box.y1 - box.y0))));
  const w = width / k;
  const h = height / k;
  return [(box.x0 + box.x1) / 2 - (w * free) / 2, (box.y0 + box.y1) / 2 - h / 2, w, h];
}

/** The view that shows the whole world width, centred, in a box of this width / height ratio. */
export function wholeView(world: ViewBox, aspect: number): ViewBox {
  const h = world[2] / aspect;
  return [world[0], world[1] + (world[3] - h) / 2, world[2], h];
}

/** The view the map opens on: the whole world, or in a box narrower than the world the world's full height, centred. */
export function openingView(world: ViewBox, aspect: number): ViewBox {
  if (aspect >= world[2] / world[3]) return wholeView(world, aspect);
  const w = world[3] * aspect;
  return [world[0] + (world[2] - w) / 2, world[1], w, world[3]];
}

/** `view` kept inside the world, and between the whole view and MAX_ZOOM times it. A view larger than the world on an axis is centred on it. */
export function clampView(view: ViewBox, world: ViewBox, whole: ViewBox): ViewBox {
  const w = Math.min(whole[2], Math.max(whole[2] / MAX_ZOOM, view[2]));
  const h = (w * whole[3]) / whole[2];
  const place = (pos: number, size: number, lo: number, total: number): number => (size >= total ? lo + (total - size) / 2 : Math.min(lo + total - size, Math.max(lo, pos)));
  return [place(view[0], w, world[0], world[2]), place(view[1], h, world[1], world[3]), w, h];
}

/** `view` zoomed by `factor` (> 1 zooms in) with the point at fractions (fx, fy) of the box held fixed. */
export function zoomedAt(view: ViewBox, factor: number, fx: number, fy: number, world: ViewBox, whole: ViewBox): ViewBox {
  const [x, y, w, h] = view;
  const nw = w / factor;
  const nh = h / factor;
  return clampView([x + fx * w - fx * nw, y + fy * h - fy * nh, nw, nh], world, whole);
}

/** `view` moved as if the map followed the pointer by (dx, dy) px in a box of `size` px. */
export function pannedBy(view: ViewBox, dx: number, dy: number, size: { width: number; height: number }, world: ViewBox, whole: ViewBox): ViewBox {
  return clampView([view[0] - (dx * view[2]) / size.width, view[1] - (dy * view[3]) / size.height, view[2], view[3]], world, whole);
}

/** True when two views differ by less than half a map unit. */
export const sameView = (a: ViewBox, b: ViewBox): boolean => a.every((v, i) => Math.abs(v - b[i]) < 0.5);

export interface Camera {
  /** The whole world, in the svg's own units. */
  world: ViewBox;
  /** The whole view and the opening view for the box's current ratio. */
  whole: ViewBox;
  opening: ViewBox;
  /** Where the view is heading, or rests. */
  goal(): ViewBox;
  /** Hold the view where it is now, cutting short an easing in progress. */
  stop(): void;
  to(target: ViewBox, animate: boolean): void;
  /** The box's width / height ratio changed: recompute the whole view and keep the reader's place. */
  fit(aspect: number): void;
}

/** Moves an svg's viewBox, easing over MOVE_MS or jumping; `onMove` runs whenever the goal changes. */
export function createCamera(svg: SVGSVGElement, onMove: () => void = () => {}): Camera {
  const read = (): ViewBox => svg.getAttribute("viewBox")!.split(" ").map(Number) as ViewBox;
  const write = (v: ViewBox): void => svg.setAttribute("viewBox", v.map((n) => +n.toFixed(2)).join(" "));
  const world = read();
  let frame = 0;
  let goal = world;
  const camera: Camera = {
    world,
    whole: world,
    opening: world,
    goal: () => goal,
    stop() {
      cancelAnimationFrame(frame);
      goal = read();
    },
    to(target, animate) {
      cancelAnimationFrame(frame);
      goal = target;
      onMove();
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
    fit(aspect) {
      const atOpening = sameView(goal, camera.opening);
      const zoom = goal[2] / camera.whole[2];
      const centre = [goal[0] + goal[2] / 2, goal[1] + goal[3] / 2];
      camera.whole = wholeView(world, aspect);
      camera.opening = openingView(world, aspect);
      const w = camera.whole[2] * zoom;
      const h = camera.whole[3] * zoom;
      camera.to(atOpening ? camera.opening : clampView([centre[0] - w / 2, centre[1] - h / 2, w, h], world, camera.whole), false);
    },
  };
  return camera;
}
