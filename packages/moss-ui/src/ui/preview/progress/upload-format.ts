/**
 * Pure formatters for the deploy upload readout.
 *
 *   <size-pair> · <rate>        e.g.  12.4/58.0 MB · 2.1 MB/s
 *
 * Two rules carry the weight:
 *
 *  • ONE unit and ONE decimal count for both halves of the size pair, chosen
 *    from `bytes_total`. The shared unit stops the left number flipping
 *    kB→MB mid-upload; the shared decimal count is what prevents `58.4/58 MB`
 *    at the final authoritative tick (deploy.rs emits bytes_uploaded ==
 *    bytes_total after the loop). Clamp order is load-bearing: format the
 *    total, ROUND it, then clamp done to the RENDERED total — a clamp on raw
 *    bytes does not prevent a rounding overshoot.
 *
 *  • The `done` integer part is left-padded with U+2007 FIGURE SPACE to the
 *    integer width of `total`. Because done ≤ total and total is constant for
 *    the whole upload, the size pair is fixed-width from first tick to last,
 *    so the tail (`/58.0 MB · 2.1 MB/s`) does not shift as done crosses
 *    9.9 → 10.0. U+2007's digit-width equality is a font property; in a font
 *    that breaks it the interior shifts by a sub-digit fraction — degraded,
 *    never broken.
 *
 * No DOM. i18n via `t()` + `formatNumber()` — never
 * `Intl.NumberFormat({ style: 'unit' })`, whose unit placement varies across
 * ICU versions and locales and would break the fixed-width guarantee.
 */

import { t, formatNumber } from "../../i18n/index";
import type { RateReading } from './upload-rate';

/**
 * Below this total, the size pair says LESS than the file count it replaced.
 *
 * The kB rung renders zero decimals, so a sub-kilobyte deploy (one edited
 * post, a commit-only push) reads `0/0 kB` for its whole life — strictly worse
 * than `Uploading… (1 / 1)`, which at least names a file. 10 kB is the first
 * total where the pair has a moving column to show. Under it, the readout
 * falls back to the file-count form.
 */
export const MIN_SIZE_READOUT_BYTES = 10_000;

/** Whether `bytes_total` is worth rendering as a size pair at all. */
export function hasSizeReadout(bytesTotal: number | null | undefined): bytesTotal is number {
  return bytesTotal != null && bytesTotal >= MIN_SIZE_READOUT_BYTES;
}

/** U+2007 — the digit-width space that reserves the `done` integer column. */
const FIGURE_SPACE = '\u2007';

interface Scale {
  unit: string;
  divisor: number;
  decimals: number;
}

/** Decimal units (1 kB = 1000 B), matching every OS transfer UI. */
function sizeScale(bytesTotal: number): Scale {
  if (bytesTotal < 1e6) return { unit: 'kB', divisor: 1e3, decimals: 0 };
  if (bytesTotal < 1e9) return { unit: 'MB', divisor: 1e6, decimals: 1 };
  return { unit: 'GB', divisor: 1e9, decimals: 2 };
}

function fixed(n: number, decimals: number): string {
  return formatNumber(n, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

/**
 * Rendered width of the integer part. Derived from the KNOWN decimal count
 * rather than by searching for a decimal separator, so it is correct for any
 * locale's separator and counts grouping marks (which occupy real columns).
 */
function integerWidth(formatted: string, decimals: number): number {
  return formatted.length - (decimals > 0 ? decimals + 1 : 0);
}

/**
 * `12.4/58.0 MB` — both numbers in the unit chosen from `total`, with the
 * `done` integer part padded to the total's integer width.
 */
export function formatSizePair(done: number, total: number): string {
  const { unit, divisor, decimals } = sizeScale(total);
  const factor = 10 ** decimals;
  const totalRounded = Math.round((total / divisor) * factor) / factor;
  // Clamp to the RENDERED total, not the raw one: the overshoot this guards
  // against is a rounding artefact. Also covers a genuine over-credit
  // (deploy.rs sums regular files only, so a symlink-heavy deploy can credit
  // more bytes than it counted).
  const doneClamped = Math.min(Math.max(done, 0) / divisor, totalRounded);

  const totalStr = fixed(totalRounded, decimals);
  const doneStr = fixed(doneClamped, decimals);
  const padWidth = Math.max(
    0,
    integerWidth(totalStr, decimals) - integerWidth(doneStr, decimals),
  );

  return t('upload.size_pair', {
    done: FIGURE_SPACE.repeat(padWidth) + doneStr,
    total: totalStr,
    unit,
  });
}

/**
 * `2.1 MB/s` — ladder chosen from the rate itself, independent of the size
 * pair's unit. Returns null when the rate is unknown, so the caller omits the
 * whole segment (separator included). Never renders "0 kB/s".
 */
export function formatRate(bytesPerSec: number | null): string | null {
  if (bytesPerSec == null || !Number.isFinite(bytesPerSec) || bytesPerSec <= 0) {
    return null;
  }
  const scale: Scale =
    bytesPerSec < 1e6
      ? { unit: 'kB', divisor: 1e3, decimals: 0 }
      : bytesPerSec < 1e9
        ? { unit: 'MB', divisor: 1e6, decimals: 1 }
        : { unit: 'GB', divisor: 1e9, decimals: 2 };
  return t('upload.rate', {
    n: fixed(bytesPerSec / scale.divisor, scale.decimals),
    unit: scale.unit,
  });
}

/** `58.0 MB` — one size on its own, same ladder as the pair. */
export function formatSize(bytes: number): string {
  const { unit, divisor, decimals } = sizeScale(bytes);
  return t('upload.size_one', { n: fixed(bytes / divisor, decimals), unit });
}

/**
 * `Published · 58.0 MB · 12.4s` — what the panel says once a publish lands.
 *
 * Before this existed the publish tier fell silent at `complete` and the next
 * tier that answered was the BUILD receipt, so the panel ended a publish by
 * reporting the rebuild that preceded it.
 *
 * A publish that shipped no bytes (commit-only, or a total under the
 * [`MIN_SIZE_READOUT_BYTES`] floor) says `Published · 12.4s`. Inventing
 * `0 MB` would read as "nothing went up", which is the opposite of what
 * happened.
 */
export function formatPublishReceipt(
  bytesTotal: number | null | undefined,
  elapsedMs: number,
): string {
  const seconds = fixed(Math.max(0, elapsedMs) / 1000, 1);
  return hasSizeReadout(bytesTotal)
    ? t('upload.published_sized', { size: formatSize(bytesTotal), seconds })
    : t('upload.published', { seconds });
}

