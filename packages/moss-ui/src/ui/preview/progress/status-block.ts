/**
 * The advisory panel's status BLOCK — the headline, the detail row and the
 * standing-condition line, plus the live region that sits beside them.
 *
 * Split out of `progress-panel-renderer.ts` when the single status line grew a
 * second register. It is one cohesive thing: three rows
 * whose whole point is that they are read as one unit, and a set of cached
 * spans that exist so an aria-atomic live region does not re-announce on every
 * 4 Hz tick.
 *
 * All DOM, no policy. What to SAY is decided by `formatStatus`
 * (`progress-panel-stage.ts`); this only paints it.
 */

import type { StatusResult, StatusDetail } from "./progress-panel-stage";

export class StatusBlockView {
  /**
   * The column that owns the three rows and the 6/12 spacing that groups them.
   * It exists for every status kind; the two extra rows only for a publish,
   * which is the only thing with a second register to show.
   */
  private statusBlockEl: HTMLElement | null = null;
  /** The status line proper (replaces the old stage label + receipt header). */
  private statusLineEl: HTMLElement | null = null;
  /**
   * Cached inner spans — created ONCE and updated in-place so that repeated
   * renders with the same content produce no childList mutation on the
   * aria-live region (which would re-announce via aria-atomic on every tick).
   */
  private statusVerbSpan: HTMLSpanElement | null = null;
  private statusSubjectSpan: HTMLSpanElement | null = null;
  private statusCountSpan: HTMLSpanElement | null = null;
  /** Detail row (`what is moving` + `how fast`) and its halves. */
  private statusDetailEl: HTMLElement | null = null;
  private statusFileEl: HTMLElement | null = null;
  private statusFileDirSpan: HTMLSpanElement | null = null;
  private statusFileBaseSpan: HTMLSpanElement | null = null;
  private statusRateSpan: HTMLSpanElement | null = null;
  /** The standing-condition line ("Rebuild paused until upload finishes"). */
  private statusConditionEl: HTMLElement | null = null;
  /**
   * The upload-milestone live region — a SIBLING of the block, never a child:
   * the status line is aria-atomic, and a nested live region is subsumed by an
   * atomic ancestor, so every milestone would re-announce the whole line.
   */
  private statusLiveEl: HTMLElement | null = null;
  /** The host panel and the row it must sit above, supplied per render. */
  private panelEl: HTMLElement | null = null;
  private taskListEl: HTMLElement | null = null;

  /**
   * Whether the block is on screen. The panel's teardown rule reads it: a
   * status-only panel (a build with advisories but no narrated tasks) must
   * survive having zero task rows.
   */
  isMounted(): boolean {
    return this.statusBlockEl !== null;
  }

