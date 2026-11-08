/**
 * Unit tests for folder-mode.ts.
 *
 * Two layers exercised:
 *   - Pure helpers (`buildSiteHeaderHtml`, `buildCardsHtml`,
 *     `buildPopulatedBodyHtml`, `escapeHtml`, `FOLDER_MICROCOPY`,
 *     `FOLDER_CARDS`, `stubCardHandler`) — directly, no DOM mount needed.
 *   - `mountFolderMode` DOM behaviour via jsdom — empty vs populated
 *     state, card click wiring, reveal-button click, theme radio change,
 *     unmount tear-down.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../../i18n/index";
import {
  FOLDER_CARDS,
  FOLDER_MICROCOPY,
  buildCardsHtml,
  buildPopulatedBodyHtml,
  buildSiteHeaderHtml,
  escapeHtml,
  isFolderModeMounted,
  mountFolderMode,
  stubCardHandler,
  unmountFolderMode,
  type FolderCardKind,
} from "../folder-mode/folder-mode";
import { host } from "../../host";

// A host with starters, unless a test says otherwise.
host.starterPicker = { open: async () => {}, takeRequest: async () => false };

// ── FOLDER_MICROCOPY (spec contract) ─────────────────────────────────────

describe("FOLDER_MICROCOPY", () => {
  it("matches the spec verbatim", () => {
    // CHANGING THIS STRING REQUIRES A SPEC UPDATE — this assertion is the
    // wire that keeps copy and UI in lockstep.
    expect(FOLDER_MICROCOPY).toBe(
      "The folder name is your site name. Rename in Finder to change it.",
    );
  });
});

// ── FOLDER_CARDS (mockup contract) ───────────────────────────────────────

describe("FOLDER_CARDS", () => {
  it("has the four cards in spec-mockup order", () => {
    expect(FOLDER_CARDS.map((c) => c.kind)).toEqual([
      "start-writing",
      "try-example",
      "import-url",
      "connect-plugin",
    ]);
  });

  it("Start writing description is the home-file CTA", () => {
    // The "Start writing" card creates the site's home file (index.md).
    expect(t(FOLDER_CARDS[0].descKey)).toBe("Create home file");
  });
});

// ── escapeHtml ───────────────────────────────────────────────────────────

describe("escapeHtml", () => {
  it("escapes HTML special characters", () => {
    expect(escapeHtml("<script>alert('x')</script>")).toBe(
      "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;",
    );
    expect(escapeHtml('"&"')).toBe("&quot;&amp;&quot;");
  });

  it("leaves benign text untouched", () => {
    expect(escapeHtml("my-blog")).toBe("my-blog");
    expect(escapeHtml("hello world")).toBe("hello world");
  });
});

// ── buildSiteHeaderHtml ──────────────────────────────────────────────────

describe("buildSiteHeaderHtml", () => {
  it("renders the folder name as an h1", () => {
    const html = buildSiteHeaderHtml("my-blog");
    expect(html).toContain("<h1");
    expect(html).toContain("my-blog");
  });

  it("escapes the folder name", () => {
    const html = buildSiteHeaderHtml("<evil>");
    expect(html).toContain("&lt;evil&gt;");
    expect(html).not.toContain("<evil>");
  });

  it("includes the spec microcopy", () => {
    const html = buildSiteHeaderHtml("my-blog");
    expect(html).toContain(FOLDER_MICROCOPY);
  });

  it("uses data-tooltip for the reveal affordance (NOT native title)", () => {
    const html = buildSiteHeaderHtml("my-blog");
    expect(html).toContain("data-tooltip");
    // Per CLAUDE.md: native `title=` is forbidden. Verify none slipped in.
    expect(html).not.toMatch(/\btitle=/);
  });

  it("no longer renders theme radios — moon is the only control", () => {
    const html = buildSiteHeaderHtml("my-blog");
    expect(html).not.toContain('data-folder-mode-theme');
  });
});

// ── buildCardsHtml ───────────────────────────────────────────────────────

describe("buildCardsHtml", () => {
  it("renders all four cards", () => {
    const html = buildCardsHtml();
    for (const card of FOLDER_CARDS) {
      expect(html).toContain(`data-folder-mode-card="${card.kind}"`);
      expect(html).toContain(t(card.titleKey));
      expect(html).toContain(t(card.descKey));
    }
  });

  it("leaves out Try an example in a host without a starter picker", () => {
    const filled = host.starterPicker;
    delete host.starterPicker;
    const html = buildCardsHtml();
    host.starterPicker = filled;
    expect(html).not.toContain('data-folder-mode-card="try-example"');
    expect(html).toContain('data-folder-mode-card="import-url"');
  });

  it("omits the 'Start your site' divider (bare empty state)", () => {
    expect(buildCardsHtml()).not.toContain("Start your site");
  });

  it("renders the spec 'or start with' text between primary and secondary cards", () => {
    expect(buildCardsHtml()).toContain("or start with");
  });

  it("marks the primary card large and the secondary cards small", () => {
    const html = buildCardsHtml();
    expect(html).toContain("moss-folder-mode__card--large");
    expect(html).toContain("moss-folder-mode__card--small");
  });

  it("attaches no Phase-2 deferral tooltips", () => {
    const html = buildCardsHtml();
    const matches = html.match(/data-tooltip="Coming in Phase 2"/g) ?? [];
    expect(matches.length).toBe(0);
  });
});

// ── buildPopulatedBodyHtml ───────────────────────────────────────────────

describe("buildPopulatedBodyHtml", () => {
  it("renders a divider + body slot", () => {
    const html = buildPopulatedBodyHtml();
    expect(html).toContain("moss-folder-mode__divider");
    expect(html).toContain("data-folder-mode-body");
  });
});

// ── stubCardHandler ──────────────────────────────────────────────────────

describe("stubCardHandler", () => {
  beforeEach(() => {
    vi.spyOn(console, "log").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("logs the kind on every click", () => {
    const toast = vi.fn();
    stubCardHandler("start-writing", toast);
    expect(console.log).toHaveBeenCalledWith("[folder-mode] card clicked: start-writing");
  });

  it("does NOT toast for any card (real handlers in editor-main own the feedback)", () => {
    const toast = vi.fn();
    for (const kind of [
      "start-writing",
      "try-example",
      "import-url",
      "connect-plugin",
    ] as const) {
      stubCardHandler(kind, toast);
    }
    expect(toast).not.toHaveBeenCalled();
  });
});

// ── mountFolderMode (DOM behaviour in jsdom) ─────────────────────────────

describe("mountFolderMode (empty state)", () => {
  let host: HTMLDivElement;
  let revealSpy: ReturnType<typeof vi.fn>;
  let toastSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    document.body.innerHTML = "";
    host = document.createElement("div");
    document.body.appendChild(host);
    revealSpy = vi.fn().mockResolvedValue(null);
    toastSpy = vi.fn();
    vi.spyOn(console, "log").mockImplementation(() => {});
  });

  afterEach(() => {
    unmountFolderMode();
    vi.restoreAllMocks();
  });

  it("renders a bare empty folder: cards only, no site-header band", () => {
    mountFolderMode(host, {
      folderPath: "/Users/me/my-blog",
      folderName: "my-blog",
      isEmpty: true,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    expect(host.querySelector(".moss-folder-mode--empty")).not.toBeNull();
    expect(host.querySelector(".moss-folder-mode__cards")).not.toBeNull();
    // The empty state is deliberately bare — no header band (heading /
    // microcopy / theme toggle) above the four cards.
    expect(host.querySelector(".moss-folder-mode__header")).toBeNull();
    expect(host.querySelector(".moss-folder-mode__heading")).toBeNull();
    expect(host.querySelector(".moss-folder-mode__microcopy")).toBeNull();
    expect(host.querySelector(".moss-folder-mode__body")).toBeNull();
  });

  it("renders all four cards", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: revealSpy,
    });
    const cards = host.querySelectorAll("[data-folder-mode-card]");
    expect(cards.length).toBe(4);
  });

  it("clicking the reveal button calls revealEntry with the folder path", () => {
    // The reveal button lives in the site-header band, which now renders only
    // in the populated state (the empty state is bare cards).
    mountFolderMode(host, {
      folderPath: "/Users/me/my-blog",
      folderName: "my-blog",
      isEmpty: false,
      revealEntry: revealSpy,
    });
    const btn = host.querySelector<HTMLButtonElement>("[data-folder-mode-reveal]");
    expect(btn).not.toBeNull();
    btn!.click();
    expect(revealSpy).toHaveBeenCalledWith("/Users/me/my-blog");
  });

  it("clicking the URL card invokes the default stub (no toast, no throw)", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    const urlCard = host.querySelector<HTMLButtonElement>(
      '[data-folder-mode-card="import-url"]',
    );
    expect(urlCard).not.toBeNull();
    expect(() => urlCard!.click()).not.toThrow();
    // The default stub does not toast (editor-main wires real handlers in
    // editor-main; the bare folder-mode mount path is silent).
    expect(toastSpy).not.toHaveBeenCalled();
  });

  it("clicking the Plugin card invokes the default stub (no toast, no throw)", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    const pluginCard = host.querySelector<HTMLButtonElement>(
      '[data-folder-mode-card="connect-plugin"]',
    );
    expect(() => pluginCard!.click()).not.toThrow();
    expect(toastSpy).not.toHaveBeenCalled();
  });

  it("clicking Start writing does NOT toast", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    const startCard = host.querySelector<HTMLButtonElement>(
      '[data-folder-mode-card="start-writing"]',
    );
    startCard!.click();
    expect(toastSpy).not.toHaveBeenCalled();
  });

  it("clicking Try an example does NOT toast", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    const card = host.querySelector<HTMLButtonElement>(
      '[data-folder-mode-card="try-example"]',
    );
    card!.click();
    expect(toastSpy).not.toHaveBeenCalled();
  });

  it("invokes onCardClick when provided", () => {
    const onCardClick = vi.fn();
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      onCardClick,
      revealEntry: revealSpy,
      showToast: toastSpy,
    });
    const cards: FolderCardKind[] = [
      "start-writing",
      "try-example",
      "import-url",
      "connect-plugin",
    ];
    for (const kind of cards) {
      host
        .querySelector<HTMLButtonElement>(`[data-folder-mode-card="${kind}"]`)!
        .click();
    }
    expect(onCardClick).toHaveBeenCalledTimes(4);
    expect(onCardClick).toHaveBeenNthCalledWith(1, "start-writing");
    expect(onCardClick).toHaveBeenNthCalledWith(2, "try-example");
    expect(onCardClick).toHaveBeenNthCalledWith(3, "import-url");
    expect(onCardClick).toHaveBeenNthCalledWith(4, "connect-plugin");
    // When override is present, the toast stub should NOT fire.
    expect(toastSpy).not.toHaveBeenCalled();
  });
});

describe("mountFolderMode (populated state)", () => {
  let host: HTMLDivElement;
  let revealSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    document.body.innerHTML = "";
    host = document.createElement("div");
    document.body.appendChild(host);
    revealSpy = vi.fn().mockResolvedValue(null);
  });

  afterEach(() => {
    unmountFolderMode();
    vi.restoreAllMocks();
  });

  it("renders site-header + body slot (no cards) for a populated folder", () => {
    const { populatedBodyHost } = mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: false,
      revealEntry: revealSpy,
    });
    expect(host.querySelector(".moss-folder-mode--populated")).not.toBeNull();
    expect(host.querySelector(".moss-folder-mode__header")).not.toBeNull();
    expect(host.querySelector(".moss-folder-mode__cards")).toBeNull();
    expect(host.querySelector(".moss-folder-mode__body")).not.toBeNull();
    expect(populatedBodyHost).not.toBeNull();
    // populatedBodyHost matches the [data-folder-mode-body] slot.
    expect(populatedBodyHost?.hasAttribute("data-folder-mode-body")).toBe(true);
  });

  it("includes the site-header microcopy even when populated", () => {
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: false,
      revealEntry: revealSpy,
    });
    expect(host.querySelector(".moss-folder-mode__microcopy")?.textContent).toBe(
      FOLDER_MICROCOPY,
    );
  });
});

describe("unmountFolderMode", () => {
  it("clears the host and updates the mounted flag", () => {
    document.body.innerHTML = "";
    const host = document.createElement("div");
    document.body.appendChild(host);
    mountFolderMode(host, {
      folderPath: "/p",
      folderName: "p",
      isEmpty: true,
      revealEntry: vi.fn().mockResolvedValue(null),
    });
    expect(isFolderModeMounted()).toBe(true);
    unmountFolderMode();
    expect(isFolderModeMounted()).toBe(false);
    expect(host.innerHTML).toBe("");
  });

  it("is idempotent (second call is a no-op)", () => {
    unmountFolderMode();
    expect(isFolderModeMounted()).toBe(false);
    // Should not throw.
    unmountFolderMode();
  });
});
