/**
 * ProgressPanelRenderer - DOM rendering for the progress panel
 * Handles creation, update, and removal of DOM elements.
 * Keeps DOM manipulation separate from business logic (ProgressPanel).
 */
import type { ProgressTask, ProgressPlacement, PanelViewModel } from "./progress-panel-stage";
import { StatusBlockView } from "./status-block";
import type { Action } from "@symbiosis-lab/moss-ui";
import { dispatchAppOp } from "../app-op-dispatch";
import { t } from "../../i18n/index";
import { createCopyButton } from "../../shared/ui/copy-button";
import { morphHeight, prefersReducedMotion } from "../../shared/ui/morph";
import { eventBus, Events, type OpenFileInEditorEvent, type ClearNoticeEvent } from '../../context/event-bus';
import { CloudFileDisclosure } from "./cloud-file-disclosure";

/** The display label for a site-relative path an advisory names: the name a
 *  reader recognizes, not the directories it took to get there. */
function basename(path: string): string {
  return path.split('/').pop() || path;
}

export class ProgressPanelRenderer {
  private panelEl: HTMLElement | null = null;
  private taskListEl: HTMLElement | null = null;
  private taskEls: Map<string, HTMLElement> = new Map();
  private onPanelPointerEnter: (() => void) | null = null;
  private onPanelPointerLeave: (() => void) | null = null;
  /**
   * The status block — headline, detail row, condition line, live region.
   * Owns its own DOM and its own cached spans (`status-block.ts`).
   */
  private readonly statusBlock = new StatusBlockView();
  /** The notices container. */
  private noticesEl: HTMLElement | null = null;
  /**
   * Notice disclosure state, keyed by `NoticeVM.key` rather than held in the
   * DOM. The notices rail is rebuilt wholesale on every render, so a
   * `<details open>` or a revealed `+N` list would die with its element — and a
   * cloud-sync stall renders every 2 seconds, which is how "expand the detail"
   * became impossible to do. Keying off notice identity survives the
   * rebuild; a notice that goes away takes its entry with it (pruned below).
   */
  private openNoticeDetails: Set<string> = new Set();
  private revealedNoticeFiles: Set<string> = new Set();
  private readonly cloudDisclosure = new CloudFileDisclosure();
  /** The ▾/▴ handle button at the bottom of the panel. */
  private handleEl: HTMLElement | null = null;
  /** Callback wired by the ProgressPanel constructor. */
  private onHandleToggle: (() => void) | null = null;
  /** Body hover/focus callbacks wired by ProgressPanel. */
  private onBodyHoverEnter: (() => void) | null = null;
  private onBodyHoverLeave: (() => void) | null = null;
  private onBodyFocusIn: (() => void) | null = null;
  private onBodyFocusOut: (() => void) | null = null;

  /**
   * The persistent structural seam and advisory dot belong to the declarative
   * `ProgressSeam` on the pill-sibling (`.moss-titlebar`) layer. They survive
   * the panel's `hide()` and this renderer's remove-when-empty teardown.
   * The panel's square top meets the seam without owning its position or
   * lifetime. See `progress-seam.ts`.
   */

  private placement: ProgressPlacement = "bottom";

  constructor(private container: HTMLElement) {}

  setCloudFilesCallback(onRequest: () => void): void { this.cloudDisclosure.setCallback(onRequest); }

  isCloudFilesOpen(): boolean {
    return this.cloudDisclosure.isOpen();
  }

  resetCloudFilesDisclosure(): void {
    this.cloudDisclosure.reset();
    this.noticesEl?.querySelector(".moss-notice-cloud-files")?.remove();
  }

  /**
   * Wire the handle toggle callback. The handle button
   * (▾/▴) calls this one method when clicked; the ProgressPanel constructor
   * registers a handler that branches on getRevealState(): pin() when
   * peek/surfaced, hide() when pinned.
   */
  setHandleCallbacks(onToggle: () => void): void {
    this.onHandleToggle = onToggle;
  }

