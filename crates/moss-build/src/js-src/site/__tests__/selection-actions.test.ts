/**
 * Tests for selection actions — desktop popover and mobile mini-bar.
 *
 * initSelectionActions():
 * - Shows a popover above text selection on desktop (mouseup inside article)
 * - Shows a bottom bar on mobile (selectionchange inside article)
 * - Hides when selection is cleared or user clicks outside
 * - Copy with attribution uses locale-appropriate formatting (Chinese/English)
 * - Popover positioned above selection bounding rect
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { initSelectionActions, captureBlocks, truncateFromEnd, truncateFromStart } from "../selection-actions";
import type { CardBlock } from "../share-card/text";

// ── Helpers ──────────────────────────────────────────────────────────────

function createArticlePage(options: { author?: string; title?: string; comments?: boolean; dataComments?: string } = {}) {
  const { author = "Test Author", title = "Test Article", comments = true, dataComments } = options;

  document.head.innerHTML = "";
  const titleEl = document.createElement("title");
  titleEl.textContent = `${title} - Test Site`;
  document.head.appendChild(titleEl);

  const metaAuthor = document.createElement("meta");
  metaAuthor.setAttribute("name", "author");
  metaAuthor.setAttribute("content", author);
  document.head.appendChild(metaAuthor);

  const main = document.createElement("main");
  const article = document.createElement("article");
  article.className = "container";

  if (dataComments !== undefined) {
    article.setAttribute("data-comments", dataComments);
  }

  const h1 = document.createElement("h1");
  h1.textContent = title;
  article.appendChild(h1);

  const p = document.createElement("p");
  p.textContent = "This is some article body text that can be selected.";
  article.appendChild(p);

  main.appendChild(article);

  // Simulate comment section injected by Rust compiler
  if (comments) {
    const commentsSection = document.createElement("section");
    commentsSection.className = "moss-comments";
    commentsSection.id = "moss-comments";
    main.appendChild(commentsSection);
  }

  document.body.appendChild(main);

  // Add an element outside the article for "outside" tests
  const outside = document.createElement("div");
  outside.id = "outside";
  outside.textContent = "This text is outside the article.";
  document.body.appendChild(outside);

  return { article, p, outside };
}

/**
 * Mock window.getSelection() to return a selection within a given node.
 * Also mocks Range.getBoundingClientRect for positioning tests.
 */
function mockSelection(
  anchorNode: Node,
  text: string,
  rect: { top: number; left: number; width: number; height: number } = {
    top: 100,
    left: 200,
    width: 150,
    height: 20,
  }
) {
  const range = document.createRange();

  // Override getBoundingClientRect on this range instance
  range.getBoundingClientRect = vi.fn(() => ({
    top: rect.top,
    left: rect.left,
    width: rect.width,
    height: rect.height,
    bottom: rect.top + rect.height,
    right: rect.left + rect.width,
    x: rect.left,
    y: rect.top,
    toJSON: () => ({}),
  }));

  // Make the fake coherent: the range must actually cover `text`, because
  // capture paths read the RANGE (not the stubbed toString). Select the
  // substring when present; otherwise make the node contain exactly `text`.
  if (anchorNode.firstChild) {
    const node = anchorNode.firstChild as Text;
    let idx = (node.textContent ?? "").indexOf(text);
    if (idx < 0) {
      node.textContent = text;
      idx = 0;
    }
    range.setStart(node, idx);
    range.setEnd(node, idx + text.length);
  }

  const selection = {
    toString: () => text,
    isCollapsed: false,
    rangeCount: 1,
    getRangeAt: () => range,
    removeAllRanges: vi.fn(),
    addRange: vi.fn(),
    anchorNode,
    anchorOffset: 0,
    focusNode: anchorNode,
    focusOffset: text.length,
    type: "Range",
    containsNode: vi.fn(),
    collapse: vi.fn(),
    collapseToEnd: vi.fn(),
    collapseToStart: vi.fn(),
    deleteFromDocument: vi.fn(),
    empty: vi.fn(),
    extend: vi.fn(),
    modify: vi.fn(),
    setBaseAndExtent: vi.fn(),
    setPosition: vi.fn(),
  } as unknown as Selection;

  vi.spyOn(window, "getSelection").mockReturnValue(selection);
  return selection;
}

function mockEmptySelection() {
  const selection = {
    toString: () => "",
    isCollapsed: true,
    rangeCount: 0,
    getRangeAt: vi.fn(),
    removeAllRanges: vi.fn(),
    addRange: vi.fn(),
    anchorNode: null,
    anchorOffset: 0,
    focusNode: null,
    focusOffset: 0,
    type: "None",
    containsNode: vi.fn(),
    collapse: vi.fn(),
    collapseToEnd: vi.fn(),
    collapseToStart: vi.fn(),
    deleteFromDocument: vi.fn(),
    empty: vi.fn(),
    extend: vi.fn(),
    modify: vi.fn(),
    setBaseAndExtent: vi.fn(),
    setPosition: vi.fn(),
  } as unknown as Selection;

  vi.spyOn(window, "getSelection").mockReturnValue(selection);
  return selection;
}

function flushRAF(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => resolve());
  });
}

