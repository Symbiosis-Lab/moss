import { i as PigmentSample, u as WatercolorSim } from "../solver-BT7K3Dxe.mjs";

//#region src/sphere/index.d.ts
interface SphereOptions {
  /** Draw every nth cell; 1 uses the complete simulation grid. */
  stride?: number;
  /** Point diameter in canvas pixels; by default follows the grid's display scale. */
  pointSize?: number;
}
interface SpherePose {
  k: number;
  lift: number;
  grain: number;
  t: number;
}
interface PigmentSphere {
  /** Draw after playSides, using samples from that sim. Returns false for missing
   * samples, a disposed renderer or lost context. grain=0 leaves the canvas untouched.
   */
  draw(a: PigmentSample | null, b: PigmentSample | null, pose: SpherePose): boolean;
  dispose(): void;
}
/** Optional draw-only renderer. No textures, vertex buffers, simulation or clock. */
declare function createSphere(sim: WatercolorSim, options?: SphereOptions): PigmentSphere;
//#endregion
export { PigmentSphere, SphereOptions, SpherePose, createSphere };