// Upload readout formatters — the size pair, the rate, and their composition.
//
// These tests prevent two misleading upload readouts:
//   • `58.4/58 MB` at the final authoritative tick (an asymmetric decimal
//     ladder overshooting its own total at the moment the user watches it
//     finish) — guarded by the shared decimal count + clamp-to-RENDERED-total;
//   • `0 kB/s` during the healthy opening stretch of a video-heavy deploy —
//     guarded by returning null for the whole rate, which the caller omits.

import { describe, it, expect, afterEach } from 'vitest';
import { formatSizePair, formatRate, formatPublishReceipt } from '../upload-format';
import { setLocale } from "../../../i18n/index";

/** U+2007 FIGURE SPACE — invisible in the running UI, load-bearing here. */
const FIGURE = '\u2007';

afterEach(() => setLocale('en'));

describe('formatSizePair', () => {
  it('renders the worked examples (en)', () => {
    expect(formatSizePair(412_000, 880_000)).toBe('412/880 kB');
    expect(formatSizePair(12.4e6, 58e6)).toBe('12.4/58.0 MB');
    expect(formatSizePair(3.2e6, 9.7e6)).toBe('3.2/9.7 MB');
    expect(formatSizePair(420e6, 1.85e9)).toBe('0.42/1.85 GB');
    expect(formatSizePair(0, 1.85e9)).toBe('0.00/1.85 GB');
  });

  it('final tick reads 58.0/58.0 MB, never 58.4/58 MB', () => {
    expect(formatSizePair(58e6, 58e6)).toBe('58.0/58.0 MB');
    // Over-credit is real: deploy.rs sums regular files only, so a
    // symlink-heavy deploy can credit more bytes than it counted.
    expect(formatSizePair(58.4e6, 58e6)).toBe('58.0/58.0 MB');
  });

  it('pads the done integer part with U+2007 to the total width', () => {
    // Exact string, not a length comparison: a plain-space pad would satisfy
    // a `.length` assertion and still shift the column in a real font.
    expect(formatSizePair(4.1e6, 58e6)).toBe(`${FIGURE}4.1/58.0 MB`);
    // Padded and unpadded forms are the same width, so the tail never moves.
    expect(formatSizePair(4.1e6, 58e6).length).toBe(
      formatSizePair(12.4e6, 58e6).length,
    );
  });

  it('picks the unit ladder from bytes_total at each boundary', () => {
    expect(formatSizePair(0, 999_999)).toContain('kB');
    expect(formatSizePair(0, 1_000_000)).toContain('MB');
    expect(formatSizePair(0, 999_999_999)).toContain('MB');
    expect(formatSizePair(0, 1_000_000_000)).toContain('GB');
    // Both halves share the unit, and the pad counts the grouping mark as the
    // column it occupies.
    expect(formatSizePair(500_000, 999_999)).toBe(
      `${FIGURE}${FIGURE}500/1,000 kB`,
    );
  });

  it('keeps the byte unit Latin in Chinese technical UI', () => {
    setLocale('zh-hans');
    expect(formatSizePair(12.4e6, 58e6)).toBe('12.4/58.0 MB');
    setLocale('zh-hant');
    expect(formatSizePair(12.4e6, 58e6)).toBe('12.4/58.0 MB');
  });
});

describe('formatRate', () => {
  it('picks its own ladder, independent of the size-pair unit (en)', () => {
    expect(formatRate(640_000)).toBe('640 kB/s');
    expect(formatRate(2.1e6)).toBe('2.1 MB/s');
    expect(formatRate(810_000)).toBe('810 kB/s');
    expect(formatRate(12.4e6)).toBe('12.4 MB/s');
  });

  it('localizes only the per-second suffix', () => {
    setLocale('zh-hans');
    expect(formatRate(2.1e6)).toBe('2.1 MB/秒');
    setLocale('zh-hant');
    expect(formatRate(2.1e6)).toBe('2.1 MB/秒');
  });

  it('unknown rate → null (the caller omits the segment)', () => {
    expect(formatRate(null)).toBeNull();
    expect(formatRate(0)).toBeNull();
    expect(formatRate(Number.POSITIVE_INFINITY)).toBeNull();
  });
});

describe('formatPublishReceipt', () => {
  it('names the bytes and the span a publish actually took', () => {
    // The publish tier must not fall silent at `complete`, or the
    // BUILD receipt answers instead, so the panel ends a publish by
    // reporting the rebuild that preceded it.
    expect(formatPublishReceipt(58e6, 12_400)).toBe('Published · 58.0 MB · 12.4s');
  });

  it('says only the span when the publish shipped no bytes', () => {
    // A commit-only publish. `0 MB` would read as "nothing went up", which is
    // the opposite of what happened.
    expect(formatPublishReceipt(null, 3_200)).toBe('Published · 3.2s');
    expect(formatPublishReceipt(400, 3_200)).toBe('Published · 3.2s');
  });
});
