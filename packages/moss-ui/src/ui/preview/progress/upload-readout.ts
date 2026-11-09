/**
 * Everything the advisory panel knows about a publish in flight.
 *
 * `deploy-progress` ticks at 4 Hz and each tick moves five things at once —
 * file counts, byte counts, the trailing-window rate, the a11y milestone set,
 * and which file is in flight. Those five were five separate fields on
 * `ProgressPanel`, mutated from three methods and read from three more; this
 * gathers them behind the two questions the panel actually asks: *what is the
 * bar's label*, and *what does the status block say*.
 *
 * The panel keeps ownership of the stage machine and of the receipt's dwell
 * timer — those are lifecycle, not readout.
 */

import { t } from "../../i18n/index";
import type { PublishLabelInput, PublishStage, StatusInput } from './progress-panel-stage';
import { UploadRateEstimator, type RateReading } from './upload-rate';
import { formatPublishReceipt, formatRate, formatSizePair, hasSizeReadout } from './upload-format';
import { CurrentFileHold, splitFilePath } from './upload-file';

export class PublishReadout {
  private uploadCurrent: number | null = null;
  private uploadTotal: number | null = null;
  private uploadBytes: number | null = null;
  private uploadBytesTotal: number | null = null;
  private rebuildMessage: string | undefined;
  /**
   * When this publish engaged (`performance.now()`), for the receipt's elapsed
   * span. It measures the whole publish the author waited through, not just
   * the upload.
   */
  private startedAt: number | null = null;

  /**
   * Trailing-window upload rate. This instance is the STATUS BLOCK's; the
   * hairline owner keeps its own — the two listeners must not share mutable
   * state. The estimator is cheap and the samples are identical.
   */
  private readonly rate = new UploadRateEstimator();
  private rateReading: RateReading | null = null;

  /**
   * Upload-progress milestones (25/50/75%) already announced this pass.
   * Milestones, not a stream: 3 announcements per deploy instead of ~240.
   * 0% and 100% are omitted — the stage transitions into and out of
   * `uploading` already announce them.
   */
  private readonly milestones = new Set<number>();
  private announce: string | null = null;

  /**
   * The 700 ms minimum hold on the in-flight filename.
   * The backend picks the largest of the twenty concurrent uploads and emits
   * it at 4 Hz; without the hold that text can change four times a second at
   * the tail of a deploy, which is unreadable and a flashing-content problem
   * besides.
   */
  private readonly file = new CurrentFileHold();

  /** Stamp the start of a publish, on the idle→engaged edge. */
  begin(now: number): void {
    this.startedAt = now;
  }

  /** Forget the whole publish — a new session, or a terminal stage. */
  reset(): void {
    this.uploadCurrent = null;
    this.uploadTotal = null;
    this.uploadBytes = null;
    this.uploadBytesTotal = null;
    this.rebuildMessage = undefined;
    this.startedAt = null;
    this.resetUpload();
  }

  /**
   * Clear the estimator window, the sticky rate, the milestone set and the
   * held filename. Called on the syncing→uploading edge so a fresh pass owns
   * its own window, and on a failure so a retry starts clean.
   */
  resetUpload(): void {
    this.rate.reset();
    this.rateReading = null;
    this.milestones.clear();
    this.announce = null;
    // A new pass must not inherit the previous one's last filename — it would
    // name a file that finished uploading minutes ago.
    this.file.reset();
  }

  noteRebuilding(message: string | undefined): void {
    this.rebuildMessage = message;
  }

  /**
   * One `uploading` tick.
   *
   * Single guard: absent or zero `bytesTotal` means the
   * bytes path is unavailable (prebuilt CLI path, commit-only deploy) — the
   * estimator is never fed and the status line keeps the file-count form.
   */
  noteUploading(
    p: {
      current: number;
      total: number;
      current_file?: string | null;
      bytes_uploaded?: number | null;
      bytes_total?: number | null;
    },
    now: number,
  ): void {
    this.uploadCurrent = p.current;
    this.uploadTotal = p.total;
    // Through the hold, never straight to the field: the stream ticks at 4 Hz
    // and the pick can change on every tick near the tail.
    this.file.offer(p.current_file, now);

    const bytesTotal = p.bytes_total;
    if (bytesTotal == null || bytesTotal <= 0) return;
    const done = p.bytes_uploaded ?? 0;
    this.uploadBytes = done;
    this.uploadBytesTotal = bytesTotal;
    this.rateReading = this.rate.sample(done, now);

    const pct = (done / bytesTotal) * 100;
    for (const milestone of [25, 50, 75]) {
      if (pct >= milestone && !this.milestones.has(milestone)) {
        this.milestones.add(milestone);
        this.announce = t('upload.aria', { pct: milestone });
      }
    }
  }

  /** Latest upload counts; null when no `uploading` tick has landed. */
  counts(): { current: number; total: number } | null {
    return this.uploadCurrent == null || this.uploadTotal == null
      ? null
      : { current: this.uploadCurrent, total: this.uploadTotal };
  }

  /** The primary bar's input for `stage`. */
  labelInput(stage: PublishStage): PublishLabelInput {
    switch (stage) {
      case 'idle':
      case 'waiting':
        // idle is unreachable — activePrimaryLabel only asks when engaged —
        // but it falls through to the waiting label for exhaustiveness.
        return { stage: 'waiting' };
      case 'preparing':
        return { stage: 'preparing' };
      case 'rebuilding':
        return { stage: 'rebuilding', message: this.rebuildMessage };
      case 'syncing':
        return { stage: 'syncing' };
      case 'uploading':
        return {
          stage: 'uploading',
          current: this.uploadCurrent ?? 0,
          total: this.uploadTotal ?? 0,
          bytesDone: this.uploadBytes ?? undefined,
          bytesTotal: this.uploadBytesTotal ?? undefined,
        };
      case 'committing':
        return { stage: 'committing' };
      case 'finalizing':
        return { stage: 'finalizing' };
      case 'complete':
        return { stage: 'complete' };
      case 'failed':
        return { stage: 'failed' };
    }
  }

  /**
   * The uploading ledger — headline size pair, detail
   * row of file + rate. Returns null when the bytes path is unavailable, and
   * then the caller keeps the file-count form.
   *
   * `hasSizeReadout`, not `> 0`: a sub-10 kB deploy would render `0/0 kB`,
   * which is less informative than the file count the verb falls back to.
   */
  ledger(verb: string, condition: string): StatusInput | null {
    const bytesTotal = this.uploadBytesTotal;
    if (!hasSizeReadout(bytesTotal)) return null;
    const file = this.file.current();
    return {
      kind: 'running',
      verb,
      // The headline keeps the SIZE PAIR alone. The rate sits with the
      // filename one register down: it is evidence for the headline, not a
      // peer of it, and the two together are what made the row incompressible.
      countText: formatSizePair(this.uploadBytes ?? 0, bytesTotal),
      detail: {
        file: file != null ? splitFilePath(file) : undefined,
        rate: formatRate(this.rateReading?.bytesPerSec ?? null) ?? undefined,
      },
      condition,
      announce: this.announce ?? undefined,
    };
  }

  /** `Published · 58.0 MB · 12.4s`, from what only the publish knows. */
  receipt(now: number): string {
    return formatPublishReceipt(this.uploadBytesTotal, now - (this.startedAt ?? now));
  }
}