/** Mock matchMedia — pass `coarse: true` to simulate mobile (pointer: coarse). */
function mockMatchMedia(opts: { coarse?: boolean } = {}) {
  vi.spyOn(window, "matchMedia").mockImplementation((query: string) => ({
    matches: opts.coarse ? query === "(pointer: coarse)" : false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

// ── Tests ────────────────────────────────────────────────────────────────

describe("initSelectionActions", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";

    // Default: desktop (no touch)
    // Remove ontouchstart if it was set
    delete (window as any).ontouchstart;

    // Mock matchMedia to report non-coarse pointer (desktop)
    mockMatchMedia();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("does nothing when no article.container exists", () => {
    // Empty page, no article
    initSelectionActions();

    expect(document.querySelector(".sel-popover")).toBeNull();
    expect(document.querySelector(".mobile-bar")).toBeNull();
  });

  test("creates popover and mobile bar elements on article page", () => {
    createArticlePage();
    initSelectionActions();

    expect(document.querySelector(".sel-popover")).not.toBeNull();
    expect(document.querySelector(".mobile-bar")).not.toBeNull();
  });

  test("uses English labels when lang is not zh", () => {
    document.documentElement.lang = "en";
    createArticlePage();
    initSelectionActions();

    const shareBtn = document.querySelector(".sel-popover .sel-share");
    expect(shareBtn?.textContent).toBe("Share");
    const commentBtn = document.querySelector(".sel-popover .sel-comment");
    expect(commentBtn?.textContent).toBe("Comment");
    const copyBtn = document.querySelector(".sel-popover .sel-copy");
    expect(copyBtn?.textContent).toBe("Copy");

    document.documentElement.lang = "";
  });

  test("uses Chinese labels when lang is zh", () => {
    document.documentElement.lang = "zh-Hans";
    createArticlePage();
    initSelectionActions();

    const shareBtn = document.querySelector(".sel-popover .sel-share");
    expect(shareBtn?.textContent).toBe("分享");
    const copyBtn = document.querySelector(".sel-popover .sel-copy");
    expect(copyBtn?.textContent).toBe("复制");

    document.documentElement.lang = "";
  });

  test("hides Comment button when no .moss-comments section exists", () => {
    createArticlePage({ comments: false });
    initSelectionActions();

    expect(document.querySelector(".sel-popover .sel-comment")).toBeNull();
    expect(document.querySelector(".sel-popover .sel-share")).not.toBeNull();
    expect(document.querySelector(".sel-popover .sel-copy")).not.toBeNull();

    expect(document.querySelector(".mobile-bar .sel-comment")).toBeNull();
    expect(document.querySelector(".mobile-bar .sel-share")).not.toBeNull();
  });

  test("hides Comment button when article has data-comments=false", () => {
    createArticlePage({ comments: true, dataComments: "false" });
    initSelectionActions();

    expect(document.querySelector(".sel-popover .sel-comment")).toBeNull();
    expect(document.querySelector(".mobile-bar .sel-comment")).toBeNull();
  });

  test("shows Comment button when comments are enabled", () => {
    createArticlePage({ comments: true });
    initSelectionActions();

    expect(document.querySelector(".sel-popover .sel-comment")).not.toBeNull();
    expect(document.querySelector(".mobile-bar .sel-comment")).not.toBeNull();
  });

  test("desktop popover has 1 separator without comments, 2 with", () => {
    createArticlePage({ comments: false });
    initSelectionActions();
    expect(document.querySelectorAll(".sel-popover .sel-sep").length).toBe(1);

    document.body.innerHTML = "";
    document.head.innerHTML = "";
    createArticlePage({ comments: true });
    initSelectionActions();
    expect(document.querySelectorAll(".sel-popover .sel-sep").length).toBe(2);
  });
});

describe("desktop popover", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";

    delete (window as any).ontouchstart;

    mockMatchMedia();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("popover appears on mouseup with selection inside article", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    mockSelection(p, "some article body");

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover");
    expect(popover).not.toBeNull();
    expect(popover!.classList.contains("visible")).toBe(true);
  });

  test("popover does NOT appear for selections outside article", async () => {
    const { outside } = createArticlePage();
    initSelectionActions();

    mockSelection(outside, "outside text");

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover");
    expect(popover!.classList.contains("visible")).toBe(false);
  });

  test("a drag that begins outside the article and ends inside it offers the card", async () => {
    // The anchor is where the reader put the cursor DOWN, so it is at the END
    // of an upward drag — and outside the article for a drag that begins below
    // it. Gating on the anchor alone hid the popover for a selection made
    // almost entirely of this article's prose.
    const { p, outside } = createArticlePage();
    initSelectionActions();

    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.setBaseAndExtent(outside.firstChild!, 20, p.firstChild!, 4);

    const origGetRange = sel.getRangeAt.bind(sel);
    sel.getRangeAt = ((i: number) => {
      const r = origGetRange(i);
      r.getBoundingClientRect = vi.fn(() => ({
        top: 100, left: 200, width: 150, height: 20,
        bottom: 120, right: 350, x: 200, y: 100, toJSON: () => ({}),
      })) as any;
      return r;
    }) as any;

    expect(sel.anchorNode).toBe(outside.firstChild);
    expect(p.contains(sel.focusNode)).toBe(true);

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    expect(
      document.querySelector(".sel-popover")!.classList.contains("visible")
    ).toBe(true);
  });

  test("a drag that runs off the end of the article keeps only the article", async () => {
    // The other half of the same relaxation: an endpoint outside the article is
    // pulled to the article's edge, so the card cannot pick up the page chrome
    // the reader dragged across on the way out.
    const { p } = createArticlePage();
    // A real prose block outside the article — the kind of thing a site footer
    // is made of, and the kind the block collector will happily pick up.
    const footer = document.createElement("footer");
    const footerP = document.createElement("p");
    footerP.textContent = "Boilerplate outside the article.";
    footer.appendChild(footerP);
    document.body.appendChild(footer);

    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      writable: true,
      configurable: true,
    });
    initSelectionActions();

    const range = document.createRange();
    range.setStart(p.firstChild!, 0);
    range.setEnd(footerP.firstChild!, 20);
    range.getBoundingClientRect = vi.fn(() => ({
      top: 100, left: 200, width: 150, height: 20,
      bottom: 120, right: 350, x: 200, y: 100, toJSON: () => ({}),
    })) as any;
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    expect(
      document.querySelector(".sel-popover")!.classList.contains("visible")
    ).toBe(true);

    (document.querySelector(".sel-copy") as HTMLElement).click();
    await new Promise((r) => setTimeout(r, 0));
    const written = (navigator.clipboard.writeText as ReturnType<typeof vi.fn>)
      .mock.calls[0][0] as string;
    expect(written).not.toContain("outside the article");
  });

  test("popover hides when selection is cleared", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    // Show popover
    mockSelection(p, "some text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover")!;
    expect(popover.classList.contains("visible")).toBe(true);

    // Clear selection
    mockEmptySelection();
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    expect(popover.classList.contains("visible")).toBe(false);
  });

  test("popover hides on mousedown outside popover", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    // Show popover
    mockSelection(p, "some text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover")!;
    expect(popover.classList.contains("visible")).toBe(true);

    // Mousedown outside popover
    document.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true })
    );

    expect(popover.classList.contains("visible")).toBe(false);
  });

  test("popover survives a right-click (the reader is opening a context menu on the selection)", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    mockSelection(p, "some text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover")!;
    expect(popover.classList.contains("visible")).toBe(true);

    // Secondary-button mousedown outside the popover must NOT dismiss it —
    // it used to vanish the instant the context menu appeared.
    document.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, button: 2 })
    );

    expect(popover.classList.contains("visible")).toBe(true);
  });

  /**
   * Give the popover a real size and the page a real visible band.
   *
   * Both are 0 in jsdom, and both are inputs to the placement — so without
   * this a "the popover is centred" assertion passes on a 0×0 box centred on
   * itself, which is how a popover that walked off the left edge of a phone
   * shipped under a green test. Returns the popover.
   */
  function stubGeometry(width: number, height: number, band = 400): HTMLElement {
    const popover = document.querySelector(".sel-popover") as HTMLElement;
    Object.defineProperty(popover, "offsetWidth", { configurable: true, value: width });
    Object.defineProperty(popover, "offsetHeight", { configurable: true, value: height });
    Object.defineProperty(document.documentElement, "clientWidth", {
      configurable: true,
      value: band,
    });
    Object.defineProperty(document.documentElement, "clientHeight", {
      configurable: true,
      value: 700,
    });
    return popover;
  }

  afterEach(() => {
    for (const prop of ["clientWidth", "clientHeight"]) {
      Object.defineProperty(document.documentElement, prop, {
        configurable: true,
        value: 0,
      });
    }
  });

  test("popover is centred above a selection with room on both sides", async () => {
    const { p } = createArticlePage();
    initSelectionActions();
    stubGeometry(212, 36);

    mockSelection(p, "some text", { top: 200, left: 150, width: 100, height: 20 });
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover") as HTMLElement;
    expect(popover.classList.contains("visible")).toBe(true);
    // Centre of the selection is 200; a 212-wide bar starts 106 to its left.
    expect(parseFloat(popover.style.left)).toBe(94);
    expect(parseFloat(popover.style.top)).toBe(200 - 36 - 8);
  });

  test("popover stays on screen for a selection in the left margin", async () => {
    // The reported bug, at the width it was reported: 全文 near the left edge
    // of a phone put the centred bar's left edge at a negative x, and the
    // first button was simply not on the screen.
    const { p } = createArticlePage();
    initSelectionActions();
    stubGeometry(212, 36, 390);

    mockSelection(p, "some text", { top: 300, left: 16, width: 40, height: 20 });
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover") as HTMLElement;
    expect(parseFloat(popover.style.left)).toBe(8);
  });

  test("popover stays on screen for a selection at the right edge", async () => {
    const { p } = createArticlePage();
    initSelectionActions();
    stubGeometry(212, 36, 390);

    mockSelection(p, "some text", { top: 300, left: 330, width: 50, height: 20 });
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover") as HTMLElement;
    expect(parseFloat(popover.style.left)).toBe(390 - 8 - 212);
  });

  test("popover drops below a selection on the first visible line", async () => {
    // Above would be off the top of the window, and pinning it to the top
    // gutter would cover the very words the reader highlighted.
    const { p } = createArticlePage();
    initSelectionActions();
    stubGeometry(212, 36);

    mockSelection(p, "some text", { top: 4, left: 150, width: 100, height: 20 });
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover") as HTMLElement;
    expect(parseFloat(popover.style.top)).toBe(24 + 8);
  });
});

