import { p as WatercolorPreset, u as WatercolorSim } from "./solver-BT7K3Dxe.mjs";

//#region src/engine/host.d.ts
interface ClockState {
  /** Sim-seconds already stepped toward the last goal. */
  t: number;
  /** The `t` last painted, or -1 if nothing has been painted yet. */
  drawn: number;
}
interface AdvanceResult {
  /** Whether `t` reached within one step of `goal` (the sim is as far along as this frame's goal asks). */
  caughtUp: boolean;
  /** Steps actually spent this call. */
  count: number;
}
declare function advance(sim: Pick<WatercolorSim, 'reset' | 'step'>, state: ClockState, goal: number, fwd: boolean, stir: number, tilt: number, relift: number, paint: (t: number) => void, preset?: WatercolorPreset): AdvanceResult;
//#endregion
//#region src/engine/shaders.d.ts
declare const DIAG_VIEWS: Record<number, string>;
//#endregion
export { advance as i, AdvanceResult as n, ClockState as r, DIAG_VIEWS as t };