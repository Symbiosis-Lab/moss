// UploadRateEstimator — trailing wall-clock window over `bytes_uploaded`.
//
// The estimator has to be correct in TWO regimes it cannot distinguish:
//   • bursty large files — hundreds of consecutive db=0 ticks, then one
//     N × 20 MB riser (concurrent chunk uploads complete in lockstep);
//   • fine-grained small files — 20 concurrent PUTs crediting several times
//     per 250 ms tick.
// These tests lock both, plus the stickiness that keeps the readout on screen
// across a ~64 s credit gap.

import { describe, it, expect } from 'vitest';
import {
  UploadRateEstimator,
  MIN_SPAN_MS,
  RATE_FLOOR_BPS,
  WINDOW_MS,
} from '../upload-rate';

const MB = 1e6;

/** Ring length, for the retention bound. Private by design; read for the test. */
function ringLength(est: UploadRateEstimator): number {
  return (est as unknown as { ring: unknown[] }).ring.length;
}

describe('UploadRateEstimator', () => {
  it('a single sample has no time base — null', () => {
    const est = new UploadRateEstimator();
    expect(est.sample(0, 0).bytesPerSec).toBeNull();
  });

  it('a span shorter than MIN_SPAN_MS is null even with a huge delta', () => {
    // 50 MB in 4 s would compute 12.5 MB/s; without the floor on the divisor
    // a single 20 MB credit landing just after the anchor invents a link speed.
    const est = new UploadRateEstimator();
    est.sample(0, 0);
    expect(est.sample(50 * MB, MIN_SPAN_MS - 1000).bytesPerSec).toBeNull();
  });

  it('bursty large-file regime: no spike, tracks the true average', () => {
    // 40 ticks of db=0 at 250 ms, then one +20 MB credit — the synchronized
    // chunk burst in the fixture. An EWMA over this sample stream spikes to
    // 20 MB / 0.25 s = 80 MB/s on every riser; a trailing window does not.
    const est = new UploadRateEstimator();
    const PERIOD_TICKS = 41; // 40 zeros + 1 credit
    const trueRate = (20 * MB) / (PERIOD_TICKS * 250 / 1000);
    let bytes = 0;
    const readings: number[] = [];
    for (let i = 0; i * 250 <= 120_000; i++) {
      if (i > 0 && i % PERIOD_TICKS === 0) bytes += 20 * MB;
      const r = est.sample(bytes, i * 250);
      // Only judge readings taken once the window is full; before that the
      // divisor is still growing.
      if (i * 250 >= WINDOW_MS + 10_000 && r.bytesPerSec != null) {
        readings.push(r.bytesPerSec);
      }
    }
    expect(readings.length).toBeGreaterThan(100);
    for (const r of readings) {
      // The hard guarantee: never anywhere near the 80 MB/s EWMA spike.
      expect(r).toBeLessThan(4 * MB);
      // The soft band. It is ±35%, not ±25%, and that is arithmetic rather
      // than sloppiness: with a 10.25 s burst period a 30 s window holds
      // either 2 or 3 credits, so the reading quantizes between
      // 2×20 MB/30.5 s and 3×20 MB/30.5 s no matter how the code is written.
      expect(r).toBeGreaterThan(trueRate * 0.65);
      expect(r).toBeLessThan(trueRate * 1.35);
    }
    // Averaged over the run the quantization cancels out.
    const mean = readings.reduce((a, b) => a + b, 0) / readings.length;
    expect(mean).toBeGreaterThan(trueRate * 0.9);
    expect(mean).toBeLessThan(trueRate * 1.1);
  });

  it('fine-grained small-file regime: converges within ±10%, same constants', () => {
    // 1 MB credited every 250 ms = 4 MB/s. The rate must be independent of chunk granularity:
    // identical code, identical constants, opposite credit granularity.
    const est = new UploadRateEstimator();
    let bytes = 0;
    let last: number | null = null;
    for (let i = 0; i * 250 <= 60_000; i++) {
      bytes += MB;
      last = est.sample(bytes, i * 250).bytesPerSec;
    }
    expect(last).not.toBeNull();
    expect(last!).toBeGreaterThan(4 * MB * 0.9);
    expect(last!).toBeLessThan(4 * MB * 1.1);
  });

  it('zero credit across the whole window, before any publish, is null — never 0', () => {
    // The opening stretch of a video-heavy deploy: the first chunk may not
    // complete for ~64 s. "0 kB/s" there would be a lie about a healthy upload.
    const est = new UploadRateEstimator();
    for (let t = 0; t <= 90_000; t += 250) {
      expect(est.sample(0, t).bytesPerSec).toBeNull();
    }
  });

  it('a rate below RATE_FLOOR_BPS, before any publish, is null', () => {
    const est = new UploadRateEstimator();
    est.sample(0, 0);
    // 500 B over 10 s = 50 B/s.
    const r = est.sample(RATE_FLOOR_BPS / 2, 10_000);
    expect(r.bytesPerSec).toBeNull();
  });

  it('retains ~1 sample/s: 5 minutes of 4 Hz ticks leaves a bounded ring', () => {
    const est = new UploadRateEstimator();
    let bytes = 0;
    for (let t = 0; t <= 300_000; t += 250) {
      bytes += 1000;
      est.sample(bytes, t);
    }
    // 30 s window ÷ 1 s downsample, plus the retained anchor.
    expect(ringLength(est)).toBeLessThanOrEqual(32);
    expect(ringLength(est)).toBeGreaterThanOrEqual(2);
  });

  it('non-monotonic input clamps the delta to 0, never negative', () => {
    const est = new UploadRateEstimator();
    est.sample(100 * MB, 0);
    const r = est.sample(50 * MB, 10_000);
    expect(r.bytesPerSec).toBeNull(); // not a negative rate
  });

  it('STICKY: a 90 s credit gap never blanks the rate', () => {
    // The synchronized-burst regression. With N large files in flight the gap
    // between credits reaches ~64 s — longer than WINDOW_MS — so a
    // window-derived guarantee would blink the segment out roughly every 64 s
    // on exactly the deploys this feature exists for.
    const est = new UploadRateEstimator();
    est.sample(0, 0);
    est.sample(20 * MB, 1000); // one credit, then silence

    let firstNonNull: number | null = null;
    for (let t = 1250; t <= 35_000; t += 250) {
      const r = est.sample(20 * MB, t).bytesPerSec;
      if (r != null && firstNonNull == null) firstNonNull = r;
    }
    expect(firstNonNull).not.toBeNull();

    // From here the window holds no credit at all: the value is held, exactly.
    const held = est.sample(20 * MB, 35_250).bytesPerSec;
    expect(held).not.toBeNull();
    for (let t = 35_500; t <= 125_000; t += 250) {
      expect(est.sample(20 * MB, t).bytesPerSec).toBe(held);
    }
  });

  it('new credit after a sticky hold REPLACES the held value', () => {
    const est = new UploadRateEstimator();
    est.sample(0, 0);
    est.sample(20 * MB, 1000);
    for (let t = 1250; t <= 60_000; t += 250) est.sample(20 * MB, t);
    const held = est.sample(20 * MB, 60_250).bytesPerSec!;

    // A 60 MB burst lands. delta/span over a ~30 s window ≈ 2 MB/s — three
    // times the held value, so an averaging-in implementation is visible.
    const after = est.sample(80 * MB, 61_000).bytesPerSec!;
    expect(after).not.toBe(held);
    expect(after).toBeGreaterThan(held * 2);
  });

  it('reset() releases stickiness AND empties the ring', () => {
    const est = new UploadRateEstimator();
    est.sample(0, 0);
    est.sample(20 * MB, 1000);
    for (let t = 1250; t <= 40_000; t += 250) est.sample(20 * MB, t);
    expect(est.sample(20 * MB, 40_250).bytesPerSec).not.toBeNull();

    est.reset();
    // Empty ring: a single sample has no time base.
    expect(est.sample(20 * MB, 40_500).bytesPerSec).toBeNull();
    // And the next deploy's zero-delta window does NOT inherit the old rate.
    for (let t = 40_750; t <= 90_000; t += 250) {
      expect(est.sample(20 * MB, t).bytesPerSec).toBeNull();
    }
  });
});