describe("copy with attribution", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";

    delete (window as any).ontouchstart;

    mockMatchMedia();

    // Mock clipboard API
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("title comes from og:title — ' - ' in a real title survives", async () => {
    document.documentElement.lang = "en";
    const { p } = createArticlePage({ author: "Alice", title: "Part One" });
    const og = document.createElement("meta");
    og.setAttribute("property", "og:title");
    og.setAttribute("content", "Part One - A Memoir");
    document.head.appendChild(og);
    initSelectionActions();

    mockSelection(p, "selected text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const copyBtn = document.querySelector(".sel-copy") as HTMLElement;
    copyBtn.click();
    await new Promise((r) => setTimeout(r, 0));

    const written = (navigator.clipboard.writeText as ReturnType<typeof vi.fn>)
      .mock.calls[0][0] as string;
    expect(written).toContain("Part One - A Memoir");
  });

  test("a stanza break the poet typed survives the copy", async () => {
    // The card draws the blank line; the copied text used to close it up,
    // because dropping empty lines is indistinguishable from dropping the
    // unselected remainder of a block when you filter line by line.
    const { p } = createArticlePage({ author: "Alice", title: "My Post" });
    p.innerHTML = "first line<br />\n<br />\nsecond stanza";
    initSelectionActions();

    const range = document.createRange();
    range.selectNodeContents(p);
    range.getBoundingClientRect = vi.fn(() => ({
      top: 100, left: 200, width: 150, height: 20,
      bottom: 120, right: 350, x: 200, y: 100, toJSON: () => ({}),
    })) as any;
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();
    (document.querySelector(".sel-copy") as HTMLElement).click();
    await new Promise((r) => setTimeout(r, 0));

    const written = (navigator.clipboard.writeText as ReturnType<typeof vi.fn>)
      .mock.calls[0][0] as string;
    expect(written).toContain("first line\n\nsecond stanza");
  });

  test("copy uses Chinese formatting when lang is zh", async () => {
    document.documentElement.lang = "zh-Hans";
    const { p } = createArticlePage({ author: "Alice", title: "My Post" });
    initSelectionActions();

    mockSelection(p, "selected text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const copyBtn = document.querySelector(".sel-copy") as HTMLElement;
    copyBtn.click();
    await new Promise((r) => setTimeout(r, 0));

    expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1);
    const written = (navigator.clipboard.writeText as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;

    // Chinese: 「text」\n—— author，title\nurl
    expect(written).toContain("\u300Cselected text\u300D");
    expect(written).toContain("\u2014\u2014"); // ——
    expect(written).toContain("\uFF0C"); // ，

    document.documentElement.lang = "";
  });

  test("copy uses English formatting when lang is not zh", async () => {
    document.documentElement.lang = "en";
    const { p } = createArticlePage({ author: "Alice", title: "My Post" });
    initSelectionActions();

    mockSelection(p, "selected text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const copyBtn = document.querySelector(".sel-copy") as HTMLElement;
    copyBtn.click();
    await new Promise((r) => setTimeout(r, 0));

    expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1);
    const written = (navigator.clipboard.writeText as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;

    // English: "text"\n— author, title\nurl
    expect(written).toContain("\u201Cselected text\u201D");
    expect(written).not.toContain("\u2014\u2014"); // NOT ——
    expect(written).toContain("\u2014 "); // — (single em-dash + space)
    expect(written).toContain(", "); // ASCII comma

    document.documentElement.lang = "";
  });
});

describe("mobile bar", () => {
  beforeEach(() => {
    // The selectionchange handler debounces 300ms; four real waits on that was
    // 1.4s of the suite. Installed here, before any handler is wired.
    vi.useFakeTimers();
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";

    // Simulate mobile: set ontouchstart
    (window as any).ontouchstart = null;

    mockMatchMedia({ coarse: true });
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    delete (window as any).ontouchstart;
    vi.restoreAllMocks();
  });

  test("mobile bar appears instead of popover on touch device", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    mockSelection(p, "some text");

    // On mobile, selectionchange is used
    document.dispatchEvent(new Event("selectionchange"));

    // Past the 300ms selectionchange debounce.
    await vi.advanceTimersByTimeAsync(350);

    const mobileBar = document.querySelector(".mobile-bar");
    expect(mobileBar).not.toBeNull();
    expect(mobileBar!.classList.contains("visible")).toBe(true);

    // Popover should NOT be visible
    const popover = document.querySelector(".sel-popover");
    expect(popover!.classList.contains("visible")).toBe(false);
  });

  test("mobile bar hides when selection is cleared", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    // Show mobile bar
    mockSelection(p, "some text");
    document.dispatchEvent(new Event("selectionchange"));
    await vi.advanceTimersByTimeAsync(350);

    const mobileBar = document.querySelector(".mobile-bar")!;
    expect(mobileBar.classList.contains("visible")).toBe(true);

    // Clear selection
    mockEmptySelection();
    document.dispatchEvent(new Event("selectionchange"));
    await vi.advanceTimersByTimeAsync(350);

    expect(mobileBar.classList.contains("visible")).toBe(false);
  });
});

