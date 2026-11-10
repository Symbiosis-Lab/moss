// The markup every full-window preview screen is built from, and the two
// things that move on one without new markup.
//
// A leaf: one shell, one meter, one escape, one CTA, and nothing that knows
// which state is being drawn. The cloud wait, the folder-unavailable template and the
// nested-site threshold screen all draw from it, so the window keeps one visual vocabulary.
//
// Markup, plus the wiring of the two button ids it emits. Where the markup
// sits is style.css's: the screen's axis is the grid line nearest the middle,
// the meter is drawn on it, and everything else hangs off it — so the grid
// alignment is layout, not a measurement any code has to repeat.

import { t } from "../../i18n/index";
import type { VaultLocation } from "./vault-location";

export function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** A quiet bordered action on the screen, by id. */
export const actionButton = (id: string, label: string) =>
  `<button type="button" class="moss-waiting-screen__cta" id="${id}">${escapeHtml(label)}</button>`;

/** The quiet bordered escape. Every state that can be wrong offers it, and it
 *  is a real destination rather than a broken version of this screen. */
export const chooseFolderButton = () => actionButton("moss-waiting-choose-folder", t("preview.waiting_choose_folder"));

/** The file manager is where the user can see what the sync client thinks it
 *  is doing, and where the provider's "keep this offline" menu item lives —
 *  the one OS-level fix moss cannot apply for them. */
export const revealFolderButton = () => actionButton("moss-waiting-reveal-folder", t("tree.reveal"));

export interface ScreenShell {
  title: string;
  /** Under the title, still above the axis: the location line. Escaped markup. */
  above?: string;
  /** The meter ({@link meter}), drawn on the axis. Absent for a verdict. */
  meter?: string;
  /** Under the axis: the sentence, hint, tip and actions. Escaped markup. */
  below?: string;
  /**
   * The travelling glow on the axis. Suppressed wherever nothing is arriving:
   * a light crossing the line says "something is moving", and on a stalled
   * download, an unreachable folder or a build failure that is the one thing
   * the screen is not claiming.
   */
  crossing?: boolean;
  /** Marker attribute on the root, for the states CSS styles differently. */
  attr?: string;
}

/**
 * One screen. The title and location stand on the axis, the meter is a run of
 * it, and what is below hangs from it — so a line appearing below (a wrapped
 * sentence, the walk's escape after ten seconds) never moves the title or the
 * meter. Without a meter the title itself stands on the line.
 */
export function screenShell({ title, above = "", meter = "", below = "", crossing = false, attr = "" }: ScreenShell): string {
  const attrs = `${attr ? ` ${attr}` : ""}${meter ? "" : " data-meterless"}`;
  return `
    <div class="moss-waiting-screen"${attrs}>
      ${crossing ? `<div class="moss-waiting-screen__crossing" data-crossing aria-hidden="true"><span></span></div>` : ""}
      <div class="moss-waiting-screen__stack moss-waiting-screen__stack--above">
        <p class="moss-waiting-screen__title" data-title>${escapeHtml(title)}</p>
        ${above}
      </div>
      ${meter}
      <div class="moss-waiting-screen__stack moss-waiting-screen__stack--below">${below}</div>
    </div>
  `;
}

/**
 * The quiet location line, directly under the title: where the folder is
 * (`location.display` — already home-abbreviated and middle-truncated by
 * `formatVaultLocation`), the untruncated form in `data-tooltip` (never the
 * native `title=` — see the moss-ui-conventions skill), and the SAME
 * reveal button every other reason offers, moved up next to the path instead
 * of into the actions row below.
 *
 * Never pair this with a spec whose `buttons` already includes
 * {@link revealFolderButton} — both render the same `#moss-waiting-reveal-folder`
 * id, and a duplicate id would leave one of the two inert.
 */
export const locationLineOf = (location?: VaultLocation) =>
  location
    ? `<div class="moss-waiting-screen__location" data-location>
        <span class="moss-waiting-screen__location-text" data-tooltip="${escapeHtml(location.full)}">${escapeHtml(location.display)}</span>
        ${revealFolderButton()}
      </div>`
    : "";

/** The muted second line every state uses for "and here is what that means". */
export const hintLine = (text: string) =>
  text ? `<p class="moss-waiting-screen__hint">${escapeHtml(text)}</p>` : "";

/** What went wrong in the words a program used, folded away under one quiet
 *  summary: the sentence above it is the person's, this is for a report. */
export const detailsFold = (summary: string, text: string) =>
  text
    ? `<details class="moss-waiting-screen__details"><summary>${escapeHtml(summary)}</summary><p>${escapeHtml(text)}</p></details>`
    : "";

/** The quieter third line, for the one thing that ends this wait for good. */
export const tipLine = (text: string) =>
  text ? `<p class="moss-waiting-screen__tip">${escapeHtml(text)}</p>` : "";

/**
 * How often the meta line's hidden live region repeats itself.
 *
 * The VISIBLE line moves on every tick — a download count, a build stage, a
 * folder count — and every one of those ticks faster than a screen reader can
 * usefully follow. So the announcement is a second, visually-hidden node that
 * is rewritten at most this often, instead of an `aria-live` attribute on the
 * line itself repeating a fast walk dozens of times.
 */
export const META_ANNOUNCE_MS = 2_000;

/**
 * The one line carrying whatever is currently true — a count, a provider, a
 * build stage — plus the throttled live region that announces it.
 *
 * One per screen: it is the screen's only aria-live region, so a second would
 * make them race.
 */
export const metaLineOf = (text: string) =>
  text
    ? `<p class="moss-waiting-screen__meta" data-meta>${escapeHtml(text)}</p>
      <span class="visually-hidden" data-meta-live aria-live="polite">${escapeHtml(text)}</span>`
    : "";