  /**
   * Render the single-line status text inside `.moss-status-line`.
   * Creates the element lazily on first non-none status; removes it on `none`.
   * Accessible: `role=alert` for `failed`, `role=status` + `aria-live=polite`
   * for everything else.
   *
   * Inner spans are CACHED as instance fields and updated via
   * `.textContent` in place — no `replaceChildren()` on repeated renders with
   * the same content. Replacing children on an aria-live region fires a childList
   * mutation that causes `aria-atomic` to re-announce even when the text has not
   * changed (triggered on every video-progress tick). Span nodes are only added/
   * removed when the STRUCTURE changes (subject/count appear or disappear).
   */
  render(panelEl: HTMLElement, taskListEl: HTMLElement | null, status: StatusResult): void {
    this.panelEl = panelEl;
    this.taskListEl = taskListEl;

    if (status.kind === "none") {
      // Removing the block takes its three rows with it, so only the cached
      // handles need clearing.
      this.statusBlockEl?.remove();
      this.statusBlockEl = null;
      this.statusLineEl = null;
      this.statusVerbSpan = null;
      this.statusSubjectSpan = null;
      this.statusCountSpan = null;
      this.dropDetailRows();
      // The live region is torn down HERE too. Left behind it would be an
      // orphaned live region attached to a panel whose status line is gone,
      // re-announcing on the next publish.
      this.statusLiveEl?.remove();
      this.statusLiveEl = null;
      return;
    }

    if (!this.statusBlockEl) {
      this.statusBlockEl = document.createElement("div");
      this.statusBlockEl.className = "moss-status-block";
      // Insert before taskListEl so it appears above the task rows.
      const insertBefore = this.taskListEl ?? null;
      if (insertBefore) {
        this.panelEl.insertBefore(this.statusBlockEl, insertBefore);
      } else {
        this.panelEl.appendChild(this.statusBlockEl);
      }
    }
    if (!this.statusLineEl) {
      this.statusLineEl = document.createElement("div");
      this.statusLineEl.className = "moss-status-line";
      this.statusLineEl.setAttribute("aria-atomic", "true");
      this.statusBlockEl.appendChild(this.statusLineEl);
    }

    // Accessibility role: alert for failures, status for everything else.
    if (status.kind === "failed") {
      this.statusLineEl.setAttribute("role", "alert");
      this.statusLineEl.removeAttribute("aria-live");
      this.statusLineEl.classList.add("moss-status-line--failed");
    } else {
      this.statusLineEl.setAttribute("role", "status");
      this.statusLineEl.setAttribute("aria-live", "polite");
      this.statusLineEl.classList.remove("moss-status-line--failed");
    }
    // data-kind drives accent/error CSS selectors on the status line.
    this.statusLineEl.setAttribute("data-kind", status.kind);

    // data-clear="true" requires kind==="receipt" and no unresolved notices
    // are present. CSS: receipt+notices → neutral; receipt+clear → accent (green).
    if (status.kind === "receipt") {
      this.statusLineEl.setAttribute("data-clear", status.clear === true ? "true" : "false");
    } else {
      this.statusLineEl.removeAttribute("data-clear");
    }

    // ── Verb span (always present, cached) ──────────────────────────────────
    if (!this.statusVerbSpan) {
      this.statusVerbSpan = document.createElement("span");
      this.statusVerbSpan.className = "moss-status-line__verb";
      this.statusLineEl.appendChild(this.statusVerbSpan);
    }
    this.statusVerbSpan.textContent = status.text;

    // ── Subject span (optional, cached; added/removed only when presence changes) ──
    if (status.subject) {
      if (!this.statusSubjectSpan) {
        this.statusSubjectSpan = document.createElement("span");
        this.statusSubjectSpan.className = "moss-status-line__subject";
        // Insert after verb (before count if it exists, otherwise at end).
        const before = this.statusCountSpan ?? null;
        if (before) {
          this.statusLineEl.insertBefore(this.statusSubjectSpan, before);
        } else {
          this.statusLineEl.appendChild(this.statusSubjectSpan);
        }
      }
      this.statusSubjectSpan.textContent = status.subject;
    } else if (this.statusSubjectSpan) {
      this.statusSubjectSpan.remove();
      this.statusSubjectSpan = null;
    }

    // ── Count span (optional, cached; added/removed only when presence changes) ──
    if (status.count) {
      if (!this.statusCountSpan) {
        this.statusCountSpan = document.createElement("span");
        this.statusCountSpan.className = "moss-status-line__count";
        // The count refreshes every tick, so suppress redundant AT announcements.
        this.statusCountSpan.setAttribute("aria-hidden", "true");
        this.statusLineEl.appendChild(this.statusCountSpan);
      }
      this.statusCountSpan.textContent = status.count;
    } else if (this.statusCountSpan) {
      this.statusCountSpan.remove();
      this.statusCountSpan = null;
    }

    // ── Detail row + condition line ──────────────────
    this.renderStatusDetail(status.detail);
    this.renderStatusCondition(status.condition);
    // The ledger modifier carries the wider 12px bottom pad. It is a class and
    // not a `:has()` selector because WebKitGTK is one of the two engines this
    // panel renders in and the render gates assert on both.
    this.statusBlockEl.classList.toggle(
      "moss-status-block--ledger",
      this.statusDetailEl !== null || this.statusConditionEl !== null,
    );

    // ── A11y live region (upload milestones) ────────────────────────────────
    // A SIBLING of the status line, never a child: `.moss-status-line` carries
    // aria-atomic="true", and a nested live region is subsumed by an atomic
    // ancestor — every milestone would re-announce the whole line (verb +
    // subject + count) instead of the milestone. Mounted as a direct child of
    // panelEl immediately before the status line.
    if (status.announce) {
      if (!this.statusLiveEl) {
        this.statusLiveEl = document.createElement("div");
        this.statusLiveEl.className = "visually-hidden";
        this.statusLiveEl.setAttribute("aria-live", "polite");
        this.panelEl.insertBefore(this.statusLiveEl, this.statusBlockEl);
      }
      // Only write on CHANGE: assigning the same text replaces the text node,
      // which is a childList mutation the live region would re-announce.
      if (this.statusLiveEl.textContent !== status.announce) {
        this.statusLiveEl.textContent = status.announce;
      }
    } else if (this.statusLiveEl) {
      this.statusLiveEl.remove();
      this.statusLiveEl = null;
    }
  }