// ── Scroll dismiss ──────────────────────────────────────────────────────

describe("scroll dismiss", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    delete (window as any).ontouchstart;
    mockMatchMedia();
  });

  afterEach(() => {
    // Only the mobile test in this block installs them, but restoring
    // unconditionally keeps a failed assertion from leaking a fake clock.
    vi.useRealTimers();
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("popover hides on scroll (desktop)", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    // Show popover
    mockSelection(p, "some text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover")!;
    expect(popover.classList.contains("visible")).toBe(true);

    // Scroll should dismiss
    document.dispatchEvent(new Event("scroll"));

    expect(popover.classList.contains("visible")).toBe(false);
  });

  test("mobile bar hides on scroll (mobile)", async () => {
    // Fake timers are scoped to THIS test, not the block: its sibling waits on
    // a real requestAnimationFrame, which vitest fakes by default — a faked
    // rAF never fires and that test would hang instead of run.
    vi.useFakeTimers();
    // Switch to mobile mode
    (window as any).ontouchstart = null;
    mockMatchMedia({ coarse: true });

    const { p } = createArticlePage();
    initSelectionActions();

    // Show mobile bar
    mockSelection(p, "some text");
    document.dispatchEvent(new Event("selectionchange"));
    await vi.advanceTimersByTimeAsync(350);

    const mobileBar = document.querySelector(".mobile-bar")!;
    expect(mobileBar.classList.contains("visible")).toBe(true);

    // Scroll should dismiss
    document.dispatchEvent(new Event("scroll"));

    expect(mobileBar.classList.contains("visible")).toBe(false);
  });
});

// ── Comment event dispatch ──────────────────────────────────────────────

