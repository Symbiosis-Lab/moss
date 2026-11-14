/**
 * Unit tests for folder-mode-handlers.
 *
 * Each card handler is exercised directly with `vi.mock`'d Tauri command
 * surface so the test runs in jsdom without a real Tauri runtime. The
 * `mountFolderMode` shell is exercised via the public `makeCardClickHandler`
 * path so we cover the full click→handler chain.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { engine } from "../../engine";

// Each test says what the engine and the host's members answer.
vi.mock('../../engine', () => ({
  engine: {
    listTree: vi.fn(),
    parseFrontmatter: vi.fn(),
  },
}));
const startWriting = vi.fn();
const importSite = vi.fn();
const openPluginsView = vi.fn();
const openStarterPicker = vi.fn(async () => {});

vi.mock("../../shared/ui/toast/toast-manager", () => ({
  showToast: vi.fn(),
  showAck: vi.fn(),
  showOrUpdateToast: vi.fn(),
  removeToastById: vi.fn(),
}));

// Imports MUST come after the vi.mock calls; vitest hoists vi.mock so
// the order in this file is fine, but the rule is documented to avoid
// future shuffles breaking the harness.
import { showToast, showAck, showOrUpdateToast, removeToastById } from "../../shared/ui/toast/toast-manager";
import {
  _pickLandingFile,
  handleConnectPlugin,
  handleImportUrl,
  handleStartWriting,
  handleTryExample,
  makeCardClickHandler,
} from "../folder-mode/folder-mode-handlers";
import { unmountFolderMode } from "../folder-mode/folder-mode";
import { host } from "../../host";
import { dir, file } from '../tree/__tests__/test-helpers';

// DOM globals (window, document, HTMLElement, CustomEvent, …) are provided
// by vitest's jsdom environment (`environment: 'jsdom'` in vitest.config.js),
// the same way every sibling editor test relies on them. No manual JSDOM
// construction or global patching needed.

function makeHost(): HTMLElement {
  const host = document.createElement("div");
  // Mirrors the real folder-mode empty-state DOM (folder-mode.ts
  // buildCardsHtml): the four cards including the URL card the inline
  // expand morphs in place. Each card carries a title + description span
  // so the collapse path's re-render has a faithful baseline.
  host.innerHTML = `
    <div class="moss-folder-mode moss-folder-mode--empty">
      <div class="moss-folder-mode__cards">
        <button data-folder-mode-card="start-writing"><span class="moss-folder-mode__card-title">Start writing</span></button>
        <div class="moss-folder-mode__cards-grid">
          <button class="moss-folder-mode__card moss-folder-mode__card--small" data-folder-mode-card="try-example"><span class="moss-folder-mode__card-title">Example</span></button>
          <button class="moss-folder-mode__card moss-folder-mode__card--small" data-folder-mode-card="import-url"><span class="moss-folder-mode__card-title">URL</span><span class="moss-folder-mode__card-description">Import from URL</span></button>
          <button class="moss-folder-mode__card moss-folder-mode__card--small" data-folder-mode-card="connect-plugin"><span class="moss-folder-mode__card-title">Plugin</span></button>
        </div>
      </div>
    </div>
  `;
  document.body.appendChild(host);
  return host;
}

beforeEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = "";
  Object.assign(host, {
    startWriting,
    importSite,
    openPluginsView,
    starterPicker: { open: openStarterPicker, takeRequest: vi.fn(async () => false) },
  });
});

afterEach(() => {
  document.body.innerHTML = "";
});

// ── Start writing ─────────────────────────────────────────────────────

describe("handleStartWriting", () => {
  it("calls onboardingStartWriting then loadFile with relative path", async () => {
    startWriting.mockResolvedValue({
      status: "ok",
      data: "/site/index.md",
    });
    const loadFile = vi.fn().mockResolvedValue(undefined);
    const host = makeHost();
    await handleStartWriting({
      folderPath: "/site",
      folderName: "site",
      loadFile,
      host,
    });
    expect(startWriting).toHaveBeenCalledWith("/site");
    expect(loadFile).toHaveBeenCalledWith("index.md");
  });
});

// ── Starter ───────────────────────────────────────────────────────────
//
// The picker's own behaviour is tested in `starter-picker-controller.test.ts`;
// here the card goes through the desktop's fill of the `starterPicker` slot.

describe("handleTryExample", () => {
  it("does nothing in a host without a starter picker", async () => {
    delete host.starterPicker;
    await expect(handleTryExample({ folderPath: "/site", folderName: "site", loadFile: vi.fn(), host: makeHost() })).resolves.toBeUndefined();
    expect(openStarterPicker).not.toHaveBeenCalled();
  });

  it("opens the host's starter picker in the folder's cards", async () => {
    const cards = makeHost();
    await handleTryExample({ folderPath: "/site", folderName: "site", loadFile: vi.fn(), host: cards });
    expect(openStarterPicker).toHaveBeenCalledWith(expect.objectContaining({ folderPath: "/site", folderName: "site", host: cards }));
  });

  it("Use this starter opens the home file the picker hands back, as a path relative to the folder, like Start writing does", async () => {
    const loadFile = vi.fn().mockResolvedValue(undefined);
    await handleTryExample({ folderPath: "/Users/x/site", folderName: "site", loadFile, host: makeHost() });
    const [open] = openStarterPicker.mock.calls[0] as unknown as [{ loadHome: (abs: string) => Promise<unknown> }];
    await open.loadHome("/Users/x/site/index.md");
    expect(loadFile).toHaveBeenCalledWith("index.md");
  });
});

// ── URL import ────────────────────────────────────────────────────────

describe("handleImportUrl", () => {
  it("renders a URL input + submit button", async () => {
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    expect(host.querySelector("[data-folder-mode-url-input]")).toBeTruthy();
    expect(host.querySelector("[data-folder-mode-url-submit]")).toBeTruthy();
  });

  it("submits the URL value via startNativeImportTask when the submit button is clicked", async () => {
    importSite.mockResolvedValue({
      status: "ok",
      data: { success: true, total_pages: "1", failed_pages: 0, error: null },
    });
    (engine.listTree as any).mockResolvedValue({
      status: "ok",
      data: dir("/site", []),
    });
    const loadFile = vi.fn().mockResolvedValue(undefined);
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile,
      host,
    });
    const input = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    )!;
    input.value = "https://example.com/post";
    host
      .querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!
      .click();
    await new Promise((r) => setTimeout(r, 30));
    expect(importSite).toHaveBeenCalledWith(
      "https://example.com/post",
      "/site",
    );
  });

  it("does NOT toast from the catch handler when startNativeImportTask returns status: 'error' (the PanelTask subscriber owns the error toast)", async () => {
    // One event, one home: a scrape failure inside
    // start_native_import_task fires handle.failed(e, false) → a single
    // persistent blocking error toast via task-toast-subscriber. The frontend
    // catch handler must NOT toast again (that was the old double-signal).
    // The subscriber is not mounted in jsdom, so the only observable here is
    // the absence of a handler toast + the restored input UI.
    importSite.mockResolvedValue({
      status: "error",
      error: "network unreachable",
    });
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    const input = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    )!;
    input.value = "https://example.com";
    host
      .querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!
      .click();
    await new Promise((r) => setTimeout(r, 30));
    // No double-signal: the catch handler does not emit an "Import failed" toast.
    expect(showToast).not.toHaveBeenCalledWith(
      { what: expect.stringContaining("Import failed"), severity: "Blocking" },
    );
    // Input is still re-enabled so the user can retry.
    expect(input.disabled).toBe(false);
  });

  it("does NOT toast from the catch handler when startNativeImportTask returns data.success === false", async () => {
    // Same rule (one event, one home) for the logical-failure path: no catch-handler toast;
    // the import task lifecycle owns the error surface. Input is re-enabled.
    importSite.mockResolvedValue({
      status: "ok",
      data: {
        success: false,
        total_pages: 0,
        failed_pages: 0,
        error: "robots.txt disallows",
      },
    });
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    const input = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    )!;
    input.value = "https://example.com";
    host
      .querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!
      .click();
    await new Promise((r) => setTimeout(r, 30));
    expect(showToast).not.toHaveBeenCalledWith(
      { what: expect.stringContaining("Import failed"), severity: "Blocking" },
    );
    expect(input.disabled).toBe(false);
  });

  it("on zero-result import, renders an inline label on the card and does NOT call showToast", async () => {
    // Zero-result routes to inline label, not showToast.
    importSite.mockResolvedValue({
      status: "ok",
      data: { success: true, total_pages: 0, failed_pages: 0, error: null },
    });
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    const input = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    )!;
    input.value = "https://example.com";
    host
      .querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!
      .click();
    await new Promise((r) => setTimeout(r, 30));

    // No toast: feedback is inline on the card.
    expect(showToast).not.toHaveBeenCalledWith(
      { what: expect.stringContaining("Nothing to import"), severity: "NeedsAction" },
    );
    // Inline label is present on the card with "Nothing to import" text.
    const card = host.querySelector('[data-folder-mode-card="import-url"]');
    const label = card?.querySelector("[data-folder-mode-url-status]");
    expect(label).not.toBeNull();
    expect(label?.textContent).toMatch(/Nothing to import/i);

    // URL input is re-rendered with the original URL as placeholder (ghost-text).
    const reInput = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    );
    expect(reInput?.getAttribute("placeholder")).toBe("https://example.com");
  });

  it("does NOT call showOrUpdateToast for progress (the progress panel is the home)", async () => {
    importSite.mockResolvedValue({
      status: "ok",
      data: { success: true, total_pages: "1", failed_pages: 0, error: null },
    });
    (engine.listTree as any).mockResolvedValue({
      status: "ok",
      data: dir("/site", []),
    });
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site", folderName: "site", loadFile: vi.fn(), host,
    });
    const input = host.querySelector<HTMLInputElement>("[data-folder-mode-url-input]")!;
    input.value = "https://example.com/post";
    host.querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!.click();
    await new Promise((r) => setTimeout(r, 30));
    expect(showOrUpdateToast).not.toHaveBeenCalledWith(
      expect.stringContaining("Importing"),
      expect.anything(),
    );
  });

  it("does NOT call removeToastById in the finally block", async () => {
    importSite.mockResolvedValue({
      status: "ok",
      data: { success: true, total_pages: "1", failed_pages: 0, error: null },
    });
    (engine.listTree as any).mockResolvedValue({
      status: "ok",
      data: dir("/site", []),
    });
    const host = makeHost();
    await handleImportUrl({ folderPath: "/site", folderName: "site", loadFile: vi.fn(), host });
    host.querySelector<HTMLInputElement>("[data-folder-mode-url-input]")!.value = "https://example.com/post";
    host.querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!.click();
    await new Promise((r) => setTimeout(r, 30));
    expect(removeToastById).not.toHaveBeenCalled();
  });
});

// ── URL card listener cleanup (should-fix B) ──────────────────────────

describe("URL card listener cleanup", () => {
  // The listeners are bound to an AbortController signal, so their
  // removal is internal to the DOM (the signal-abort path does NOT route
  // through removeEventListener). We therefore verify cleanup
  // BEHAVIOURALLY: count how many live capture-phase document-click
  // listeners respond to a click by tracking `stopPropagation` /
  // `collapse` effects. A leaked listener would fire on a detached card
  // (its `card` reference) instead of the live one.
  //
  // To make "how many listeners are live" observable, we register our
  // OWN capture-phase document listener that fires FIRST (added before
  // any expand) and records each document click. The card's own
  // click-outside listener, when live, collapses the expanded card; we
  // assert collapse happens exactly once and only while a listener
  // should be live.

  // The click-outside listener is added in a deferred (setTimeout 0)
  // tick; give it room to register.
  const tick = () => new Promise((r) => setTimeout(r, 5));

  function card(host: HTMLElement): HTMLElement {
    return host.querySelector<HTMLElement>(
      '[data-folder-mode-card="import-url"]',
    )!;
  }
  function isExpanded(host: HTMLElement): boolean {
    return card(host).classList.contains(
      "moss-folder-mode__card--url-expanded",
    );
  }

  async function expand(host: HTMLElement) {
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    await tick();
  }

  function pressEsc(host: HTMLElement) {
    const input = host.querySelector<HTMLInputElement>(
      "[data-folder-mode-url-input]",
    )!;
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  }

  // Click somewhere OUTSIDE the card to trigger the click-outside path.
  function clickOutside() {
    document.body.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  }

  afterEach(() => {
    // Defensive: tear down any live URL-card listeners between tests.
    unmountFolderMode();
  });

  it("collapses on a click outside while expanded", async () => {
    const host = makeHost();
    await expand(host);
    expect(isExpanded(host)).toBe(true);
    clickOutside();
    expect(isExpanded(host)).toBe(false);
  });

  it("Esc removes the click-outside listener (no stale collapse afterward)", async () => {
    const host = makeHost();
    await expand(host);
    pressEsc(host);
    expect(isExpanded(host)).toBe(false);
    // Re-expand WITHOUT going through wireUrlCard's teardown-of-prior:
    // manually mark expanded so a stale listener (if any survived Esc)
    // would wrongly collapse it on the next outside click.
    card(host).classList.add("moss-folder-mode__card--url-expanded");
    clickOutside();
    // No stale Esc-era listener fired → still "expanded".
    expect(isExpanded(host)).toBe(true);
  });

  it("does not stack listeners across rapid expand→Esc→expand→Esc cycles", async () => {
    const host = makeHost();
    for (let i = 0; i < 3; i++) {
      await expand(host);
      pressEsc(host);
      expect(isExpanded(host)).toBe(false);
    }
    // After the cycles, manually mark expanded. If listeners had stacked,
    // multiple stale handlers would still be live; none should fire since
    // each was torn down at its collapse.
    card(host).classList.add("moss-folder-mode__card--url-expanded");
    clickOutside();
    expect(isExpanded(host)).toBe(true);
  });

  it("tears down the live click-outside listener on folder-mode unmount", async () => {
    const host = makeHost();
    await expand(host); // expanded, listener live, NOT collapsed
    expect(isExpanded(host)).toBe(true);
    unmountFolderMode();
    // Re-mark the (now detached) card as expanded; a leaked listener
    // would collapse it on the next document click.
    card(host).classList.add("moss-folder-mode__card--url-expanded");
    clickOutside();
    expect(isExpanded(host)).toBe(true);
  });
});

// ── Plugin drill-down ─────────────────────────────────────────────────

describe("handleConnectPlugin", () => {
  it("invokes open_plugins_view via Tauri IPC so the main webview opens the catalog", async () => {
    openPluginsView.mockResolvedValue({
      status: "ok",
      data: null,
    });
    await handleConnectPlugin({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host: makeHost(),
    });
    expect(openPluginsView).toHaveBeenCalledWith("/site");
    expect(showAck).toHaveBeenCalledWith("Opening plugins…");
  });

  it("warns and surfaces a fallback toast when the IPC call fails", async () => {
    openPluginsView.mockResolvedValue({
      status: "error",
      error: "emit_to(main) failed",
    });
    await handleConnectPlugin({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host: makeHost(),
    });
    expect(showToast).toHaveBeenCalledWith(
      { what: expect.stringContaining("Could not open plugins"), severity: "NeedsAction" },
    );
  });
});

// ── makeCardClickHandler routes to the right handler ──────────────────

describe("makeCardClickHandler", () => {
  it("routes try-example kind to handleTryExample (which opens the starter picker)", async () => {
    const handler = makeCardClickHandler({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host: makeHost(),
    });
    handler("try-example");
    await new Promise((r) => setTimeout(r, 10));
    expect(openStarterPicker).toHaveBeenCalled();
  });

  it("routes import-url kind to handleImportUrl (which renders the URL input)", async () => {
    const host = makeHost();
    const handler = makeCardClickHandler({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    handler("import-url");
    await new Promise((r) => setTimeout(r, 10));
    expect(host.querySelector("[data-folder-mode-url-input]")).toBeTruthy();
  });

  it("routes connect-plugin kind to handleConnectPlugin (which calls openPluginsView)", async () => {
    openPluginsView.mockResolvedValue({
      status: "ok",
      data: null,
    });
    const handler = makeCardClickHandler({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host: makeHost(),
    });
    handler("connect-plugin");
    await new Promise((r) => setTimeout(r, 10));
    expect(openPluginsView).toHaveBeenCalledWith("/site");
  });
});

// ── Instant feedback (the click changes the screen) ───────────────────

/**
 * The rule under test: a
 * click changes the screen in the frame it is received, and every exit path
 * ends somewhere the user can see. These assert the RULE, not the mechanism:
 * feedback is asserted while the command is still pending, failure is asserted
 * as something on screen, and the guard is asserted by retrying.
 */
