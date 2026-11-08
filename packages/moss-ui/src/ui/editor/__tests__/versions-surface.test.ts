/**
 * Versions surface tests — screen one (list, save, empty state) of the
 * Versions panel.
 * Scope
 * follows the click (no in-panel toggle — the switching-scope test is
 * deleted, not adapted, since the control it drove no longer exists), the
 * "+" save control replaces the always-open field, no day grouping or
 * "Current" row, no footer.
 *
 * Data commands are mocked at the engine client (packages/moss-ui/src/ui/engine.ts),
 * the one way the panel reaches them.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";

const {
  mockListVersions,
  mockSaveVersion,
  mockReadVersion,
  mockRestoreVersion,
} = vi.hoisted(() => ({
  mockListVersions: vi.fn(),
  mockSaveVersion: vi.fn(),
  mockReadVersion: vi.fn(),
  mockRestoreVersion: vi.fn(),
}));

vi.mock("../../engine", () => ({
  engine: {
    listVersions: mockListVersions,
    saveVersion: mockSaveVersion,
    readVersion: mockReadVersion,
    restoreVersion: mockRestoreVersion,
  },
}));

const mockOnExit = vi.fn();
const mockSetTitleBar = vi.fn();

/** The most recent content handed to `ctx.setTitleBar` — where the back
 *  crumb, subject and Restore button all live now.
 *  The breadcrumb
 *  strip itself is mocked away in this suite (a bare root div, no
 *  `.nav-bc-strip`), so this is the only place those elements exist. */
function titleBar(): HTMLElement {
  return mockSetTitleBar.mock.calls.at(-1)![0] as HTMLElement;
}

import { renderVersionsSurface } from "../versions-surface";
import type { VersionRow, VersionList, ChangedPage } from "@symbiosis-lab/moss-ui";

const drain = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

function ok<T>(data: T): { status: "ok"; data: T } {
  return { status: "ok", data };
}

function row(overrides: Partial<VersionRow> = {}): VersionRow {
  return {
    id: "v1",
    published_at: new Date(2026, 8, 13, 14, 2).toISOString(),
    target: "moss",
    trigger: "publish",
    label: null,
    changed: 0,
    added: 0,
    removed: 0,
    ...overrides,
  };
}

function list(rows: VersionRow[], overrides: Partial<VersionList> = {}): VersionList {
  return {
    rows,
    live_id: null,
    has_multiple_targets: false,
    store_dir: "/tmp/store",
    changed_since: [],
    unchanged: [],
    ...overrides,
  };
}

function changedPage(overrides: Partial<ChangedPage> = {}): ChangedPage {
  return { source_path: "notes/harbor.md", verb: "edited", ...overrides };
}

function mount(scope: "page" | "folder" | "site" = "page", path = "notes/harbor.md", title?: string) {
  const root = document.createElement("div");
  document.body.appendChild(root);
  const handle = renderVersionsSurface(root, {
    scope,
    path,
    title: title ?? (scope === "page" ? path.split("/").pop()! : "blog"),
    setTitleBar: mockSetTitleBar,
    onExit: mockOnExit,
  });
  return { root, handle };
}

/** Opens the "+"/cancel toggle's field, standing in for the click a real
 *  save does — every save-flow test needs this first now that the field
 *  isn't always open. The toggle is one persistent button.
 *  Not a "+" that gets torn down. */
function openSaveControl(root: HTMLElement): void {
  root.querySelector<HTMLButtonElement>(".moss-versions__save-toggle")!.click();
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 8, 13, 16, 0));
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  // A restore-confirm modal renders into document.body, outside `root` — a
  // modal a previous test left behind must not shadow the next one's (same
  // cleanup delete-confirm-modal.test.ts uses, for the same reason).
  document.querySelectorAll(".moss-modal, .moss-modal-backdrop").forEach((n) => n.remove());
});

