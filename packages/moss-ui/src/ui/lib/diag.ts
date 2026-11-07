// Lightweight diagnostic instrumentation for energy + layout-shift diagnosis.
//
// Everything funnels through @tauri-apps/plugin-log → the shared moss.log
// (~/Library/Logs/host.moss.publisher/moss.log), tagged `[diag:*]` to match the
// existing Rust-side `[diag] …` / `[phase] …` conventions. One file, one
// `tail -f`, captures both webviews and the Rust backend.
//
// Enabled by default in dev builds. Kill-switch matches the codebase's
// `moss.tree.trace` / `moss.preview.morph` convention:
//   localStorage['moss.diag'] = '0'  → silence
//   localStorage['moss.diag'] = '1'  → force-on (even in a release build)
//
// Design rule: PASSIVE counters + exactly ONE slow summary timer. We never add
// a requestAnimationFrame or polling loop here — that would itself burn the
// energy we are trying to measure. Callers increment counters in loops/handlers
// that already run; the summary timer drains them on a coarse cadence.

import { info as logInfo, warn as logWarn } from '../log';

let context = 'app';
const enabled = computeEnabled();

function computeEnabled(): boolean {
  try {
    const v = localStorage.getItem('moss.diag');
    if (v === '0') return false;
    if (v === '1') return true;
  } catch {
    /* localStorage unavailable (e.g. sandboxed context) — fall through */
  }
  try {
    // Vite injects import.meta.env. Never run diagnostics under unit tests (they
    // crash on missing DOM observers and leak timers); default ON in dev, OFF in
    // prod.
    const env = (import.meta as unknown as { env?: { DEV?: boolean; MODE?: string; TEST?: boolean } }).env;
    if (env?.MODE === 'test' || env?.TEST) return false;
    return Boolean(env?.DEV);
  } catch {
    return false;
  }
}

export function diagEnabled(): boolean {
  return enabled;
}

// ── Counters (drained by the summary timer) ──────────────────────────────────
const counters = new Map<string, number>();

export function count(key: string, n = 1): void {
  if (!enabled) return;
  counters.set(key, (counters.get(key) ?? 0) + n);
}

// ── Direct event logs → moss.log ─────────────────────────────────────────────
export function diag(sub: string, msg: string): void {
  if (!enabled) return;
  void logInfo(`[diag:${sub}] ${msg}`);
}

export function diagWarn(sub: string, msg: string): void {
  if (!enabled) return;
  void logWarn(`[diag:${sub}] ${msg}`);
}

// Rate-limited event log — collapses floods (e.g. a shift on every keystroke) to
// at most one line per `everyMs` per `key`, tallying how many were suppressed.
const rateState = new Map<string, { last: number; skipped: number }>();

export function diagRateLimited(sub: string, key: string, msg: string, everyMs = 1000): void {
  if (!enabled) return;
  const skipped = rateGate(key, everyMs);
  if (skipped === null) return;
  const extra = skipped > 0 ? ` (+${skipped} similar suppressed)` : '';
  void logInfo(`[diag:${sub}] ${msg}${extra}`);
}

/**
 * The limiter itself, without the sink: returns how many lines were suppressed
 * since the last one that spoke, or `null` when this call must stay quiet.
 *
 * Split out of `diagRateLimited` for the focus probe, which floods the same way
 * but cannot use that helper: the probe is ALWAYS ON and logs through its own
 * injected sink, so going through `diagRateLimited` would put it back behind the
 * `moss.diag` kill switch — silent in exactly the release build the field report
 * came from.
 */
export function rateGate(key: string, everyMs: number): number | null {
  const now = performance.now();
  const e = rateState.get(key) ?? { last: -Infinity, skipped: 0 };
  if (now - e.last < everyMs) {
    e.skipped += 1;
    rateState.set(key, e);
    return null;
  }
  rateState.set(key, { last: now, skipped: 0 });
  return e.skipped;
}

// ── The one summary timer ────────────────────────────────────────────────────
let timer: ReturnType<typeof setInterval> | null = null;
let windowStart = 0;
let idleWindows = 0;

/**
 * Start the periodic activity summary for this webview. Idempotent. The only
 * timer this module owns; a coarse interval (default 5s) keeps its own energy
 * cost negligible while still localizing which subsystem is hot.
 */
export function startDiagSummary(ctx: string, intervalMs = 5000): void {
  context = ctx;
  if (!enabled || timer) return;
  windowStart = performance.now();
  void logInfo(
    `[diag:summary] ${context} diagnostics ON — window=${intervalMs}ms ` +
      `(set localStorage['moss.diag']='0' to silence)`,
  );
  timer = setInterval(flush, intervalMs);
}

export function stopDiagSummary(): void {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
}

function flush(): void {
  const dur = Math.round(performance.now() - windowStart);
  windowStart = performance.now();
  const visible = document.visibilityState;
  if (counters.size === 0) {
    // Sparse idle heartbeat: affirm "alive but quiet" so a silent log reads
    // as idle, not frozen — stays at INFO (unlike the DEBUG-only lines) because that disambiguation is the
    // whole point and must survive in a release build's default log level.
    // Every ~5 min, not ~30s: a long idle window needs one "still here" line,
// not sixty.
    idleWindows += 1;
    if (idleWindows % 60 === 1) {
      void logInfo(`[diag:summary] ${context} idle — no counted activity (visible=${visible})`);
    }
    return;
  }
  idleWindows = 0;
  const parts = [...counters.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([k, v]) => `${k}=${v}`)
    .join(' ');
  counters.clear();
  void logInfo(`[diag:summary] ${context} window=${dur}ms visible=${visible} ${parts}`);
}
