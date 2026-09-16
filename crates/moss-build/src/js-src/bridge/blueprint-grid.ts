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
  maxAlpha: number;       // peak alpha at crossing point
  segmentLength: number;  // pixels, e.g. 8
  minLineWidth: number;   // line width far from particles
  maxLineWidth: number;   // line width at particle center
}

const EPSILON = 1e-10;
const MIN_ALPHA = 0.01;

/**
 * Pixel pitch of the grid — horizontal lines are stroked at y = k × this.
 *
 * Exported because the cloud waiting screen aligns its progress meter to a
 * real grid line (see `alignWaitingScreenToGrid`). Two constants would drift,
 * and the failure would be silent: a meter floating one pixel off a line still
 * looks deliberate.
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

/**
 * Line-crossing alpha: product of perpendicular and parallel Gaussians,
 * summed across all particles so brightness accumulates naturally.
 * Two particles on the same line make it brighter than one.
 * Clamped to [0, 1].
 */
export function computeLineAlpha(
  linePos: number,
  segCenter: number,
  particles: Particle[],
  sigmaPerp: number,
  sigmaParallel: number,
  isHorizontal: boolean,
): number {
  const twoSigmaPerpSq = 2 * sigmaPerp * sigmaPerp;
  const twoSigmaParallelSq = 2 * sigmaParallel * sigmaParallel;
  let sum = 0;
  for (const p of particles) {
    const dPerp = isHorizontal ? (p.y - linePos) : (p.x - linePos);
    const perpAlpha = Math.exp(-(dPerp * dPerp) / twoSigmaPerpSq);
    if (perpAlpha < MIN_ALPHA) continue;
    const dParallel = isHorizontal ? (segCenter - p.x) : (segCenter - p.y);
    sum += perpAlpha * Math.exp(-(dParallel * dParallel) / twoSigmaParallelSq);
  }
  return Math.min(sum, 1);
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
  maxAlpha: 0.25,
  segmentLength: 8,
  minLineWidth: 0.1,
  maxLineWidth: 0.4,
};

