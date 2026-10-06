/**
 * projection.ts — the places-explorer world map's coordinate space.
 *
 * A Patterson (2014) cylindrical projection over the shared `world.svg` the
 * explorer draws its points onto — the full world, uncropped, not the
 * separate per-page static map (a fixed 720x480 canvas that crops the
 * sides; see `VIEWBOX_WIDTH`/`VIEWBOX_HEIGHT` in
 * `crates/moss-build/src/build/place_map/geometry.rs`). Longitude is linear,
 * and y = K1*phi + K2*phi^5 + K3*phi^7 + K4*phi^9 (the published polynomial).
 * The same K1..K4 constants and the same polynomial drive the Rust side's
 * `PATTERSON_K1..K4`/`patterson_y` — the two are independent implementations
 * of the one published formula, not a shared library, so
 * `__tests__/projection.test.ts` reimplements it from scratch, against
 * hardcoded expected numbers rather than this module's own constants, as a
 * cross-check rather than trusting that "looks the same" actually is.
 *
 * `SCALE` (world units per radian) is set so the poles exactly reach the
 * canvas's top and bottom edge, the same as the per-page map. `WORLD_WIDTH`
 * is then derived from that same `SCALE` rather than fixed independently, so
 * the antimeridian lands exactly on the left/right edges (x = 0 and
 * x = WORLD_WIDTH) instead of being cropped off-canvas: `WORLD_WIDTH =
 * 2 * PI * SCALE = WORLD_HEIGHT * PI / patterson_y(PI / 2)`, about 842.
 */

export const WORLD_HEIGHT = 480;

const PATTERSON_K1 = 1.0148;
const PATTERSON_K2 = 0.23185;
const PATTERSON_K3 = -0.14499;
const PATTERSON_K4 = 0.02406;

function pattersonY(phi: number): number {
  const phi2 = phi * phi;
  const phi4 = phi2 * phi2;
  return phi * (PATTERSON_K1 + phi4 * (PATTERSON_K2 + phi2 * (PATTERSON_K3 + PATTERSON_K4 * phi2)));
}

/** `patterson_y`'s derivative, for `unproject`'s Newton-Raphson latitude solve. */
function pattersonYDerivative(phi: number): number {
  const phi2 = phi * phi;
  const phi4 = phi2 * phi2;
  const phi6 = phi4 * phi2;
  const phi8 = phi4 * phi4;
  return PATTERSON_K1 + 5 * PATTERSON_K2 * phi4 + 7 * PATTERSON_K3 * phi6 + 9 * PATTERSON_K4 * phi8;
}

const Y_AT_POLE = pattersonY(Math.PI / 2);
const SCALE = WORLD_HEIGHT / 2 / Y_AT_POLE;
export const WORLD_WIDTH = (WORLD_HEIGHT * Math.PI) / Y_AT_POLE;
const TRANSLATE_X = WORLD_WIDTH / 2;
const TRANSLATE_Y = WORLD_HEIGHT / 2;

/** Project a latitude/longitude pair (degrees) onto the world canvas. */
export function project(latitude: number, longitude: number): { x: number; y: number } {
  const lambda = (longitude * Math.PI) / 180;
  const phi = (latitude * Math.PI) / 180;
  return {
    x: TRANSLATE_X + SCALE * lambda,
    y: TRANSLATE_Y - SCALE * pattersonY(phi),
  };
}

/**
 * Invert `project`. Longitude is linear and inverts directly; latitude
 * needs Newton-Raphson (`pattersonY` has no closed-form inverse) — eight
 * iterations comfortably clears the 1e-6 degree tolerance the round-trip
 * test asks for, since each iteration roughly squares the number of
 * correct digits near the well-conditioned (non-polar) latitudes a map
 * reader actually clicks.
 */
export function unproject(x: number, y: number): { lat: number; lng: number } {
  const lambda = (x - TRANSLATE_X) / SCALE;
  const targetY = (TRANSLATE_Y - y) / SCALE;
  let phi = targetY; // first-order guess: patterson_y(phi) ~= phi near the equator
  for (let i = 0; i < 8; i++) {
    const delta = (pattersonY(phi) - targetY) / pattersonYDerivative(phi);
    phi -= delta;
  }
  return {
    lat: (phi * 180) / Math.PI,
    lng: (lambda * 180) / Math.PI,
  };
}