  /**
   * Pointer enter/leave on the panel glass itself (part of the hover union
   * that prevents flicker). The peeked panel opens under the very pointer that triggered it, so
   * the seam band fires mouseleave the moment the peek blooms — these inputs
   * let the panel count "on the glass" as still-inside and keep the peek open.
   */
  setHoverCallbacks(onEnter: () => void, onLeave: () => void): void {
    this.onPanelPointerEnter = onEnter;
    this.onPanelPointerLeave = onLeave;
  }

  /**
   * Wire durable body hover + focus callbacks. These drive
   * the `bodyHovered` / `bodyFocused` suppressors in ProgressPanel so that
   * `checkAutoClose()` is suppressed while the pointer is on the glass or any
   * descendant has keyboard focus.
   */
  setBodyCallbacks(
    onHoverEnter: () => void,
    onHoverLeave: () => void,
    onFocusIn: () => void,
    onFocusOut: () => void,
  ): void {
    this.onBodyHoverEnter = onHoverEnter;
    this.onBodyHoverLeave = onHoverLeave;
    this.onBodyFocusIn = onFocusIn;
    this.onBodyFocusOut = onFocusOut;
  }

  /**
   * Expose the root panel element so ProgressPanel can
   * move focus into the glass on seam-click (keyboard-open). Returns null when
   * the panel is not mounted (minimized or not yet created).
   */
  getPanelEl(): HTMLElement | null {
    return this.panelEl;
  }

  /**
   * Render the status line + empty notices container from a
   * PanelViewModel. Manages panel lifecycle (creates/removes panel element) and
   * drives grow/retract animation. Called from ProgressPanel.renderPanel().
   *
   * Render the panel from a PanelViewModel — status line + notices rail + handle.
   */
  render(model: PanelViewModel): void {
    this._renderModel(model);
  }

  /** PanelViewModel render path — status line + panel lifecycle. */
  private _renderModel(model: PanelViewModel): void {
    const isHidden = model.reveal === "minimized";
    const hasContent = model.status.kind !== "none" || model.notices.length > 0;

    // Create panel if there is status/notice content to show AND panel is not
    // minimized. Don't create a panel for hidden/ambient content — "success
    // makes no sound": an ambient task while hidden must
    // not grow the glass. renderNarrated() may still create the panel for tasks.
    if (!this.panelEl && hasContent && !isHidden) {
      this.createPanel();
    }

    if (!this.panelEl) return;

    this.updatePosition();

    // Don't update the status line content when hidden — a minimized panel
    // should not have new content painted into it (silence discipline).
    if (!isHidden) {
      this.statusBlock.render(this.panelEl, this.taskListEl, model.status);
    }

    // Render notices rail.
    if (!isHidden) {
      this.renderNotices(model.notices);
    }

    // Render the ▾/▴ handle button. A hidden panel keeps
    // the handle it has: removing it here shortened the box by the handle's
    // height BEFORE `applyVisibility` measured `from`, so a close snapped down
    // by ~26px and then animated the rest. `morphHeight`'s contract is that
    // every DOM change lives inside its `mutate`. The kept handle is inside a
    // zero-height `overflow:hidden` box with `visibility:hidden`, so it is
    // neither painted nor reachable.
    if (!isHidden) {
      this.renderHandle(model.reveal);
    }

    // LAST: the height morph measures the box, so everything that decides the
    // height must already be in it.
    this.applyVisibility(isHidden);

    // Note: panel teardown is NOT done here — renderNarrated() is responsible
    // for tearing down when there are no tasks and no status line. This prevents
    // a false teardown when the panel was created by a previous renderNarrated()
    // call and _renderModel is called with status.kind === 'none'.
  }

