/**
 * ProgressSeam — the PERSISTENT, declarative seam + severity dot inside the
 * `.moss-pill-cluster`.
 *
 * What it owns (declarative — NO timers, NO geometry):
 *   (a) the quiet resting SEAM: a 1.5px line in `--moss-color-accent-quiet`,
 *       spanning ONLY the pill's straight underside. Placement is pure CSS
 *       (`top:100%; left/right: var(--moss-pill-radius)` in the cluster) —
 *       this module never measures or positions anything.
 *   (b) the severity DOT: an 8px mark with a 1.5px cream (`--moss-color-bg`)
 *       ring and NO halo, CSS-anchored to the seam's RIGHT end (the LEFT
 *       origin is buried under the L→R rebuild fill). `data-origin="right"`
 *       records this. Color = `--broken` (red) or `--degraded` (amber) — the
 *       seam dot is the SOLE owner of this element via `setTopSeverity()`.
 *   (c) `setTopSeverity("broken"|"degraded"|"none")` / `setVisible(visible)`
 *       — the only mutators.
 *
 * Why the cluster mount: mounting INSIDE the cluster makes the seam ride the
 * same single anchor as the pill, so there is nothing to re-measure and
 * nothing to go stale — critical since the divider's CSS var
 * (`--action-panel-width` → `--moss-safe-center-x`) can move the pill with no
 * resize and no titlebar mutation to catch. Persistence still holds — the
 * cluster (like the titlebar) is never torn down; only the progress panel is.
 *
 * Relationship to `preview-hairline.ts`: that module stays the TRANSIENT
 * build-fill ANIMATOR (its grow/dissolve/trickle timers are untouched). Its
 * track shares the seam's CSS geometry, so the green L→R fill rides exactly
 * over the quiet seam. Two elements, one line: quiet seam always (here) +
 * green fill during a build (there).
 *
 * # Reveal inputs
 *
 * The seam also owns the REVEAL TARGET: a padded (~16px tall) invisible
 * hit-region straddling the line, keyboard-focusable. The disclosure glyph
 * (▾/▴) lives in the panel glass as a handle button — there is no cue here.
 * The seam emits INPUTS ONLY via `setCallbacks({ onHoverEnter, onHoverLeave,
 * onClick })` — it holds NO visibility state and never opens/closes the
 * panel. `ProgressPanel` is the sole code that maps an input to a
 * `RevealState` transition.
 *
 * # setTopSeverity — SOLE dot owner
 *
 * The seam dot is the SOLE surface that maps "top advisory tier" to a visual
 * affordance on the persistent pill layer. `ProgressPanel` emits the
 * computed `topSeverity` from its view-model; `PreviewManager` wires it to
 * `seam.setTopSeverity()`. The seam holds only the current severity enum —
 * never a copy of the advisory set.
 */
import { t } from "../../i18n/index";

export interface SeamCallbacks {
  onHoverEnter?: () => void;
  onHoverLeave?: () => void;
  onClick?: () => void;
}

export type TopSeverity = "broken" | "degraded" | "none";

export class ProgressSeam {
  private seamEl: HTMLElement | null = null;
  private dotEl: HTMLElement | null = null;
  private hitEl: HTMLElement | null = null;
  /** The `.moss-pill-cluster` host — the CSS anchor every child rides. Stored
   *  at mount so the dot mounts against an explicit reference, not seamEl's
   *  parentElement chain. */
  private clusterEl: HTMLElement | null = null;
  /** Current severity; drives the dot's modifier class and hit aria-label. */
  private currentSeverity: TopSeverity = "none";
  /** The seam + reveal hit-region are HIDDEN at rest. They show only when the
   * panel has content (a build/job/advisory), driven by setVisible(). A
   * persistent line on an idle pill reads as "stuck progress" and offers a
   * reveal handle with no panel behind it. */
  private visible = false;
  private callbacks: SeamCallbacks = {};

  /** Mount the quiet resting seam inside the pill cluster. Idempotent; a
   *  no-op when the cluster chrome isn't present yet (jsdom without chrome). */
  mount(): void {
    if (this.seamEl) return;
    const cluster = document.querySelector<HTMLElement>(".moss-pill-cluster");
    if (!cluster) return;

    const seam = document.createElement("div");
    seam.className = "moss-progress-seam";
    seam.setAttribute("aria-hidden", "true");
    cluster.appendChild(seam);
    this.seamEl = seam;
    this.clusterEl = cluster;

    // The padded reveal target. An invisible (~16px) hit-region
    // straddling the seam, keyboard-focusable so the panel is reachable WITHOUT
    // hover (Windows/touch). The disclosure glyph (▾/▴) is on
    // the handle button inside the panel glass — no cue child here.
    const hit = document.createElement("button");
    hit.className = "moss-seam-hit";
    hit.type = "button";
    hit.setAttribute("role", "button");
    hit.setAttribute("tabindex", "0");
    hit.setAttribute("aria-label", t('preview.seam_show_progress'));
    hit.setAttribute("data-tooltip-position", "bottom");

    cluster.appendChild(hit);
    this.hitEl = hit;
    this.wireHitRegion(hit);
    this.applyVisible(); // hidden at rest until setVisible(true)
  }

  /**
   * Register a sink for the seam's hover and click reveal inputs. The seam never
   * mutates visibility — `ProgressPanel` (the single owner) maps these to
   * `RevealState` transitions. Mirrors the renderer's `setCallbacks(onHide)`.
   * Idempotent; the latest callbacks win.
   */
  setCallbacks(callbacks: SeamCallbacks): void {
    this.callbacks = callbacks;
  }