  /**
   * The detail row: the file in flight on the left, the transfer rate on the
   * right.
   *
   * `aria-hidden`, deliberately. The status line above is an `aria-atomic`
   * live region and this row changes several times a second — announcing it
   * would drown the milestone announcements that carry the actual progress.
   * The row is evidence for the eye; the ear is served by `status.announce`.
   *
   * The filename crossfades on swap rather than cutting. Backend already holds
   * a pick for 700 ms (`CurrentFileHold`), so the fade marks a real change of
   * subject instead of dressing up a strobe.
   */
  private renderStatusDetail(detail: StatusDetail | undefined): void {
    if (!this.statusBlockEl || !detail) {
      this.dropDetailRow();
      return;
    }

    if (!this.statusDetailEl) {
      this.statusDetailEl = document.createElement("div");
      this.statusDetailEl.className = "moss-status-detail";
      this.statusDetailEl.setAttribute("aria-hidden", "true");
      // After the status line, before the condition line if one is mounted.
      this.statusBlockEl.insertBefore(
        this.statusDetailEl,
        this.statusConditionEl ?? null,
      );
    }

    if (detail.file) {
      if (!this.statusFileEl) {
        this.statusFileEl = document.createElement("span");
        this.statusFileEl.className = "moss-status-detail__file";
        this.statusFileDirSpan = document.createElement("span");
        this.statusFileDirSpan.className = "moss-status-detail__dir";
        this.statusFileBaseSpan = document.createElement("span");
        this.statusFileBaseSpan.className = "moss-status-detail__base";
        this.statusFileEl.append(this.statusFileDirSpan, this.statusFileBaseSpan);
        this.statusDetailEl.insertBefore(this.statusFileEl, this.statusDetailEl.firstChild);
      }
      const changed = this.statusFileBaseSpan!.textContent !== detail.file.base;
      this.statusFileDirSpan!.textContent = detail.file.dir;
      this.statusFileBaseSpan!.textContent = detail.file.base;
      if (changed) this.crossfade(this.statusFileEl);
    } else if (this.statusFileEl) {
      this.statusFileEl.remove();
      this.statusFileEl = null;
      this.statusFileDirSpan = null;
      this.statusFileBaseSpan = null;
    }

    if (detail.rate) {
      if (!this.statusRateSpan) {
        this.statusRateSpan = document.createElement("span");
        this.statusRateSpan.className = "moss-status-detail__rate";
        this.statusDetailEl.appendChild(this.statusRateSpan);
      }
      this.statusRateSpan.textContent = detail.rate;
    } else if (this.statusRateSpan) {
      this.statusRateSpan.remove();
      this.statusRateSpan = null;
    }
  }

  /**
   * 110 ms opacity fade on the element whose text just changed. Uses the Web
   * Animations API rather than a class toggle so no reflow-forcing dance and
   * no timer bookkeeping is needed — a second call simply supersedes the
   * first. Skipped where `animate` is unavailable (jsdom) and under
   * `prefers-reduced-motion`, where a swap this frequent is exactly the
   * motion the preference is about.
   */
  private crossfade(el: HTMLElement): void {
    if (typeof el.animate !== "function") return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 110, easing: "ease-out" });
  }

  /**
   * The standing-condition line — one sentence about the whole operation,
   * e.g. "Rebuild paused until upload finishes". It sits on its own row
   * because it is not progress: sharing the headline row (as the old
   * `subject` slot did) made it the only shrinkable child of a full nowrap
   * line, so it ellipsized away every time there were numbers to show.
   */
  private renderStatusCondition(condition: string | undefined): void {
    if (!this.statusBlockEl || !condition) {
      this.statusConditionEl?.remove();
      this.statusConditionEl = null;
      return;
    }
    if (!this.statusConditionEl) {
      this.statusConditionEl = document.createElement("div");
      this.statusConditionEl.className = "moss-status-condition";
      this.statusBlockEl.appendChild(this.statusConditionEl);
    }
    if (this.statusConditionEl.textContent !== condition) {
      this.statusConditionEl.textContent = condition;
    }
  }

  /** Unmount the detail row and forget its cached spans. */
  private dropDetailRow(): void {
    this.statusDetailEl?.remove();
    this.statusDetailEl = null;
    this.statusFileEl = null;
    this.statusFileDirSpan = null;
    this.statusFileBaseSpan = null;
    this.statusRateSpan = null;
  }

  /** Unmount both extra rows (status gone, or the whole panel torn down). */
  destroy(): void {
    this.statusBlockEl?.remove();
    this.statusBlockEl = null;
    this.statusLineEl = null;
    this.statusVerbSpan = null;
    this.statusSubjectSpan = null;
    this.statusCountSpan = null;
    this.statusLiveEl?.remove();
    this.statusLiveEl = null;
    this.dropDetailRows();
  }

  /** Unmount both extra rows. */
  private dropDetailRows(): void {
    this.dropDetailRow();
    this.statusConditionEl?.remove();
    this.statusConditionEl = null;
  }
}