  /**
   * Render the notices rail — red/amber notices derived from
   * grouped advisories. Each NoticeVM becomes a `.moss-notice` row with:
   *   - a severity dot (.moss-severity-dot--broken or --degraded)
   *   - reason-first text + inline files
   *   - a focusable +N button when hiddenFiles exist
   *   - command field (buildCommandField) when command is set
   *   - action affordance (buildActionAffordance) when action is set
   *   - an expandable detail region when detail is set
   */
  private renderNotices(notices: import("./progress-panel").NoticeVM[]): void {
    if (!this.panelEl) return;

    // Ensure the notices container exists.
    if (!this.noticesEl) {
      this.noticesEl = document.createElement("div");
      this.noticesEl.className = "moss-notices";
      this.panelEl.appendChild(this.noticesEl);
    }

    // Full rebuild — small list, changes rarely. Disclosure state lives in the
    // key sets above, not in the elements being discarded here.
    const focusedCloudControl = document.activeElement?.classList.contains("moss-notice-cloud-files-toggle")
      ? "summary" : document.activeElement?.classList.contains("moss-notice-cloud-files-refresh")
        ? "refresh" : null;
    const scrollTop = this.noticesEl.scrollTop;
    this.noticesEl.replaceChildren();

    // Forget disclosure state for notices that are gone, so the sets can't grow
    // without bound over a long session.
    const live = new Set(notices.map((n) => n.key));
    for (const key of [...this.openNoticeDetails]) {
      if (!live.has(key)) this.openNoticeDetails.delete(key);
    }
    for (const key of [...this.revealedNoticeFiles]) {
      if (!live.has(key)) this.revealedNoticeFiles.delete(key);
    }

    if (!notices.some((notice) => notice.cloudFiles)) this.resetCloudFilesDisclosure();
    if (notices.length === 0) {
      return;
    }

    for (const notice of notices) {
      const row = document.createElement("div");
      row.className = "moss-notice";

      // Severity dot
      const dot = document.createElement("span");
      dot.className = `moss-severity-dot moss-severity-dot--${notice.severity}`;
      dot.setAttribute("aria-hidden", "true");
      row.appendChild(dot);

      // Reason text (the head — em-dash split was done in groupNotices)
      const reasonSpan = document.createElement("span");
      reasonSpan.className = "moss-notice-reason";
      reasonSpan.textContent = notice.reason;
      row.appendChild(reasonSpan);

      // Inline files — item 3: insert a real " · " text node before the files
      // span so screen readers announce "reason · files" (not "reasonfiles").
      // A CSS ::before pseudo-element would be invisible to the a11y tree.
      if (notice.files.length > 0) {
        row.appendChild(document.createTextNode(" · "));
        row.appendChild(this.buildFileList(notice.files, "moss-notice-files"));
      }

      // +N disclosure button for hidden files — already revealed once, so it
      // stays revealed across the rebuild.
      if (notice.hiddenFiles.length > 0 && this.revealedNoticeFiles.has(notice.key)) {
        row.appendChild(this.buildFileList(
          notice.hiddenFiles,
          "moss-notice-files moss-notice-files--revealed",
        ));
      } else if (notice.hiddenFiles.length > 0) {
        const moreBtn = document.createElement("button");
        moreBtn.type = "button";
        moreBtn.className = "moss-notice-more";
        moreBtn.textContent = `+${notice.hiddenFiles.length}`;
        moreBtn.setAttribute(
          "aria-label",
          t('editmisc.show_more_files', { n: notice.hiddenFiles.length }),
        );
        // On click, reveal the hidden files inline
        moreBtn.addEventListener("click", () => {
          this.revealedNoticeFiles.add(notice.key);
          moreBtn.replaceWith(this.buildFileList(
            notice.hiddenFiles,
            "moss-notice-files moss-notice-files--revealed",
          ));
        });
        row.appendChild(moreBtn);
      }

      // Command field (Action::Command → { run })
      if (notice.command) {
        row.appendChild(this.buildCommandField(notice.command.run));
      }

      // Action affordance (InApp/Link)
      if (notice.action) {
        const aff = this.buildActionAffordance(notice.action, notice.key);
        if (aff) row.appendChild(aff);
      }

      // Expandable detail region
      if (notice.detail && notice.cloudFiles) {
        const hint = document.createElement("span");
        hint.className = "moss-notice-cloud-hint";
        hint.textContent = notice.detail;
        row.appendChild(hint);
      } else if (notice.detail) {
        const detailEl = document.createElement("details");
        detailEl.className = "moss-notice-detail";
        const summary = document.createElement("summary");
        summary.className = "moss-notice-detail-toggle";
        summary.textContent = t('progress.details');
        detailEl.appendChild(summary);
        const content = document.createElement("span");
        content.className = "moss-notice-detail-content";
        content.textContent = notice.detail;
        detailEl.appendChild(content);
        detailEl.open = this.openNoticeDetails.has(notice.key);
        detailEl.addEventListener("toggle", () => {
          if (detailEl.open) this.openNoticeDetails.add(notice.key);
          else this.openNoticeDetails.delete(notice.key);
        });
        row.appendChild(detailEl);
      }

      if (notice.cloudFiles) {
        row.appendChild(this.cloudDisclosure.render(notice.cloudFiles));
      }

      this.noticesEl.appendChild(row);
    }
    if (focusedCloudControl) {
      this.noticesEl.querySelector<HTMLElement>(focusedCloudControl === "summary"
        ? ".moss-notice-cloud-files-toggle" : ".moss-notice-cloud-files-refresh")?.focus({ preventScroll: true });
    }
    this.noticesEl.scrollTop = scrollTop;
  }

