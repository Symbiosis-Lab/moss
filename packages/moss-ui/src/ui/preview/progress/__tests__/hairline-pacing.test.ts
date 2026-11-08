// deployFraction — the hairline's one monotonic 0→1 across a whole publish.
//
// These lock the two properties the old wiring broke: the bar covered ONLY the
// upload stage (so it restarted at 0 mid-publish), and it was opened via a path
// that armed the NProgress trickle (so it raced ahead of the true byte fraction
// and, being monotonic, could never come back — "fast at first, then very
// slow"). Pure function, no DOM.

import { describe, test, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import * as path from 'node:path';

import { deployFraction, type DeployTick, type DeployStageName } from '../hairline-pacing';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function tick(stage: DeployStageName, over: Partial<DeployTick> = {}): DeployTick {
  return {
    stage,
    current: 0,
    total: 0,
    message: '',
    bytes_uploaded: null,
    bytes_total: null,
    ...over,
  } as DeployTick;
}

describe('deployFraction', () => {
  test('never decreases across the real stage sequence', () => {
    const sequence: DeployTick[] = [
      tick('preparing'),
      tick('rebuilding'),
      tick('syncing'),
      tick('uploading', { bytes_uploaded: 0, bytes_total: 1000 }),
      tick('uploading', { bytes_uploaded: 500, bytes_total: 1000 }),
      tick('uploading', { bytes_uploaded: 1000, bytes_total: 1000 }),
      tick('committing'),
      tick('finalizing'),
    ];
    const values = sequence.map((p) => deployFraction(p)!);
    for (let i = 1; i < values.length; i++) {
      expect(values[i]).toBeGreaterThanOrEqual(values[i - 1]);
    }
    // And it is a real traverse, not a flatline pinned at either end.
    expect(values[0]).toBeLessThan(0.1);
    expect(values.at(-1)!).toBeGreaterThan(0.9);
    expect(values.at(-1)!).toBeLessThan(1);
  });

  test('the upload stage is the largest span — it is the only honest signal', () => {
    const uploadSpan =
      deployFraction(tick('uploading', { bytes_uploaded: 1, bytes_total: 1 }))! -
      deployFraction(tick('uploading', { bytes_uploaded: 0, bytes_total: 1 }))!;
    const buildSpan =
      deployFraction(tick('syncing'))! - deployFraction(tick('rebuilding'))!;
    expect(uploadSpan).toBeGreaterThan(buildSpan);
  });

  test('the tail (committing→finalizing) is short — a pause near the end reads as slower', () => {
    // Harrison et al., UIST 2007: functions that pause near the CONCLUSION were
    // all perceived as slower than linear. These stages are cheap and always
    // last, so they must not sit on a nearly-full bar for long.
    const tail = deployFraction(tick('finalizing'))! - deployFraction(tick('committing'))!;
    expect(tail).toBeLessThan(0.1);
  });

  test('bytes outrank file counts when a tick carries both', () => {
    // 9 of 10 FILES sent but only 10% of the BYTES: a video-heavy publish
    // crawling through its big tail. The bar must sit at the byte fraction —
    // byte-truth is this module's stated reason to exist. (Verified gap: with
    // the branches swapped, every prior test still passed.)
    const v = deployFraction(
      tick('uploading', { current: 9, total: 10, bytes_uploaded: 100, bytes_total: 1000 }),
    )!;
    const start = deployFraction(tick('uploading', { bytes_uploaded: 0, bytes_total: 1 }))!;
    const end = deployFraction(tick('uploading', { bytes_uploaded: 1, bytes_total: 1 }))!;
    expect(v).toBeCloseTo(start + (end - start) * 0.1, 10);
  });

  test('a barely-started upload reads as barely started, whatever the head cost', () => {
    // The report this table was re-cut for: 4 MB of a 540 MB first publish, and
    // the hairline sat near half. The head before `uploading` must be small
    // enough that the byte fraction — the only measured quantity in a deploy —
    // dominates the bar's position from the first tick.
    const v = deployFraction(
      tick('uploading', { bytes_uploaded: 4 * 1024 * 1024, bytes_total: 540 * 1024 * 1024 }),
    )!;
    expect(v).toBeLessThan(0.12);
  });

  test('the head before the upload is thin — it reports nothing, so it may not spend the bar', () => {
    // Every non-upload stage parks at its span start (Rust emits current:0,
    // total:0 for them), so any width they own is a jump, not pacing.
    expect(deployFraction(tick('syncing'))!).toBeLessThan(0.1);
  });

  test('falls back to file counts when the prebuilt path emits no byte fields', () => {
    const half = deployFraction(tick('uploading', { current: 5, total: 10 }))!;
    const none = deployFraction(tick('uploading', { current: 0, total: 10 }))!;
    const all = deployFraction(tick('uploading', { current: 10, total: 10 }))!;
    expect(half).toBeGreaterThan(none);
    expect(all).toBeGreaterThan(half);
  });

  test('a tick with no usable denominator parks at the span start, it does not invent motion', () => {
    expect(deployFraction(tick('uploading'))).toBe(
      deployFraction(tick('uploading', { bytes_uploaded: 0, bytes_total: 1 })),
    );
  });

  test('terminal stages return null — complete()/cancel() own the ending', () => {
    expect(deployFraction(tick('complete'))).toBeNull();
    expect(deployFraction(tick('failed'))).toBeNull();
  });

  test('a bogus denominator cannot produce NaN or overshoot', () => {
    expect(deployFraction(tick('uploading', { bytes_uploaded: 9e9, bytes_total: 10 }))).toBeLessThanOrEqual(1);
    expect(deployFraction(tick('uploading', { bytes_uploaded: -5, bytes_total: 10 }))).toBeGreaterThanOrEqual(0);
  });
});

describe('the pill/panel weld (source-CSS guards)', () => {
  const css = () => readFileSync(path.resolve(__dirname, '../../../style.css'), 'utf8');

  // (The anchored panel's own `background: var(--moss-chrome-pill-bg)` — the
  // other half of the dark-mode weld fix — is pinned in progress-panel.test.ts,
  // alongside the rest of that rule's glass assertions.)

  test('the opaque dark #141414 fill cannot reach the anchored panel', () => {
    // Same specificity (0,2,0) as the anchored rule and LATER in the file, so
    // without the :not() it silently won and painted the panel near-black under
    // a translucent pill.
    for (const m of css().matchAll(/([^\n{}]*\.moss-progress-panel[^\n{}]*)\{\s*background:\s*#141414/g)) {
      expect(m[1]).toContain(':not([data-anchor="nav-pill"])');
    }
  });
});