describe("renderVersionsSurface", () => {
  test("renders newest first with a full date+time per row, Live on the live row", async () => {
    const manual = row({
      id: "v-manual",
      published_at: new Date(2026, 8, 13, 15, 10).toISOString(),
      trigger: "manual",
      label: "Before the launch edit",
    });
    const publishToday = row({ id: "v-pub-today", published_at: new Date(2026, 8, 13, 14, 2).toISOString() });
    const publishYesterday = row({ id: "v-pub-yday", published_at: new Date(2026, 8, 12, 9, 31).toISOString() });
    mockListVersions.mockResolvedValueOnce(
      ok(list([manual, publishToday, publishYesterday], { live_id: "v-pub-today" })),
    );

    const { root } = mount();
    await drain();

    // No more day headings or a "Current" row — every row is one of these.
    const ids = Array.from(root.querySelectorAll(".moss-versions__row")).map((el) => (el as HTMLElement).dataset.id);
    expect(ids).toEqual(["v-manual", "v-pub-today", "v-pub-yday"]);

    const liveRow = root.querySelector('[data-id="v-pub-today"]')!;
    expect(liveRow.querySelector(".moss-versions__badge")?.textContent).toBe("Live");
    expect(liveRow.querySelector(".moss-versions__time")?.textContent).toBe("Sep 13, 14:02");
    expect(root.querySelector('[data-id="v-pub-yday"] .moss-versions__time')?.textContent).toBe("Sep 12, 09:31");
  });

  test("page scope renders exactly the rows the command returned and derives nothing", async () => {
    const rows = [row({ id: "a" }), row({ id: "b", published_at: new Date(2026, 8, 12, 9, 0).toISOString() })];
    mockListVersions.mockResolvedValueOnce(ok(list(rows)));

    const { root } = mount("page", "notes/harbor.md");
    await drain();

    expect(mockListVersions).toHaveBeenCalledWith("page", "notes/harbor.md");
    expect(root.querySelectorAll(".moss-versions__row").length).toBe(rows.length);
  });

  test("folder scope passes the folder's own path through, never null", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([])));
    mount("folder", "文字/纽约诸法门", "纽约诸法门");
    await drain();

    // Before this redesign, any non-page scope silently discarded its path
    // and always asked for the whole site — this is the fix.
    expect(mockListVersions).toHaveBeenCalledWith("folder", "文字/纽约诸法门");
  });

  test("site scope opens on the root path", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([])));
    mount("site", "", "blog");
    await drain();

    expect(mockListVersions).toHaveBeenCalledWith("site", "");
  });

  test("empty state shows the two lines and the save control still offers a +", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([])));

    const { root } = mount();
    await drain();

    const empty = root.querySelector(".moss-versions__empty")!;
    expect(empty.querySelector(".moss-versions__empty-title")?.textContent).toBe("No versions yet.");
    expect(empty.querySelector(".moss-versions__empty-body")?.textContent).toContain(
      "moss saves one every time you publish",
    );
    const toggle = root.querySelector<HTMLButtonElement>(".moss-versions__save-toggle")!;
    expect(toggle).toBeTruthy();
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(root.querySelector(".moss-versions__save-field")).toBeFalsy();
  });

  test("the toggle morphs into a field; Escape cancels without writing anything", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([])));
    const { root } = mount();
    await drain();

    const toggle = root.querySelector<HTMLButtonElement>(".moss-versions__save-toggle")!;
    expect(root.querySelector(".moss-versions__save-field")).toBeFalsy();
    openSaveControl(root);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const field = root.querySelector<HTMLInputElement>(".moss-versions__save-field")!;
    expect(field).toBeTruthy();

    field.value = "abandoned";
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(root.querySelector(".moss-versions__save-field")).toBeFalsy();
    // Same persistent button, morphed back — not torn down and replaced.
    expect(root.querySelector(".moss-versions__save-toggle")).toBe(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(mockSaveVersion).not.toHaveBeenCalled();
  });

  test("the toggle's glyph is an SVG stroke morph (two paths whose `d` changes), never a text-character swap", async () => {
    // Reduced motion: the tween paints its END pose and resolves at once
    // (same trick control-panel-sync.test.ts's glyph test uses) — the frames
    // between are a render gate's question, this test is about the shape.
    vi.stubGlobal("matchMedia", (q: string) => ({
      matches: q.includes("reduced-motion"), media: q,
      addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
      onchange: null, dispatchEvent: () => false,
    }));
    mockListVersions.mockResolvedValueOnce(ok(list([])));
    const { root } = mount();
    await drain();

    const toggle = root.querySelector<HTMLButtonElement>(".moss-versions__save-toggle")!;
    const marks = () => [...toggle.querySelectorAll(".moss-add-glyph__mk")].map((p) => p.getAttribute("d"));
    expect(toggle.querySelectorAll(".moss-add-glyph__mk").length).toBe(2);
    expect(toggle.textContent?.trim()).toBe(""); // no "+"/"×" text character anywhere
    const plus = marks();

    openSaveControl(root);
    const cross = marks();
    expect(cross).not.toEqual(plus);
    // Both strokes are still drawn — a cross is the plus turned, not one
    // bar hidden and another revealed.
    expect(cross.every((d) => d && d.includes("L"))).toBe(true);

    toggle.click();
    expect(marks()).toEqual(plus); // byte-identical on the way back
  });

  test("confirming the + calls saveVersion with the trimmed name or null and prepends the row", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([row({ id: "existing" })])));
    const { root } = mount();
    await drain();

    const toggle = root.querySelector<HTMLButtonElement>(".moss-versions__save-toggle")!;
    openSaveControl(root);
    const field = root.querySelector<HTMLInputElement>(".moss-versions__save-field")!;
    field.value = "  My Version  ";
    const newRow = row({
      id: "new-1",
      trigger: "manual",
      label: "My Version",
      published_at: new Date(2026, 8, 13, 16, 0).toISOString(),
    });
    mockSaveVersion.mockResolvedValueOnce(ok(newRow));
    root.querySelector<HTMLButtonElement>(".moss-versions__save-confirm")!.click();
    await drain();

    expect(mockSaveVersion).toHaveBeenCalledWith("My Version");
    const names = Array.from(root.querySelectorAll(".moss-versions__name")).map((n) => n.textContent);
    expect(names[0]).toBe("My Version");
    // Back to "+" — the control doesn't stay open across a confirm, and it's
    // the SAME persistent toggle button, morphed, not a new one.
    expect(root.querySelector(".moss-versions__save-toggle")).toBe(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");

    openSaveControl(root);
    mockSaveVersion.mockResolvedValueOnce(ok(row({ id: "new-2", trigger: "manual", label: null })));
    root.querySelector<HTMLButtonElement>(".moss-versions__save-confirm")!.click();
    await drain();
    expect(mockSaveVersion).toHaveBeenLastCalledWith(null);
  });

  // The "‹ Editor" exit link and the subject are built by this surface and
  // handed to `ctx.setTitleBar` (round six) — mocked here as `titleBar()`,
  // since the real strip/mode layer lives in tree/render.ts, outside what
  // this file's mount() harness renders (a bare root div). See
  // tree/__tests__/controller.test.ts's "FileTree.setTitleBar" suite for the
  // mount/crossfade coverage, and editor-main-versions.test.ts for the
  // open()/unmount() wiring.

  test("the caption names the scope; the subject itself lives in the title bar", async () => {
    // ctx.title no longer renders anywhere in the surface's own markup — it
    // is in the title bar now (titleBar()). Only the scope caption is this
    // surface's own DOM to assert here.
    mockListVersions.mockResolvedValueOnce(ok(list([])));
    const { root: pageRoot } = mount("page", "notes/harbor.md");
    await drain();
    expect(pageRoot.querySelector(".moss-versions__caption")?.textContent).toBe("Versions");
    // ".md" stays — the title bar has no file/folder icon, so the extension
    // is the only visible indicator of the file type.
    expect(titleBar().querySelector(".mode-subject-name")?.textContent).toBe("harbor.md");

    mockListVersions.mockResolvedValueOnce(ok(list([])));
    const { root: folderRoot } = mount("folder", "文字/纽约诸法门", "纽约诸法门");
    await drain();
    expect(folderRoot.querySelector(".moss-versions__caption")?.textContent).toBe("Versions");
    expect(titleBar().querySelector(".mode-subject-name")?.textContent).toBe("纽约诸法门");

    mockListVersions.mockResolvedValueOnce(ok(list([])));
    const { root: siteRoot } = mount("site", "", "blog");
    await drain();
    expect(siteRoot.querySelector(".moss-versions__caption")?.textContent).toBe("Site versions");
    // Site scope: just the bold site name, no path prefix.
    expect(titleBar().querySelector(".mode-subject")?.textContent).toBe("blog");
  });
});