describe("comment button", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    delete (window as any).ontouchstart;
    mockMatchMedia();
  });

  afterEach(() => {
    document.body.innerHTML = "";
    document.body.className = "";
    document.head.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("dispatches moss:quote-comment CustomEvent with selected text", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    mockSelection(p, "quoted passage");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const commentBtn = document.querySelector(
      ".sel-popover .sel-comment"
    ) as HTMLElement;
    expect(commentBtn).not.toBeNull();

    let receivedText = "";
    document.addEventListener("moss:quote-comment", ((e: CustomEvent) => {
      receivedText = e.detail.text;
    }) as EventListener);

    commentBtn.click();

    expect(receivedText).toBe("quoted passage");
  });

  test("selected heading text reaches actions without the anchor #", async () => {
    const { article } = createArticlePage();

    // Real moss heading markup: text + permalink anchor with aria-hidden "#"
    const h = document.createElement("h2");
    h.textContent = "二、禅修与生活";
    const a = document.createElement("a");
    a.className = "moss-heading-anchor";
    a.setAttribute("href", "#x");
    const span = document.createElement("span");
    span.setAttribute("aria-hidden", "true");
    span.textContent = "#";
    a.appendChild(span);
    h.appendChild(a);
    article.appendChild(h);

    initSelectionActions();

    // Real selection spanning the WHOLE heading element — raw
    // Range.toString() includes the "#", the capture path must not.
    const range = document.createRange();
    range.setStart(h, 0);
    range.setEnd(h, h.childNodes.length);
    // jsdom has no layout, so Range.getBoundingClientRect is unimplemented and
    // the popover-positioning path would throw inside the rAF — an unhandled
    // error that fails the run even though every assertion passes. The helper
    // above stubs it for ranges it builds; this hand-built one needs the same.
    range.getBoundingClientRect = vi.fn(() => ({
      top: 100,
      left: 50,
      width: 100,
      height: 20,
      bottom: 120,
      right: 150,
      x: 50,
      y: 100,
      toJSON: () => ({}),
    })) as unknown as Range["getBoundingClientRect"];
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    let receivedText = "";
    document.addEventListener("moss:quote-comment", ((e: CustomEvent) => {
      receivedText = e.detail.text;
    }) as EventListener);
    (document.querySelector(".sel-popover .sel-comment") as HTMLElement).click();

    expect(receivedText).toBe("二、禅修与生活");
  });

  test("hides popover after dispatching comment event", async () => {
    const { p } = createArticlePage();
    initSelectionActions();

    mockSelection(p, "some text");
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    await flushRAF();

    const popover = document.querySelector(".sel-popover")!;
    expect(popover.classList.contains("visible")).toBe(true);

    const commentBtn = popover.querySelector(".sel-comment") as HTMLElement;
    commentBtn.click();

    expect(popover.classList.contains("visible")).toBe(false);
  });
});

// ── Context extraction (captureContext) ──────────────────────────────────

