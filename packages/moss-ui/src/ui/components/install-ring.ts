/**
 * InstallRing — the plugin tile's install affordance: a plus that bends into a
 * progress ring around the icon and collapses into a minus.
 *
 * One object, five states, TWO STROKES throughout. Nothing is swapped in or
 * out: the plus's horizontal bar becomes the ring's track and then the minus,
 * and its vertical bar becomes the progress arc and then retracts to nothing.
 *
 * Three things about this file specifically:
 *
 *   - The stroke arithmetic — how a stroke becomes a path, and which of length
 *     and shape leads on the way — is `shared/icon-morph.ts`, shared with
 *     every other glyph moss morphs. What is local here is the ring's own
 *     geometry and its five poses.
 *   - The ring is 64 units over the 48px tile, so it encircles the icon with
 *     ~8px of air, the way an iOS app downloads. It is pointer-transparent;
 *     the tile keeps the hit area.
 *   - `run()` is the ONLY animation path. Every install, every uninstall,
 *     every channel — bundled copy or six-second download — plays the same
 *     choreography, because a user should not have to learn that a fast
 *     plugin looks different from a slow one.
 */

import {
  type Pose, drawStroke, lerpPose, prefersReducedMotion, strokeD, tweenPose,
} from '../shared/icon-morph';

const TAU = Math.PI * 2;

const CX = 32, CY = 32;
const R_RING = 28.5;                 // clears the icon's rounded corners
const K_RING = 1 / R_RING;
const CIRC = TAU * R_RING;
/* The plus/minus bar length. At 11 units over a 64px box the resting mark
   was an 11px bar in a 2.5px stroke, 2.3× the length and 1.7× the weight of
   the ~4.7px/1.5px corner badge, which read as too big. See W_REST below;
   length alone was not the whole of it. */
const BAR = 7;
const ANCHOR_DEG = 135;              // bottom-left — where moss's badge sits
/* Stroke weight at the two ends of the morph — see `Marks.draw`, which
   interpolates between them on the same `ringness` term the track opacity
   uses. Written into `--install-ring-w`, read by progress.css. */
const W_REST = 1.6, W_RING = 2.5;

/** A point on the ring, with the clockwise tangent there. */
function onRing(deg: number): { P: [number, number]; phi: number } {
  const a = (deg * Math.PI) / 180;
  return { P: [CX + R_RING * Math.cos(a), CY + R_RING * Math.sin(a)], phi: a + Math.PI / 2 };
}
const ANCHOR = onRing(ANCHOR_DEG);
const P0 = ANCHOR.P;

/* ── the poses ─────────────────────────────────────────────────────────
   Stroke A is present in every state — the bar plus and minus share, so it
   becomes the track, the part of the ring that is always whole. Stroke B is
   what distinguishes plus from minus, so it becomes the arc, the part that
   measures. The plus loses its vertical bar to become a minus, and that is
   exactly the bar that carried the progress.

   A bar with bias .5 looks identical at phi and phi+180, so of the two ways
   to write each one we take the one nearer the ring's tangent (225°): the
   plus opens with a ±45° roll rather than a 135° spin.                     */