describe("card clicks give feedback before the work finishes", () => {
  /** A promise the test resolves by hand, so "still pending" is observable. */
  function deferred<T>() {
    let settle!: (v: T) => void;
    const promise = new Promise<T>((r) => {
      settle = r;
    });
    return { promise, settle };
  }

  const flush = () => new Promise((r) => setTimeout(r, 0));

  function clickCard(host: HTMLElement, loadFile = vi.fn().mockResolvedValue(undefined)) {
    const handler = makeCardClickHandler({
      folderPath: "/site",
      folderName: "site",
      loadFile,
      host,
    });
    return { handler, loadFile };
  }

  it("marks the clicked card busy before onboardingStartWriting resolves", async () => {
    const gate = deferred<{ status: string; data: string }>();
    startWriting.mockReturnValue(gate.promise);
    const host = makeHost();
    const card = host.querySelector<HTMLElement>('[data-folder-mode-card="start-writing"]')!;
    const { handler, loadFile } = clickCard(host);

    handler("start-writing");

    // No await between the click and this assertion: the paint has to have
    // happened in the click's own frame, not after the round trip.
    expect(card.getAttribute("aria-busy")).toBe("true");
    expect(card.classList.contains("moss-is-busy")).toBe(true);
    expect(card.textContent).toContain("Creating");
    expect(card.textContent).not.toContain("Start writing");

    gate.settle({ status: "ok", data: "/site/index.md" });
    await flush();
    expect(loadFile).toHaveBeenCalledWith("index.md");
  });

  it("never sets the disabled property (it would drop focus out of the panel)", async () => {
    const gate = deferred<{ status: string; data: string }>();
    startWriting.mockReturnValue(gate.promise);
    const host = makeHost();
    const card = host.querySelector<HTMLButtonElement>('[data-folder-mode-card="start-writing"]')!;
    clickCard(host).handler("start-writing");
    expect(card.disabled).toBe(false);
    expect(card.getAttribute("aria-disabled")).toBe("true");
    gate.settle({ status: "ok", data: "/site/index.md" });
    await flush();
  });

  it("tells the user when the work fails, and lets them try again", async () => {
    startWriting.mockResolvedValue({
      status: "error",
      error: "disk full",
    });
    const host = makeHost();
    const card = host.querySelector<HTMLElement>('[data-folder-mode-card="start-writing"]')!;
    const { handler, loadFile } = clickCard(host);

    handler("start-writing");
    await flush();

    // On screen, in words — not a console line.
    expect(showToast).toHaveBeenCalledWith({
      what: expect.stringContaining("disk full"),
      severity: "NeedsAction",
    });
    expect(loadFile).not.toHaveBeenCalled();
    // Terminal state: the card is a card again, not a stuck spinner.
    expect(card.hasAttribute("aria-busy")).toBe(false);
    expect(card.textContent).toContain("Start writing");

    // The guard released, so the retry actually runs.
    startWriting.mockResolvedValue({
      status: "ok",
      data: "/site/index.md",
    });
    handler("start-writing");
    await flush();
    expect(loadFile).toHaveBeenCalledWith("index.md");
  });

  it("surfaces a thrown failure the same way a failed result is surfaced", async () => {
    startWriting.mockRejectedValue(new Error("boom"));
    const host = makeHost();
    clickCard(host).handler("start-writing");
    await flush();
    expect(showToast).toHaveBeenCalledWith({
      what: expect.stringContaining("boom"),
      severity: "NeedsAction",
    });
  });

  it("an impatient second click does not create a second file", async () => {
    const gate = deferred<{ status: string; data: string }>();
    startWriting.mockReturnValue(gate.promise);
    const host = makeHost();
    const card = host.querySelector<HTMLElement>('[data-folder-mode-card="start-writing"]')!;
    const { handler } = clickCard(host);

    handler("start-writing");
    handler("start-writing");
    handler("start-writing");

    expect(startWriting).toHaveBeenCalledTimes(1);
    // The repeat click still lands on a visibly busy card, rather than a
    // control that looks idle while the work runs.
    expect(card.getAttribute("aria-busy")).toBe("true");
    gate.settle({ status: "ok", data: "/site/index.md" });
    await flush();
  });

  it("leaves the URL card interactive — its expansion is its own feedback", async () => {
    const host = makeHost();
    clickCard(host).handler("import-url");
    await flush();
    const card = host.querySelector<HTMLElement>('[data-folder-mode-card="import-url"]')!;
    // `.moss-is-busy` dims the card, and opacity applies to descendants — it
    // would grey out the input the click just revealed.
    expect(card.classList.contains("moss-is-busy")).toBe(false);
    expect(card.querySelector("[data-folder-mode-url-input]")).toBeTruthy();
  });

  it("says what is missing when Import is clicked with an empty field", async () => {
    const host = makeHost();
    await handleImportUrl({
      folderPath: "/site",
      folderName: "site",
      loadFile: vi.fn(),
      host,
    });
    const errorEl = host.querySelector<HTMLElement>("[data-folder-mode-url-error]")!;
    expect(errorEl.hidden).toBe(true);
    host.querySelector<HTMLButtonElement>("[data-folder-mode-url-submit]")!.click();
    await flush();
    expect(errorEl.hidden).toBe(false);
    expect(errorEl.textContent).toBeTruthy();
    expect(importSite).not.toHaveBeenCalled();
  });
});