  /**
   * The files a notice names, each one clickable into the editor.
   *
   * Every advisory that names a file names one the author has to go and edit —
   * a missing image reference, a misplaced stylesheet, a page that failed to
   * render. Reading the name and then hunting for it in the tree is work the
   * panel can do, so the name IS the way in. Plain text here would leave the
   * one obvious next action unavailable.
   *
   * `file` is the site-root-relative path an advisory names (`Advisory.item`
   * on the Rust side), not a bare filename — a nested source keeps its
   * directory. The button shows only the basename (the panel is dense; the
   * full path would crowd a row with several files) but opens and clicks
   * through the FULL path, via `data-tooltip` and the emitted event
   * respectively — never `title=` (UI Conventions).
   *
   * Buttons, not links: nothing navigates, and `.moss-link--subtle` resets the
   * user-agent button chrome so it still reads as text (UI Conventions).
   */
  private buildFileList(files: string[], className: string): HTMLElement {
    const wrap = document.createElement("span");
    wrap.className = className;
    files.forEach((file, i) => {
      if (i > 0) wrap.appendChild(document.createTextNode(", "));
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "moss-link--subtle moss-notice-file";
      btn.textContent = basename(file);
      btn.setAttribute("data-tooltip", file);
      btn.setAttribute("aria-label", t('progress.open_file', { file }));
      btn.addEventListener("click", () => {
        eventBus.emit<OpenFileInEditorEvent>(Events.OPEN_FILE_IN_EDITOR, { path: file });
      });
      wrap.appendChild(btn);
    });
    return wrap;
  }

  /**
   * Inline SVG chevron for the handle button — ONE down-pointing glyph;
   * the open (collapse) state rotates this same node 180° via `.is-open` in
   * CSS (morph, don't swap: design-vocabulary Motion).
   * Matches the nav arrow SVGs in index.html (same viewBox 0 0 24 24,
   * stroke-width 2, same path) so the chevron itself reads consistently with
   * the nav-pill arrows. The 16px width/height attributes set the size — the
   * handle keeps its own .moss-surface-handle chrome (it is NOT a full 36px
   * nav-button circle, which would be too heavy for a peek handle).
   */
  private _handleSvg(): string {
    return `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>`;
  }