describe("captureBlocks", () => {
  /**
   * Helper: create paragraphs within a container, select a substring within
   * one paragraph, and return the Selection. Uses real DOM ranges.
   */
  function buildParagraphs(...texts: string[]): HTMLElement[] {
    const els: HTMLElement[] = [];
    for (const text of texts) {
      const p = document.createElement("p");
      p.textContent = text;
      document.body.appendChild(p);
      els.push(p);
    }
    return els;
  }

  function selectRange(
    startNode: Node,
    startOffset: number,
    endNode: Node,
    endOffset: number
  ): Selection {
    const range = document.createRange();
    range.setStart(startNode, startOffset);
    range.setEnd(endNode, endOffset);

    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    return sel;
  }

  afterEach(() => {
    document.body.innerHTML = "";
    window.getSelection()?.removeAllRanges();
  });

  // 1. Mid-paragraph selection
  test("mid-paragraph selection returns 1 block with prefix + highlighted + suffix segments", () => {
    const fullText = "在书中，他提到什么应当被计算机自动化：你不该去自动化你喜欢做的事。";
    const selected = "你不该去自动化你喜欢做的事。";
    const [p] = buildParagraphs(fullText);

    const textNode = p.firstChild!;
    const start = fullText.indexOf(selected);
    const sel = selectRange(textNode, start, textNode, start + selected.length);

    const blocks = captureBlocks(sel);

    // Should have at least 1 block for the selection paragraph
    const selectionBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted));
    expect(selectionBlock).toBeDefined();
    expect(selectionBlock!.tag).toBe("P");

    // Find the highlighted segment
    const highlighted = selectionBlock!.lines.flat().filter(s => s.highlighted);
    expect(highlighted).toHaveLength(1);
    expect(highlighted[0].text).toBe(selected);

    // The prefix segment
    const prefix = selectionBlock!.lines.flat().filter(s => !s.highlighted);
    expect(prefix.length).toBeGreaterThanOrEqual(1);
    expect(prefix[0].text).toBe("在书中，他提到什么应当被计算机自动化：");
  });

  // 2. Full paragraph selection
  test("full paragraph selection returns 1 highlighted block + sibling blocks", () => {
    const [p1, p2, p3] = buildParagraphs(
      "Previous paragraph context.",
      "The entire paragraph is selected.",
      "Following paragraph context."
    );

    const textNode = p2.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);

    // Should have the selection block with only a highlighted segment
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted));
    expect(selBlock).toBeDefined();
    expect(selBlock!.lines.flat()).toHaveLength(1);
    expect(selBlock!.lines.flat()[0].highlighted).toBe(true);
    expect(selBlock!.lines.flat()[0].text).toBe("The entire paragraph is selected.");

    // Should have sibling blocks (before and/or after)
    expect(blocks.length).toBeGreaterThanOrEqual(2); // at least selection + one sibling
  });

  // 3. Selection at paragraph end — should pull next sibling
  test("selection at paragraph end pulls next sibling for after-context", () => {
    const [p1, p2] = buildParagraphs(
      "Start of paragraph, then the selected part.",
      "Next paragraph provides after context."
    );

    const textNode = p1.firstChild!;
    const fullText = p1.textContent!;
    const selected = "the selected part.";
    const start = fullText.indexOf(selected);
    const sel = selectRange(textNode, start, textNode, fullText.length);

    const blocks = captureBlocks(sel);

    // Selection block: prefix (unhighlighted) + selected (highlighted), no suffix
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted))!;
    expect(selBlock.lines.flat().find(s => s.highlighted)!.text).toBe(selected);

    // Should have an after-sibling block
    const afterBlock = blocks.find(b =>
      !b.lines.flat().some(s => s.highlighted) &&
      b.lines.flat().some(s => s.text.includes("Next paragraph"))
    );
    expect(afterBlock).toBeDefined();
  });

  // 4. Selection at paragraph start — should pull previous sibling
  test("selection at paragraph start pulls previous sibling for before-context", () => {
    const [p1, p2] = buildParagraphs(
      "Previous paragraph provides before context.",
      "Selected text at start, then more text follows."
    );

    const textNode = p2.firstChild!;
    const selected = "Selected text at start,";
    const sel = selectRange(textNode, 0, textNode, selected.length);

    const blocks = captureBlocks(sel);

    // Should have a before-sibling block
    const beforeBlock = blocks.find(b =>
      !b.lines.flat().some(s => s.highlighted) &&
      b.lines.flat().some(s => s.text.includes("Previous paragraph"))
    );
    expect(beforeBlock).toBeDefined();

    // Selection block should have highlighted + suffix
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted))!;
    expect(selBlock.lines.flat().find(s => s.highlighted)!.text).toBe(selected);
    expect(selBlock.lines.flat().some(s => !s.highlighted && s.text.includes("then more text"))).toBe(true);
  });

  // 5. Multi-paragraph selection
  test("multi-paragraph selection returns separate block for each paragraph", () => {
    const [p1, p2, p3, p4] = buildParagraphs(
      "Before sibling.",
      "First selected paragraph content.",
      "Second selected paragraph content.",
      "After sibling."
    );

    const startNode = p2.firstChild!;
    const endNode = p3.firstChild!;
    const sel = selectRange(startNode, 6, endNode, 15);
    // Selected in p2: "selected paragraph content."
    // Selected in p3: "Second selected"

    const blocks = captureBlocks(sel);

    // Should have blocks with highlighted segments from both paragraphs
    const highlightedBlocks = blocks.filter(b => b.lines.flat().some(s => s.highlighted));
    expect(highlightedBlocks).toHaveLength(2);

    // First highlighted block: has prefix "First " + highlighted rest
    expect(highlightedBlocks[0].lines.flat().some(s => !s.highlighted && s.text === "First ")).toBe(true);
    expect(highlightedBlocks[0].lines.flat().some(s => s.highlighted)).toBe(true);

    // Second highlighted block: has highlighted part + suffix
    expect(highlightedBlocks[1].lines.flat().some(s => s.highlighted)).toBe(true);
    expect(highlightedBlocks[1].lines.flat().some(s => !s.highlighted && s.text.includes("paragraph content."))).toBe(true);
  });

  // 6. Sibling clause truncation (before sibling)
  test("before-sibling is truncated to whole clauses nearest to quote", () => {
    const [p1, p2] = buildParagraphs(
      "句子一。句子二。句子三。",
      "Selected text here."
    );

    const textNode = p2.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);

    // Before-sibling should contain clauses from the END of p1
    const beforeBlock = blocks.find(b =>
      !b.lines.flat().some(s => s.highlighted) &&
      blocks.indexOf(b) < blocks.findIndex(b2 => b2.lines.flat().some(s => s.highlighted))
    );
    expect(beforeBlock).toBeDefined();
    // Should include "句子三。" at minimum (nearest to quote)
    const beforeText = beforeBlock!.lines.flat().map(s => s.text).join("");
    expect(beforeText).toContain("句子三。");
  });

  // 7. Sibling too long, no clause boundary — omitted entirely
  test("sibling with no clause boundary within budget is omitted", () => {
    // Create a long sibling with no clause-ending punctuation at all
    const longNoPunct = "A".repeat(300);
    const [p1, p2] = buildParagraphs(longNoPunct, "Selected text.");

    const textNode = p2.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);

    // The before-sibling should be omitted (no clean clause break)
    const beforeBlocks = blocks.filter(b =>
      !b.lines.flat().some(s => s.highlighted) &&
      blocks.indexOf(b) < blocks.findIndex(b2 => b2.lines.flat().some(s => s.highlighted))
    );
    expect(beforeBlocks).toHaveLength(0);
  });

  // 8. Budget respects same-block prefix first
  test("long same-block prefix consumes budget, leaving little for siblings", () => {
    // Same-block prefix of 150 chars leaves ~50 for siblings (budget is ~200 total)
    const prefix = "X".repeat(150);
    const [p0, p1] = buildParagraphs(
      "Sibling before paragraph with context.",
      prefix + "SELECTED"
    );

    const textNode = p1.firstChild!;
    const fullText = p1.textContent!;
    const start = fullText.indexOf("SELECTED");
    const sel = selectRange(textNode, start, textNode, fullText.length);

    const blocks = captureBlocks(sel);

    // The same-block prefix should be shown in full
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted))!;
    const prefixSeg = selBlock.lines.flat().find(s => !s.highlighted);
    expect(prefixSeg).toBeDefined();
    expect(prefixSeg!.text).toBe(prefix);

    // With 150 consumed, remaining budget is ~50 which may not fit a full clause from sibling
    // The before-sibling may be omitted or heavily truncated
    const totalNonHighlighted = blocks
      .flatMap(b => b.lines.flat())
      .filter(s => !s.highlighted)
      .reduce((sum, s) => sum + s.text.length, 0);
    expect(totalNonHighlighted).toBeLessThanOrEqual(250); // generous upper bound
  });

  // 9. After sibling truncation — clauses from start
  test("after-sibling is truncated to whole clauses from the start", () => {
    const [p1, p2] = buildParagraphs(
      "Selected text here.",
      "First clause，second clause。third clause。"
    );

    const textNode = p1.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);

    // After-sibling should contain clauses from the START of p2
    const afterBlock = blocks.find(b =>
      !b.lines.flat().some(s => s.highlighted) &&
      blocks.indexOf(b) > blocks.findIndex(b2 => b2.lines.flat().some(s => s.highlighted))
    );
    expect(afterBlock).toBeDefined();
    const afterText = afterBlock!.lines.flat().map(s => s.text).join("");
    // Should start from the beginning of the sibling
    expect(afterText.startsWith("First clause")).toBe(true);
  });

  // 10. Tag detection
  test("block tag reflects the element type", () => {
    const h2 = document.createElement("h2");
    h2.textContent = "Heading text to select";
    document.body.appendChild(h2);

    const textNode = h2.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted));
    expect(selBlock).toBeDefined();
    expect(selBlock!.tag).toBe("H2");
  });

  // 11. Empty/collapsed selection returns empty array
  test("empty selection returns empty array", () => {
    buildParagraphs("Some text.");
    // Don't select anything
    const sel = window.getSelection()!;
    sel.removeAllRanges();

    const blocks = captureBlocks(sel);
    expect(blocks).toEqual([]);
  });

  // 12. Max 1 sibling per side
  test("at most 1 sibling block per side", () => {
    const [p1, p2, p3, p4, p5] = buildParagraphs(
      "Far before.",
      "Near before.",
      "Selected text.",
      "Near after.",
      "Far after."
    );

    const textNode = p3.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);

    // Should have at most 3 blocks: before-sibling, selection, after-sibling
    expect(blocks.length).toBeLessThanOrEqual(3);

    // Verify no "Far before" or "Far after" content
    const allText = blocks.flatMap(b => b.lines.flat()).map(s => s.text).join("");
    expect(allText).not.toContain("Far before");
    expect(allText).not.toContain("Far after");
  });

  // 13. Segments within a block are ordered correctly
  test("segments are ordered: prefix, highlighted, suffix within a block", () => {
    const fullText = "Before the quote, the highlighted part, after the quote.";
    const selected = "the highlighted part,";
    const [p] = buildParagraphs(fullText);

    const textNode = p.firstChild!;
    const start = fullText.indexOf(selected);
    const sel = selectRange(textNode, start, textNode, start + selected.length);

    const blocks = captureBlocks(sel);
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted))!;

    // Order: unhighlighted prefix, highlighted, unhighlighted suffix
    expect(selBlock.lines.flat().length).toBe(3);
    expect(selBlock.lines.flat()[0].highlighted).toBe(false);
    expect(selBlock.lines.flat()[0].text).toBe("Before the quote, ");
    expect(selBlock.lines.flat()[1].highlighted).toBe(true);
    expect(selBlock.lines.flat()[1].text).toBe(selected);
    expect(selBlock.lines.flat()[2].highlighted).toBe(false);
    expect(selBlock.lines.flat()[2].text).toBe(" after the quote.");
  });

  // 14. LI elements are recognized
  test("selection inside LI returns LI tag", () => {
    const ul = document.createElement("ul");
    const li = document.createElement("li");
    li.textContent = "List item text";
    ul.appendChild(li);
    document.body.appendChild(ul);

    const textNode = li.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted));
    expect(selBlock).toBeDefined();
    expect(selBlock!.tag).toBe("LI");
  });

  // 15. BLOCKQUOTE elements are recognized
  test("selection inside BLOCKQUOTE returns BLOCKQUOTE tag", () => {
    const bq = document.createElement("blockquote");
    bq.textContent = "Quoted text content";
    document.body.appendChild(bq);

    const textNode = bq.firstChild!;
    const sel = selectRange(textNode, 0, textNode, textNode.textContent!.length);

    const blocks = captureBlocks(sel);
    const selBlock = blocks.find(b => b.lines.flat().some(s => s.highlighted));
    expect(selBlock).toBeDefined();
    expect(selBlock!.tag).toBe("BLOCKQUOTE");
  });
});