// ── pickLandingFile precedence chain ──────────────────────────────────

describe("pickLandingFile (precedence chain)", () => {
  // Helper: build a TreeNode-shaped object the way `listTree` returns it.
  function tree(...mdNames: string[]) {
    return {
      status: "ok",
      data: dir("/site", mdNames.map((name) => file(`/site/${name}`))),
    };
  }

  it("returns null when no markdown files exist", async () => {
    (engine.listTree as any).mockResolvedValue(tree());
    const result = await _pickLandingFile("/site");
    expect(result).toBeNull();
  });

  it("step 1: prefers the most-recently-dated post over index.md", async () => {
    (engine.listTree as any).mockResolvedValue(
      tree("index.md", "2024-01-15-old.md", "2026-05-20-new.md"),
    );
    (engine.parseFrontmatter as any).mockImplementation(
      async (abs: string) => {
        // index.md has no date; the two dated posts return their date.
        if (abs.endsWith("index.md")) {
          return { status: "ok", data: { frontmatter: {} } };
        }
        if (abs.includes("2024-01-15")) {
          return {
            status: "ok",
            data: { frontmatter: { date: "2024-01-15" } },
          };
        }
        if (abs.includes("2026-05-20")) {
          return {
            status: "ok",
            data: { frontmatter: { date: "2026-05-20" } },
          };
        }
        return { status: "ok", data: { frontmatter: {} } };
      },
    );
    const result = await _pickLandingFile("/site");
    expect(result).toBe("2026-05-20-new.md");
  });

  it("step 2: falls through to index.md when no dated posts exist", async () => {
    (engine.listTree as any).mockResolvedValue(
      tree("about.md", "contact.md", "index.md"),
    );
    (engine.parseFrontmatter as any).mockResolvedValue({
      status: "ok",
      data: { frontmatter: {} },
    });
    const result = await _pickLandingFile("/site");
    expect(result).toBe("index.md");
  });

  it("step 2: matches index.md case-insensitively", async () => {
    (engine.listTree as any).mockResolvedValue(tree("INDEX.MD"));
    (engine.parseFrontmatter as any).mockResolvedValue({
      status: "ok",
      data: { frontmatter: {} },
    });
    const result = await _pickLandingFile("/site");
    expect(result).toBe("INDEX.MD");
  });

  it("step 2: prefers index.md over other home-stem candidates (readme/main)", async () => {
    (engine.listTree as any).mockResolvedValue(
      tree("readme.md", "main.md", "index.md"),
    );
    (engine.parseFrontmatter as any).mockResolvedValue({
      status: "ok",
      data: { frontmatter: {} },
    });
    const result = await _pickLandingFile("/site");
    expect(result).toBe("index.md");
  });

  it("step 3: alphabetical fallback when no dated or home-stem matches", async () => {
    (engine.listTree as any).mockResolvedValue(
      tree("zebra.md", "alpha.md", "mango.md"),
    );
    (engine.parseFrontmatter as any).mockResolvedValue({
      status: "ok",
      data: { frontmatter: {} },
    });
    const result = await _pickLandingFile("/site");
    expect(result).toBe("alpha.md");
  });
});
