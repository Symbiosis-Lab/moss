// Blueprint Grid Background Animation
// Fixed grid revealed by particle line-crossings — the grid breathes,
// glowing where particles cross grid lines and fading elsewhere.
//
// Algorithm: A fixed grid of evenly-spaced horizontal and vertical lines is
// drawn as short segments. When a particle crosses (or nears) a line, the
// entire line lights up: brightest at the crossing point, fading along the
// line in both directions. Alpha per particle is the product of two Gaussians,
// summed across all particles (so brightness accumulates naturally):
//   perpAlpha = exp(-dPerp²/(2σPerp²))     — how close to the line
//   parallelAlpha = exp(-dParallel²/(2σParallel²)) — distance along the line
// The grid never moves; only its visibility changes as particles drift.

export interface Particle {
  x: number;  // normalized [0, 1]
  y: number;
  vx: number;
  vy: number;
}

export interface BlueprintGridConfig {
  particleCount: number;
  speed: number;
  repulsion: number;
  attraction: number;
  dampening: number;
  gridSpacing: number;    // pixels — cell size for square grid cells
  sigmaPerp: number;      // Gaussian σ for perpendicular distance (line activation)
  sigmaParallel: number;  // Gaussian σ for parallel distance (glow spread along line)
  segmentLength: number;  // pixels, e.g. 8
  minLineWidth: number;   // line width far from particles
  maxLineWidth: number;   // line width at particle center
}

const EPSILON = 1e-10;
const MIN_ALPHA = 0.01;

/**
 * Pixel pitch of the grid — horizontal lines are stroked at y = k × this.
 *
 * Exported so a page can align its own marks to a real grid line. Two
 * constants would drift, and the failure would be silent: a mark floating one
 * pixel off a line still looks deliberate.
 */
export const GRID_SPACING = 40;

// --- Pure functions (testable without DOM) ---

export function createParticles(count: number, speed: number): Particle[] {
  return Array.from({ length: count }, () => ({
    x: Math.random(),
    y: Math.random(),
    vx: (Math.random() - 0.5) * speed * 2,
    vy: (Math.random() - 0.5) * speed * 2,
  }));
}

export function updatePhysics(
  particles: Particle[],
  mouseX: number,
  mouseY: number,
  config: BlueprintGridConfig,
): void {
  const n = particles.length;

  for (let i = 0; i < n; i++) {
    const p = particles[i];

    // Particle-particle interaction: repulsion + tangential orbit
    // Repulsion pushes apart; tangential component makes them swirl like planets
    for (let j = i + 1; j < n; j++) {
      const q = particles[j];
      const dx = q.x - p.x;
      const dy = q.y - p.y;
      const distSq = dx * dx + dy * dy + EPSILON;

      // Radial repulsion (inverse-square)
      const force = config.repulsion / distSq;
      const fx = force * dx;
      const fy = force * dy;
      p.vx -= fx;
      p.vy -= fy;
      q.vx += fx;
      q.vy += fy;

      // Tangential orbit: perpendicular to the radial direction
      // Creates a consistent clockwise swirl between nearby particles
      const dist = Math.sqrt(distSq);
      const tangent = 0.0000004 / distSq;
      const tx = tangent * (-dy / dist);
      const ty = tangent * (dx / dist);
      p.vx += tx;
      p.vy += ty;
      q.vx -= tx;
      q.vy -= ty;
    }

    // Mouse interaction: linear attraction + exponential close-range repulsion
    // Creates a "swing" effect — particles orbit the cursor, flung away when too close
    const mdx = mouseX - p.x;
    const mdy = mouseY - p.y;
    const mDistSq = mdx * mdx + mdy * mdy;
    const mDist = Math.sqrt(mDistSq + EPSILON);
    // Attraction: cubic — stronger when farther, weakens as particles approach
    // Particles rush in fast then slow down near the cursor
    p.vx += config.attraction * mdx * mDistSq;
    p.vy += config.attraction * mdy * mDistSq;
    // Repulsion: exponential kick when within close range of cursor
    const mouseRepulse = 0.00003 * Math.exp(-mDist / 0.005);
    p.vx -= mouseRepulse * (mdx / mDist);
    p.vy -= mouseRepulse * (mdy / mDist);

    // Dampen velocity
    p.vx *= config.dampening;
    p.vy *= config.dampening;

    // Cap velocity
    const speedSq = p.vx * p.vx + p.vy * p.vy;
    if (speedSq > config.speed * config.speed) {
      const scale = config.speed / Math.sqrt(speedSq);
      p.vx *= scale;
      p.vy *= scale;
    }

    // Apply velocity
    p.x += p.vx;
    p.y += p.vy;

    // Wrap bounds
    if (p.x < 0) p.x += 1;
    else if (p.x > 1) p.x -= 1;
    if (p.y < 0) p.y += 1;
    else if (p.y > 1) p.y -= 1;
  }
}

// --- Canvas integration ---

const DEFAULT_CONFIG: BlueprintGridConfig = {
  particleCount: 12,
  speed: 0.0025,
  repulsion: 0.0000002,
  attraction: 0.006,
  dampening: 0.985,
  gridSpacing: GRID_SPACING,
  sigmaPerp: 0.018,
  sigmaParallel: 0.18,
  segmentLength: 8,
  minLineWidth: 0.1,
  maxLineWidth: 0.4,
};

