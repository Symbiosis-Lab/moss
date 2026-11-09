/**
 * PreviewHairline — a 1.5px determinate progress bar on the BOTTOM EDGE of the
 * nav pill that tracks a rebuild left→right, like a real progress bar.
 *
 * Why: morphing in place updates the preview seamlessly, so a
 * rebuild leaves no visible trace. This bar is the ambient cue: it appears as a
 * dot at the left when the rebuild kicks off, GROWS rightward with the build's
 * real progress (the `ProgressUpdate.percentage` stream), and reaches full
 * length exactly when the morph finishes — then dissolves.
 *
 * Driven by the EXISTING progress pipeline (no new event) — see
 * PreviewManager.handleProgressUpdate / refresh(). Build progress fills 0→90%
 * of the bar; the final 10% is reserved for the morph apply, so the bar visibly
 * completes when the content lands (not a beat early at build-done).
 *
 * Placement: a TRACK div inside
 * `.moss-pill-cluster`, CSS-anchored to the pill's straight bottom run
 * (`top:100%; left/right: var(--moss-pill-radius)`), containing the FILL
 * whose width is a PERCENTAGE of the track. No pill-rect measuring: the old
 * titlebar mount measured the pill and re-measured only on window resize, so
 * a divider-var pill move stranded the bar at its last-measured x (the
 * misaligned-hairline bug). Colour is the mode-adaptive --moss-color-accent.
 */
export class PreviewHairline {
  private el: HTMLElement | null = null;
  private trackEl: HTMLElement | null = null;
  private fraction = 0;
  private active = false;
  private dissolveTimer = 0;
  private pendingTimer = 0;
  /** The dissolve fade-out cleanup timer. TRACKED (not anonymous) so clearTimers()
   *  can cancel it: an untracked timer fired ~500ms into a later start()/upload
   *  and zeroed `active`/`fraction` mid-run (cancel()→retry race). */
  private dissolveAnimTimer = 0;

  /** Visible dot width at 0% so the bar's start reads as a point, not nothing. */
  private static readonly DOT_PX = 3;
  /** Build progress fills up to this; the morph completes the rest. */
  private static readonly BUILD_CAP = 0.9;
  /** Hold at full length this long before dissolving (lets the eye catch it). */
  private static readonly COMPLETE_HOLD_MS = 220;
  /** Time for the width to animate to full — MUST match the CSS `width`
   *  transition (.moss-preview-hairline). The reached-full callback (pill flash)
   *  fires after this, so the flash coincides with the bar finishing. */
  private static readonly GROW_MS = 300;
  /** Opacity fade-out duration (must match the CSS opacity transition). */
  private static readonly DISSOLVE_MS = 500;

  private mount(): void {
    if (this.el) return;
    const cluster = document.querySelector<HTMLElement>(".moss-pill-cluster");
    if (!cluster) return;
    const track = document.createElement("div");
    track.className = "moss-preview-hairline-track";
    track.setAttribute("aria-hidden", "true");
    const el = document.createElement("div");
    el.className = "moss-preview-hairline";
    track.appendChild(el);
    cluster.appendChild(track);
    this.trackEl = track;
    this.el = el;
  }

  private applyWidth(): void {
    if (!this.el) return;
    // Percentage of the CSS-anchored track; max() keeps the 0% start visible
    // as a dot. No pixel measuring — the track IS the straight run.
    this.el.style.width = this.active
      ? `max(${PreviewHairline.DOT_PX}px, ${(this.fraction * 100).toFixed(1)}%)`
      : "0";
  }

  /**
   * Start a DETERMINATE pass — dot at the left. For work that reports real
   * progress fractions (a machine-scoped download's bytes). Drive it with
   * setUploadProgress; finish with complete()/cancel(). start() delegates
   * here — see its comment for why the two names survive one body.
   */
  startDeterminate(): void {
    this.mount();
    if (!this.el) return;
    this.clearTimers();
    this.el.classList.remove("dissolving");
    this.active = true;
    this.fraction = 0;
    this.applyWidth();
    this.el.classList.add("active");
  }

  /**
   * Rebuild started — appear as a dot at the left. Idempotent. Real progress
   * moves it from here (setBuildProgress); complete() fills it to full.
   *
   * start() does not trickle. The bar moves only on real progress ticks, so a bare
   * start() leaves it where it is.
   */
  start(): void {
    this.startDeterminate();
  }

