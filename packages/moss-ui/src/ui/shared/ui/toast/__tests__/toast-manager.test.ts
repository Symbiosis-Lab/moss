/**
 * Unit tests for the toast manager — focused on three behaviors:
 *
 *   1. A toast is an Advisory rendered ephemerally.
 *      `showToast` takes an Advisory-shaped `{ what, severity, action?,
 *      durationMs? }` (no private `ToastType` union); `severity` drives the
 *      icon/class — ShippedDegraded→info, NeedsAction→warning, Blocking→error.
 *      `showAck` is the separate pure-confirmation surface (the design's `Ack`),
 *      always success-styled.
 *   2. showToast() with a repeated `id` must COALESCE (update in place),
 *      not destroy + recreate. Recreating replays the slide-in animation
 *      (flicker) and, with no `id`, stacks duplicate toasts (spam).
 *   3. A first-class `progress` option renders/updates a progress bar on
 *      the SAME element so its CSS width-transition can animate smoothly.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

import {
  showToast,
  showAck,
  updateToast,
  showOrUpdateToast,
  removeToastById,
  configureToastRuntime,
  _clearTrackedToasts,
  _resetToastRuntime,
} from "../toast-manager";
import { getToastRuntime } from "../toast-runtime";

// The host capabilities are injected (toast-manager is pure — it imports no
// @tauri-apps module and no bindings), so the seam is what the tests drive.
const logError = vi.fn();
const mockedInfo = vi.fn();
const openUrl = vi.fn<(url: string) => Promise<void>>();
// A route as a host installs one (the desktop's sends .onion URLs to its Tor
// opener); the toast only sees the generic UrlOpenRoute seam.
const openOnionUrl =
  vi.fn<(url: string) => Promise<{ status: "ok"; data: unknown } | { status: "error"; error: string }>>();
const onionRoute = { name: "onion", matches: (url: string) => new URL(url).hostname.endsWith(".onion"), open: openOnionUrl };

let container: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  container.id = "moss-toast-container";
  document.body.appendChild(container);
  // Only logInfo — tests exercising openUrl/logError/urlRoutes install their
  // own fuller configureToastRuntime locally (and reset it in their own
  // afterEach); this file-wide default just makes "[toast] shown" assertable
  // everywhere without every describe block repeating the wiring.
  mockedInfo.mockClear();
  configureToastRuntime({ logInfo: mockedInfo });
});

afterEach(() => {
  _clearTrackedToasts();
  vi.runAllTimers();
  vi.useRealTimers();
  container.remove();
});

const toastsWithId = (id: string) =>
  container.querySelectorAll(`[data-toast-id="${id}"]`);
const fillOf = (el: Element | null) =>
  el?.querySelector(".moss-progress-bar-fill") as HTMLElement | null;

describe("toast is an Advisory rendered ephemerally", () => {
  test("ShippedDegraded → info (no icon, info class)", () => {
    showToast({ what: "Video shipped full-size", severity: "ShippedDegraded" });
    const el = container.querySelector(".moss-toast")!;
    expect(el.className).toBe("moss-toast info");
    // info has no leading icon
    expect(el.querySelector(".moss-toast-icon")).toBeNull();
    expect(el.querySelector(".moss-toast-message")!.textContent).toBe(
      "Video shipped full-size"
    );
  });

  test("NeedsAction → warning (warning class + warning icon)", () => {
    showToast({ what: "Install FFmpeg to optimize videos", severity: "NeedsAction" });
    const el = container.querySelector(".moss-toast")!;
    expect(el.className).toBe("moss-toast warning");
    expect(el.querySelector(".moss-toast-icon")).not.toBeNull();
  });

  test("Blocking → error (error class, no icon)", () => {
    showToast({ what: "Deployment failed", severity: "Blocking" });
    const el = container.querySelector(".moss-toast")!;
    expect(el.className).toBe("moss-toast error");
    expect(el.querySelector(".moss-toast-icon")).toBeNull();
  });

  test("an action renders as a toast action button", () => {
    showToast({
      what: "Trial expired",
      severity: "Blocking",
      action: { label: "Subscribe", url: "https://mosspub.com/pricing" },
    });
    const link = container.querySelector(".moss-toast-action") as HTMLAnchorElement;
    expect(link).not.toBeNull();
    expect(link.textContent).toContain("Subscribe");
    expect(link.dataset.url).toBe("https://mosspub.com/pricing");
  });

  test("durationMs sets the auto-dismiss timer (bounded by options elsewhere)", () => {
    showToast({ what: "x", severity: "NeedsAction", durationMs: 4000 });
    const el = container.querySelector(".moss-toast");
    expect(el).not.toBeNull();
    vi.advanceTimersByTime(3999);
    expect(container.querySelector(".moss-toast")).not.toBeNull();
    vi.advanceTimersByTime(2); // cross the 4000ms threshold
    vi.advanceTimersByTime(300); // fade-out animation
    expect(container.querySelector(".moss-toast")).toBeNull();
  });

  test("showAck is a success-styled confirmation (success class + check icon)", () => {
    showAck("Copied");
    const el = container.querySelector(".moss-toast")!;
    expect(el.className).toBe("moss-toast success");
    expect(el.querySelector(".moss-toast-icon")).not.toBeNull(); // success check
    expect(el.querySelector(".moss-toast-message")!.textContent).toBe("Copied");
  });

  test("logs an error's words, and for any other toast only its id and variant", () => {
    showToast({ what: "Could not make the page", severity: "Blocking" }, { id: "my-id" });
    expect(vi.mocked(mockedInfo)).toHaveBeenCalledWith("[toast] shown id=my-id variant=error text=Could not make the page");
    showToast({ what: "a private detail", severity: "NeedsAction" });
    showAck("another private detail");
    for (const call of vi.mocked(mockedInfo).mock.calls) {
      expect(String(call[0])).not.toContain("private detail");
    }
  });

  test("logs id=(none) when the toast has no id", () => {
    showAck("Copied");
    expect(vi.mocked(mockedInfo)).toHaveBeenCalledWith("[toast] shown id=(none) variant=success");
  });
});

describe("showToast coalesce-on-id (anti-flicker / anti-spam)", () => {
  test("repeated showToast with same id keeps ONE element (no recreate)", () => {
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", persistent: true });
    const first = container.querySelector('[data-toast-id="dl"]');
    expect(first).not.toBeNull();

    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", persistent: true });

    // Exactly one toast with this id, and it's the SAME DOM node — the
    // create primitive must not have torn down + rebuilt it.
    expect(toastsWithId("dl").length).toBe(1);
    expect(container.querySelector('[data-toast-id="dl"]')).toBe(first);
  });

  test("coalesce updates the message text in place", () => {
    showAck("Importing… 1 pages", { id: "imp" });
    const el = container.querySelector('[data-toast-id="imp"]')!;

    showAck("Importing… 7 pages", { id: "imp" });

    expect(el.querySelector(".moss-toast-message")!.textContent).toBe(
      "Importing… 7 pages"
    );
    expect(toastsWithId("imp").length).toBe(1);
  });

  test("coalesce updates type class and icon", () => {
    showToast({ what: "working", severity: "ShippedDegraded" }, { id: "t", persistent: true });
    const el = container.querySelector('[data-toast-id="t"]')!;
    expect(el.querySelector(".moss-toast-icon")).toBeNull(); // info has no icon

    showAck("done", { id: "t" });

    expect(el.className).toBe("moss-toast success");
    expect(el.querySelector(".moss-toast-icon")).not.toBeNull(); // success icon added
  });

  test("coalesce with no actions clears a prior action button (faithful replace)", () => {
    showToast(
      { what: "Update available (v9)", severity: "ShippedDegraded" },
      {
        id: "dl",
        persistent: true,
        actions: [{ label: "Update Now", onClick: () => {} }],
      }
    );
    const el = container.querySelector('[data-toast-id="dl"]')!;
    expect(el.querySelector(".moss-toast-action")).not.toBeNull();

    // Download starts: same id, no actions → the button must go away.
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", persistent: true });

    expect(el.querySelector(".moss-toast-action")).toBeNull();
  });

  test("coalescing onto a fading (dismissing) toast cancels its scheduled removal", () => {
    showToast({ what: "Downloading…", severity: "ShippedDegraded" }, { id: "dl", progress: 10 });
    removeToastById("dl"); // begins 300ms fade-out + a scheduled removal

    // A late same-id update lands before the fade completes — it must revive
    // the toast AND prevent the already-scheduled removal from deleting it.
    showToast({ what: "Downloading…", severity: "ShippedDegraded" }, { id: "dl", progress: 50 });
    vi.advanceTimersByTime(400); // past the original removal timer

    const el = container.querySelector('[data-toast-id="dl"]');
    expect(el).not.toBeNull();
    expect(fillOf(el)!.style.width).toBe("50%");
  });
});

describe("toast action with copyValue → canonical copy button", () => {
  test("renders the canonical copy button, not an anchor link", () => {
    showToast(
      { what: "Logs sent: LOG-ABCD-0001", severity: "ShippedDegraded" },
      { actions: [{ label: "Copy", copyValue: "LOG-ABCD-0001" }] }
    );

    const btn = container.querySelector(".moss-copy-btn");
    expect(btn).not.toBeNull();
    expect(btn!.tagName).toBe("BUTTON");
    // A copy affordance is not a navigation — no anchor link is rendered.
    expect(container.querySelector(".moss-toast-action")).toBeNull();
  });

  test("clicking the copy button writes copyValue to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { actions: [{ label: "Copy", copyValue: "TICKET-1" }] }
    );
    const btn = container.querySelector(".moss-copy-btn") as HTMLButtonElement;
    btn.click();
    await Promise.resolve();
    await Promise.resolve();

    expect(writeText).toHaveBeenCalledWith("TICKET-1");
  });

  test("a mix of a copy action and a url action renders one button + one anchor", () => {
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      {
        actions: [
          { label: "View", url: "https://example.com" },
          { label: "Copy", copyValue: "TICKET-2" },
        ],
      }
    );

    expect(container.querySelector(".moss-toast-action")).not.toBeNull();
    expect(container.querySelector(".moss-copy-btn")).not.toBeNull();
  });

  test("coalescing from a copy action to no actions removes the copy button", () => {
    showToast(
      { what: "Sending logs…", severity: "ShippedDegraded" },
      { id: "send-logs", persistent: true, actions: [{ label: "Copy", copyValue: "X" }] }
    );
    const el = container.querySelector('[data-toast-id="send-logs"]')!;
    expect(el.querySelector(".moss-copy-btn")).not.toBeNull();

    showToast({ what: "Sending logs…", severity: "ShippedDegraded" }, { id: "send-logs", persistent: true });

    expect(el.querySelector(".moss-copy-btn")).toBeNull();
  });

  test("coalescing from a copy action to a url action swaps element types", () => {
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { id: "swap", actions: [{ label: "Copy", copyValue: "X" }] }
    );
    const el = container.querySelector('[data-toast-id="swap"]')!;
    expect(el.querySelector(".moss-copy-btn")).not.toBeNull();
    expect(el.querySelector(".moss-toast-action")).toBeNull();

    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { id: "swap", actions: [{ label: "View", url: "https://example.com" }] }
    );

    expect(el.querySelector(".moss-copy-btn")).toBeNull();
    const link = el.querySelector(".moss-toast-action") as HTMLAnchorElement;
    expect(link).not.toBeNull();
    expect(link.dataset.url).toBe("https://example.com");
  });

  test("a copy action renders correctly on a non-dismissible toast (no close button)", () => {
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      {
        id: "nodismiss",
        persistent: true,
        dismissible: false,
        actions: [{ label: "Copy", copyValue: "X" }],
      }
    );
    const el = container.querySelector('[data-toast-id="nodismiss"]')!;
    expect(el.querySelector(".moss-copy-btn")).not.toBeNull();
    expect(el.querySelector(".moss-toast-close")).toBeNull();
  });
});

describe("inlineCopy → copy button rides on the message's line", () => {
  test("renders the copy button INSIDE the message, with no actions row", () => {
    showAck("Logs sent: LOG-ABCD-0001", { inlineCopy: "LOG-ABCD-0001" });

    const messageEl = container.querySelector(".moss-toast-message")!;
    const btn = messageEl.querySelector(".moss-copy-btn");
    expect(btn).not.toBeNull();
    expect(btn!.tagName).toBe("BUTTON");
    // The actions row is grid-row 2 — anything in it lands on the NEXT line.
    // An inline copy affordance must not create that row at all.
    expect(container.querySelector(".moss-toast-actions")).toBeNull();
  });

  test("clicking it writes the inlineCopy value to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    showAck("Logs sent: TICKET-9", { inlineCopy: "TICKET-9" });
    (container.querySelector(".moss-copy-btn") as HTMLButtonElement).click();
    await Promise.resolve();
    await Promise.resolve();

    expect(writeText).toHaveBeenCalledWith("TICKET-9");
  });

  test("renders icon-only, so the id reads as the line's content", () => {
    showAck("Logs sent: LOG-1", { inlineCopy: "LOG-1" });

    const btn = container.querySelector(".moss-copy-btn")!;
    expect(btn.querySelector(".moss-copy-btn__label")).toBeNull();
    // Icon-only needs a non-visual name plus the portal tooltip
    // (data-tooltip, never title=). Both surfaces that show this toast — the
    // shell and the launcher — call initTooltips().
    expect(btn.getAttribute("aria-label")).toBeTruthy();
    expect(btn.getAttribute("data-tooltip")).toBeTruthy();
    expect(btn.getAttribute("title")).toBeNull();
  });

  test("updateToast places it inside the message, not the actions row", () => {
    showAck("Sending logs…", { id: "send-logs", persistent: true, dot: "yellow" });
    updateToast("send-logs", {
      message: "Logs sent: LOG-ABCD-0001",
      dot: "green",
      inlineCopy: "LOG-ABCD-0001",
    });

    const el = container.querySelector('[data-toast-id="send-logs"]')!;
    expect(el.querySelector(".moss-toast-message .moss-copy-btn")).not.toBeNull();
    expect(el.querySelector(".moss-toast-actions")).toBeNull();
  });

  test("a later message-only patch keeps the button (updateToast is a partial patch)", () => {
    showAck("Sending logs…", { id: "send-logs", persistent: true });
    updateToast("send-logs", { message: "Logs sent: X-1", inlineCopy: "X-1" });
    updateToast("send-logs", { message: "Logs sent: X-1 (mirrored)" });

    const el = container.querySelector('[data-toast-id="send-logs"]')!;
    const btn = el.querySelector(".moss-toast-message .moss-copy-btn");
    expect(btn).not.toBeNull();
    expect(el.querySelector(".moss-toast-message")!.textContent).toContain("mirrored");
  });

  test("an empty inlineCopy clears the button", () => {
    showAck("Logs sent: X-1", { id: "send-logs", persistent: true, inlineCopy: "X-1" });
    const el = container.querySelector('[data-toast-id="send-logs"]')!;
    expect(el.querySelector(".moss-copy-btn")).not.toBeNull();

    updateToast("send-logs", { inlineCopy: "" });

    expect(el.querySelector(".moss-copy-btn")).toBeNull();
  });

  test("coalescing without inlineCopy drops it (showToast is a faithful replace)", () => {
    showAck("Logs sent: X-1", { id: "send-logs", persistent: true, inlineCopy: "X-1" });
    const el = container.querySelector('[data-toast-id="send-logs"]')!;
    expect(el.querySelector(".moss-copy-btn")).not.toBeNull();

    showToast({ what: "Failed to send logs", severity: "Blocking" }, { id: "send-logs" });

    expect(el.querySelector(".moss-copy-btn")).toBeNull();
  });
});

describe("inlineLink → the destination rides on the message's line", () => {
  beforeEach(() => {
    openUrl.mockReset();
    configureToastRuntime({ logError, logInfo: mockedInfo, openUrl, urlRoutes: [onionRoute] });
  });
  afterEach(() => _resetToastRuntime());

  test("renders an anchor INSIDE the message, with no actions row", () => {
    showAck("Published to", { inlineLink: { label: "example-garden.mosspub.com", url: "https://example-garden.mosspub.com/" } });

    const messageEl = container.querySelector(".moss-toast-message")!;
    const link = messageEl.querySelector(".moss-toast-inline-link") as HTMLAnchorElement;
    expect(link).not.toBeNull();
    expect(link.textContent).toBe("example-garden.mosspub.com");
    // No trailing "→": it is the end of a sentence, not a "go to X" button.
    expect(link.textContent).not.toContain("→");
    // An action would land in grid-row 2 — i.e. on a second line, forcing the
    // sentence to name the destination twice.
    expect(container.querySelector(".moss-toast-actions")).toBeNull();
  });

  test("clicking it opens the URL through the injected host opener", async () => {
    openUrl.mockResolvedValue(undefined);
    showAck("Published to", { inlineLink: { label: "example-garden.mosspub.com", url: "https://example-garden.mosspub.com/" } });

    const link = container.querySelector(".moss-toast-inline-link") as HTMLAnchorElement;
    await link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

    await vi.waitFor(() => expect(openUrl).toHaveBeenCalledWith("https://example-garden.mosspub.com/"));
  });

  test("it survives a message-only patch, and `null` clears it", () => {
    showAck("Published to", {
      id: "deploy-success",
      persistent: true,
      inlineLink: { label: "example-garden.mosspub.com", url: "https://example-garden.mosspub.com/" },
    });
    // `textContent =` wipes the message's children; the live value is mirrored
    // onto the dataset so the patch can put the link back.
    updateToast("deploy-success", { message: "Published to" });
    expect(container.querySelector(".moss-toast-inline-link")).not.toBeNull();

    updateToast("deploy-success", { inlineLink: null });
    expect(container.querySelector(".moss-toast-inline-link")).toBeNull();
  });
});

describe("awaitAttention → the countdown only spends time the user was here", () => {
  const setPresent = (present: boolean) => {
    Object.defineProperty(document, "visibilityState", {
      value: present ? "visible" : "hidden",
      configurable: true,
    });
    vi.spyOn(document, "hasFocus").mockReturnValue(present);
    document.dispatchEvent(new Event("visibilitychange"));
  };

  afterEach(() => {
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    vi.restoreAllMocks();
  });

  test("a toast that lands while the user is away is still there when they return", () => {
    // The failure this exists for: a publish finishes while the author is in
    // another app, a plain 9s timer expires against a screen nobody is looking
    // at, and they come back to no confirmation at all.
    setPresent(false);
    showAck("Published to", { duration: 9000, awaitAttention: true });

    vi.advanceTimersByTime(60_000);
    vi.advanceTimersByTime(300);
    expect(container.querySelector(".moss-toast")).not.toBeNull();

    // They come back — now the clock starts.
    setPresent(true);
    vi.advanceTimersByTime(8999);
    expect(container.querySelector(".moss-toast")).not.toBeNull();

    vi.advanceTimersByTime(1);
    vi.advanceTimersByTime(300); // fade-out
    expect(container.querySelector(".moss-toast")).toBeNull();
  });

  test("a blurred window is NOT an absent user — only a hidden one is", () => {
    // moss's shell hosts sibling child webviews (editor panel, plugin browser).
    // Clicking into one blurs the shell while the toast is still in plain
    // sight; gating on focus would pause the countdown there and turn a 9s
    // confirmation into a permanent one — the exact behaviour awaitAttention
    // replaced.
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    showAck("Published to", { duration: 9000, awaitAttention: true });

    vi.advanceTimersByTime(9000);
    vi.advanceTimersByTime(300);
    expect(container.querySelector(".moss-toast")).toBeNull();
  });

  test("closing cancels the live countdown, so it can't reach a later same-id toast", () => {
    // The leak: an attention-gated timer holds a visibilitychange listener and
    // fires removeToastById(id). Closing must cancel whichever countdown is
    // CURRENT — an updateToast replaces the tracked handle, so cancelling the
    // create-time one leaves the live orphan to delete whatever is wearing
    // that id when it finally elapses.
    showAck("Published to", { id: "deploy-success", duration: 3000, awaitAttention: true });
    updateToast("deploy-success", { duration: 9000 });
    (container.querySelector(".moss-toast-close") as HTMLElement).click();
    vi.advanceTimersByTime(300); // fade-out
    expect(container.querySelector(".moss-toast")).toBeNull();

    showAck("Publishing…", { id: "deploy-success", persistent: true });
    vi.advanceTimersByTime(60_000);
    expect(container.querySelector('[data-toast-id="deploy-success"]')).not.toBeNull();
  });

  test("without it, the duration is plain wall-clock", () => {
    setPresent(false);
    showAck("Copied", { duration: 3000 });

    vi.advanceTimersByTime(3000);
    vi.advanceTimersByTime(300);
    expect(container.querySelector(".moss-toast")).toBeNull();
  });
});

describe("URL action click → .onion routes through the Tor-aware opener", () => {
  beforeEach(() => {
    logError.mockReset();
    openUrl.mockReset();
    openOnionUrl.mockReset();
    configureToastRuntime({ logError, logInfo: mockedInfo, openUrl, urlRoutes: [onionRoute] });
  });

  afterEach(() => {
    _resetToastRuntime();
  });

  test("a regular URL still opens via the generic opener", async () => {
    openUrl.mockResolvedValue(undefined);
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { actions: [{ label: "View site", url: "https://example.mosspub.com" }] }
    );
    const link = container.querySelector(".moss-toast-action") as HTMLAnchorElement;
    await link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(openUrl).toHaveBeenCalledWith("https://example.mosspub.com"));
    expect(openOnionUrl).not.toHaveBeenCalled();
  });

  test("a .onion URL opens via openOnionUrl, not the generic opener", async () => {
    openOnionUrl.mockResolvedValue({
      status: "ok",
      data: { opened_with: "tor-browser" },
    });
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { actions: [{ label: "View site", url: "http://abcdef1234567890.onion/" }] }
    );
    const link = container.querySelector(".moss-toast-action") as HTMLAnchorElement;
    await link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await vi.waitFor(() =>
      expect(openOnionUrl).toHaveBeenCalledWith("http://abcdef1234567890.onion/")
    );
    expect(openUrl).not.toHaveBeenCalled();
  });

  test("a failed openOnionUrl logs the error and does NOT fall back to the generic opener", async () => {
    // Even the OS opener (openOnionUrl's own fallback) can fail to launch —
    // the frontend still just logs it rather than trying its own opener.
    openOnionUrl.mockResolvedValue({
      status: "error",
      error: "failed to open the address: No such file or directory",
    });
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { actions: [{ label: "View site", url: "http://abcdef1234567890.onion/" }] }
    );
    const link = container.querySelector(".moss-toast-action") as HTMLAnchorElement;
    await link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(logError).toHaveBeenCalled());
    expect(openUrl).not.toHaveBeenCalled();
  });
});

describe("action variant → primary (filled button) / secondary (muted link)", () => {
  const actionEls = () =>
    Array.from(container.querySelectorAll(".moss-toast-action")) as HTMLElement[];

  test("a command (onClick) renders a <button>; a navigation (url) renders an <a>", () => {
    // The primary/command action must be a real button so keyboard Space
    // activates it and a screen reader announces "button", not "link".
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      {
        actions: [
          { label: "Undo", onClick: () => {} },
          { label: "View", url: "https://example.com" },
        ],
      }
    );
    const [cmd, nav] = actionEls();
    expect(cmd.tagName).toBe("BUTTON");
    expect((cmd as HTMLButtonElement).type).toBe("button");
    expect(nav.tagName).toBe("A");
    expect(nav.getAttribute("href")).toBe("https://example.com");
  });

  test("a primary action is a filled <button> with the --primary class and NO trailing arrow", () => {
    showToast(
      { what: "moss v9 is ready", severity: "ShippedDegraded" },
      { actions: [{ label: "Update & Restart", variant: "primary", onClick: () => {} }] }
    );
    const [el] = actionEls();
    expect(el).not.toBeUndefined();
    expect(el.tagName).toBe("BUTTON");
    expect(el.classList.contains("moss-toast-action--primary")).toBe(true);
    // A command button is not a "go to X" navigation — no trailing arrow
    // (same reasoning as the copyValue affordance).
    // Exact equality already says it: a command button is not a "go to X"
    // navigation, so the renderer must not append the arrow it gives anchors.
    expect(el.textContent).toBe("Update & Restart");
  });

  test("a secondary action is an <a> with the --secondary class and keeps the arrow", () => {
    showToast(
      { what: "moss v9 is ready", severity: "ShippedDegraded" },
      { actions: [{ label: "Release notes", variant: "secondary", url: "https://example.com" }] }
    );
    const [el] = actionEls();
    expect(el.tagName).toBe("A");
    expect(el.classList.contains("moss-toast-action--secondary")).toBe(true);
    // A navigation keeps the trailing "→".
    expect(el.textContent).toBe("Release notes →");
  });

  test("an un-varianted command keeps the arrow and adds no variant class", () => {
    showToast(
      { what: "x", severity: "ShippedDegraded" },
      { actions: [{ label: "Undo", onClick: () => {} }] }
    );
    const [el] = actionEls();
    expect(el.classList.contains("moss-toast-action--primary")).toBe(false);
    expect(el.classList.contains("moss-toast-action--secondary")).toBe(false);
    expect(el.textContent).toBe("Undo →");
  });

  test("a primary action coalesced in place keeps its filled variant (faithful replace)", () => {
    const opts = {
      id: "up",
      persistent: true,
      actions: [{ label: "Update & Restart", variant: "primary" as const, onClick: () => {} }],
    };
    showToast({ what: "moss v9 is ready", severity: "ShippedDegraded" }, opts);
    const el = container.querySelector('[data-toast-id="up"]')!;
    // Re-show same id → coalesce path (updateExistingToast rebuilds actions).
    showToast({ what: "moss v9 is ready", severity: "ShippedDegraded" }, opts);
    const action = el.querySelector(".moss-toast-action")!;
    expect(action.classList.contains("moss-toast-action--primary")).toBe(true);
    expect(action.textContent).toBe("Update & Restart");
  });
});

// `updateToast` never creates. That no-op is the entire mechanism behind the
// dismissal is not observable from a caller (the toast system has no
// dismissal callback), so "patch it if it is still there" is how a late
// refinement — a finished pre-download relabelling the update button — avoids
// reopening a toast the user closed.
describe("updateToast — patch-only, never resurrect", () => {
  test("patching a DISMISSED id is a NO-OP — no toast comes back", () => {
    showToast(
      { what: "moss v2.0.0 is ready.", severity: "ShippedDegraded" },
      { id: "update-check", persistent: true, actions: [{ label: "Update & Restart" }] }
    );
    removeToastById("update-check");
    vi.advanceTimersByTime(400); // let the fade-out removal complete
    expect(toastsWithId("update-check").length).toBe(0);

    updateToast("update-check", { actions: [{ label: "Restart Now" }] });

    expect(toastsWithId("update-check").length).toBe(0);
    expect(container.textContent).not.toContain("Restart Now");
  });

  test("patching an id that never existed is a NO-OP", () => {
    const htmlBefore = container.innerHTML;
    updateToast("never-shown", { message: "x", actions: [{ label: "Restart Now" }] });
    expect(container.innerHTML).toBe(htmlBefore);
    expect(toastsWithId("never-shown").length).toBe(0);
  });
});

describe("progress bar option", () => {
  test("showToast with numeric progress renders a determinate fill", () => {
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", progress: 42 });
    const fill = fillOf(container.querySelector('[data-toast-id="dl"]'));
    expect(fill).not.toBeNull();
    expect(fill!.style.width).toBe("42%");
    expect(fill!.classList.contains("indeterminate")).toBe(false);
  });

  test("showToast with progress: -1 renders an indeterminate fill", () => {
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", progress: -1 });
    const fill = fillOf(container.querySelector('[data-toast-id="dl"]'));
    expect(fill).not.toBeNull();
    expect(fill!.classList.contains("indeterminate")).toBe(true);
    expect(fill!.style.width).toBe("100%");
  });

  test("coalescing to a new percent updates the SAME fill element (enables smooth transition)", () => {
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", progress: 10 });
    const fillBefore = fillOf(container.querySelector('[data-toast-id="dl"]'));

    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", progress: 80 });
    const fillAfter = fillOf(container.querySelector('[data-toast-id="dl"]'));

    expect(fillAfter).toBe(fillBefore); // same node ⇒ CSS width-transition animates
    expect(fillAfter!.style.width).toBe("80%");
  });

  test("coalescing without progress removes the bar (e.g. Downloading → Installing)", () => {
    showToast({ what: "Downloading update...", severity: "ShippedDegraded" }, { id: "dl", progress: 100 });
    expect(fillOf(container.querySelector('[data-toast-id="dl"]'))).not.toBeNull();

    showToast({ what: "Installing update... Restarting.", severity: "ShippedDegraded" }, {
      id: "dl",
      persistent: true,
    });

    expect(container.querySelector('[data-toast-id="dl"] .moss-toast-progress')).toBeNull();
  });

  test("updateToast can set progress in place", () => {
    showOrUpdateToast("Downloading update...", { id: "dl", duration: 5000 });
    updateToast("dl", { progress: 55 });

    const fill = fillOf(container.querySelector('[data-toast-id="dl"]'));
    expect(fill).not.toBeNull();
    expect(fill!.style.width).toBe("55%");
  });

  test("progress is clamped to 0–100", () => {
    showToast({ what: "x", severity: "ShippedDegraded" }, { id: "dl", progress: 150 });
    const fill = fillOf(container.querySelector('[data-toast-id="dl"]'));
    expect(fill!.style.width).toBe("100%");
  });
});

describe("advisory toasts self-dismiss after ~6s (default timer)", () => {
  test("a Blocking advisory with no duration auto-removes after 6000ms", () => {
    showToast({ what: "Deployment failed", severity: "Blocking" });
    expect(container.querySelector(".moss-toast")).not.toBeNull();

    // Still present at 3s — proves it is NOT the 3s Ack timer.
    vi.advanceTimersByTime(3000);
    expect(container.querySelector(".moss-toast")).not.toBeNull();

    // Still present just before 6s…
    vi.advanceTimersByTime(2999);
    expect(container.querySelector(".moss-toast")).not.toBeNull();

    // …gone once the 6000ms threshold + 300ms fade-out elapse.
    vi.advanceTimersByTime(2);
    vi.advanceTimersByTime(300);
    expect(container.querySelector(".moss-toast")).toBeNull();
  });

  test("a NeedsAction (warning) advisory also self-dismisses at 6000ms", () => {
    showToast({ what: "Install FFmpeg", severity: "NeedsAction" });
    vi.advanceTimersByTime(6000);
    vi.advanceTimersByTime(300); // fade-out
    expect(container.querySelector(".moss-toast")).toBeNull();
  });

  test("persistent: true opts out — the advisory is NOT auto-removed", () => {
    showToast({ what: "Deployment failed", severity: "Blocking" }, { persistent: true });
    const el = container.querySelector(".moss-toast");
    expect(el).not.toBeNull();
    // The ✕ close button still renders (dismissible defaults true).
    expect(el!.querySelector(".moss-toast-close")).not.toBeNull();

    vi.advanceTimersByTime(60000); // well past any auto-dismiss window
    vi.advanceTimersByTime(300);
    expect(container.querySelector(".moss-toast")).not.toBeNull();
  });

  test("the ✕ close button still renders on an auto-dismissing advisory", () => {
    showToast({ what: "Deployment failed", severity: "Blocking" });
    const el = container.querySelector(".moss-toast")!;
    expect(el.querySelector(".moss-toast-close")).not.toBeNull();
  });

  test("an explicit options.duration still wins over the 6s default", () => {
    showToast({ what: "x", severity: "Blocking" }, { duration: 2000 });
    vi.advanceTimersByTime(2000);
    vi.advanceTimersByTime(300); // fade-out
    expect(container.querySelector(".moss-toast")).toBeNull();
  });
});

describe("toast text is text", () => {
  /// A toast message carries vault data — a filename in a delete confirmation,
  /// a subscriber's email. Both the create and the update path must set it
  /// as text, or the same string is markup on first show and text on the
  /// second.
  test("a message that looks like markup renders as characters, not elements", () => {
    showToast({ what: '<img src=x onerror="window.__pwned = true">', severity: "Blocking" });

    const el = document.querySelector(".moss-toast-message") as HTMLElement;
    expect(el).not.toBeNull();
    expect(el.querySelector("img")).toBeNull();
    expect(el.textContent).toBe('<img src=x onerror="window.__pwned = true">');
  });
});

describe("configureToastRuntime merges onto the installed runtime", () => {
  afterEach(() => _resetToastRuntime());

  test("a later call with other members keeps the earlier call's members", () => {
    const stubOpen = vi.fn<(url: string) => Promise<void>>().mockResolvedValue(undefined);
    configureToastRuntime({ openUrl: stubOpen });
    configureToastRuntime({ logError });

    expect(getToastRuntime().openUrl).toBe(stubOpen);
    expect(getToastRuntime().logError).toBe(logError);
  });
});
