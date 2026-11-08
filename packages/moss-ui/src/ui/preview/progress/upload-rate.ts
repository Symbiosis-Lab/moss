/**
 * UploadRateEstimator — a trailing WALL-CLOCK window over `bytes_uploaded`.
 *
 * Why a window and not an EWMA: `bytes_uploaded` is
 * NOT a 4 Hz stream. The Rust ticker merely READS an AtomicU64 that a point
 * process credits — one `fetch_add(file_size)` after each small-file PUT, one
 * `fetch_add(CHUNK_SIZE)` after each 20 MB PATCH. On a video-heavy deploy the
 * atomic is a staircase with treads up to ~64 s (N concurrent large files start
 * together and stay in lockstep) and risers of N × 20 MB. An EWMA over that
 * sample stream decays to ~0 between risers and spikes to 80 MB/s on arrival.
 *
 * A trailing window reads only TOTAL bytes over TOTAL time, so it is
 * insensitive to how the credit is granulated: the same constants are correct
 * for a 120 MB riser every 64 s and for 40 small PUTs per second.
 *
 * Granularity-independence is not enough on its own — a window can still go
 * EMPTY when the credit gap exceeds it. That second axis (synchronization) is
 * handled by STICKINESS, not by window size: once a rate has been published it
 * is held until new credit arrives or `reset()` is called at the end of the
 * uploading stage. Sizing the window past the ~64 s worst-case gap would make
 * the average stale on every deploy to fix a case stickiness fixes for free.
 *
 * Pure: no DOM, no i18n, no timers. `sample()` takes `nowMs` as a parameter so
 * the suite drives it with plain numbers; production passes `performance.now()`
 * (monotonic — immune to a system clock adjustment mid-deploy).
 */

export interface RateReading {
  /**
   * Bytes/sec over the trailing window, or null when not yet knowable.
   * STICKY: once non-null, holds its last value until new credit arrives or
   * `reset()` is called. Never returns to null mid-upload.
   */
  bytesPerSec: number | null;
}

/**
 * Averaging length. Deliberately NOT sized to exceed the ~64 s worst-case
 * credit gap — see the stickiness note above. Long enough to average several
 * credits in the fine-grained regime, short enough that a genuine throughput
 * change shows within half a minute.
 */
export const WINDOW_MS = 30_000;

/**
 * Floor on the divisor. A single 20 MB credit landing 0.5 s after the anchor
 * would compute 40 MB/s on a 2 MB/s link. Consequence: a deploy that finishes
 * in under 5 s never shows a rate at all — intended.
 */
export const MIN_SPAN_MS = 5_000;

/** Retention downsample: the ticker offers 4 samples/s; we retain ~1/s. */
export const SAMPLE_MS = 1_000;

/** Below 1 kB/s, publish null rather than "0 kB/s" / "1 kB/s". */
export const RATE_FLOOR_BPS = 1_000;

interface Sample {
  t: number;
  bytes: number;
}

export class UploadRateEstimator {
  private ring: Sample[] = [];
  /**
   * Timestamp of the last APPENDED sample. The append decision is made against
   * this, not against `ring[last].t` — the tail entry is overwritten in place
   * on every tick (so the newest reading is always current), and comparing
   * against a tail that keeps moving forward would mean we never append again.
   */
  private lastAppendMs = Number.NEGATIVE_INFINITY;
  private lastRate: number | null = null;

  sample(bytesUploaded: number, nowMs: number): RateReading {
    // 1. Append at most one entry per SAMPLE_MS; otherwise refresh the tail.
    const tail = this.ring[this.ring.length - 1];
    if (!tail || nowMs - this.lastAppendMs >= SAMPLE_MS) {
      this.ring.push({ t: nowMs, bytes: bytesUploaded });
      this.lastAppendMs = nowMs;
    } else {
      tail.t = nowMs;
      tail.bytes = bytesUploaded;
    }

    // 2. Evict past the window, but always keep ONE entry at or before the
    //    window start as the anchor, and never drop below two entries.
    const cutoff = nowMs - WINDOW_MS;
    while (this.ring.length > 2 && this.ring[1].t <= cutoff) {
      this.ring.shift();
    }

    const anchor = this.ring[0];
    const latest = this.ring[this.ring.length - 1];
    const span = latest.t - anchor.t;
    // Defensive max(0): the source is fetch_add-only, therefore monotonic.
    const delta = Math.max(0, latest.bytes - anchor.bytes);

    // 3. No time base yet → nothing knowable. Not a stickiness hole: the
    //    anchor is retained at/before the window start, so once the span has
    //    reached MIN_SPAN_MS it never falls back below it within one pass —
    //    only reset() (which clears lastRate too) returns here.
    if (span < MIN_SPAN_MS) return { bytesPerSec: null };

    // 4. No credit inside the window. This is the HEALTHY large-file case (no
    //    chunk has completed yet), not an error — and it is what keeps
    //    "0 kB/s" off the screen. Fall through to the sticky value.
    if (delta === 0) return { bytesPerSec: this.lastRate };

    const rate = delta / (span / 1000);
    if (!Number.isFinite(rate) || rate < RATE_FLOOR_BPS) {
      return { bytesPerSec: this.lastRate };
    }

    this.lastRate = rate;
    return { bytesPerSec: rate };
  }

  /** Empty the ring AND clear the sticky value (end of an uploading stage). */
  reset(): void {
    this.ring = [];
    this.lastAppendMs = Number.NEGATIVE_INFINITY;
    this.lastRate = null;
  }
}
