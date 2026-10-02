// Shared by the solver and its default preset's tuning curves.
export const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));
export const smooth = (a: number, b: number, t: number): number => { const x = clamp01((t - a) / (b - a)); return x * x * (3 - 2 * x); };