describe("renderVersionsSurface — screen two, a page's version", () => {
  test("a page version shows the source read-only and Restore calls restoreVersion in_place then exits with the undo id", async () => {
    const publishRow = row({ id: "v1", published_at: new Date(2026, 8, 13, 14, 2).toISOString() });
    mockListVersions.mockResolvedValueOnce(ok(list([publishRow], { live_id: "v1" })));
    const { root } = mount("page", "notes/harbor.md");
    await drain();

    mockReadVersion.mockResolvedValueOnce(ok({ kept: true, text: "# Notes from the harbor" }));
    root.querySelector<HTMLElement>('[data-id="v1"]')!.click();
    await drain();

    expect(mockReadVersion).toHaveBeenCalledWith("v1", "notes/harbor.md");
    expect(root.querySelector(".moss-versions__source")?.textContent).toBe("# Notes from the harbor");
    // v1 is the cached list's live_id — a "Live" badge renders inline beside
    // the version's own moment/label text in the title bar.
    // The badge reuses the list row's own pattern.
    expect(titleBar().querySelector(".mode-subject .moss-versions__badge")?.textContent).toBe("Live");

    mockRestoreVersion.mockResolvedValueOnce(
      ok({ restored: ["notes/harbor.md"], failed: [], trashed: [], undo_id: "undo-1" }),
    );
    titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!.click();
    await drain();

    expect(mockRestoreVersion).toHaveBeenCalledWith("v1", "notes/harbor.md", "in_place");
    expect(mockOnExit).toHaveBeenCalledWith(
      { undoId: "undo-1", path: "notes/harbor.md", message: "Restored harbor.md from Sep 13, 14:02" },
    );
  });

  test("Restore sits in the title bar beside the moment text, with the explanation as its tooltip — no visible note line", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([row({ id: "v1" })])));
    const { root } = mount("page", "notes/harbor.md");
    await drain();

    mockReadVersion.mockResolvedValueOnce(ok({ kept: true, text: "# Notes" }));
    root.querySelector<HTMLElement>('[data-id="v1"]')!.click();
    await drain();

    const bar = titleBar();
    const restoreBtn = bar.querySelector<HTMLButtonElement>(".moss-versions__restore")!;
    expect(restoreBtn).toBeTruthy(); // same title bar as the subject, not a caption row below it
    expect(bar.querySelector(".mode-subject")).not.toBeNull();
    expect(restoreBtn.textContent).toBe('Restore "harbor.md"');
    expect(restoreBtn.getAttribute("data-tooltip")).toBe(
      "Restoring saves a version of the page as it is now, so you can undo.",
    );
    // The note used to render as its own line; it's tooltip-only content now.
    expect(root.querySelector(".moss-versions__detail-note")).toBeFalsy();
  });

  test("a version whose bytes were not kept disables Restore and shows the reason", async () => {
    mockListVersions.mockResolvedValueOnce(ok(list([row({ id: "v1" })])));
    const { root } = mount("page", "notes/harbor.md");
    await drain();

    mockReadVersion.mockResolvedValueOnce(ok({ kept: false, text: null }));
    root.querySelector<HTMLElement>('[data-id="v1"]')!.click();
    await drain();

    expect(root.querySelector(".moss-versions__not-kept")?.textContent).toBe(
      "The bytes of this file were not kept, so this version cannot be restored.",
    );
    expect(titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!.disabled).toBe(true);
    expect(root.querySelector(".moss-versions__source")).toBeFalsy();
  });
});

