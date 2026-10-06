// Paces a sim's own clock toward a goal position, one frame at a time, and
// paints once the clock has caught up. This is the "scroll host" half of a
// scroll-driven wash: it carries no opinion about where `goal` comes from -- a scroll position, a
// timer, a test harness driving it by hand -- only how a sim's clock is
// stepped and when a caller should repaint. A consumer who wants the sim
// driven by real time or a timeline instead of scroll calls this the same
// way with a different `goal`.
import { smooth } from './math.js';
import { DEFAULT_PRESET, type WatercolorPreset } from './preset.js';
import type { WatercolorSim } from './solver.js';

export interface ClockState {
  /** Sim-seconds already stepped toward the last goal. */
  t: number;
  /** The `t` last painted, or -1 if nothing has been painted yet. */
  drawn: number;
}

export interface AdvanceResult {
  /** Whether `t` reached within one step of `goal` (the sim is as far along as this frame's goal asks). */
  caughtUp: boolean;
  /** Steps actually spent this call. */
  count: number;
}

export function advance(
  sim: Pick<WatercolorSim, 'reset' | 'step'>,
  state: ClockState,
  goal: number,
  fwd: boolean,
  stir: number,
  tilt: number,
  relift: number,
  paint: (t: number) => void,
  preset: WatercolorPreset = DEFAULT_PRESET,
): AdvanceResult {
  if (state.t > goal + preset.dt) { sim.reset(); state.t = 0; state.drawn = -1; }
  let budget = preset.stepsPerFrame, count = 0;
  while (state.t < goal && state.t < preset.tTotal && budget-- > 0) {
    sim.step(fwd, state.t, smooth(preset.tCure, preset.tTotal, state.t), stir, tilt, relift);
    state.t += preset.dt; count++;
  }
  if (state.t >= goal - preset.dt && state.t !== state.drawn) { paint(state.t); state.drawn = state.t; }
  return { caughtUp: state.t >= goal - preset.dt, count };
}
