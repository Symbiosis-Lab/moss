import type { WatercolorSim } from '../engine/solver.js';

interface SimContext {
  gl: WebGL2RenderingContext;
  width: number;
  height: number;
  ground: 'paper' | 'transparent';
  page: { tint: [number, number, number]; dark: boolean };
}

// Shared by the engine and optional renderers; never attached to a public sim.
const contexts = new WeakMap<WatercolorSim, SimContext>();
export const registerSim = (sim: WatercolorSim, context: SimContext): void => { contexts.set(sim, context); };
export const forgetSim = (sim: WatercolorSim): void => { contexts.delete(sim); };
export function contextFor(sim: WatercolorSim): SimContext {
  const context = contexts.get(sim);
  if (!context) throw new Error('Watercolor context unavailable (foreign or disposed sim)');
  return context;
}