const pose = {
  plus: (): Pose => ({
    a: { P: P0, phi: Math.PI,         k: 0, len: BAR, bias: .5, op: 1 },
    b: { P: P0, phi: 3 * Math.PI / 2, k: 0, len: BAR, bias: .5, op: 1 },
  }),
  minus: (): Pose => ({
    a: { P: P0, phi: Math.PI,         k: 0, len: BAR, bias: .5, op: 1 },
    b: { P: P0, phi: 3 * Math.PI / 2, k: 0, len: 0,   bias: .5, op: 0 },
  }),
  ring: (pct: number): Pose => ({
    a: { P: P0,       phi: ANCHOR.phi, k: K_RING, len: CIRC,                 bias: .5, op: 1 },
    b: { P: ANCHOR.P, phi: ANCHOR.phi, k: K_RING, len: CIRC * (pct / 100),   bias: 0,  op: 1 },
  }),
  /* A check is ONE BENT LINE, not a cross — the arms must share an endpoint
     at the elbow. Both at 45°, short arm .45 of the long one (Feather's
     `M20 6L9 17l-5-5` proportions); each arm anchored at its own midpoint,
     half an arm from the elbow, so the ends meet to the unit; the elbow then
     placed so the mark's BOUNDING BOX centres on the badge point. Eyeballed
     offsets gave a cross. */
  check: (): Pose => {
    const LONG = 11.5, SHORT = 5.2, Q = Math.SQRT1_2;
    const tipA: [number, number] = [LONG * Q, -LONG * Q];    // elbow → up-right
    const tipB: [number, number] = [-SHORT * Q, -SHORT * Q]; // elbow → up-left
    const mid = (i: 0 | 1) =>
      (Math.min(0, tipA[i], tipB[i]) + Math.max(0, tipA[i], tipB[i])) / 2;
    const E: [number, number] = [P0[0] - mid(0), P0[1] - mid(1)];
    return {
      a: { P: [E[0] + tipA[0] / 2, E[1] + tipA[1] / 2], phi: 3 * Math.PI / 4, k: 0, len: LONG,  bias: .5, op: 1 },
      b: { P: [E[0] + tipB[0] / 2, E[1] + tipB[1] / 2], phi: 5 * Math.PI / 4, k: 0, len: SHORT, bias: .5, op: 1 },
    };
  },
  fail: (): Pose => ({
    a: { P: P0, phi: 5 * Math.PI / 4, k: 0, len: 9, bias: .5, op: 1 },
    b: { P: P0, phi: 3 * Math.PI / 4, k: 0, len: 9, bias: .5, op: 1 },
  }),
};

export type RestPose = 'plus' | 'minus';

/* ── the element ───────────────────────────────────────────────────────── */

const SVG_NS = 'http://www.w3.org/2000/svg';

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

class Marks {
  readonly svg: SVGSVGElement;
  private readonly a: SVGPathElement;
  private readonly b: SVGPathElement;

  constructor(host: HTMLElement) {
    this.svg = document.createElementNS(SVG_NS, 'svg');
    this.svg.setAttribute('viewBox', '0 0 64 64');
    this.svg.setAttribute('aria-hidden', 'true');
    this.svg.classList.add('install-ring');
    this.a = document.createElementNS(SVG_NS, 'path');
    this.a.setAttribute('class', 'install-ring__mk install-ring__mk--track');
    this.b = document.createElementNS(SVG_NS, 'path');
    this.b.setAttribute('class', 'install-ring__mk');
    this.svg.append(this.a, this.b);
    host.appendChild(this.svg);
  }

  /* The track's fade is DERIVED, not passed in: stroke A is at full strength
     while it is a bar and settles to a quiet track in proportion to how much
     of the ring's curvature it currently has. As an argument it was a thing
     every call site had to remember, and the ones that forgot produced a
     check with one pale arm and a track that popped on the first collapse
     frame. A property of the pose belongs to the pose.

     STROKE WIDTH is derived from the same term, and for a reason the opacity
     does not share: the two ends of this morph want different weights. A
     resting plus or minus is a small mark on a quiet tile and should be as
     light as the badge it replaced; a progress arc is a readout someone
     watches for six seconds and must hold its own around a 48px icon. One
     constant cannot be both — at 2.5 the plus was heavy, at 1.6 the download
     arc goes spidery. So the weight rides `ringness` too: 1.6 wherever the
     stroke is straight (plus, minus, ✓, ✗ — all k=0) and 2.5 once it is the
     ring, interpolating with the shape rather than switching under it. */
  draw(p: Pose): void {
    const ringness = Math.min(1, Math.abs(p.a.k) / K_RING);
    this.a.setAttribute('d', drawStroke(p.a));
    this.b.setAttribute('d', drawStroke(p.b));
    this.a.style.opacity = String(p.a.op);
    this.b.style.opacity = String(p.b.op);
    this.a.style.setProperty('--install-ring-track-op', String(lerp(1, .22, ringness)));
    this.svg.style.setProperty('--install-ring-w', String(lerp(W_REST, W_RING, ringness)));
  }

  ink(tone: 'accent' | 'muted' | 'error'): void {
    this.svg.dataset.tone = tone;
  }