// ── Truncation helpers (truncateFromEnd / truncateFromStart) ──────────

describe("truncateFromEnd", () => {
  test("returns full text when it fits within budget", () => {
    expect(truncateFromEnd("abc", 5)).toBe("abc");
  });
  test("returns null when no clause boundary in window", () => {
    expect(truncateFromEnd("no punct here", 5)).toBeNull();
  });
  test("returns last clause after punctuation in window", () => {
    expect(truncateFromEnd("a,b,c", 3)).toBe("c");
  });
  test("handles CJK punctuation", () => {
    expect(truncateFromEnd("句子一。句子二。句子三", 6)).toBe("句子三");
  });
});

describe("truncateFromStart", () => {
  test("returns full text when it fits within budget", () => {
    expect(truncateFromStart("abc", 5)).toBe("abc");
  });
  test("returns null when no clause boundary in window", () => {
    expect(truncateFromStart("no punct here", 5)).toBeNull();
  });
  test("returns first clause up to punctuation", () => {
    expect(truncateFromStart("a,b,c", 3)).toBe("a,");
  });
  test("handles CJK punctuation", () => {
    expect(truncateFromStart("句子一。句子二。句子三", 6)).toBe("句子一。");
  });
});

// ── Extraction junk stripping ────────────────────────────────────────────