describe("renderVersionsSurface — screen three, a site version", () => {
  /** Mounts in site scope, lands on the site-version screen for `siteRow`,
   *  with `listVersions`'s drill-down call resolved to `detail`. */
  async function openSiteVersion(siteRow: VersionRow, detail: Partial<VersionList> = {}) {
    mockListVersions.mockResolvedValueOnce(ok(list([siteRow])));
    const { root } = mount("site", "", "blog");
    await drain();

    mockListVersions.mockResolvedValueOnce(ok(list([], detail)));
    root.querySelector<HTMLElement>(`[data-id="${siteRow.id}"]`)!.click();
    await drain();
    return { root };
  }

  test("a site version lists Changed since then and Unchanged from the command", async () => {
    const siteRow = row({ id: "s1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    const { root } = await openSiteVersion(siteRow, {
      changed_since: [
        changedPage({ source_path: "notes/harbor.md", verb: "edited" }),
        changedPage({ source_path: "new-page.md", verb: "added" }),
        changedPage({ source_path: "about.md", verb: "deleted" }),
      ],
      unchanged: ["home.md"],
    });

    expect(mockListVersions).toHaveBeenLastCalledWith("site", "s1");
    const groups = Array.from(root.querySelectorAll(".moss-versions__group")).map((g) => g.textContent);
    expect(groups).toEqual(["Changed since then", "Unchanged"]);

    const subs = Array.from(root.querySelectorAll(".moss-versions__row .moss-versions__by")).map((n) => n.textContent);
    expect(subs).toEqual(["edited since", "added since", "deleted since"]);

    const names = Array.from(root.querySelectorAll(".moss-versions__row .moss-versions__name")).map(
      (n) => n.textContent,
    );
    expect(names).toEqual(["harbor.md", "new-page.md", "about.md", "home.md"]);
  });

  test("a row within a site version opens that page's version with the path set", async () => {
    const siteRow = row({ id: "s1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    const { root } = await openSiteVersion(siteRow, {
      changed_since: [changedPage({ source_path: "notes/harbor.md", verb: "edited" })],
      unchanged: ["home.md"],
    });

    mockReadVersion.mockResolvedValueOnce(ok({ kept: true, text: "old content" }));
    root.querySelector<HTMLElement>(".moss-versions__row")!.click();
    await drain();

    // Same version id as the site version — the page is read AT that moment.
    expect(mockReadVersion).toHaveBeenCalledWith("s1", "notes/harbor.md");
    expect(root.querySelector(".moss-versions__source")?.textContent).toBe("old content");
  });

  test("Restore site opens the modal with the counts; confirming calls restoreVersion with a null path", async () => {
    const siteRow = row({ id: "s1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    const { root } = await openSiteVersion(siteRow, {
      changed_since: [
        changedPage({ source_path: "a.md", verb: "edited" }),
        changedPage({ source_path: "b.md", verb: "added" }),
        changedPage({ source_path: "c.md", verb: "deleted" }),
      ],
      unchanged: [],
    });

    // Same title bar as page scope, and the note is a tooltip, not a line.
    const siteRestoreBtn = titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!;
    expect(siteRestoreBtn.getAttribute("data-tooltip")).toBe(
      "Every page goes back to that moment. Saved as a version first, so you can undo.",
    );
    expect(root.querySelector(".moss-versions__detail-note")).toBeFalsy();

    siteRestoreBtn.click();

    expect(document.querySelector(".moss-modal-title")?.textContent).toBe("Restore the whole site to Sep 13, 11:20?");
    // 3 changed_since entries, 1 of them "added" (created after this moment,
    // so it is trashed rather than reverted) — pages that "go back" excludes it.
    expect(document.querySelector(".moss-modal-body")?.textContent).toBe(
      "2 pages go back to how they were then. 1 pages added since then move to the Trash. Everything as it is now is saved as a version first, so you can undo.",
    );

    mockRestoreVersion.mockResolvedValueOnce(
      ok({ restored: ["a.md", "c.md"], failed: [], trashed: ["b.md"], undo_id: "undo-9" }),
    );
    document.querySelector<HTMLButtonElement>('[data-action="confirm"]')!.click();
    await drain();

    expect(mockRestoreVersion).toHaveBeenCalledWith("s1", null, "in_place");
    expect(mockOnExit).toHaveBeenCalledWith(
      { undoId: "undo-9", path: null, message: "Restored Whole site from Sep 13, 11:20" },
    );
  });

  test("cancelling the modal calls nothing", async () => {
    const siteRow = row({ id: "s1" });
    await openSiteVersion(siteRow, { changed_since: [changedPage()], unchanged: [] });

    titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!.click();
    document.querySelector<HTMLButtonElement>('[data-action="cancel"]')!.click();
    await drain();

    expect(mockRestoreVersion).not.toHaveBeenCalled();
    expect(mockOnExit).not.toHaveBeenCalled();
  });

  test("a site restore that failed on some pages exits with the partial count", async () => {
    const siteRow = row({ id: "s1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    await openSiteVersion(siteRow, { changed_since: [changedPage(), changedPage({ source_path: "b.md" })], unchanged: [] });

    titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!.click();
    mockRestoreVersion.mockResolvedValueOnce(
      ok({ restored: ["a.md"], failed: [{ path: "b.md", error: "locked" }], trashed: [], undo_id: "undo-7" }),
    );
    document.querySelector<HTMLButtonElement>('[data-action="confirm"]')!.click();
    await drain();

    expect(mockOnExit).toHaveBeenCalledWith(
      { undoId: "undo-7", path: null, message: "Restored 1 of 2 pages" },
    );
  });
});

describe("renderVersionsSurface — folder scope", () => {
  /** Mounts in folder scope, lands on the folder-version screen for
   *  `folderRow`, with the drill-down (always literal scope 'site' on the
   *  wire — file header, "Two backend gaps") resolved to `detail`. */
  async function openFolderVersion(folderRow: VersionRow, detail: Partial<VersionList> = {}) {
    mockListVersions.mockResolvedValueOnce(ok(list([folderRow])));
    const { root } = mount("folder", "文字/纽约诸法门", "纽约诸法门");
    await drain();

    mockListVersions.mockResolvedValueOnce(ok(list([], detail)));
    root.querySelector<HTMLElement>(`[data-id="${folderRow.id}"]`)!.click();
    await drain();
    return { root };
  }

  test("the drill-down still asks the wire's existing shape (scope 'site'), and the surface filters to the folder itself", async () => {
    const folderRow = row({ id: "f1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    const { root } = await openFolderVersion(folderRow, {
      changed_since: [
        changedPage({ source_path: "文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md", verb: "edited" }),
        changedPage({ source_path: "项目/moss.md", verb: "edited" }), // outside the folder
      ],
      unchanged: ["文字/纽约诸法门/冬至的饺子.md", "项目/indie-reader.md"], // second entry outside the folder
    });

    expect(mockListVersions).toHaveBeenLastCalledWith("site", "f1");
    const names = Array.from(root.querySelectorAll(".moss-versions__row .moss-versions__name")).map(
      (n) => n.textContent,
    );
    expect(names).toEqual(["藏传佛教寺庙里的瑜伽.md", "冬至的饺子.md"]);
  });

  test("restoring a folder version passes the folder's own path, never null, and names the folder", async () => {
    const folderRow = row({ id: "f1", published_at: new Date(2026, 8, 13, 11, 20).toISOString() });
    await openFolderVersion(folderRow, {
      changed_since: [changedPage({ source_path: "文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md", verb: "edited" })],
      unchanged: [],
    });

    const restoreBtn = titleBar().querySelector<HTMLButtonElement>(".moss-versions__restore")!;
    expect(restoreBtn.getAttribute("data-tooltip")).toBe(
      "The pages in this folder go back to that moment; everything outside it is untouched. Saved as a version first, so you can undo.",
    );
    restoreBtn.click();
    expect(document.querySelector(".moss-modal-title")?.textContent).toBe('Restore "纽约诸法门" to Sep 13, 11:20?');

    mockRestoreVersion.mockResolvedValueOnce(
      ok({ restored: ["文字/纽约诸法门/藏传佛教寺庙里的瑜伽.md"], failed: [], trashed: [], undo_id: "undo-f1" }),
    );
    document.querySelector<HTMLButtonElement>('[data-action="confirm"]')!.click();
    await drain();

    // Never null: a folder restore must not fall through to a whole-site one.
    expect(mockRestoreVersion).toHaveBeenCalledWith("f1", "文字/纽约诸法门", "in_place");
    expect(mockOnExit).toHaveBeenCalledWith(
      { undoId: "undo-f1", path: null, message: 'Restored 纽约诸法门 from Sep 13, 11:20' },
    );
  });
});