  destroy(): void {
    this.svg.remove();
  }
}

/* ── pacing ────────────────────────────────────────────────────────────
   The floor is what a backend that reports NOTHING gets: it eases
   asymptotically toward CEILING and never arrives, so an unmeasured arc still
   moves and still cannot claim "done" before the work says so.

   It is strictly a stand-in. Once any real measurement arrives the floor is
   retired for the rest of the run and the arc is the measurement — see
   `sawReal` in `play()`. An arc paced by a clock beside a number read off the
   work is two answers to one question, and the picture wins.

   TAU_MS is tuned by measurement, not taste: at 1100 the floor read 52% at
   900ms into a six-second download whose truth was 15% — the arc rushed
   ahead and then visibly stalled, the single most distrusted behaviour a
   progress indicator has. At 4000 it reads ~17% there.                     */
const CEILING = 92, TAU_MS = 4000;
const MIN_CYCLE = 560;   // floor on how briefly the ring may be on screen
const CLOSE_FROM = 70;   // where an instant install has grown to before closing
const UNCURL = 430, CURL = 440, SWEEP = 320, HOLD = 260, SETTLE = 520;
const MIN_ARC = 4;       // with round caps, 1% is a dot

const floorAt = (ms: number) => CEILING * (1 - Math.exp(-ms / TAU_MS));

/**
 * What the arc reads this frame, while the work is still in flight.
 *
 * `sawReal` — not `real !== null` — is what retires the floor, so a feed that
 * reports 40% and then one null frame does not hand the arc back to a floor
 * that by now reads far higher and would lurch it forward. Monotonic in
 * `shown`, so nothing ever runs backwards, and capped at 99: only the work
 * finishing may close the ring.
 */
function arcValue(shown: number, real: number | null, sawReal: boolean, t: number): number {
  const paced = sawReal ? 0 : floorAt(t);
  return Math.min(99, Math.max(shown, MIN_ARC, real ?? 0, paced));
}

const wait = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

/** Text the ring asks its caller for. i18n stays at the boundary. */
export interface RingLabels {
  /** Tooltip while working, e.g. "Installing… 28%". `pct` null = unknown. */
  busy(pct: number | null): string;
  /** Visible-at-rest caption. Undefined = no caption. */
  caption(pct: number | null): string | undefined;
  /** Caption at the moment the work lands. Undefined = clear it. */
  done?: string;
  /** Tooltip once settled. */
  rest: string;
  /** Caption + tooltip if the work fails. */
  failedCaption: string;
  failedTooltip: string;
}

export interface RunOptions {
  /** true = plus → ring → minus. false = minus → ring → plus. */
  installing: boolean;
  /**
   * The work. It must resolve when the user can SEE the result — not when a
   * command returns. Resolving early is the one thing that makes the ring
   * lie. `false` means it failed.
   */
  work: () => Promise<boolean>;
  /**
   * Real progress, polled per frame. Return null when there is none — the
   * paced floor covers it and the caption declines to invent a number.
   *
   * Supplying this changes the ARC as well as the text: from the first
   * non-null reading onward the arc is your number, not the floor's, so the
   * arc and the percentage never disagree on screen.
   */
  progress?: () => number | null;
  labels: RingLabels;
}

export type RunOutcome = 'installed' | 'removed' | 'failed';

/**
 * The install affordance on one tile. Create it once, `run()` per click.
 */
export class InstallRing {
  private readonly marks: Marks;
  private at: RestPose;
  /**
   * What is on screen right now. Every tween starts HERE rather than from the
   * semantic rest pose, which is what lets a retry uncurl out of the ✗ it is
   * currently showing instead of jump-cutting to a plus first.
   */
  private cur: Pose;
  private busy = false;
  /**
   * Set by `destroy()`. Every clock in `play()` consults it, because removing
   * the SVG does not stop a rAF loop, a timer, or an awaited promise — and a
   * ring destroyed mid-install (Settings closed during a download) otherwise
   * paints a detached node at 60fps for the life of the window.
   */
  private dead = false;

  constructor(host: HTMLElement, at: RestPose) {
    this.marks = new Marks(host);
    this.at = at;
    this.cur = at === 'plus' ? pose.plus() : pose.minus();
    this.marks.ink(at === 'plus' ? 'accent' : 'muted');
    this.marks.draw(this.cur);
  }