describe("extraction junk stripping", () => {
  function selectRange(
    startNode: Node,
    startOffset: number,
    endNode: Node,
    endOffset: number
  ): Selection {
    const range = document.createRange();
    range.setStart(startNode, startOffset);
    range.setEnd(endNode, endOffset);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    return sel;
  }

  function makeHeading(text: string): HTMLElement {
    // Mirrors moss-core render_heading output:
    // <h2 id>text<a class="moss-heading-anchor"><span aria-hidden>#</span></a></h2>
    const h = document.createElement("h2");
    h.textContent = text;
    const a = document.createElement("a");
    a.className = "moss-heading-anchor";
    a.setAttribute("href", "#x");
    a.setAttribute("aria-label", "Permalink to this section");
    const span = document.createElement("span");
    span.setAttribute("aria-hidden", "true");
    span.textContent = "#";
    a.appendChild(span);
    h.appendChild(a);
    return h;
  }

  afterEach(() => {
    document.body.innerHTML = "";
    window.getSelection()?.removeAllRanges();
  });

  test("before-sibling heading context sheds the anchor #", () => {
    document.body.appendChild(makeHeading("二、"));
    const p = document.createElement("p");
    p.textContent = "按宗派，纽约禅堂算是临济宗。";
    document.body.appendChild(p);

    const sel = selectRange(p.firstChild!, 0, p.firstChild!, 12);
    const blocks = captureBlocks(sel);

    expect(blocks[0].tag).toBe("H2");
    expect(blocks[0].lines.flat()[0].text).toBe("二、");
  });

  test("heading inside a multi-block selection sheds the anchor #", () => {
    const p1 = document.createElement("p");
    p1.textContent = "对大家表示欢迎之后。";
    const h = makeHeading("二、");
    const p2 = document.createElement("p");
    p2.textContent = "按宗派。";
    document.body.append(p1, h, p2);

    const sel = selectRange(p1.firstChild!, 0, p2.firstChild!, 4);
    const blocks = captureBlocks(sel);

    const headingBlock = blocks.find((b) => b.tag === "H2")!;
    const joined = headingBlock.lines.flat().map((s) => s.text).join("");
    expect(joined).toBe("二、");
  });

  test("math svg <title> TeX source never reaches block text, offsets stay aligned", () => {
    const p = document.createElement("p");
    p.appendChild(document.createTextNode("状态 "));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("class", "moss-math");
    svg.setAttribute("role", "img");
    const t = document.createElementNS("http://www.w3.org/2000/svg", "title");
    t.textContent = "S_t";
    svg.appendChild(t);
    p.appendChild(svg);
    p.appendChild(document.createTextNode("的演化很有趣，值得研究。"));
    document.body.appendChild(p);

    // Select "的演化" — the boundary sits AFTER the svg <title> text node,
    // so an unmapped raw offset would shift the highlight by 3 chars.
    const tail = p.childNodes[2];
    const sel = selectRange(tail, 0, tail, 3);
    const blocks = captureBlocks(sel);

    const blk = blocks.find((b) => b.lines.flat().some((s) => s.highlighted))!;
    const joined = blk.lines.flat().map((s) => s.text).join("");
    expect(joined).toBe("状态 的演化很有趣，值得研究。");
    expect(joined).not.toContain("S_t");
    expect(blk.lines.flat().find((s) => s.highlighted)!.text).toBe("的演化");
  });
});

// ── The author's line breaks ─────────────────────────────────────────────

describe("hard line breaks survive capture", () => {
  function selectAll(node: Node): Selection {
    const range = document.createRange();
    range.selectNodeContents(node);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    return sel;
  }

  afterEach(() => {
    document.body.innerHTML = "";
    window.getSelection()?.removeAllRanges();
  });

  test("a poem arrives as one line per verse, not one run-on line", () => {
    // What moss emits for a three-line stanza: hard_line_breaks is on by
    // default, so each newline the poet typed is a <br /> plus a source
    // newline the browser collapses away.
    const p = document.createElement("p");
    p.innerHTML = "一行诗<br />\n二行诗<br />\n三行诗";
    document.body.appendChild(p);

    const blocks = captureBlocks(selectAll(p));
    const blk = blocks.find((b) => b.lines.flat().some((s) => s.highlighted))!;

    expect(blk.lines.map((l) => l.map((s) => s.text).join(""))).toEqual([
      "一行诗",
      "二行诗",
      "三行诗",
    ]);
  });

  test("an emoji earlier in the paragraph does not shift the highlight", () => {
    // Range offsets count UTF-16 code units; an emoji is two of them. A map
    // built one entry per *code point* runs one short from the emoji onward,
    // so every later boundary lands a character early and the card highlights
    // "orld" where the reader selected "world".
    const p = document.createElement("p");
    p.textContent = "😀 hello world";
    document.body.appendChild(p);

    const text = p.firstChild!;
    const range = document.createRange();
    range.setStart(text, 9); // "world"
    range.setEnd(text, 14);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    const blocks = captureBlocks(sel);
    const blk = blocks.find((b) => b.lines.flat().some((s) => s.highlighted))!;
    const highlighted = blk.lines
      .flat()
      .filter((s) => s.highlighted)
      .map((s) => s.text)
      .join("");
    expect(highlighted).toBe("world");
  });

  test("a paragraph wrapped across source lines stays one line", () => {
    const p = document.createElement("p");
    p.innerHTML = "one sentence\n   continued here";
    document.body.appendChild(p);

    const blocks = captureBlocks(selectAll(p));
    const blk = blocks.find((b) => b.lines.flat().some((s) => s.highlighted))!;

    expect(blk.lines).toHaveLength(1);
    expect(blk.lines[0].map((s) => s.text).join("")).toBe(
      "one sentence continued here"
    );
  });

  test("a list item carries the marker the page shows", () => {
    const ol = document.createElement("ol");
    ol.setAttribute("start", "3");
    ol.innerHTML = "<li>third</li><li>fourth</li>";
    document.body.appendChild(ol);
    const ul = document.createElement("ul");
    ul.innerHTML = "<li>bullet</li>";
    document.body.appendChild(ul);

    const selected = (blocks: CardBlock[]) =>
      blocks.find((b) => b.lines.flat().some((s) => s.highlighted))!;

    const items = document.querySelectorAll("li");
    expect(selected(captureBlocks(selectAll(items[1]))).marker).toBe("4.");
    expect(selected(captureBlocks(selectAll(items[2]))).marker).toBe("•");
  });
});