  /**
   * Render the handle button at the bottom of the
   * panel. The handle is the ONLY disclosure control (replaces seam-click-to-pin
   * and the old minimize ⌄). Not called while minimized — the caller leaves the
   * existing handle in the collapsing box (see `_renderModel`).
   * Uses inline SVG chevron (UP = collapse, DOWN = keep-open).
   * An open panel — `pinned` OR auto-`surfaced` — shows the UP/collapse chevron
   * and aria-expanded=true; only a transient hover-`peek` shows DOWN/keep-open.
   */
  private renderHandle(reveal: PanelViewModel["reveal"]): void {
    if (!this.panelEl) return;

    // A surfaced panel is already open, so its handle COLLAPSES (same as pinned).
    // Only a peek (hover-open) offers keep-open.
    const showCollapse = reveal === "pinned" || reveal === "surfaced";
    const label = showCollapse ? t('progress.collapse') : t('progress.keep_open');

    if (!this.handleEl) {
      const btn = document.createElement("button");
      // The handle keeps its own .moss-surface-handle chrome; only the SVG
      // chevron inside it matches the nav arrows (see _handleSvg). The SVG's
      // width/height attributes set the glyph size — no nav-button circle here.
      btn.className = "moss-surface-handle";
      btn.type = "button";
      // No transition on the chevron rotation under reduced-motion
      if (prefersReducedMotion()) {
        btn.classList.add("reduced-motion");
      }
      btn.addEventListener("click", () => {
        this.onHandleToggle?.();
      });
      // Inline SVG chevron matching the nav arrows — written ONCE.
      btn.innerHTML = this._handleSvg();
      this.panelEl.appendChild(btn);
      this.handleEl = btn;
    }

    this.handleEl.classList.toggle("is-open", showCollapse);
    this.handleEl.setAttribute("aria-label", label);
    this.handleEl.setAttribute("aria-expanded", showCollapse ? "true" : "false");
  }


  /**
   * Render narrated task rows (non-preview tone==="narrated" tasks
   * from the narration pipeline). Called from ProgressPanel.renderPanel() in
   * the new model path so narrated tasks appear in the panel without needing
   * the legacy render() call.
   *
   * @param tasks   Active task rows to render.
   * @param isHidden Whether the panel body should be hidden. Defaults to false.
   *   Must match the current RevealState. Drives grow/retract if the panel was
   *   just created by this call.
   */
  renderNarrated(tasks: ProgressTask[], isHidden = false): void {
    if (tasks.length === 0) {
      // No tasks: tear down only if there is also no status line content AND no
      // notices content, so a status-only or notice-only panel (e.g. during a
      // build with advisories but no narrated tasks) is not destroyed.
      // The status line / notices presence drives the panel lifecycle in
      // the model path; task absence alone is not enough.
      const hasNotices = this.noticesEl && this.noticesEl.childElementCount > 0;
      if (this.panelEl && !this.statusBlock.isMounted() && !hasNotices) {
        this.panelEl.remove();
        this.panelEl = null;
        this.taskListEl = null;
        this.statusBlock.destroy();
        this.noticesEl = null;
        this.handleEl = null;
        this.taskEls.clear();
      }
      return;
    }

    // Ensure there's a panel to mount the rows in. The status line may have
    // already created it; if not, create one now.
    if (!this.panelEl) {
      this.createPanel();
    }
    if (!this.panelEl) return;

    // Ensure position is applied whenever we enter this path (the panel may
    // have been freshly created by this call). Visibility comes last — see
    // applyVisibility.
    this.updatePosition();

    // Ensure task list container exists.
    if (!this.taskListEl) {
      this.taskListEl = document.createElement("div");
      this.taskListEl.className = "moss-progress-task-list";
      this.panelEl.appendChild(this.taskListEl);
    }

    const activeIds = new Set(tasks.map((t) => t.id));

    // Remove rows that are no longer in the narrated set.
    Array.from(this.taskEls.entries()).forEach(([id, el]) => {
      if (!activeIds.has(id)) {
        el.remove();
        this.taskEls.delete(id);
      }
    });

    for (const task of tasks) {
      const existing = this.taskEls.get(task.id);
      if (existing) {
        this.updateTaskElement(existing, task);
      } else {
        const el = this.createTaskElement(task);
        this.taskEls.set(task.id, el);
        this.taskListEl.appendChild(el);
      }
    }

    this.applyVisibility(isHidden);
  }