/**
 * The actions row. Omitted entirely when there is nothing to offer.
 *
 * `hidden` is not a caller's convenience: `.moss-waiting-screen__actions` is
 * `display: flex`, which beats the UA's `[hidden] { display: none }`, so the
 * attribute alone does not hide the row — the paired rule in style.css does.
 * A hand-rolled copy of this markup is how the checking screen's 10s escape
 * once rendered from second zero, which is why the row has one owner.
 */
export const actionsRow = (
  buttons: string[],
  { attr = "", hidden = false }: { attr?: string; hidden?: boolean } = {},
) => {
  const live = buttons.filter(Boolean);
  if (!live.length) return "";
  return `<div class="moss-waiting-screen__actions"${attr ? ` ${attr}` : ""}${hidden ? " hidden" : ""}>${live.join("\n")}</div>`;
};

// ── The meter ──────────────────────────────────────────────────────────────

/**
 * How the meter reads.
 *
 * `live` — something is arriving or being built. `stalled` — the fraction is
 * still true but nothing is moving, so the fill dims. `unknown` — there is a
 * wait but no fraction to draw it with, so there is no fill at all rather than
 * a bar pinned at zero claiming progress information moss does not have.
 */
export type MeterMode = "live" | "stalled" | "unknown";

/** A real fraction to draw. Absent ⇒ the meter is indeterminate. */
export interface ScreenFraction {
  value: number;
  max: number;
}

/**
 * The meter: a run of one grid line, lit to show how far along we are.
 *
 * Every full-window state that is a WAIT draws one, determinate or not: it is
 * the lit part of the screen's axis, a real grid line. A verdict (an
 * unreachable folder, a build failure) draws none: nothing is arriving.
 *
 * Indeterminate is not a lesser meter. A build has no honest fraction, and a
 * full green bar during one would read as "done" — the exact claim the gate is
 * contradicting. So the fill is hidden and the `role` dropped, and the meta
 * line carries the announcement; a labelled role-less <div> is dropped by
 * assistive tech anyway.
 */
export function meter(mode: MeterMode, fraction?: ScreenFraction): string {
  // `Number(...)` before interpolation: everything else on this screen is
  // escaped, and these two are the only values from a backend payload that
  // reach an attribute raw. Coercing keeps the file safe by inspection rather
  // than by tracing where the numbers came from.
  const aria = fraction
    ? ` role="progressbar" aria-valuemin="0" aria-valuemax="${Number(fraction.max)}"` +
      ` aria-valuenow="${Math.min(Number(fraction.value), Number(fraction.max))}"` +
      ` aria-label="${escapeHtml(t("preview.waiting_meter_label"))}"`
    : "";
  return `<div class="moss-waiting-screen__meter" data-mode="${mode}"${aria}>
          <div class="moss-waiting-screen__meter-track"></div>
          <div class="moss-waiting-screen__meter-fill" style="width: ${fillWidth(fraction)}"></div>
        </div>`;
}

/** The fill's width, as a CSS length. Zero when there is no fraction to draw. */
function fillWidth(fraction?: ScreenFraction): string {
  if (!fraction || fraction.max <= 0) return "0.00%";
  return `${Math.min(100, (fraction.value / fraction.max) * 100).toFixed(2)}%`;
}

// ── Moving a screen that is already up ─────────────────────────────────────

/**
 * Move the two things that can change without new markup: the meter's fraction
 * and the sentence under it.
 *
 * Every screen's live tick comes through here, because re-rendering instead
 * costs four things that ARE the design: the crossing glow restarts from 0%
 * and so never crosses (7s animation, 2s life), the fill's width transition
 * can't fire because a fresh element renders at its final width, the live
 * region is recreated rather than updated (which screen readers do not
 * reliably announce), and whatever the user had focused is blurred — which on
 * the walk's screen also restarts the 10s clock that reveals its escape.
 *
 * `announce` is the caller's throttle decision (see `META_ANNOUNCE_MS`); the
 * visible line always moves.
 *
 * False means the markup on screen is not what these values need — no meter
 * where one is wanted, or a meter whose determinate-ness doesn't match — and
 * the caller must render instead. That is rare (a phase crossing, a provider
 * becoming known); a value change is every tick.
 */
export function applyScreenValues(
  container: HTMLElement,
  values: { meta: string; fraction?: ScreenFraction },
  announce: boolean,
): boolean {
  const meterEl = container.querySelector<HTMLElement>(".moss-waiting-screen__meter");
  const fill = container.querySelector<HTMLElement>(".moss-waiting-screen__meter-fill");
  if (values.fraction) {
    // `role` present ⇔ the meter was determinate when this screen was
    // rendered. This is also what refuses the cloud's phase-1 → phase-2
    // crossing: the last file landing turns a determinate meter indeterminate,
    // which is new markup.
    if (!meterEl || !fill || !meterEl.hasAttribute("role")) return false;
    fill.style.width = fillWidth(values.fraction);
    meterEl.setAttribute("aria-valuemax", String(Number(values.fraction.max)));
    meterEl.setAttribute(
      "aria-valuenow",
      String(Math.min(Number(values.fraction.value), Number(values.fraction.max))),
    );
  } else if (meterEl?.hasAttribute("role")) {
    return false;
  }

  const line = container.querySelector<HTMLElement>("[data-meta]");
  // A meta line that must appear or disappear is a shape change, not a value.
  if (!!line !== !!values.meta) return false;
  if (line) {
    if (line.textContent !== values.meta) line.textContent = values.meta;
    if (announce) {
      const live = container.querySelector<HTMLElement>("[data-meta-live]");
      if (live && live.textContent !== values.meta) live.textContent = values.meta;
    }
  }
  return true;
}
