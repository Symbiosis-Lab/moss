/**
 * Tests for ProgressPanelRenderer
 * Tests DOM rendering, placement, task management, and close button behavior.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ProgressPanelRenderer } from "../progress-panel-renderer";
import { ProgressSeam } from "../progress-seam";
import { groupNotices } from "../progress-panel-stage";
import type { ProgressTask, AdvisoryEntry, PanelViewModel, NoticeVM } from "../progress-panel";
import { eventBus, Events, type OpenFileInEditorEvent } from "../../../context/event-bus";

// Helper to create a ProgressTask with defaults
function createTask(overrides: Partial<ProgressTask> = {}): ProgressTask {
  return {
    id: "task-1",
    label: "Test Task",
    progress: 50,
    source: "moss",
    ...overrides,
  };
}

// Helper to build a minimal PanelViewModel
function vm(overrides: Partial<PanelViewModel> = {}): PanelViewModel {
  return {
    status: { text: "", kind: "none" },
    notices: [],
    reveal: "surfaced",
    topSeverity: "none",
    ...overrides,
  };
}

describe("ProgressPanelRenderer", () => {
  let container: HTMLElement;
  let renderer: ProgressPanelRenderer;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    renderer = new ProgressPanelRenderer(container);
    // No setCallbacks() — deleted. setHandleCallbacks/setHoverCallbacks are the API.
  });

  afterEach(() => {
    renderer.destroy();
    document.body.removeChild(container);
  });

  describe("Panel Creation and Destruction", () => {
    it("creates panel element when a running status is rendered", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel");
      expect(panel).toBeTruthy();
    });

    it("creates panel element when notices are present", () => {
      renderer.render(vm({
        notices: [{ key: "ffmpeg", severity: "broken", reason: "FFmpeg missing", files: [], hiddenFiles: [] }],
        status: { text: "Building", kind: "running" },
      }));
      expect(container.querySelector(".moss-progress-panel")).toBeTruthy();
    });

    it("creates panel element when narrated task is rendered", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Syncing" })]);
      expect(container.querySelector(".moss-progress-panel")).toBeTruthy();
    });

    it("removes panel element when no narrated tasks remain and no status", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1" })]);
      expect(container.querySelector(".moss-progress-panel")).toBeTruthy();

      renderer.renderNarrated([]);
      expect(container.querySelector(".moss-progress-panel")).toBeNull();
    });

    // The ▾/▴ handle
    // is the only disclosure control.
    it("has no panel-scoped close button", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const closeBtn = container.querySelector(".moss-close-button");
      expect(closeBtn).toBeNull();
    });

    it("creates task list container for narrated tasks", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1" })]);
      const taskList = container.querySelector(".moss-progress-task-list");
      expect(taskList).toBeTruthy();
    });

    it("cleans up on destroy", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      expect(container.querySelector(".moss-progress-panel")).toBeTruthy();

      renderer.destroy();
      expect(container.querySelector(".moss-progress-panel")).toBeNull();
    });
  });

  describe("Placement", () => {
    it("applies bottom placement class", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel") as HTMLElement;
      expect(panel.classList.contains("placement-bottom")).toBe(true);
      expect(panel.classList.contains("placement-center")).toBe(false);
    });
  });

  describe("Closed State", () => {
    it("`.closed` tracks the reveal, and a freshly created panel starts closed", () => {
      // Born closed, so the first open is a morph rather than a snap.
      renderer.renderNarrated([createTask({ id: "narrated:1" })], true /* isHidden */);
      const panelEl = container.querySelector(".moss-progress-panel") as HTMLElement;
      expect(panelEl.classList.contains("closed")).toBe(true);

      renderer.renderNarrated([createTask({ id: "narrated:1" })], false);
      expect(panelEl.classList.contains("closed")).toBe(false);
    });
  });

  describe("Task Rendering (via renderNarrated)", () => {
    it("renders narrated task with label and NO per-row progress encoding (bar OR glyph)", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Deploying", progress: 50 })]);

      const label = container.querySelector(".moss-progress-task-label");
      expect(label?.textContent).toBe("Deploying");

      // The nav-pill hairline is the SOLE progress encoding — a row carries
      // neither a per-row fill nor a per-row spinner/state glyph (the glyph
      // duplicated the hairline and read as an interactive control).
      expect(container.querySelector(".moss-progress-bar-fill")).toBeNull();
      expect(container.querySelector(".moss-progress-bar-track")).toBeNull();
      expect(container.querySelector(".moss-progress-task-state")).toBeNull();
    });

    it("renders no state glyph when progress is null", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", progress: null })]);

      expect(container.querySelector(".moss-progress-bar-fill")).toBeNull();
      expect(container.querySelector(".moss-progress-task-state")).toBeNull();
    });

    it("renders no state glyph when progress reaches 100", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Done", progress: 100 })]);

      const label = container.querySelector(".moss-progress-task-label");
      expect(label?.textContent).toBe("Done");
      expect(container.querySelector(".moss-progress-bar-fill")).toBeNull();
      expect(container.querySelector(".moss-progress-task-state")).toBeNull();
    });

    it("renders the textual (N/M) count carried in the label, never a fill", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Converting videos (3/6)", progress: 50 })]);
      const label = container.querySelector(".moss-progress-task-label");
      expect(label?.textContent).toBe("Converting videos (3/6)");
      expect(container.querySelector(".moss-progress-bar-fill")).toBeNull();
    });

    it("renders task detail when provided", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", detail: "Step 2/5" })]);
      const detail = container.querySelector(".moss-progress-task-detail");
      expect(detail?.textContent).toBe("Step 2/5");
    });

    it("awaiting row (has an action) gets data-awaiting so its full directive can wrap, and clears on transition", () => {
      // An awaiting Job's label is a full-sentence directive (a degraded
      // notice), not a short progress label — data-awaiting lets CSS wrap it
      // instead of clipping to one ellipsised line.
      const action = { InApp: { op: "SignIn", args: null, label: "Open Settings" } } as const;
      const directive = "Couldn't connect mosspub.com — verify your email in Settings";
      renderer.renderNarrated([createTask({ id: "await:1", label: directive, action })]);
      const row = container.querySelector(".moss-progress-task");
      expect(row?.hasAttribute("data-awaiting")).toBe(true);
      // Full directive on the label span (never sliced) — wrapping shows it whole.
      expect(container.querySelector(".moss-progress-task-label")?.textContent).toBe(directive);

      // Re-render the SAME id with no action → data-awaiting must CLEAR, so a later
      // running/receipt row never inherits the label wrap.
      renderer.renderNarrated([createTask({ id: "await:1", label: "Deploying" })]);
      expect(container.querySelector(".moss-progress-task")?.hasAttribute("data-awaiting")).toBe(false);
    });

    it("updates existing narrated task element on re-render (label only, no bar, no glyph)", () => {
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Step 1", progress: 25 })]);
      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Step 2", progress: 100 })]);

      const label = container.querySelector(".moss-progress-task-label");
      expect(label?.textContent).toBe("Step 2");

      expect(container.querySelector(".moss-progress-bar-fill")).toBeNull();
      expect(container.querySelector(".moss-progress-task-state")).toBeNull();
    });

    it("removes narrated task elements that are no longer active", () => {
      renderer.renderNarrated([
        createTask({ id: "narrated:1", label: "Task A" }),
        createTask({ id: "narrated:2", label: "Task B" }),
      ]);
      expect(container.querySelectorAll(".moss-progress-task").length).toBe(2);

      renderer.renderNarrated([createTask({ id: "narrated:1", label: "Task A" })]);
      expect(container.querySelectorAll(".moss-progress-task").length).toBe(1);
    });
  });

  describe("The seam and advisory dot persist independently of panel children", () => {
    it("the renderer does not create a seam inside the panel", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel")!;
      expect(panel.querySelector(".moss-progress-seam")).toBeNull();
    });

    it("the renderer does not create an advisory dot inside the panel", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel")!;
      expect(panel.querySelector(".moss-advisory-dot")).toBeNull();
    });

    it("the advisory dot survives panel TEARDOWN (it's a ProgressSeam sibling, not a panel child)", () => {
      const titlebar = document.createElement("div");
      titlebar.className = "moss-titlebar";
      titlebar.innerHTML = `<div class="moss-pill-cluster"><div class="moss-nav-pill"></div></div>`;
      document.body.appendChild(titlebar);

      const seam = new ProgressSeam();
      seam.setTopSeverity('broken');
      expect(titlebar.querySelector(".moss-severity-dot")).not.toBeNull();

      const r2 = new ProgressPanelRenderer(titlebar);
      r2.renderNarrated([createTask({ id: "narrated:1" })]);
      expect(document.querySelector(".moss-progress-panel")).toBeTruthy();

      r2.renderNarrated([]); // teardown: panel removed
      expect(document.querySelector(".moss-progress-panel")).toBeNull();

      // The dot persists — it was never the panel's child.
      expect(titlebar.querySelector(".moss-severity-dot")).not.toBeNull();

      r2.destroy();
      seam.destroy();
      titlebar.remove();
    });

    it("the advisory dot survives panel hide() (renderer marks panel .closed, dot untouched)", () => {
      const titlebar = document.createElement("div");
      titlebar.className = "moss-titlebar";
      titlebar.innerHTML = `<div class="moss-pill-cluster"><div class="moss-nav-pill"></div></div>`;
      document.body.appendChild(titlebar);

      const seam = new ProgressSeam();
      seam.setTopSeverity('degraded');

      const r2 = new ProgressPanelRenderer(titlebar);
      r2.renderNarrated([createTask({ id: "narrated:1" })], true /* isHidden */);
      // The dot is unaffected by the panel's hidden state.
      expect(titlebar.querySelector(".moss-severity-dot")).not.toBeNull();

      r2.destroy();
      seam.destroy();
      titlebar.remove();
    });

    it("anchored panel mounts INSIDE the pill cluster with no inline geometry", () => {
      const titlebar = document.createElement("div");
      titlebar.className = "moss-titlebar";
      titlebar.innerHTML = `<div class="moss-pill-cluster"><div class="moss-nav-pill"></div></div>`;
      document.body.appendChild(titlebar);

      const r2 = new ProgressPanelRenderer(titlebar);
      r2.render(vm({ status: { text: "Building", kind: "running" } }));

      const panel = document.querySelector<HTMLElement>(".moss-progress-panel")!;
      expect(panel.parentElement?.classList.contains("moss-pill-cluster")).toBe(true);
      expect(panel.getAttribute("data-anchor")).toBe("nav-pill");
      expect(panel.style.left).toBe("");
      expect(panel.style.top).toBe("");

      r2.destroy();
      titlebar.remove();
    });
  });

  describe("No Drag Behavior", () => {
    it("does not have drag state properties", () => {
      expect((renderer as any).isDragging).toBeUndefined();
      expect((renderer as any).dragStartX).toBeUndefined();
      expect((renderer as any).dragStartY).toBeUndefined();
    });

    it("panel mousedown in bottom placement does not initiate dragging", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel") as HTMLElement;

      panel.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true,
        cancelable: true,
        clientX: 100,
        clientY: 100,
      }));

      // No dragging class should appear
      expect(panel.classList.contains("dragging")).toBe(false);
      expect(document.body.classList.contains("progress-panel-dragging")).toBe(false);
    });

    it("does not persist position to localStorage", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      expect(localStorage.getItem("moss-progress-panel-position")).toBeNull();
    });

    it("does not add user-positioned class", () => {
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const panel = container.querySelector(".moss-progress-panel") as HTMLElement;
      expect(panel.classList.contains("user-positioned")).toBe(false);
    });
  });

  describe("Advisory notices via notices rail", () => {
    // Build a PanelViewModel from raw AdvisoryEntry[] using groupNotices.
    function vmFromAdvisories(advisories: AdvisoryEntry[]): PanelViewModel {
      return vm({
        status: { text: "Building", kind: "running" },
        notices: groupNotices(advisories),
      });
    }

    const adv = (item: string | null, reason: string, task = "videos", scope?: AdvisoryEntry["scope"]): AdvisoryEntry => ({
      task,
      item,
      reason,
      severity: "ShippedDegraded",
      scope,
    });

    it("per-file advisories render as .moss-notice rows (not .moss-advisory-zone)", () => {
      const reason = "still downloading from iCloud, will optimize on next preview";
      renderer.render(vmFromAdvisories([
        adv("aimeili.MOV", reason),
        adv("Winter-Song.mov", reason),
        adv("Ave-Maria.mov", reason),
      ]));

      // New path: notices rail, not advisory zone.
      expect(container.querySelector(".moss-advisory-zone")).toBeNull();
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBeGreaterThan(0);
      // groupNotices collapses same-reason per-file advisories into one notice
      // with files listed inline.
      const noticeText = Array.from(notices).map((n) => n.textContent ?? "").join(" ");
      expect(noticeText).toContain(reason);
    });

    it("groupNotices collapse: same-reason files → one notice listing files inline", () => {
      const reason = "still downloading from iCloud, will optimize on next preview";
      renderer.render(vmFromAdvisories([
        adv("a.mov", reason),
        adv("b.mov", reason),
      ]));

      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(1);
      // Files are listed inline in .moss-notice-files
      const filesEl = notices[0].querySelector(".moss-notice-files");
      expect(filesEl).not.toBeNull();
      const filesText = filesEl!.textContent ?? "";
      expect(filesText).toContain("a.mov");
      expect(filesText).toContain("b.mov");
    });

    it("a nested file is the way into the file — the button shows the name, but opens the full path", () => {
      // Reading a filename and then hunting for it in the tree is work the panel
      // can do. Plain text here would name the problem and withhold the fix.
      // `item` is the site-relative path, not a bare filename — a nested source
      // (e.g. under an album folder) must still open at its real location.
      const opened: string[] = [];
      const off = eventBus.on<OpenFileInEditorEvent>(
        Events.OPEN_FILE_IN_EDITOR,
        (e) => opened.push(e.path),
      );
      renderer.render(vmFromAdvisories([
        adv("图片/摄影/x.jpg", "2 files are missing"),
      ]));

      const btn = container.querySelector("button.moss-notice-file") as HTMLButtonElement | null;
      expect(btn).not.toBeNull();
      expect(btn!.textContent).toBe("x.jpg");
      expect(btn!.getAttribute("data-tooltip")).toBe("图片/摄影/x.jpg");
      // .moss-link--subtle is what resets the user-agent button chrome, so the
      // name still reads as text rather than as a form control.
      expect(btn!.classList.contains("moss-link--subtle")).toBe(true);

      btn!.click();
      expect(opened).toEqual(["图片/摄影/x.jpg"]);
      off();
    });

    it("every revealed +N filename is clickable too, not just the first six", () => {
      const opened: string[] = [];
      const off = eventBus.on<OpenFileInEditorEvent>(
        Events.OPEN_FILE_IN_EDITOR,
        (e) => opened.push(e.path),
      );
      const reason = "still downloading from iCloud";
      renderer.render(vmFromAdvisories([
        adv("a.mov", reason), adv("b.mov", reason), adv("c.mov", reason),
        adv("d.mov", reason), adv("e.mov", reason), adv("f.mov", reason),
        adv("g.mov", reason),
      ]));

      (container.querySelector("button.moss-notice-more") as HTMLButtonElement).click();
      const revealed = container.querySelector(".moss-notice-files--revealed")!;
      const hidden = revealed.querySelector("button.moss-notice-file") as HTMLButtonElement;
      hidden.click();
      expect(opened).toEqual(["g.mov"]);
      off();
    });

    it("different-reason advisories render as separate notices", () => {
      renderer.render(vmFromAdvisories([
        adv("a.MOV", "reason X"),
        adv("c.MOV", "reason Y (unique)"),
      ]));
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(2);
    });

    it("build-wide advisory (item null) renders as a broken .moss-notice", () => {
      renderer.render(vmFromAdvisories([
        { task: "videos", item: null, reason: "FFmpeg isn't installed", severity: "Blocking" },
      ]));
      const dot = container.querySelector(".moss-severity-dot--broken");
      expect(dot).not.toBeNull();
      const notice = container.querySelector(".moss-notice");
      expect(notice!.textContent).toContain("FFmpeg isn't installed");
    });

    it("NeedsAction + scope:File → renders as broken notice (.moss-severity-dot--broken)", () => {
      // noticeTier maps NeedsAction+File to "broken" — verify this flows through
      // groupNotices → vmFromAdvisories → renderNotices → .moss-severity-dot--broken.
      // A broken file-scoped NeedsAction is a per-file link/asset that needs the
      // user (e.g. a missing asset at a specific path).
      renderer.render(vmFromAdvisories([
        {
          task: "images",
          item: "assets/header.png",
          reason: "missing source file",
          severity: "NeedsAction",
          scope: "File",
        },
      ]));
      // Must be broken (not degraded) because NeedsAction+File → broken.
      const brokenDot = container.querySelector(".moss-severity-dot--broken");
      expect(brokenDot).not.toBeNull();
      const degradedDot = container.querySelector(".moss-severity-dot--degraded");
      expect(degradedDot).toBeNull();
      const notice = container.querySelector(".moss-notice");
      expect(notice!.textContent).toContain("missing source file");
    });

    it("+N hiddenFiles → .moss-notice-more button renders AND clicking it reveals the hidden filenames", () => {
      // FILES_BUDGET=6: feed 9 files of the SAME (task, reason) so groupNotices
      // puts 6 inline and 3 in hiddenFiles. Fewer than 7 means hiddenFiles is
      // empty and the +N button is never emitted (the previous test was trivially
      // passing with only 5 files).
      const reason = "iCloud downloading";
      renderer.render(vmFromAdvisories([
        adv("a.mov", reason), adv("b.mov", reason), adv("c.mov", reason),
        adv("d.mov", reason), adv("e.mov", reason), adv("f.mov", reason),
        adv("g.mov", reason), adv("h.mov", reason), adv("i.mov", reason),
      ]));

      // One collapsed notice (same task+reason).
      const notices = container.querySelectorAll(".moss-notice");
      expect(notices.length).toBe(1);

      // The "+3 more" button must be present (FILES_BUDGET=6, 9 files → 3 hidden).
      const moreBtn = container.querySelector("button.moss-notice-more") as HTMLButtonElement | null;
      expect(moreBtn).not.toBeNull();
      expect(moreBtn!.textContent).toContain("+3");

      // Clicking reveals the hidden filenames inline.
      moreBtn!.click();

      // The button should be replaced by a span listing the hidden files.
      expect(container.querySelector("button.moss-notice-more")).toBeNull();
      const revealedSpan = container.querySelector(".moss-notice-files--revealed");
      expect(revealedSpan).not.toBeNull();
      // All three hidden filenames must appear in the revealed span.
      const revealedText = revealedSpan!.textContent ?? "";
      expect(revealedText).toContain("g.mov");
      expect(revealedText).toContain("h.mov");
      expect(revealedText).toContain("i.mov");
    });
  });

  // renderStatusLine + renderNarrated
  describe("renderStatusLine + renderNarrated", () => {
    it("render() with running status emits .moss-status-line[role=status][aria-live=polite]", () => {
      renderer.render(vm({
        status: { text: "Generating pages", count: "24 / 47", kind: "running" },
      }));
      const line = container.querySelector(".moss-status-line");
      expect(line).not.toBeNull();
      expect(line!.getAttribute("role")).toBe("status");
      expect(line!.getAttribute("aria-live")).toBe("polite");
      expect(line!.getAttribute("aria-atomic")).toBe("true");
      expect(line!.textContent).toContain("Generating pages");
    });

    it("render() with failed status emits role=alert and --failed class", () => {
      renderer.render(vm({
        status: { text: "Publish failed", kind: "failed" },
      }));
      const line = container.querySelector(".moss-status-line");
      expect(line).not.toBeNull();
      expect(line!.getAttribute("role")).toBe("alert");
      expect(line!.classList.contains("moss-status-line--failed")).toBe(true);
    });

    it("render() running status with subject renders subject span", () => {
      renderer.render(vm({
        status: { text: "Optimizing", subject: "intro.mov", count: "3 / 12", kind: "running" },
      }));
      const line = container.querySelector(".moss-status-line");
      expect(line!.textContent).toContain("Optimizing");
      expect(line!.textContent).toContain("intro.mov");
      expect(line!.textContent).toContain("3 / 12");
    });

    it("count span carries aria-hidden=true to suppress tick-level AT noise", () => {
      renderer.render(vm({
        status: { text: "Generating", count: "12 / 47", kind: "running" },
      }));
      const countSpan = container.querySelector(".moss-status-line__count");
      expect(countSpan).not.toBeNull();
      expect(countSpan!.getAttribute("aria-hidden")).toBe("true");
    });

    it("second render with SAME content does not replace verb span node (no spurious AT)", () => {
      // A childList mutation on an aria-live region fires re-announcement even
      // when text is unchanged. The cached-span path must update .textContent
      // in place, so the node reference is STABLE across identical renders.
      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const verb1 = container.querySelector(".moss-status-line__verb");
      expect(verb1).not.toBeNull();

      renderer.render(vm({ status: { text: "Building", kind: "running" } }));
      const verb2 = container.querySelector(".moss-status-line__verb");

      // Same DOM node — NOT a new element.
      expect(verb2).toBe(verb1);
    });

    it("receipt status with clear:true sets data-clear='true'", () => {
      renderer.render(vm({ status: { text: "Built · 47 pages", kind: "receipt", clear: true } }));
      const line = container.querySelector(".moss-status-line");
      expect(line).not.toBeNull();
      expect(line!.getAttribute("data-kind")).toBe("receipt");
      expect(line!.getAttribute("data-clear")).toBe("true");
    });

    it("receipt status with clear:false sets data-clear='false'", () => {
      renderer.render(vm({ status: { text: "Built · 47 pages", kind: "receipt", clear: false } }));
      const line = container.querySelector(".moss-status-line");
      expect(line).not.toBeNull();
      expect(line!.getAttribute("data-kind")).toBe("receipt");
      expect(line!.getAttribute("data-clear")).toBe("false");
    });

    it("renderNarrated([narratedTask]) still emits a .moss-progress-task row", () => {
      const task: ProgressTask = {
        id: "narrated:1",
        label: "Syncing to Matters",
        progress: null,
        source: "moss",
      };
      renderer.renderNarrated([task]);
      const rows = container.querySelectorAll(".moss-progress-task");
      expect(rows.length).toBeGreaterThan(0);
      const labels = Array.from(rows).map((r) => r.querySelector(".moss-progress-task-label")?.textContent ?? "");
      expect(labels.some((l) => l.includes("Syncing to Matters"))).toBe(true);
    });
  });

  // renderNotices
  describe("renderNotices — notices rail", () => {
    it("shows full Unicode cloud paths in a collapsible disclosure and preserves focus", async () => {
      const request = vi.fn();
      renderer.setCloudFilesCallback(request);
      const notice: NoticeVM = {
        key: "cloud-sync-unavailable::Couldn’t read 12 files",
        severity: "broken",
        reason: "Couldn’t read 12 files",
        files: [], hiddenFiles: [],
        cloudFiles: { count: 12, status: "idle", files: [] },
      };
      renderer.render(vm({ notices: [notice] }));
      const summary = container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!;
      expect(summary.textContent).toContain("12");
      expect(container.querySelector(".moss-notice-cloud-files")?.hasAttribute("open")).toBe(false);
      summary.focus();
      summary.click();
      await vi.waitFor(() => expect(request).toHaveBeenCalledOnce());
      notice.cloudFiles = {
        count: 12, status: "loaded", files: ["长目录/記事/🌿-long file name.md"],
      };
      renderer.render(vm({ notices: [notice] }));
      expect(container.querySelector(".moss-notice-cloud-files li")?.textContent)
        .toBe("长目录/記事/🌿-long file name.md");
      expect(document.activeElement).toBe(container.querySelector(".moss-notice-cloud-files-toggle"));
      expect(container.querySelector(".moss-notice-cloud-files")?.hasAttribute("open")).toBe(true);
      container.querySelector<HTMLElement>(".moss-notice-cloud-files-toggle")!.click();
      await vi.waitFor(() => expect(container.querySelector(".moss-notice-cloud-files")?.hasAttribute("open")).toBe(false));
    });

    it("keeps the retry hint visible and uses one disclosure for an unknown legacy count", () => {
      renderer.render(vm({ notices: [{
        key: "cloud-sync-unavailable::unknown", severity: "broken",
        reason: "Couldn’t read some files", detail: "moss will retry. Examples: a.md.",
        files: [], hiddenFiles: [],
        cloudFiles: { count: null, status: "idle", files: [] },
      }] }));
      expect(container.querySelector(".moss-notice-cloud-hint")?.textContent)
        .toContain("moss will retry");
      expect(container.querySelectorAll(".moss-notice-detail")).toHaveLength(0);
      expect(container.querySelectorAll(".moss-notice-cloud-files")).toHaveLength(1);
      expect(container.querySelector(".moss-notice-cloud-files-toggle")?.textContent)
        .toBe("Show files");
    });

    it("ignores a queued toggle from a disclosure removed by reset or destroy", () => {
      const request = vi.fn();
      renderer.setCloudFilesCallback(request);
      const notice: NoticeVM = {
        key: "cloud-sync-unavailable::one", severity: "broken", reason: "Couldn’t read one file",
        files: [], hiddenFiles: [], cloudFiles: { count: 1, status: "idle", files: [] },
      };
      renderer.render(vm({ notices: [notice] }));
      const stale = container.querySelector<HTMLDetailsElement>(".moss-notice-cloud-files")!;
      renderer.resetCloudFilesDisclosure();
      expect(stale.isConnected).toBe(false);
      stale.open = true;
      stale.dispatchEvent(new Event("toggle"));
      expect(request).not.toHaveBeenCalled();
      expect(renderer.isCloudFilesOpen()).toBe(false);
      renderer.render(vm({ notices: [notice] }));
      const destroyed = container.querySelector<HTMLDetailsElement>(".moss-notice-cloud-files")!;
      renderer.destroy();
      destroyed.open = true;
      destroyed.dispatchEvent(new Event("toggle"));
      expect(request).not.toHaveBeenCalled();
      expect(renderer.isCloudFilesOpen()).toBe(false);
    });

    it("keeps the full list nodes and rail scroll position across an unchanged supervisor render", () => {
      const files = Array.from({ length: 180 }, (_, index) => `文章/長いフォルダー/${index}.md`);
      const notice: NoticeVM = {
        key: "cloud-sync-unavailable::180", severity: "broken", reason: "Couldn’t read 180 files",
        files: [], hiddenFiles: [], cloudFiles: { count: 180, status: "loaded", files },
      };
      renderer.render(vm({ notices: [notice] }));
      const rail = container.querySelector<HTMLElement>(".moss-notices")!;
      const list = rail.querySelector(".moss-notice-cloud-files ul");
      rail.scrollTop = 87;
      renderer.render(vm({ notices: [notice] }));
      expect(rail.scrollTop).toBe(87);
      expect(rail.querySelector(".moss-notice-cloud-files ul")).toBe(list);
    });
    // Most cases here don't care about notice identity, so the helper mints a
    // positional key; a case that DOES care (disclosure state across renders)
    // passes its own.
    function vmN(input: Array<Omit<NoticeVM, "key"> & { key?: string }>): PanelViewModel {
      const notices: NoticeVM[] = input.map((n, i) => ({ key: n.key ?? `n${i}`, ...n }));
      return {
        status: { text: "Building", kind: "running" },
        notices,
        reveal: "surfaced",
        topSeverity: notices.some((n) => n.severity === "broken") ? "broken" : "none",
      };
    }

    it("broken notice → .moss-severity-dot--broken + reason-first text", () => {
      renderer.render(vmN([{
        severity: "broken",
        reason: "FFmpeg not found",
        files: [],
        hiddenFiles: [],
      }]));
      const dot = container.querySelector(".moss-severity-dot--broken");
      expect(dot).not.toBeNull();
      expect(dot!.getAttribute("aria-hidden")).toBe("true");
      const notice = container.querySelector(".moss-notice");
      expect(notice).not.toBeNull();
      expect(notice!.textContent).toContain("FFmpeg not found");
    });

    it("degraded notice → .moss-severity-dot--degraded", () => {
      renderer.render(vmN([{
        severity: "degraded",
        reason: "Still downloading from iCloud",
        files: [],
        hiddenFiles: [],
      }]));
      const dot = container.querySelector(".moss-severity-dot--degraded");
      expect(dot).not.toBeNull();
    });

    // (A "+N button renders with the right count" case used to sit here. It is
    // strictly contained by the grouping-level test above — which asserts the
    // count AND the click reveal from real advisories — and by the
    // survives-a-re-render case below.)

    it("no hiddenFiles → no .moss-notice-more button", () => {
      renderer.render(vmN([{
        severity: "broken",
        reason: "Missing file",
        files: ["a.mov"],
        hiddenFiles: [],
      }]));
      expect(container.querySelector("button.moss-notice-more")).toBeNull();
    });

    it("command → .moss-advisory-command with Copy button labelled exactly 'Copy'", () => {
      renderer.render(vmN([{
        severity: "degraded",
        reason: "FFmpeg missing",
        files: [],
        hiddenFiles: [],
        command: { run: "brew install ffmpeg" },
      }]));
      const cmdEl = container.querySelector(".moss-advisory-command");
      expect(cmdEl).not.toBeNull();
      const copyBtn = cmdEl!.querySelector(".moss-copy-btn");
      expect(copyBtn).not.toBeNull();
      // Icon-only mode: the canonical copy button has no text label visible
      // (label:'') — the copy icon SVG is present
      expect(copyBtn!.querySelector('.moss-copy-btn__icon--copy')).not.toBeNull();
    });

    it("action:InApp → button.moss-job-action whose click calls dispatchAppOp", () => {
      const action = { InApp: { op: "SignIn", args: null, label: "Open Settings" } } as const;
      renderer.render(vmN([{
        severity: "degraded",
        reason: "Sign in required",
        files: [],
        hiddenFiles: [],
        action,
      }]));
      const btn = container.querySelector("button.moss-job-action");
      expect(btn).not.toBeNull();
      expect(btn!.textContent).toBe("Open Settings");
    });

    it("action:Acknowledge → a dismiss button that names the notice's own key", async () => {
      // The one affordance that removes the notice instead of repairing what it
      // reports. It has to carry the notice key, because that is the only
      // identity the panel can clear a grouped notice by.
      const { eventBus, Events } = await import("../../../context/event-bus");
      const seen: string[] = [];
      const off = eventBus.on(Events.CLEAR_NOTICE, (e: { task: string }) => seen.push(e.task));
      renderer.render(vmN([{
        key: "markdown::two copies share one set of comments",
        severity: "degraded",
        reason: "two copies share one set of comments",
        files: [],
        hiddenFiles: [],
        action: { Acknowledge: { label: "I’ve checked" } },
      }]));
      const btn = container.querySelector<HTMLButtonElement>("button.moss-job-action");
      expect(btn).not.toBeNull();
      expect(btn!.textContent).toBe("I’ve checked");
      btn!.click();
      expect(seen).toEqual(["markdown::two copies share one set of comments"]);
      off();
    });

    it("detail → an expandable region inside the notice", () => {
      renderer.render(vmN([{
        severity: "broken",
        reason: "Preview server failed",
        files: [],
        hiddenFiles: [],
        detail: "EADDRINUSE: address already in use",
      }]));
      const notice = container.querySelector(".moss-notice");
      expect(notice!.textContent).toContain("EADDRINUSE");
    });

    // A cloud-sync stall re-renders the rail every 2 seconds. The
    // rail is rebuilt wholesale, so any disclosure state living in the discarded
    // elements would snap shut on the next tick. Expanding the detail would
    // be impossible. State is keyed off NoticeVM.key instead.
    describe("disclosure state survives the 2s re-render", () => {
      const stalled = {
        key: "sync::Still downloading",
        severity: "degraded" as const,
        reason: "Still downloading",
        files: ["a.png", "b.png", "c.png"],
        hiddenFiles: ["d.png", "e.png"],
        detail: "247 files are still in the cloud",
      };

      it("an opened detail is still open after the next render", () => {
        renderer.render(vmN([stalled]));
        const detail = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        expect(detail.open).toBe(false);
        detail.open = true;
        detail.dispatchEvent(new Event("toggle"));

        renderer.render(vmN([stalled]));
        const after = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        expect(after).not.toBe(detail); // genuinely a fresh element
        expect(after.open).toBe(true);
      });

      it("a detail the user closed stays closed", () => {
        renderer.render(vmN([stalled]));
        const detail = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        detail.open = true;
        detail.dispatchEvent(new Event("toggle"));
        detail.open = false;
        detail.dispatchEvent(new Event("toggle"));

        renderer.render(vmN([stalled]));
        const after = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        expect(after.open).toBe(false);
      });

      it("a revealed +N list stays revealed after the next render", () => {
        renderer.render(vmN([stalled]));
        (container.querySelector("button.moss-notice-more") as HTMLButtonElement).click();
        expect(container.querySelector(".moss-notice-files--revealed")!.textContent)
          .toContain("d.png");

        renderer.render(vmN([stalled]));
        expect(container.querySelector("button.moss-notice-more")).toBeNull();
        expect(container.querySelector(".moss-notice-files--revealed")!.textContent)
          .toContain("e.png");
      });

      it("a notice that goes away does not resurrect its open state later", () => {
        renderer.render(vmN([stalled]));
        const detail = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        detail.open = true;
        detail.dispatchEvent(new Event("toggle"));

        // Sync finishes: the notice disappears, then a later unrelated stall
        // reuses the same key. It should start collapsed, like any new notice.
        renderer.render(vmN([{
          key: "other::Something else",
          severity: "degraded",
          reason: "Something else",
          files: [],
          hiddenFiles: [],
        }]));
        renderer.render(vmN([stalled]));
        const after = container.querySelector("details.moss-notice-detail") as HTMLDetailsElement;
        expect(after.open).toBe(false);
      });
    });

    it("accessible name leads with the tier word (broken/degraded)", () => {
      renderer.render(vmN([{
        severity: "broken",
        reason: "FFmpeg not found",
        files: [],
        hiddenFiles: [],
      }]));
      const notice = container.querySelector(".moss-notice");
      const dot = notice!.querySelector(".moss-severity-dot--broken");
      expect(dot).not.toBeNull();
    });

    it("notice with files has a ' · ' text node connector between reason and files", () => {
      // The connector must be a real DOM text node (not a CSS ::before) so that
      // screen readers include it in the accessible name, producing
      // "reason · files" rather than running the two together.
      renderer.render(vmN([{
        severity: "broken",
        reason: "Not found",
        files: ["hero.png"],
        hiddenFiles: [],
      }]));
      const notice = container.querySelector(".moss-notice")!;
      // Collect all child nodes (including text nodes) of the notice row.
      const childNodes = Array.from(notice.childNodes);
      // Find the text node that carries the connector.
      const connector = childNodes.find(
        (n) => n.nodeType === Node.TEXT_NODE && n.textContent?.includes("·"),
      );
      expect(connector).not.toBeUndefined();
      expect(connector!.textContent).toBe(" · ");
    });

    it("notice WITHOUT files has no ' · ' connector", () => {
      renderer.render(vmN([{
        severity: "broken",
        reason: "FFmpeg missing",
        files: [],
        hiddenFiles: [],
      }]));
      const notice = container.querySelector(".moss-notice")!;
      const childNodes = Array.from(notice.childNodes);
      const connector = childNodes.find(
        (n) => n.nodeType === Node.TEXT_NODE && n.textContent?.includes("·"),
      );
      expect(connector).toBeUndefined();
    });
  });

  // Handle button renderHandle + setHandleCallbacks
  describe("renderHandle — bottom handle button", () => {
    function vmH(reveal: PanelViewModel["reveal"]): PanelViewModel {
      return {
        status: { text: "Building", kind: "running" },
        notices: [],
        reveal,
        topSeverity: "none",
      };
    }

    it("pinned → button.moss-surface-handle[aria-expanded=true][aria-label=Collapse]", () => {
      renderer.render(vmH("pinned"));
      const btn = container.querySelector("button.moss-surface-handle");
      expect(btn).not.toBeNull();
      expect(btn!.getAttribute("aria-expanded")).toBe("true");
      expect(btn!.getAttribute("aria-label")).toBe("Collapse");
    });

    it("peek → button.moss-surface-handle[aria-expanded=false][aria-label='Keep open']", () => {
      renderer.render(vmH("peek"));
      const btn = container.querySelector("button.moss-surface-handle");
      expect(btn).not.toBeNull();
      expect(btn!.getAttribute("aria-expanded")).toBe("false");
      expect(btn!.getAttribute("aria-label")).toBe("Keep open");
    });

    it("surfaced → button.moss-surface-handle[aria-expanded=true][aria-label=Collapse]", () => {
      // A surfaced panel is already open — its handle collapses (like pinned),
      // not keep-open. Only a transient hover-peek offers keep-open.
      renderer.render(vmH("surfaced"));
      const btn = container.querySelector("button.moss-surface-handle");
      expect(btn).not.toBeNull();
      expect(btn!.getAttribute("aria-expanded")).toBe("true");
      expect(btn!.getAttribute("aria-label")).toBe("Collapse");
      // Chevron poses UP via the rotation class, same as pinned.
      expect(btn!.classList.contains("is-open")).toBe(true);
    });

    it("minimized → no handle on a panel that never opened, and the open panel KEEPS its handle when it closes", () => {
      renderer.render(vmH("minimized"));
      expect(container.querySelector("button.moss-surface-handle")).toBeNull();

      // Closing must not shorten the box before the height morph measures it:
      // the handle used to be removed ahead of applyVisibility, so a close
      // snapped down by the handle's height and animated only the rest. It
      // stays, clipped by the zero-height `overflow:hidden` panel.
      renderer.render(vmH("pinned"));
      const open = container.querySelector("button.moss-surface-handle");
      renderer.render(vmH("minimized"));
      expect(container.querySelector("button.moss-surface-handle")).toBe(open);
    });

    it("morph, don't swap: ONE persistent chevron node — open state is a rotation class", () => {
      // The open/closed states are poses of the same SVG (CSS rotates it via
      // .is-open); rewriting the markup per state would jump-cut the morph.
      renderer.render(vmH("peek"));
      const btn = container.querySelector("button.moss-surface-handle")!;
      const svg = btn.querySelector("svg");
      expect(svg).not.toBeNull();
      expect(btn.classList.contains("is-open")).toBe(false);

      renderer.render(vmH("pinned"));
      const btn2 = container.querySelector("button.moss-surface-handle")!;
      expect(btn2.querySelector("svg")).toBe(svg); // same node, new pose
      expect(btn2.classList.contains("is-open")).toBe(true);

      renderer.render(vmH("peek"));
      expect(btn2.querySelector("svg")).toBe(svg);
      expect(btn2.classList.contains("is-open")).toBe(false);
    });

    it("setHandleCallbacks: click when peek → calls onToggle (should pin)", () => {
      const onToggle = vi.fn();
      renderer.setHandleCallbacks(onToggle);
      renderer.render(vmH("peek"));
      const btn = container.querySelector("button.moss-surface-handle") as HTMLButtonElement;
      expect(btn).not.toBeNull();
      btn.click();
      expect(onToggle).toHaveBeenCalledTimes(1);
    });

    it("setHandleCallbacks: click when pinned → calls onToggle (should hide)", () => {
      const onToggle = vi.fn();
      renderer.setHandleCallbacks(onToggle);
      renderer.render(vmH("pinned"));
      const btn = container.querySelector("button.moss-surface-handle") as HTMLButtonElement;
      expect(btn).not.toBeNull();
      btn.click();
      expect(onToggle).toHaveBeenCalledTimes(1);
    });
  });

});