  /**
   * Open or close the drawer by morphing its REAL height.
   * `scaleY` scaled the children, so every line of advisory text was squashed
   * for the full 300ms and un-squashed into place; with height + the panel's
   * `overflow:hidden` the text is at final size every frame and merely partly
   * revealed.
   *
   * `.closed` carries `visibility:hidden`, which CSS holds `visible` for the
   * whole transition and flips only at the end — that is the deferred
   * `display:none` the old `.retract`/`.hidden` pair needed a timer, a
   * transitionend handler and a rAF to fake. `morphHeight` owns re-entrancy and
   * the dropped-transitionend fallback, and returns without writing any style
   * under `prefers-reduced-motion`, so the class alone jump-cuts.
   *
   * Call this AFTER the content that decides the height is in the DOM.
   */
  private applyVisibility(isHidden: boolean): void {
    const el = this.panelEl;
    if (!el || el.classList.contains("closed") === isHidden) return;
    void morphHeight(el, () => el.classList.toggle("closed", isHidden));
  }

  /**
   * A copyable monospace command field for the `Action::Command`
   * affordance, showing `run` with a Copy control. Radius `6px` (bottom of the
   * ladder: 18 → 12 → 8 → 6). The Copy button writes `run` to the clipboard.
   */
  private buildCommandField(run: string): HTMLElement {
    const field = document.createElement("div");
    field.className = "moss-advisory-command";

    const code = document.createElement("code");
    code.className = "moss-advisory-command-text";
    code.textContent = run;
    field.appendChild(code);

    field.appendChild(createCopyButton({
      value: run,
      label: '',
      ariaLabel: t('editmisc.copy_command', { name: run }),
    }));

    return field;
  }

  /**
   * Render the recovery affordance for a Job's `Awaiting` interrupt or a Failed
   * advisory. `InApp` -> a button that dispatches the op
   * (e.g. the domain 403's "Open Settings"); `Command` -> the copyable field;
   * `Link` -> an anchor; `Acknowledge` -> a dismiss button; `None` -> nothing.
   * The UI names the intent and hands it to `dispatchAppOp`, which owns the
   * op -> effect table.
   */
  private buildActionAffordance(action: Action, noticeKey?: string): HTMLElement | null {
    if (action === "None") return null;
    if ("Acknowledge" in action) {
      // The only affordance that removes a notice rather than repairing what
      // it reports, so it needs the notice's identity. A task row has no key
      // and never carries an act-report.
      if (noticeKey === undefined) return null;
      const btn = document.createElement("button");
      btn.className = "moss-job-action moss-link--subtle";
      btn.type = "button";
      btn.textContent = action.Acknowledge.label;
      btn.addEventListener("click", () => {
        eventBus.emit<ClearNoticeEvent>(Events.CLEAR_NOTICE, { task: noticeKey });
      });
      return btn;
    }
    if ("Command" in action) {
      // Always use the localized "Copy" label — the producer's label is
      // informational only; "Copy" names the button's action.
      return this.buildCommandField(action.Command.run);
    }
    if ("InApp" in action) {
      const btn = document.createElement("button");
      btn.className = "moss-job-action moss-link--subtle";
      btn.type = "button";
      btn.textContent = action.InApp.label;
      const { op, args } = action.InApp;
      btn.addEventListener("click", () => dispatchAppOp(op, args));
      return btn;
    }
    if ("Link" in action) {
      const anchor = document.createElement("a");
      anchor.className = "moss-job-action moss-link--subtle";
      anchor.href = action.Link.href;
      anchor.target = "_blank";
      anchor.rel = "noopener noreferrer";
      anchor.textContent = action.Link.label;
      return anchor;
    }
    return null;
  }