  /** Repaint at rest without animating — for a truthful re-render. */
  set(at: RestPose): void {
    if (this.busy) return;
    this.at = at;
    this.cur = at === 'plus' ? pose.plus() : pose.minus();
    this.marks.ink(at === 'plus' ? 'accent' : 'muted');
    this.marks.draw(this.cur);
  }

  /**
   * Paint the settled ✗ of a failure that happened earlier — reopening the
   * panel after a download died must still say so, rather than showing a tile
   * that looks like it was never touched. `run()` starts from what is on
   * screen, so the retry uncurls out of this ✗ instead of jump-cutting.
   */
  failed(labels: RingLabels): void {
    if (this.busy) return;
    this.cur = pose.fail();
    this.marks.ink('error');
    this.marks.draw(this.cur);
    this.onText?.(labels.failedCaption, labels.failedTooltip);
  }

  get isBusy(): boolean {
    return this.busy;
  }

  destroy(): void {
    this.dead = true;
    this.marks.destroy();
  }

  /** Tween from whatever is on screen to `to`, and remember where we landed. */
  private async tween(to: Pose, ms: number): Promise<void> {
    if (this.dead) return;
    const from = this.cur;
    this.cur = to;
    await tweenPose(from, to, ms, (p) => { if (!this.dead) this.marks.draw(p); });
  }

  /**
   * Play one install or uninstall. Returns what the tile should now be.
   *
   * There is no fast path and no slow path: an 80ms bundled copy and a
   * six-second download run these same four beats. The only difference is
   * that the fast one reaches its closing sweep sooner — it keeps GROWING
   * through the remainder of MIN_CYCLE rather than freezing at 4%, because a
   * hold at 4% reads as stuck.
   */
  async run(opts: RunOptions): Promise<RunOutcome> {
    // Re-entrancy: report the state the tile is ALREADY in. `at` is where the
    // ring rests, so a plus means not installed — reporting 'removed' here
    // would tell the caller an uninstall just happened.
    if (this.busy || this.dead) return this.at === 'plus' ? 'removed' : 'installed';
    this.busy = true;
    try {
      return await this.play(opts);
    } finally {
      this.busy = false;
    }
  }