export interface BlueprintGridOptions {
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

/**
 * The events that move the point the particles swing around. Pointer events
 * stop once a touch turns into a scroll, so touches are read as well: a finger
 * leads the particles as a cursor does.
 */
const POINTER_EVENTS = ['pointermove', 'touchstart', 'touchmove'] as const;

/** The viewport position an event from `POINTER_EVENTS` reports, if any. */
export function pointerPoint(e: Event): { clientX: number; clientY: number } | null {
  const touches = (e as TouchEvent).touches;
  if (touches) return touches[0] ?? null;
  const m = e as MouseEvent;
  return typeof m.clientX === 'number' ? m : null;
}

export function startBlueprintGrid(container: HTMLElement, options: BlueprintGridOptions = {}): () => void {
  const canvas = options.canvas ?? container.querySelector<HTMLCanvasElement>('#blueprint-canvas');
  if (!canvas) return () => {};

  // jsdom (used by vitest) ships HTMLCanvasElement.prototype.getContext as a
  // "Not implemented" stub that throws. Real browsers always return either a
  // context or null.
  let rawCtx: CanvasRenderingContext2D | null;
  try {
    rawCtx = canvas.getContext('2d');
  } catch {
    return () => {};
  }
  if (!rawCtx) return () => {};
  // TS doesn't carry the null narrowing into the inner closures.
  const ctx: CanvasRenderingContext2D = rawCtx;

  const config = DEFAULT_CONFIG;
  const maxAlpha = options.maxAlpha ?? 0.25;
  const particles = createParticles(config.particleCount, config.speed);
  let mouseX = 0.5;
  let mouseY = 0.5;
  let rafId = 0;
  let width = 0;
  let height = 0;
  const twoSigmaPerpSq = 2 * config.sigmaPerp * config.sigmaPerp;
  const twoSigmaParallelSq = 2 * config.sigmaParallel * config.sigmaParallel;
  const crossings: Array<{ at: number; perpAlpha: number }> = [];

  let isDark = document.documentElement.getAttribute('data-theme') === 'dark';
  // People who ask for reduced motion get the grid at rest: one still frame
  // per size or theme, no loop.
  const reduceMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches === true;

  const themeObserver = new MutationObserver(() => {
    isDark = document.documentElement.getAttribute('data-theme') === 'dark';
    if (reduceMotion) paint();
  });
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });

  const observer = new ResizeObserver((entries) => {
    for (const entry of entries) {
      width = entry.contentRect.width;
      height = entry.contentRect.height;
      canvas.width = width * devicePixelRatio;
      canvas.height = height * devicePixelRatio;
      ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
    }
    if (reduceMotion) paint();
  });
  observer.observe(container);

  const pointerTarget = options.pointerTarget ?? container;
  const onPointer = (e: Event) => {
    const p = pointerPoint(e);
    if (!p) return;
    const rect = container.getBoundingClientRect();
    mouseX = (p.clientX - rect.left) / rect.width;
    mouseY = (p.clientY - rect.top) / rect.height;
  };
  // Passive, so a touch still scrolls the page.
  for (const type of POINTER_EVENTS) pointerTarget.addEventListener(type, onPointer, { passive: true });

  function paint() {
    options.onFrame?.();
    ctx.clearRect(0, 0, width, height);
    if (width === 0 || height === 0) return;
    ctx.strokeStyle = options.color?.() || (isDark ? 'rgb(90,155,255)' : 'rgb(20,60,130)');
    paintLines(true);
    paintLines(false);
  }

  // One family of lines: horizontal ones step down the height and run along
  // the width; vertical ones the other way round. A line lights where particles
  // cross it, brightest at each crossing and fading along it.
  function paintLines(horizontal: boolean) {
    const along = horizontal ? width : height;
    const across = horizontal ? height : width;
    const step = config.gridSpacing / across;
    const segLen = config.segmentLength / along;
    for (let line = step; line < 1; line += step) {
      crossings.length = 0;
      for (const p of particles) {
        const dPerp = (horizontal ? p.y : p.x) - line;
        const perpAlpha = Math.exp(-(dPerp * dPerp) / twoSigmaPerpSq);
        if (perpAlpha >= MIN_ALPHA) crossings.push({ at: horizontal ? p.x : p.y, perpAlpha });
      }
      if (crossings.length === 0) continue;

      const c = line * across;
      for (let s = 0; s < 1; s += segLen) {
        const mid = s + segLen / 2;
        let sum = 0;
        for (const x of crossings) {
          const d = mid - x.at;
          sum += x.perpAlpha * Math.exp(-(d * d) / twoSigmaParallelSq);
        }
        const k = Math.min(sum, 1);
        if (k < MIN_ALPHA) continue;
        const a0 = s * along;
        const a1 = Math.min((s + segLen) * along, along);
        ctx.globalAlpha = k * maxAlpha;
        ctx.lineWidth = config.minLineWidth + k * (config.maxLineWidth - config.minLineWidth);
        ctx.beginPath();
        if (horizontal) { ctx.moveTo(a0, c); ctx.lineTo(a1, c); }
        else { ctx.moveTo(c, a0); ctx.lineTo(c, a1); }
        ctx.stroke();
      }
    }
  }

  function frame() {
    updatePhysics(particles, mouseX, mouseY, config);
    paint();
    rafId = requestAnimationFrame(frame);
  }

  if (!reduceMotion) rafId = requestAnimationFrame(frame);

  return () => {
    cancelAnimationFrame(rafId);
    observer.disconnect();
    themeObserver.disconnect();
    for (const type of POINTER_EVENTS) pointerTarget.removeEventListener(type, onPointer);
  };
}