  /** Reconcile the task row's action affordance to match `task.action`. */
  private reconcileTaskAction(el: HTMLElement, task: ProgressTask): void {
    el.querySelector(".moss-progress-task-action")?.remove();
    // Mark awaiting rows so their label may wrap (full degraded-notice text)
    // rather than clip to one ellipsised line. An awaiting task's directive is
    // a full sentence (e.g. "Couldn't connect mosspub.com — verify your email
    // in Settings"), unlike a running row's short label; the single-line
    // ellipsis at .moss-progress-task-label (style.css) truncated it. Set/clear
    // HERE — the one seam called from both buildTaskContent and
    // updateTaskElement — so an awaiting→terminal transition always clears it
    // and a later running/receipt row never inherits the wrap.
    if (task.action && task.action !== "None") {
      el.toggleAttribute("data-awaiting", true);
      const aff = this.buildActionAffordance(task.action);
      if (aff) {
        aff.classList.add("moss-progress-task-action");
        el.appendChild(aff);
      }
    } else {
      el.toggleAttribute("data-awaiting", false);
    }
  }

  destroy(): void {
    if (this.panelEl) {
      this.panelEl.remove();
      this.panelEl = null;
      this.taskListEl = null;
    }
    this.statusBlock.destroy();
    this.noticesEl = null;
    this.handleEl = null;
    this.taskEls.clear();
  }

  private createPanel(): void {
    this.panelEl = document.createElement("div");
    // Born closed: the height morph needs a zero start, and before this the
    // first reveal snapped open because nothing applied the collapsed state.
    this.panelEl.className = "moss-progress-panel closed";

    // Include the glass in the hover region so moving into it does not close the panel.
    // pointerenter/leave (not mouse*) so pen hover behaves the same.
    this.panelEl.addEventListener("pointerenter", () => {
      this.onPanelPointerEnter?.();
    });
    this.panelEl.addEventListener("pointerleave", () => {
      this.onPanelPointerLeave?.();
    });

    // Durable body hover + focus. These drive the
    // bodyHovered/bodyFocused suppressors in ProgressPanel so checkAutoClose
    // is suppressed while the pointer is on the glass or a descendant is focused.
    this.panelEl.addEventListener("pointerenter", () => {
      this.onBodyHoverEnter?.();
    });
    this.panelEl.addEventListener("pointerleave", () => {
      this.onBodyHoverLeave?.();
    });
    this.panelEl.addEventListener("focusin", () => {
      this.onBodyFocusIn?.();
    });
    this.panelEl.addEventListener("focusout", () => {
      this.onBodyFocusOut?.();
    });

    // The persistent seam and advisory dot live on the pill-sibling titlebar
    // layer, owned by `ProgressSeam`, so they outlive this panel's hide() and
    // teardown. The panel's square top meets that fixed seam.

    // Task list container
    this.taskListEl = document.createElement("div");
    this.taskListEl.className = "moss-progress-task-list";
    this.panelEl.appendChild(this.taskListEl);

    this.mountPanel();
  }

  /**
   * Mount the panel in the NON-CLIPPED titlebar layer (so it can grow from
   * UNDER the nav pill — `#moss-preview-container` is `overflow:hidden` and
   * clips upward, and its `--moss-safe-center-x` does NOT align under the pill).
   * Mirrors preview-hairline.ts. Falls back to the constructor container when
   * no titlebar/pill exists (jsdom unit tests + the E2E test-mode mount).
   */
  private mountPanel(): void {
    if (!this.panelEl) return;
    const cluster = document.querySelector<HTMLElement>(".moss-pill-cluster");
    if (cluster) {
      this.panelEl.setAttribute("data-anchor", "nav-pill");
      // Pure CSS placement inside the cluster (weld + centering live in the
      // [data-anchor="nav-pill"] rule). Nothing to measure, nothing to observe.
      cluster.appendChild(this.panelEl);
    } else {
      // Fallback (no shell chrome): legacy in-container mount.
      this.container.appendChild(this.panelEl);
    }
  }

