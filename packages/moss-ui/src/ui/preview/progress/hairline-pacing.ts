/**
 * deployFraction — map a `DeployProgress` tick onto ONE monotonic 0→1 for the
 * nav-pill hairline. Pure; no DOM, no timers.
 *
 * # Why this exists
 *
 * The hairline tracks the OPERATION, not the current phase: Material's rule
 * for a multi-stage indicator is to show overall progress rather than each
 * activity's own progress, because a bar that restarts at 0 for the next
 * phase reads as having gone backwards.
 *
 * # The weights
 *
 * Each stage owns a span of the bar; within `uploading` the span is filled by
 * real bytes. The spans are contiguous and ascending, so the value never
 * decreases across a stage change — the one invariant every progress-indicator
 * guideline agrees on (VA.gov: a determinate bar "should not return or retreat
 * once it progresses").
 *
 * `uploading` owns 82 of the 96 points on offer, and the head before it is
 * deliberately thin (8 points): `deployFraction` returns the span START for
 * every non-upload stage, because Rust emits `current: 0, total: 0` for them,
 * so a larger head would be an instantaneous jump with zero bytes behind it
 * rather than a paced estimate. The cost is honest and accepted: a long
 * pre-deploy rebuild leaves the bar sitting near the left. The progress PANEL
 * narrates that stage in words ("Rebuilding site"); the hairline is a
 * magnitude, and it has no magnitude to show until bytes move.
 *
 * The tail (committing/finalizing) stays short and back-loaded. Harrison et al.,
 * "Rethinking the Progress Bar" (UIST 2007) measured this directly: functions
 * that pause near the END are perceived as SLOWER than linear, and the two
 * functions perceived as faster than linear were the ones accelerating toward
 * completion. So the cheap stages that always run last must not be allowed to
 * stall a nearly-full bar.
 *
 * The remaining 4% above `finalizing` belongs to `complete()`, which fills to
 * full and flashes the pill — the hairline's existing "the finishing step owns
 * the last slice" contract.
 */

/**
 * The stages of a deploy, structurally mirroring the generated `DeployStage`.
 * `deployFraction` takes the shape it needs, so a caller holding a real
 * `DeployProgress` passes it unchanged.
 */
export type DeployStageName =
  | "preparing"
  | "rebuilding"
  | "syncing"
  | "uploading"
  | "committing"
  | "finalizing"
  | "verifying"
  | "complete"
  | "failed";

/** The fields of a `DeployProgress` tick that bear on bar position. */
export interface DeployTick {
  stage: DeployStageName;
  current: number;
  total: number;
  bytes_uploaded?: number | null;
  bytes_total?: number | null;
}

/** [start, end] span of the overall bar owned by each stage. */
const SPANS: Record<DeployStageName, readonly [number, number]> = {
  preparing: [0.0, 0.01],
  rebuilding: [0.01, 0.05],
  syncing: [0.05, 0.08],
  uploading: [0.08, 0.9],
  committing: [0.9, 0.94],
  finalizing: [0.94, 0.96],
  // Publish-verified-live: fires after the guard's `complete` while the live
  // verdict is pending. Parks at finalizing's end — monotonic — until the
  // coarse bus event completes the bar on command resolution.
  verifying: [0.96, 0.96],
  // Terminal stages own no span — the caller drives complete()/cancel().
  complete: [1.0, 1.0],
  failed: [0.0, 0.0],
};

/**
 * The overall 0→1 for this tick, or `null` when the stage carries no bar
 * position (the terminal `complete`/`failed`, which the caller handles).
 *
 * Within `uploading`, `bytes_uploaded / bytes_total` interpolates the span. When
 * the byte fields are absent — the prebuilt/CLI deploy path emits file counts
 * only — `current / total` is used instead, and when neither is usable the tick
 * parks at the span's start rather than inventing motion.
 */
export function deployFraction(p: DeployTick): number | null {
  if (p.stage === "complete" || p.stage === "failed") return null;

  const [start, end] = SPANS[p.stage];
  if (p.stage !== "uploading") return start;

  const within = uploadRatio(p);
  return start + (end - start) * within;
}

/** Fraction of the upload stage done, in [0,1]. Bytes preferred; files are the
 *  documented fallback for the prebuilt path, which emits no byte fields. */
function uploadRatio(p: DeployTick): number {
  const total = p.bytes_total ?? 0;
  if (total > 0) return clamp01((p.bytes_uploaded ?? 0) / total);
  if (p.total > 0) return clamp01(p.current / p.total);
  return 0;
}

function clamp01(n: number): number {
  if (!Number.isFinite(n)) return 0;
  return Math.min(1, Math.max(0, n));
}