  private async play({ installing, work, progress, labels }: RunOptions): Promise<RunOutcome> {
    const reduced = prefersReducedMotion();
    this.marks.ink(installing ? 'accent' : 'muted');

    // 1 · uncurl: whatever is showing — plus, minus, or a ✗ from a failed
    //     attempt — opens into the ring at ~0%.
    await this.tween(pose.ring(MIN_ARC), UNCURL);

    // 2 · the paced arc, running until the work resolves.
    //
    // A REJECTION is a failure, not an absence of a result. Without the catch
    // `ok` stays null, the loop below never reaches its exit condition, and
    // the ring is pinned busy forever — which also freezes the whole catalog,
    // since a busy ring suppresses re-renders. tauri-specta rethrows an
    // Error-instance IPC failure rather than returning `{status:'error'}`, and
    // `eventBus.emit` runs subscribers unguarded, so this is reachable from
    // two directions.
    let ok: boolean | null = null;
    const settled = work().then((r) => { ok = r; }, () => { ok = false; });

    // Reduced motion skips the legibility floor too: MIN_CYCLE exists so the
    // arc has time to be READ, and there is no motion left to read.
    const minCycle = reduced ? 0 : MIN_CYCLE;
    const t0 = performance.now();
    let shown = MIN_ARC, doneAt: number | null = null, atDone = 0;
    // The floor may only drive the ARC, and only while there is no measurement
    // to drive it instead. Reporting the floor as the percentage would print an
    // invented number and label it a measurement — because the floor reads 58%
    // at four seconds, a real 60-second download would show 84% at ten and then
    // crawl. `told` is the largest REAL number seen, so the text is either true
    // or absent.
    //
    // But "the text is true" is not enough on its own: for as long as the floor
    // ALSO drove the arc, the two disagreed on screen. Measured on a six-second
    // stack download, the floor reads 52% at 900ms where the truth is 15% — a
    // ring more than a third of the way round beside a tooltip saying 15%. A
    // user reads the picture first and the number as the correction, which
    // makes the whole affordance look like it is guessing. So `sawReal` retires
    // the floor for the rest of the run the moment a real number arrives:
    //
    //   no measurement ever  → floor drives the arc, caption says the verb
    //   measurement exists   → the arc IS the measurement
    //
    // Nothing jumps at the handover. `shown` stays monotonic, and the first
    // frame lands ~16ms in where the floor is ~0.4% — below MIN_ARC.
    let told: number | null = null;
    let sawReal = false;
    let stopFrames = false;
    // The frame loop paints; it does not DECIDE when the arc is over. WebKit
    // suspends rAF for an occluded window, and an install the user minimizes
    // must still finish — with the loop as the only clock, the ring stayed
    // pinned (and the catalog inert) for as long as the window was hidden.
    // The timer below is the authority; the frames stop when it says so.
    const frames = new Promise<void>((res) => {
      const frame = () => {
        const t = performance.now() - t0;
        if (stopFrames || this.dead || (ok !== null && t >= minCycle)) return res();
        const real = ok === null ? (progress?.() ?? null) : null;
        if (real !== null) sawReal = true;
        if (ok === null) {
          shown = arcValue(shown, real, sawReal, t);
        } else {
          // Finished faster than the minimum legible cycle: keep growing
          // through the remainder rather than holding.
          if (doneAt === null) { doneAt = t; atDone = shown; }
          const k = Math.min(1, (t - doneAt) / Math.max(1, minCycle - doneAt));
          shown = Math.max(shown, atDone + (CLOSE_FROM - atDone) * k);
        }
        this.cur = pose.ring(shown);
        this.marks.draw(this.cur);
        // The number is shown only where one is REAL; the arc is identical
        // either way. Where there is none the caption says the verb instead.
        if (real !== null) told = Math.max(told ?? 0, Math.round(real));
        this.emit(labels, told);
        requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    });
    await Promise.race([
      frames,
      settled.then(async () => {
        await wait(Math.max(0, minCycle - (performance.now() - t0)));
      }),
    ]);
    stopFrames = true;
    await settled;
    if (this.dead) return installing ? 'installed' : 'removed';

    // 3 · fail, or close the ring and hold.
    if (ok === false) {
      this.marks.ink('error');
      await this.tween(pose.fail(), CURL);
      this.onText?.(labels.failedCaption, labels.failedTooltip);
      return 'failed';
    }
    await this.tween(pose.ring(100), SWEEP);
    // A percentage that climbs to 99 and then blanks is the shape of a
    // failure. If a real number was ever shown, land it on 100.
    this.onText?.(told === null ? labels.done : labels.caption(100), labels.rest);
    // The holds are NOT motion — they are how long a finished state is on
    // screen — so reduced motion keeps them. Skipping them painted the full
    // ring and the ✓ inside one microtask chain, i.e. never: two end states
    // that existed only mid-tween, which is the thing the rule forbids.
    if (!this.dead) await wait(HOLD);

    // 4 · curl back down.
    if (installing) {
      await this.tween(pose.check(), CURL);
      if (!this.dead) await wait(SETTLE);
      this.marks.ink('muted');
      await this.tween(pose.minus(), 300);
      this.at = 'minus';
      this.onText?.(undefined, labels.rest);
      return 'installed';
    }
    // Uninstall has no success to celebrate — straight back to the plus.
    this.marks.ink('accent');
    await this.tween(pose.plus(), CURL);
    this.at = 'plus';
    this.onText?.(undefined, labels.rest);
    return 'removed';
  }

  /** Caller-supplied sink for the caption and the tooltip. */
  onText: ((caption: string | undefined, tooltip: string) => void) | undefined;

  private emit(labels: RingLabels, pct: number | null): void {
    this.onText?.(labels.caption(pct), labels.busy(pct));
  }
}

/** Exported for the render gate and the unit tests — not for callers. */
export const __testing = {
  strokeD, pose, lerpPose, floorAt, arcValue, CIRC, R_RING, ANCHOR, Marks,
  W_REST, W_RING,
};