  private createTaskElement(task: ProgressTask): HTMLElement {
    const el = document.createElement("div");
    el.className = "moss-progress-task";
    el.setAttribute("data-task-id", task.id);
    // A media sub-job nests under its Build parent — mark it so
    // the CSS indents it (`.moss-progress-task--nested`) and so structure tests
    // can assert the parent relationship via `data-parent`.
    this.applyTaskParent(el, task);
    this.applyTaskNarrate(el, task);

    // Build inner content
    this.buildTaskContent(el, task);

    return el;
  }

  /** Reflect a task's parent (nesting) onto its row element. */
  private applyTaskParent(el: HTMLElement, task: ProgressTask): void {
    if (task.parentId) {
      el.setAttribute("data-parent", task.parentId);
      el.classList.add("moss-progress-task--nested");
    } else {
      el.removeAttribute("data-parent");
      el.classList.remove("moss-progress-task--nested");
    }
  }

  /** Mark a row whose label is a stage SENTENCE (`ProgressTask.narrate`) so the
   * cascade wraps it within a budget instead of eliding it to one line — same
   * shape and same reason as the `[data-awaiting]` carve-out. */
  private applyTaskNarrate(el: HTMLElement, task: ProgressTask): void {
    if (task.narrate) el.setAttribute("data-narrate", "");
    else el.removeAttribute("data-narrate");
  }

  private updateTaskElement(el: HTMLElement, task: ProgressTask): void {
    // Keep the nesting marker in lockstep (a row can gain/lose
    // its parent across upserts — e.g. VideoConversionProgress arriving before vs
    // after the Build parent Job spawn).
    this.applyTaskParent(el, task);
    this.applyTaskNarrate(el, task);

    // Update label
    const labelEl = el.querySelector(".moss-progress-task-label");
    if (labelEl) {
      labelEl.textContent = task.label;
    }

    // Update detail
    let detailEl = el.querySelector(
      ".moss-progress-task-detail",
    ) as HTMLElement | null;
    if (task.detail) {
      if (detailEl) {
        detailEl.textContent = task.detail;
      } else {
        detailEl = document.createElement("span");
        detailEl.className = "moss-progress-task-detail";
        detailEl.textContent = task.detail;
        const infoRow = el.querySelector(".moss-progress-task-info");
        infoRow?.appendChild(detailEl);
      }
    } else if (detailEl) {
      detailEl.remove();
    }

    // Reconcile the Awaiting action affordance (Running -> Awaiting adds
    // the button; Awaiting -> terminal removes it).
    this.reconcileTaskAction(el, task);
  }

  private buildTaskContent(el: HTMLElement, task: ProgressTask): void {
    // Info row: label + optional detail. The row carries NO per-row progress
    // encoding — neither a fill BAR nor a STATE glyph (⟳/✓). The nav-pill
    // hairline is the sole progress encoding for every build; a per-row glyph
    // only duplicated it and, sitting at the row's right edge, read as an
    // interactive retry/refresh control it never was. An individual long task's
    // sub-progress is the textual `(N/M)` count carried in the label.
    const infoRow = document.createElement("div");
    infoRow.className = "moss-progress-task-info";

    const label = document.createElement("span");
    label.className = "moss-progress-task-label";
    label.textContent = task.label;
    infoRow.appendChild(label);

    if (task.detail) {
      const detail = document.createElement("span");
      detail.className = "moss-progress-task-detail";
      detail.textContent = task.detail;
      infoRow.appendChild(detail);
    }

    el.appendChild(infoRow);

    // The Awaiting interrupt's recovery affordance, below the info row.
    this.reconcileTaskAction(el, task);
  }

  private updatePosition(): void {
    if (!this.panelEl) return;
    this.panelEl.classList.add("placement-bottom");
  }
}
