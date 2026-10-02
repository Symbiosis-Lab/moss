// Seeded sampling for the paper generator. seededRandom is the same
// generator the engine's original noise paper uses, so both share one
// implementation.
export type Rng = () => number;

export function seededRandom(seed: number): Rng {
  let a0 = seed;
  return () => {
    a0 = (a0 + 0x6d2b79f5) | 0;
    let t = Math.imul(a0 ^ (a0 >>> 15), 1 | a0);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function normal(rng: Rng): number {
  return Math.sqrt(-2 * Math.log(1 - rng())) * Math.cos(2 * Math.PI * rng());
}

/** A lognormal sample with the given arithmetic mean and log-space standard deviation. */
export function lognormal(rng: Rng, mean: number, logSd: number): number {
  const mu = Math.log(mean) - (logSd * logSd) / 2;
  return Math.exp(mu + logSd * normal(rng));
}

/** An axial angle in [0, π): the doubled angle is von Mises around 0 with concentration kappa (Best & Fisher 1979). */
export function vonMisesAxial(rng: Rng, kappa: number): number {
  if (kappa <= 0) return Math.PI * rng();
  const tau = 1 + Math.sqrt(1 + 4 * kappa * kappa);
  const rho = (tau - Math.sqrt(2 * tau)) / (2 * kappa);
  const r = (1 + rho * rho) / (2 * rho);
  for (;;) {
    const z = Math.cos(Math.PI * rng());
    const f = (1 + r * z) / (r + z);
    const c = kappa * (r - f);
    const u2 = rng();
    if (c * (2 - c) - u2 > 0 || Math.log(c / u2) + 1 - c >= 0) {
      const phi = (rng() > 0.5 ? 1 : -1) * Math.acos(Math.max(-1, Math.min(1, f)));
      return (((phi / 2) % Math.PI) + Math.PI) % Math.PI;
    }
  }
}
