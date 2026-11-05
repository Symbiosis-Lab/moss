/**
 * Two-stroke icon morphing — the shared engine.
 *
 * moss draws a family of small glyphs (plus, minus, cross, check, progress
 * ring) as the SAME two strokes posed differently, and moves between them by
 * rewriting each stroke's `d` per frame.
 * This file is
 * the arithmetic behind them, with no opinion about any particular glyph.
 *
 * Two things are worth knowing before using it.
 *
 * `d` is rewritten from JS on purpose. CSS `d: path()` is unshipped in stable
 * Safari, and moss's windows are WebKit (WKWebView / WebKitGTK) — a CSS-driven
 * morph animates in a chromium render gate and jump-cuts in the shipped app.
 * Nor can a transform help: `transform-origin` on an SVG child resolves
 * against a bounding box that moves every frame, and no affine map turns a
 * straight line into a circle.
 *
 * A stroke is not a path, it is five numbers: an anchor P, a tangent angle
 * `phi`, a curvature `k` (= 1/R, zero being a straight bar), an arc length
 * `len`, and `bias` — where P sits along it. Bending a bar into a ring is `k`
 * moving off zero; wrapping it further is `len` growing. Every glyph moss
 * morphs is expressible this way, which is what lets one engine serve all of
 * them.
 */

const TAU = Math.PI * 2;

export interface Stroke {
  P: [number, number];
  phi: number;
  k: number;
  len: number;
  bias: number;
  /** Opacity, so a stroke can retract and fade on the same clock. */
  op: number;
}
export interface Pose { a: Stroke; b: Stroke }

const f3 = (n: number) => Number(n.toFixed(3));

/** The `d` for one stroke. Empty string when the stroke has collapsed. */
export function strokeD(
  P: [number, number], phi: number, k: number, len: number, bias: number,
): string {
  if (len < 0.01) return '';
  const u = [Math.cos(phi), Math.sin(phi)];
  const back = len * bias, fwd = len * (1 - bias);
  if (Math.abs(k) < 1e-6) {
    return `M${f3(P[0] - u[0] * back)},${f3(P[1] - u[1] * back)}`
         + `L${f3(P[0] + u[0] * fwd)},${f3(P[1] + u[1] * fwd)}`;
  }
  const R = 1 / k;
  const O = [P[0] + R * Math.cos(phi + Math.PI / 2), P[1] + R * Math.sin(phi + Math.PI / 2)];
  const aP = Math.atan2(P[1] - O[1], P[0] - O[0]);
  const sweep = Math.min(len / R, TAU);
  const at = (a: number): [number, number] => [O[0] + R * Math.cos(a), O[1] + R * Math.sin(a)];
  const a0 = aP - sweep * bias;
  if (sweep >= TAU - 1e-4) {
    const p0 = at(a0), pm = at(a0 + Math.PI);
    return `M${f3(p0[0])},${f3(p0[1])}`
         + `A${f3(R)},${f3(R)} 0 0 1 ${f3(pm[0])},${f3(pm[1])}`
         + `A${f3(R)},${f3(R)} 0 0 1 ${f3(p0[0])},${f3(p0[1])}`;
  }
  const p0 = at(a0), p1 = at(a0 + sweep);
  return `M${f3(p0[0])},${f3(p0[1])}`
       + `A${f3(R)},${f3(R)} 0 ${sweep > Math.PI ? 1 : 0} 1 ${f3(p1[0])},${f3(p1[1])}`;
}

export const drawStroke = (s: Stroke): string => strokeD(s.P, s.phi, s.k, s.len, s.bias);

/* ── interpolation ─────────────────────────────────────────────────────
   Length and shape do NOT share a clock, and which one leads reverses with
   the direction.

   Growing, shape leads: the stroke arrives on its final circle early and then
   grows along it — the bar bends, then wraps. `phi` rides the same clock as
   `k` because together they decide WHICH circle the stroke lies on (the
   centre is P + (1/k)·n(phi)); on separate clocks a correctly-curved arc sits
   on a circle whose centre is still swinging into place, and swings out of
   its own box.

   Collapsing, length leads: straightening a 179-unit arc while it is still
   179 units long is a bar three tiles wide, sweeping. The ring retracts to a
   short arc at the anchor first — on R = 28.5 an 11-unit arc is already
   nearly straight — and only then uncurves. One rule either way: never be
   long and wrong at the same time.

   The direction is DERIVED from the two poses, never declared by the caller.
   As an argument it was a thing every call site had to get right, and a
   morph played with the wrong clock order is the exact failure above.       */
const CURVE_BY  = 0.45;
const SHRINK_BY = 0.55;
const SHAPE_AT  = 0.35;

export const ease = (t: number) => (t < .5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const seg = (t: number, a: number, b: number) => ease(Math.max(0, Math.min(1, (t - a) / (b - a))));

/** Shortest signed way round, so 225°→315° is +90 and not −270. */
function lerpAngle(a: number, b: number, t: number): number {
  let d = (b - a) % TAU;
  if (d > Math.PI) d -= TAU;
  if (d < -Math.PI) d += TAU;
  return a + d * t;
}

/** `lt` drives length/bias/opacity; `gt` drives the shape (P, phi, k). */
const lerpStroke = (x: Stroke, y: Stroke, lt: number, gt: number): Stroke => ({
  P: [lerp(x.P[0], y.P[0], gt), lerp(x.P[1], y.P[1], gt)],
  phi: lerpAngle(x.phi, y.phi, gt),
  k: lerp(x.k, y.k, gt),
  len: lerp(x.len, y.len, lt),
  bias: lerp(x.bias, y.bias, lt),
  op: lerp(x.op, y.op, lt),
});

const totalLen = (p: Pose) => p.a.len + p.b.len;

/** `t` is RAW (0..1); both clocks and the easing are applied here. */
export function lerpPose(x: Pose, y: Pose, t: number): Pose {
  const collapsing = totalLen(y) < totalLen(x);
  const lt = collapsing ? seg(t, 0, SHRINK_BY) : ease(t);
  const gt = collapsing ? seg(t, SHAPE_AT, 1)  : seg(t, 0, CURVE_BY);
  return { a: lerpStroke(x.a, y.a, lt, gt), b: lerpStroke(x.b, y.b, lt, gt) };
}

/** Read at call time, never cached — a mid-session OS toggle must be respected. */
export function prefersReducedMotion(): boolean {
  return typeof matchMedia === 'function'
    && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Play `from` → `to`, handing each frame's pose to `paint`. Under reduced
 * motion the end pose is painted once and the promise resolves immediately —
 * the destination is never skipped, only the way there.
 */
export function tweenPose(
  from: Pose, to: Pose, ms: number, paint: (p: Pose) => void,
): Promise<void> {
  if (prefersReducedMotion()) {
    paint(to);
    return Promise.resolve();
  }
  const t0 = performance.now();
  return new Promise<void>((res) => {
    const frame = () => {
      const t = Math.min(1, (performance.now() - t0) / ms);
      paint(lerpPose(from, to, t));
      if (t < 1) requestAnimationFrame(frame); else res();
    };
    requestAnimationFrame(frame);
  });
}