  /**
   * Determinate fill: the fraction IS the on-screen bar position; monotonic
   * within one pass. Shared by build progress AND deploy-upload progress — the
   * nav-pill hairline is the SOLE progress encoding for both operations.
   * Callers whose finishing step has no span of its own (the build's morph)
   * reserve it BEFORE calling — see setBuildProgress. The deploy's spans
   * (progress/hairline-pacing.ts) already stop at 0.96; multiplying them by BUILD_CAP
   * here rescaled the whole documented geometry (finalizing rendered at 82.8%,
   * not 96%) and reserved the finishing slice twice.
   */
  private applyDeterminate(fraction: number): void {
    // Self-starts via startDeterminate() (now identical to start(), see its
    // comment) if no pass is active yet — a caller whose first tick already
    // carries a real fraction (setUploadProgress on an idle bar) needs no
    // separate start() call.
    if (!this.active) this.startDeterminate();
    const f = Math.min(Math.max(fraction, 0), 1);
    // Never go backwards within one pass (progress can report jitter).
    this.fraction = Math.max(this.fraction, f);
    this.applyWidth();
  }

  /** Build progress in [0,1] → [0, BUILD_CAP]: the build stream has no span
   *  table, so the morph's reserved slice is applied here (the final 10%,
   *  completed via complete() when the morph lands). */
  setBuildProgress(fraction: number): void {
    this.applyDeterminate(fraction * PreviewHairline.BUILD_CAP);
  }

  /**
   * Deploy UPLOAD progress (bytes uploaded / bytes total). Reuses the same
   * determinate fill, measured in bytes rather than files. The
   * caller (PreviewManager) calls start() on the syncing→uploading transition to
   * reset, then this per tick, then complete() on deploy complete.
   */
  setUploadProgress(fraction: number): void {
    this.applyDeterminate(fraction);
  }

  /**
   * Morph finished — fill to full length; once the bar REACHES full (after the
   * width transition), run `onReachedFull` (the pill flash, so it coincides
   * with completion rather than the start of the grow), hold briefly, dissolve.
   */
  complete(onReachedFull?: () => void): void {
    // Ensure the bar is visible even if the rebuild emitted no intermediate
    // progress (a blink-fast build): start() shows the dot, then the jump to
    // full animates via the CSS width transition, so it still GROWS rather than
    // appearing pre-full.
    if (!this.active) this.start();
    if (!this.el) return;
    this.clearTimers();
    this.fraction = 1;
    this.applyWidth(); // width transition to full begins (GROW_MS)
    this.dissolveTimer = window.setTimeout(() => {
      // Bar has reached full — flash now, hold, then dissolve.
      onReachedFull?.();
      this.dissolveTimer = window.setTimeout(
        () => this.dissolve(),
        PreviewHairline.COMPLETE_HOLD_MS,
      );
    }, PreviewHairline.GROW_MS);
  }

  /**
   * Schedule a complete()+dissolve after `ms` (fallback for a no-change rebuild
   * that triggers no refresh/morph). Cancelable via cancelPendingDissolve() —
   * the refresh path cancels it and completes precisely when the morph lands.
   */
  dissolveAfter(ms: number): void {
    if (this.pendingTimer) clearTimeout(this.pendingTimer);
    this.pendingTimer = window.setTimeout(() => {
      this.pendingTimer = 0;
      this.complete();
    }, ms);
  }

  /** Cancel a pending dissolveAfter (a refresh/morph will apply → hold). */
  cancelPendingDissolve(): void {
    if (this.pendingTimer) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = 0;
    }
  }

  /**
   * Abort the bar WITHOUT filling to full or flashing — used when a deploy
   * FAILS. Unlike complete() (which finishes the bar + flashes the pill to mark
   * success), this just fades the partial fill out: failure makes no green.
   */
  cancel(): void {
    this.clearTimers();
    this.dissolve();
  }

  private dissolve(): void {
    const el = this.el;
    if (!el) return;
    if (this.dissolveTimer) {
      clearTimeout(this.dissolveTimer);
      this.dissolveTimer = 0;
    }
    el.classList.remove("active");
    el.classList.add("dissolving");
    if (this.dissolveAnimTimer) clearTimeout(this.dissolveAnimTimer);
    this.dissolveAnimTimer = window.setTimeout(() => {
      this.dissolveAnimTimer = 0;
      el.classList.remove("dissolving");
      this.active = false;
      this.fraction = 0;
      this.applyWidth();
    }, PreviewHairline.DISSOLVE_MS);
  }

  private clearTimers(): void {
    if (this.dissolveTimer) {
      clearTimeout(this.dissolveTimer);
      this.dissolveTimer = 0;
    }
    if (this.pendingTimer) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = 0;
    }
    if (this.dissolveAnimTimer) {
      clearTimeout(this.dissolveAnimTimer);
      this.dissolveAnimTimer = 0;
    }
  }

  destroy(): void {
    this.clearTimers();
    this.trackEl?.remove();
    this.trackEl = null;
    this.el = null;
  }
}