  /**
   * Wire the hit-region's pointer/keyboard inputs to the (panel-owned) callbacks.
   * The seam emits inputs only; it carries NO visibility state and never touches
   * the panel DOM. The disclosure cue is gone — the handle button
   * inside the panel glass (▾/▴) is the sole disclosure glyph.
   */
  private wireHitRegion(hit: HTMLElement): void {
    hit.addEventListener("mouseenter", () => {
      this.callbacks.onHoverEnter?.();
    });
    hit.addEventListener("mouseleave", () => {
      this.callbacks.onHoverLeave?.();
    });
    // Keyboard/touch reach: Enter/Space act as a click. Works without hover.
    hit.addEventListener("click", () => this.callbacks.onClick?.());
    hit.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " " || e.key === "Spacebar") {
        e.preventDefault();
        this.callbacks.onClick?.();
      }
    });
  }

  /**
   * Set/clear a hover description on the reveal band.
   *
   * Moved here from `PreviewHairline.setTooltip` (which put it on the fill
   * TRACK). The track sits at z(--z-toast)+2 so its green fill paints over the
   * panel; giving it `pointer-events:auto` therefore made it outrank this band
   * (z:2) and swallow every hover on the line during a publish — the user
   * reached for the detail panel and got a tooltip instead.
   *
   * USE ONLY WHEN THE PANEL IS NOT TELLING THE SAME STORY. A tooltip and a
   * popover must not fire from one gesture (PatternFly: "if content is very
   * short and does not include links, use a Tooltip instead" — i.e. pick one).
   * The deploy path does NOT set this: its numbers are already the panel's count
   * slot. The borrowed ambient-download pass DOES, because it opens no panel.
   */
  setTooltip(text: string | null): void {
    if (!this.hitEl) this.mount();
    if (!this.hitEl) return;
    if (text) this.hitEl.setAttribute("data-tooltip", text);
    else this.hitEl.removeAttribute("data-tooltip");
  }

  /**
   * SOLE owner of the seam dot. Creates/colors/hides the dot
   * element (`.moss-severity-dot` + `--broken` / `--degraded` modifier), and
   * updates the hit-region's `aria-label` dynamically so the accessible name
   * reflects the severity (non-tautological: "broken" and "degraded" names
   * differ from the at-rest "Show progress"). Self-mounts if not yet mounted.
   *
   * The panel emits `topSeverity` from its view-model PreviewManager
   * wires it here. The seam holds only the current severity enum — never a copy
   * of the advisory set.
   */
  setTopSeverity(severity: TopSeverity): void {
    this.currentSeverity = severity;
    this.mount(); // ensure seam exists; no-op if already mounted or no chrome

    // Update the hit-region aria-label to reflect severity.
    if (this.hitEl) {
      switch (severity) {
        case "broken":
          this.hitEl.setAttribute("aria-label", t('preview.seam_build_errors'));
          break;
        case "degraded":
          this.hitEl.setAttribute("aria-label", t('preview.seam_warnings'));
          break;
        default:
          this.hitEl.setAttribute("aria-label", t('preview.seam_show_progress'));
      }
    }

    if (severity === "none") {
      this.dotEl?.remove();
      this.dotEl = null;
      return;
    }

    if (!this.clusterEl) return;

    // Create the dot if not yet present.
    if (!this.dotEl) {
      this.dotEl = document.createElement("div");
      this.dotEl.className = "moss-severity-dot";
      this.dotEl.setAttribute("aria-hidden", "true");
      // Records the seam's RIGHT origin (the left origin is buried under the
      // L→R rebuild fill). Pixel position is pure CSS; this attr locks the
      // intent so it can't silently flip to "left".
      this.dotEl.setAttribute("data-origin", "right");
      this.clusterEl.appendChild(this.dotEl);
    }

    // Apply modifier class for color — remove the other one to keep exactly one.
    this.dotEl.classList.toggle("moss-severity-dot--broken", severity === "broken");
    this.dotEl.classList.toggle("moss-severity-dot--degraded", severity === "degraded");
  }

  /**
   * Show/hide the seam line + the reveal hit-region. The seam is NOT shown at
   * rest — only when the panel has content (a build/job/advisory). This keeps a
   * persistent green line off the idle pill AND removes the reveal dead-end (no
   * hit-region when there's nothing to reveal). The severity dot is controlled
   * separately by setTopSeverity(); when a severity exists, content is present
   * too, so the seam shows alongside the dot. Idempotent.
   */
  setVisible(visible: boolean): void {
    if (visible === this.visible) return;
    this.visible = visible;
    if (visible) this.mount(); // ensure it exists before showing
    this.applyVisible();
  }

  private applyVisible(): void {
    this.seamEl?.classList.toggle("is-hidden", !this.visible);
    this.hitEl?.classList.toggle("is-hidden", !this.visible);
  }

  /** Tear down the seam + dot + hit-region. (The cluster host is never
   *  torn down by this module — only the elements it created.) */
  destroy(): void {
    this.dotEl?.remove();
    this.dotEl = null;
    this.hitEl?.remove();
    this.hitEl = null;
    this.seamEl?.remove();
    this.seamEl = null;
    this.clusterEl = null;
    this.currentSeverity = "none";
    this.callbacks = {};
  }
}