// `onFrame` is an optional per-frame callback. It exists so the SHELL/editor
// call sites can count frames for diagnostics WITHOUT this shared module
// importing the diag → @tauri-apps/plugin-log chain — this file is also bundled
// into the preview-only iframe-bridge, which must stay free of Tauri/log
// machinery (the preview is sacred). See preview-manager.ts for the counter.
export function startBlueprintGrid(container: HTMLElement, onFrame?: () => void): () => void {
  const canvas = container.querySelector<HTMLCanvasElement>('#blueprint-canvas');
  if (!canvas) return () => {};

  // jsdom (used by vitest) ships HTMLCanvasElement.prototype.getContext as a
  // "Not implemented" stub that throws. Real browsers always return either a
  // context or null. Wrap so unit tests that mount any consumer of this module
  // (folder-mode, preview empty state, launcher) don't blow up.
  let rawCtx: CanvasRenderingContext2D | null;
  try {
    rawCtx = canvas.getContext('2d');
  } catch {
    return () => {};
  }
  if (!rawCtx) return () => {};
  // TS doesn't carry the null narrowing into the inner `frame()` closure;
  // re-bind to a definitely non-null local that the closure can capture.
  const ctx: CanvasRenderingContext2D = rawCtx;

  const config = DEFAULT_CONFIG;
  const particles = createParticles(config.particleCount, config.speed);
  let mouseX = 0.5;
  let mouseY = 0.5;
  let rafId = 0;
  let width = 0;
  let height = 0;

  // Detect dark mode from data-theme attribute (maintained by followTheme)
  let isDark = document.documentElement.getAttribute('data-theme') === 'dark';
  const themeObserver = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.type === 'attributes' && mutation.attributeName === 'data-theme') {
        isDark = document.documentElement.getAttribute('data-theme') === 'dark';
      }
    }
  });
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });

  // Resize canvas to match container
  const observer = new ResizeObserver((entries) => {
    for (const entry of entries) {
      width = entry.contentRect.width;
      height = entry.contentRect.height;
      canvas.width = width * devicePixelRatio;
      canvas.height = height * devicePixelRatio;
      ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
    }
  });
  observer.observe(container);

  // Mouse tracking on container (canvas has pointer-events: none)
  const onMouseMove = (e: MouseEvent) => {
    const rect = container.getBoundingClientRect();
    mouseX = (e.clientX - rect.left) / rect.width;
    mouseY = (e.clientY - rect.top) / rect.height;
  };
  container.addEventListener('mousemove', onMouseMove);

  function frame() {
    onFrame?.();
    updatePhysics(particles, mouseX, mouseY, config);
    ctx.clearRect(0, 0, width, height);

    if (width === 0) {
      rafId = requestAnimationFrame(frame);
      return;
    }

    const segLen = config.segmentLength / width; // normalize segment length
    const twoSigmaPerpSq = 2 * config.sigmaPerp * config.sigmaPerp;
    const twoSigmaParallelSq = 2 * config.sigmaParallel * config.sigmaParallel;

    // Square grid: convert pixel spacing to normalized spacing per axis
    const hSpacing = config.gridSpacing / height; // horizontal lines step in y
    const vSpacing = config.gridSpacing / width;  // vertical lines step in x

    // Reusable array for per-line pre-filtered crossings
    const crossings: Array<{ crossPos: number; perpAlpha: number }> = [];

    // Draw horizontal grid lines
    for (let gy = hSpacing; gy < 1; gy += hSpacing) {
      crossings.length = 0;
      for (const p of particles) {
        const dPerp = p.y - gy;
        const perpAlpha = Math.exp(-(dPerp * dPerp) / twoSigmaPerpSq);
        if (perpAlpha >= MIN_ALPHA) crossings.push({ crossPos: p.x, perpAlpha });
      }
      if (crossings.length === 0) continue;

      for (let x = 0; x < 1; x += segLen) {
        const segCenter = x + segLen / 2;
        let sum = 0;
        for (const c of crossings) {
          const dP = segCenter - c.crossPos;
          sum += c.perpAlpha * Math.exp(-(dP * dP) / twoSigmaParallelSq);
        }
        const clamped = Math.min(sum, 1);
        if (clamped < MIN_ALPHA) continue;
        const a = clamped * config.maxAlpha;
        const lw = config.minLineWidth + clamped * (config.maxLineWidth - config.minLineWidth);
        ctx.beginPath();
        ctx.moveTo(x * width, gy * height);
        ctx.lineTo(Math.min((x + segLen) * width, width), gy * height);
        ctx.strokeStyle = isDark
          ? `rgba(90,155,255,${a})`
          : `rgba(20,60,130,${a})`;
        ctx.lineWidth = lw;
        ctx.stroke();
      }
    }

    // Draw vertical grid lines
    for (let gx = vSpacing; gx < 1; gx += vSpacing) {
      crossings.length = 0;
      for (const p of particles) {
        const dPerp = p.x - gx;
        const perpAlpha = Math.exp(-(dPerp * dPerp) / twoSigmaPerpSq);
        if (perpAlpha >= MIN_ALPHA) crossings.push({ crossPos: p.y, perpAlpha });
      }
      if (crossings.length === 0) continue;

      for (let y = 0; y < 1; y += segLen) {
        const segCenter = y + segLen / 2;
        let sum = 0;
        for (const c of crossings) {
          const dP = segCenter - c.crossPos;
          sum += c.perpAlpha * Math.exp(-(dP * dP) / twoSigmaParallelSq);
        }
        const clamped = Math.min(sum, 1);
        if (clamped < MIN_ALPHA) continue;
        const a = clamped * config.maxAlpha;
        const lw = config.minLineWidth + clamped * (config.maxLineWidth - config.minLineWidth);
        ctx.beginPath();
        ctx.moveTo(gx * width, y * height);
        ctx.lineTo(gx * width, Math.min((y + segLen) * height, height));
        ctx.strokeStyle = isDark
          ? `rgba(90,155,255,${a})`
          : `rgba(20,60,130,${a})`;
        ctx.lineWidth = lw;
        ctx.stroke();
      }
    }

    rafId = requestAnimationFrame(frame);
  }

  rafId = requestAnimationFrame(frame);

  // Return cleanup function
  return () => {
    cancelAnimationFrame(rafId);
    container.removeEventListener('mousemove', onMouseMove);
    observer.disconnect();
    themeObserver.disconnect();
  };
}
