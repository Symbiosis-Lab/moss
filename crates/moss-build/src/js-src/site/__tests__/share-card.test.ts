/**
 * Tests for share-card v2 — Canvas-rendered quote card with context layout,
 * theme support, and lightbox fallback.
 *
 * jsdom does not support Canvas, so we mock:
 * - HTMLCanvasElement.prototype.getContext -> returns a mock 2D context
 * - HTMLCanvasElement.prototype.toBlob -> calls callback with a fake Blob
 * - HTMLCanvasElement.prototype.toDataURL -> returns a data URI string
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import {
  buildCardCanvas,
  findCoverSource,
  formatCardTitle,
  buildPalette,
  shareAsImage,
  extractDomain,
  wrapText,
  wrapSegments,
  addCjkLatinSpacing,
  drawJustifiedLine,
  drawJustifiedWrappedLine,
  findQrSource,
} from "../share-card";
import { buildVerticalCardCanvas } from "../share-card/vertical";
import type { CardSegment, CardBlock } from "../selection-actions";
import type { LineSpan, WrappedLine } from "../share-card";

// ── Canvas mocks ─────────────────────────────────────────────────────────

function createMockContext(): Record<string, any> {
  return {
    scale: vi.fn(),
    fillRect: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn((text: string) => ({ width: text.length * 10 })),
    drawImage: vi.fn(),
    fillStyle: "",
    font: "",
    textBaseline: "",
    textAlign: "",
    strokeStyle: "",
    lineWidth: 1,
    globalAlpha: 1.0,
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
    strokeRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
  };
}

let mockCtx: Record<string, any>;

beforeEach(() => {
  mockCtx = createMockContext();

  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
    mockCtx as unknown as CanvasRenderingContext2D
  );

  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation(
    function (this: HTMLCanvasElement, cb: BlobCallback) {
      cb(new Blob(["fake-png"], { type: "image/png" }));
    }
  );

  vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue(
    "data:image/png;base64,FAKE"
  );

  // Mock fetch (for QR loading — returns 404 by default)
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    new Response("", { status: 404 })
  );

  // Mock ClipboardItem (not available in jsdom)
  if (typeof globalThis.ClipboardItem === "undefined") {
    (globalThis as any).ClipboardItem = class ClipboardItem {
      constructor(public items: Record<string, Blob>) {}
    };
  }

  // Mock URL.createObjectURL/revokeObjectURL (not available in jsdom)
  if (typeof URL.createObjectURL !== "function") {
    URL.createObjectURL = vi.fn().mockReturnValue("blob:fake");
    URL.revokeObjectURL = vi.fn();
  }

  // Default to light mode
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.lang = "zh";
});

afterEach(() => {
  document.body.innerHTML = "";
  // Clean up <head> elements added during tests (canonical, og:url, etc.)
  document.querySelectorAll('link[rel="canonical"], meta[property="og:url"]').forEach(el => el.remove());
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.lang = "";
  vi.restoreAllMocks();
});

// ── buildCardCanvas basics ───────────────────────────────────────────────

describe("buildCardCanvas", () => {
  test("returns an HTMLCanvasElement", () => {
    const canvas = buildCardCanvas(
      "Hello world",
      "My Post",
      "Alice"
    );
    expect(canvas).toBeInstanceOf(HTMLCanvasElement);
  });

  test("canvas has retina dimensions (2x) with 375 logical width", () => {
    const canvas = buildCardCanvas(
      "Hello world",
      "My Post",
      "Alice"
    );
    expect(canvas.width).toBe(750); // 375 * 2
    expect(canvas.height).toBeGreaterThanOrEqual(600); // 300 * 2 minimum
  });

  test("canvas style dimensions match logical size", () => {
    const canvas = buildCardCanvas(
      "Short text",
      "Title",
      "Author"
    );
    expect(canvas.style.width).toBe("375px");
  });

  test("scales context for retina", () => {
    buildCardCanvas("Text", "Title", "Author");
    expect(mockCtx.scale).toHaveBeenCalledWith(2, 2);
  });

  test("draws background fill", () => {
    buildCardCanvas(
      "Selected quote",
      "Article Title",
      "Author Name"
    );
    // Background fill should be the first fillRect call
    expect(mockCtx.fillRect).toHaveBeenCalled();
  });

  test("draws title and author in bottom bar", () => {
    buildCardCanvas(
      "Selected quote",
      "Article Title",
      "Author Name"
    );

    const allText = mockCtx.fillText.mock.calls
      .map((c: any[]) => c[0])
      .join(" ");
    expect(allText).toContain("Article Title");
    expect(allText).toContain("Author Name");
  });
});

// ── Short text mode (<=40 chars) ─────────────────────────────────────────

describe("short text mode", () => {
  const shortText = "Short quote"; // 11 chars

  test("wraps text in corner brackets", () => {
    buildCardCanvas(shortText, "Title", "Author");

    const textCalls = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const bodyText = textCalls.find(
      (t: string) => t.includes("\u300C") && t.includes(shortText)
    );
    expect(bodyText).toBeDefined();
    expect(bodyText).toContain("\u300C");
    expect(bodyText).toContain("\u300D");
  });

  test("centers text horizontally", () => {
    buildCardCanvas(shortText, "Title", "Author");

    // The text containing the quote should be positioned via measureText centering
    const quoteCalls = mockCtx.fillText.mock.calls.filter((c: any[]) =>
      c[0].includes("\u300C")
    );
    expect(quoteCalls.length).toBeGreaterThan(0);
    // X position should not be the padding (36) — it should be centered
    for (const call of quoteCalls) {
      const x = call[1];
      // Centered text won't be at the exact padding value
      expect(typeof x).toBe("number");
    }
  });

  test("draws at full alpha", () => {
    buildCardCanvas(shortText, "Title", "Author");

    // After drawing the quote, globalAlpha should be 1.0
    // We check the mock context's globalAlpha field
    expect(mockCtx.globalAlpha).toBe(1.0);
  });
});

// ── Long text mode (>40 chars) ───────────────────────────────────────────

describe("long text mode", () => {
  const longText =
    "This is a longer piece of text that exceeds forty characters for testing context layout";

  test("draws selected text without corner brackets", () => {
    buildCardCanvas(longText, "Title", "Author");

    const textCalls = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    // No corner brackets in long mode
    const bracketCalls = textCalls.filter(
      (t: string) => t.includes("\u300C") || t.includes("\u300D")
    );
    expect(bracketCalls).toHaveLength(0);
  });

  // The two tests that used to sit here passed a `context:` option that the
  // card no longer has, so they asserted nothing block rendering could break —
  // both stayed green with `drawBlocks` deleted. Opacity is covered for real in
  // "block-aware rendering", against `blocks`.
});

// ── Cover image ──────────────────────────────────────────────────────────

describe("cover source policy", () => {
  test("og:image meta alone yields no cover — generated og cards are not covers", () => {
    const meta = document.createElement("meta");
    meta.setAttribute("property", "og:image");
    meta.setAttribute("content", "https://example.com/_moss/og/abc.png");
    document.head.appendChild(meta);
    try {
      expect(findCoverSource()).toBeNull();
    } finally {
      meta.remove();
    }
  });

  test("the build's data-share-cover on article.container is the one cover source", () => {
    const article = document.createElement("article");
    article.className = "container";
    article.setAttribute("data-share-cover", "/posts/a/assets/hero.jpg");
    document.body.appendChild(article);
    try {
      expect(findCoverSource()).toBe(
        new URL("/posts/a/assets/hero.jpg", location.href).href,
      );
    } finally {
      article.remove();
    }
  });

  test("an article without the attribute has no cover", () => {
    // Absent means "no cover strip" — the page chose no cover, and nothing
    // (least of all the generated og card) stands in for it.
    const article = document.createElement("article");
    article.className = "container";
    document.body.appendChild(article);
    try {
      expect(findCoverSource()).toBeNull();
    } finally {
      article.remove();
    }
  });
});

describe("cover image", () => {
  test("cover strip draws at 140pt", () => {
    const fakeImg = new Image();
    Object.defineProperty(fakeImg, "naturalWidth", { value: 800 });
    Object.defineProperty(fakeImg, "naturalHeight", { value: 400 });

    buildCardCanvas("Quote text", "Title", "Author", {
      coverImg: fakeImg,
    });

    const coverCall = mockCtx.drawImage.mock.calls[0];
    expect(coverCall[0]).toBe(fakeImg);
    // 9-arg drawImage: last two are destination width/height
    expect(coverCall[7]).toBe(375);
    expect(coverCall[8]).toBe(140);
  });

  test("skips cover image when null", () => {
    buildCardCanvas("Quote text", "Title", "Author", {
      coverImg: null,
    });

    // No drawImage calls for cover (there could be QR calls)
    expect(mockCtx.drawImage).not.toHaveBeenCalled();
  });
});

// ── QR code ──────────────────────────────────────────────────────────────

describe("QR code", () => {
  test("draws QR image at 56pt bottom-right when provided", () => {
    const fakeQr = new Image();
    Object.defineProperty(fakeQr, "naturalWidth", { value: 100 });
    Object.defineProperty(fakeQr, "naturalHeight", { value: 100 });

    buildCardCanvas("Quote text", "Title", "Author", {
      qrImg: fakeQr,
    });

    expect(mockCtx.drawImage).toHaveBeenCalled();
    const qrCall = mockCtx.drawImage.mock.calls.find(
      (c: any[]) => c[0] === fakeQr
    );
    expect(qrCall).toBeDefined();
    // QR drawn at 56x56, right edge on the 36pt margin (375 - 36 - 56 = 283)
    expect(qrCall![1]).toBe(283);
    expect(qrCall![3]).toBe(56);
    expect(qrCall![4]).toBe(56);
  });

  test("dark mode draws a paper plate behind the QR (never an inverted code)", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    try {
      const rectStyles: string[] = [];
      mockCtx.fillRect = vi.fn(() => {
        rectStyles.push(String(mockCtx.fillStyle));
      });
      const fakeQr = new Image();
      buildCardCanvas("Quote text", "Title", "Author", {
        qrImg: fakeQr,
      });
      // A paper-colored plate must be painted under the QR
      expect(rectStyles).toContain("#f5f0e6");
    } finally {
      document.documentElement.removeAttribute("data-theme");
    }
  });

  test("domain sits on the meta line whether or not the QR loaded", () => {
    buildCardCanvas("Quote text", "题目", "Author", {
      qrImg: null,
    });

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    // Domain rides the author meta line (author · domain), not a QR caption
    const metaCall = allText.find((t: string) => t.includes("localhost"));
    expect(metaCall).toBeDefined();
    expect(metaCall).toContain("Author");
    expect(metaCall).toContain("·");
  });
});

// ── QR source policy ─────────────────────────────────────────────────────
//
// Replaces the old `qrKeyForPathname` suite, which asserted that this file
// rebuilt the QR filename the same way `qr_key_for_url_path` does in
// src-tauri/src/build/media/qr.rs. Those cases proved the two implementations
// agreed on the inputs someone thought to write down — and said nothing about
// the inputs that actually broke (a folder note, whose code the build never
// emitted at all). The reader computes nothing now, so the only thing left to
// assert is that it reads what the build published and invents nothing.

describe("QR source policy", () => {
  test("the build's data-share-qr on article.container is the one QR source", () => {
    document.body.innerHTML = `<article class="container" data-share-qr="/qr/posts/a.svg"></article>`;
    expect(findQrSource()).toBe("http://localhost:3000/qr/posts/a.svg");
  });

  test("a page without the attribute has no QR — and nothing is guessed from its path", () => {
    // The page's own URL is no longer an input. A card on a page the build gave
    // no code (unpublished site, draft) draws none, rather than fetching a path
    // it reasoned its way to and getting a silent 404.
    document.body.innerHTML = `<article class="container"></article>`;
    expect(findQrSource()).toBeNull();
  });

  test("a relative attribute resolves against the page, not the site root", () => {
    document.body.innerHTML = `<article class="container" data-share-qr="qr/a.svg"></article>`;
    expect(findQrSource()).toBe("http://localhost:3000/qr/a.svg");
  });

  test("a server that never answers still produces a card", async () => {
    // The whole Share action awaits this fetch. Without a deadline, a server
    // that accepts the connection and goes quiet leaves the reader tapping a
    // button that does nothing — no card, no error, no toast.
    document.body.innerHTML = `<article class="container" data-share-qr="/qr/a.svg"></article>`;
    let sawSignal: AbortSignal | undefined;
    vi.spyOn(globalThis, "fetch").mockImplementation(((_u: string, init: any) => {
      sawSignal = init?.signal;
      return new Promise((_resolve, reject) => {
        // A real AbortSignal.timeout settles this; jsdom's does too.
        init?.signal?.addEventListener("abort", () => reject(new Error("aborted")));
      });
    }) as typeof fetch);

    await shareAsImage("Hello there, this is the quote", "Title", "Author");

    expect(sawSignal).toBeInstanceOf(AbortSignal);
    expect(document.querySelector(".share-card-overlay")).not.toBeNull();
  }, 10000);
});

// ── Bottom bar metadata ──────────────────────────────────────────────────

describe("bottom bar", () => {
  test("CJK title wrapped in 《》, author·domain meta line, no ── prefix, no year", () => {
    buildCardCanvas(
      "引文文字",
      "我的文章",
      "赵智沉"
    );

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const titleCall = allText.find((t: string) => t.includes("我的文章"));
    expect(titleCall).toBe("《我的文章》");
    // Box-drawing prefix is gone
    expect(allText.some((t: string) => t.includes("─"))).toBe(false);

    const metaCall = allText.find((t: string) => t.includes("赵智沉"));
    expect(metaCall).toBeDefined();
    expect(metaCall).toContain("·");
    expect(metaCall).toContain("localhost");

    // No year anywhere — extractYear's fallback fabricated the share-year
    expect(allText.some((t: string) => /\b20\d\d\b/.test(t))).toBe(false);
  });

  test("English title renders italic serif, no brackets", () => {
    const callFonts: Array<{ text: string; font: string }> = [];
    mockCtx.fillText = vi.fn((text: string) => {
      callFonts.push({ text, font: String(mockCtx.font) });
    });

    buildCardCanvas("Quote", "On Writing", "Jane Doe");

    const titleCall = callFonts.find((c) => c.text.includes("On Writing"));
    expect(titleCall).toBeDefined();
    expect(titleCall!.text).not.toContain("《");
    expect(titleCall!.font.startsWith("italic 13px")).toBe(true);
    expect(titleCall!.font).not.toContain("sans-serif");
  });

  test("empty author leaves the meta line as domain alone", () => {
    buildCardCanvas("Quote", "题目", "");

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const metaCall = allText.find((t: string) => t.includes("localhost"));
    expect(metaCall).toBe("localhost");
    expect(allText.some((t: string) => /\b20\d\d\b/.test(t))).toBe(false);
  });

  test("overlong author·domain meta line is ellipsized inside the title measure", () => {
    const fakeQr = new Image();
    const longAuthor = "非常非常非常非常非常非常非常非常非常非常长的作者名";
    buildCardCanvas("引文", "题目", longAuthor, {
      qrImg: fakeQr,
    });

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const metaCall = allText.find((t: string) => t.includes("非常"));
    expect(metaCall).toBeDefined();
    expect(metaCall!.endsWith("…")).toBe(true);
    // Mock measure: 10px/char; must fit 231px (303 − QR reserve) → ≤23 chars
    expect([...metaCall!].length).toBeLessThanOrEqual(23);
  });

  test("rule line is drawn as fillRect", () => {
    buildCardCanvas(
      "Quote text",
      "Title",
      "Author"
    );

    // At least 2 fillRect calls: background + rule line
    expect(mockCtx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

// ── shareAsImage ─────────────────────────────────────────────────────────

describe("shareAsImage", () => {
  test("always shows lightbox with card image and pill-bar buttons", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: vi.fn().mockReturnValue(true),
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "share", {
      value: vi.fn().mockResolvedValue(undefined),
      writable: true,
      configurable: true,
    });

    await shareAsImage("Hello", "Title", "Author");

    const overlay = document.querySelector(".share-card-overlay");
    expect(overlay).not.toBeNull();
    expect(overlay!.querySelector("img")).not.toBeNull();
    expect(overlay!.querySelector(".pill-bar")).not.toBeNull();
    // Two buttons: share and download
    const buttons = overlay!.querySelectorAll(".pill-bar-btn");
    expect(buttons).toHaveLength(2);
  });

  test("a second Share replaces the card on screen instead of stacking one behind it", async () => {
    await shareAsImage("Hello", "Title", "Author");
    await shareAsImage("Hello", "Title", "Author");

    expect(document.querySelectorAll(".share-card-overlay")).toHaveLength(1);

    // And dismissing it leaves nothing behind — not a second identical card.
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelectorAll(".share-card-overlay")).toHaveLength(0);
  });

  test("the card is encoded to PNG once, not once per consumer", async () => {
    // The preview and the shared file are the same image. Encoding it twice —
    // once via toBlob for the file, once via toDataURL for the <img> — is the
    // most expensive thing on the path, doubled for nothing.
    // The suite's setup already mocks both; read those rather than re-spying.
    const toDataURL = HTMLCanvasElement.prototype.toDataURL as unknown as ReturnType<typeof vi.fn>;
    const toBlob = HTMLCanvasElement.prototype.toBlob as unknown as ReturnType<typeof vi.fn>;
    toDataURL.mockClear();
    toBlob.mockClear();

    await shareAsImage("Hello", "Title", "Author");

    expect(toBlob).toHaveBeenCalledTimes(1);
    expect(toDataURL).not.toHaveBeenCalled();
  });

  test("fires navigator.share simultaneously with showing lightbox", async () => {
    const mockShare = vi.fn().mockResolvedValue(undefined);
    const mockCanShare = vi.fn().mockReturnValue(true);

    Object.defineProperty(navigator, "share", {
      value: mockShare,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "canShare", {
      value: mockCanShare,
      writable: true,
      configurable: true,
    });

    await shareAsImage("Hello", "Title", "Author");

    // Share was called
    expect(mockShare).toHaveBeenCalledTimes(1);
    const shareArg = mockShare.mock.calls[0][0];
    expect(shareArg.files[0].name).toBe("Title.png");
    expect(shareArg.files[0].type).toBe("image/png");

    // Lightbox is also showing
    const overlay = document.querySelector(".share-card-overlay");
    expect(overlay).not.toBeNull();
  });

  test("falls back to clipboard when share unavailable, lightbox still shown", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "share", {
      value: undefined,
      writable: true,
      configurable: true,
    });

    const mockWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { write: mockWrite },
      writable: true,
      configurable: true,
    });

    await shareAsImage("Quote text", "Title", "Author");

    expect(mockWrite).toHaveBeenCalledTimes(1);
    // Toast shown
    expect(document.querySelector(".share-toast")).not.toBeNull();
    // Lightbox also shown
    expect(document.querySelector(".share-card-overlay")).not.toBeNull();
  });

  test("lightbox shown even when both share and clipboard fail", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "share", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "clipboard", {
      value: { write: vi.fn().mockRejectedValue(new Error("blocked")) },
      writable: true,
      configurable: true,
    });

    await shareAsImage("Quote text", "Title", "Author");

    const overlay = document.querySelector(".share-card-overlay");
    expect(overlay).not.toBeNull();
    expect(overlay!.querySelector(".pill-bar")).not.toBeNull();
  });

  test("falls back to clipboard when share rejects (user cancel)", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: vi.fn().mockReturnValue(true),
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "share", {
      value: vi.fn().mockRejectedValue(new Error("Share canceled")),
      writable: true,
      configurable: true,
    });

    const mockWrite = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { write: mockWrite },
      writable: true,
      configurable: true,
    });

    await shareAsImage("Quote text", "Title", "Author");

    expect(mockWrite).toHaveBeenCalledTimes(1);
    // Lightbox still present
    expect(document.querySelector(".share-card-overlay")).not.toBeNull();
  });

  test("lightbox dismiss on backdrop click", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "clipboard", {
      value: { write: vi.fn().mockRejectedValue(new Error("blocked")) },
      writable: true,
      configurable: true,
    });

    await shareAsImage("Quote text", "Title", "Author");

    const overlay = document.querySelector(".share-card-overlay")!;
    expect(overlay).not.toBeNull();

    // Click on the overlay backdrop (not inner content)
    overlay.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(document.querySelector(".share-card-overlay")).toBeNull();
  });

  test("lightbox dismiss on Escape key", async () => {
    Object.defineProperty(navigator, "canShare", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "clipboard", {
      value: { write: vi.fn().mockRejectedValue(new Error("blocked")) },
      writable: true,
      configurable: true,
    });

    await shareAsImage("Quote text", "Title", "Author");

    expect(document.querySelector(".share-card-overlay")).not.toBeNull();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".share-card-overlay")).toBeNull();
  });
});

// ── extractDomain ────────────────────────────────────────────────────────

describe("extractDomain", () => {
  test("strips a leading www. — display keeps the short name", () => {
    // Machine URLs bake the serving host (www for CDN-fronted domains);
    // the card DISPLAYS the memorable apex.
    const link = document.createElement("link");
    link.setAttribute("rel", "canonical");
    link.setAttribute("href", "https://www.example.com/writings/post/");
    document.head.appendChild(link);
    try {
      expect(extractDomain()).toBe("example.com");
    } finally {
      link.remove();
    }
  });

  test("reads hostname from link[rel=canonical]", () => {
    const link = document.createElement("link");
    link.rel = "canonical";
    link.href = "https://example.com/writings/test";
    document.head.appendChild(link);

    expect(extractDomain()).toBe("example.com");
  });

  test("reads hostname from meta[property=og:url] when no canonical", () => {
    const meta = document.createElement("meta");
    meta.setAttribute("property", "og:url");
    meta.content = "https://example.org/article";
    document.head.appendChild(meta);

    expect(extractDomain()).toBe("example.org");
  });

  test("falls back to window.location.hostname when neither present", () => {
    // No canonical or og:url in DOM
    const domain = extractDomain();
    // In jsdom, hostname is "localhost"
    expect(domain).toBe("localhost");
  });
});

// ── wrapText kinsoku ─────────────────────────────────────────────────────

describe("wrapText kinsoku", () => {
  // Mock measureText: each char = 10px width
  // maxWidth = 50 means 5 chars per line

  test("never starts a line with 。，！？）》」", () => {
    for (const punct of ["。", "，", "！", "？", "）", "》", "」"]) {
      const text = "一二三四五" + punct + "六七八九十";
      const lines = wrapText(mockCtx as any, text, 50);
      for (let i = 1; i < lines.length; i++) {
        expect(lines[i][0]).not.toBe(punct);
      }
    }
  });

  /**
   * `。」` is two tokens. The 。 hung past the measure and the 」 then found the
   * line already flushed, so it opened the next one alone — a whole column of
   * one bracket on the vertical card, landing on the title rule. The single-mark
   * cases above cannot see it: they never present a second mark.
   */
  test("a closing run after a hang goes down whole, and never hangs twice", () => {
    const lines = wrapText(mockCtx as any, "一二三四五六七八九十。」", 50);
    expect(lines[lines.length - 1]).toBe("十。」");
    for (const line of lines) {
      expect(mockCtx.measureText(line).width).toBeLessThanOrEqual(50 + 10);
    }
  });

  test("never ends a line with （《「", () => {
    const noEnd = ["（", "《", "「"];
    for (const punct of noEnd) {
      const text = "一二三四" + punct + "六七八九十一";
      const lines = wrapText(mockCtx as any, text, 50);
      for (let i = 0; i < lines.length - 1; i++) {
        expect(lines[i][lines[i].length - 1]).not.toBe(punct);
      }
    }
  });

  test("handles mixed Chinese-English without orphaned punctuation", () => {
    const text = "Hello世界，very good！end";
    const lines = wrapText(mockCtx as any, text, 100);
    for (let i = 1; i < lines.length; i++) {
      expect("，！。".includes(lines[i][0])).toBe(false);
    }
  });
});

// ── addCjkLatinSpacing ──────────────────────────────────────────────────

describe("addCjkLatinSpacing", () => {
  test("inserts thin space between CJK and English", () => {
    const result = addCjkLatinSpacing("中文English");
    expect(result).toContain("中文");
    expect(result).toContain("English");
    expect(result.length).toBeGreaterThan("中文English".length);
    // Should contain thin space U+2009
    expect(result).toContain("\u2009");
  });

  test("inserts thin space between CJK and numbers", () => {
    const result = addCjkLatinSpacing("中文123");
    expect(result).toContain("\u2009");
  });

  test("inserts thin space between Latin and CJK", () => {
    const result = addCjkLatinSpacing("test测试");
    expect(result).toContain("\u2009");
  });

  test("does not double-space when manual space exists", () => {
    const result = addCjkLatinSpacing("中文 English");
    expect(result).toBe("中文 English");
  });

  test("leaves pure Latin text unchanged", () => {
    expect(addCjkLatinSpacing("hello world")).toBe("hello world");
  });

  test("leaves pure CJK text unchanged", () => {
    expect(addCjkLatinSpacing("你好世界")).toBe("你好世界");
  });
});

// ── drawBottomBar title truncation ──────────────────────────────────────

describe("drawBottomBar title truncation", () => {
  test("wraps long title to 2 lines when QR present", () => {
    const fakeQr = new Image();
    Object.defineProperty(fakeQr, "naturalWidth", { value: 100 });
    Object.defineProperty(fakeQr, "naturalHeight", { value: 100 });

    // 10px/char mock; QR reserve 72 → titleMaxWidth 231 → 23 chars/line.
    const longTitle =
      "别去自动化你爱的事情：从个人计算机到生成式人工智能的漫长历史与反思";
    buildCardCanvas("Quote", longTitle, "Author", {
      qrImg: fakeQr,
    });

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const titleLines = allText.filter(
      (t: string) =>
        t.includes("《") || t.includes("》") || t.includes("自动化") || t.includes("漫长")
    );
    expect(titleLines.length).toBeGreaterThanOrEqual(2);
    // First line opens with the 书名号
    expect(titleLines[0].startsWith("《")).toBe(true);
  });

  test("shows full title when short enough", () => {
    buildCardCanvas("Quote", "短题", "Author");

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    expect(allText).toContain("《短题》");
    expect(allText.some((t: string) => t.endsWith("…"))).toBe(false);
  });
});

// ── Title wrapping in bottom bar ─────────────────────────────────────────

describe("drawBottomBar title wrapping", () => {
  test("title capped at 2 lines with ellipsis when very long", () => {
    const veryLongTitle =
      "这是一个非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常长的标题用来测试截断功能";
    buildCardCanvas("Quote", veryLongTitle, "Author");

    const allText = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    // A clipped 《》 title keeps its closing bracket: the tail is "…》"
    const hasEllipsis = allText.some((t: string) => t.endsWith("…》"));
    expect(hasEllipsis).toBe(true);
    const titleLines = allText.filter(
      (t: string) => t.includes("非常") || t.includes("截断") || t.includes("《")
    );
    expect(titleLines.length).toBeLessThanOrEqual(2);
  });
});


// ── Token-aware wrapping ─────────────────────────────────────────────────

describe("token-aware wrapping", () => {
  test("English words never break mid-word", () => {
    // Mock measure: 10px/char → 100px holds 10 chars
    const lines = wrapText(mockCtx as any, "alpha beta gamma", 100);
    expect(lines).toEqual(["alpha beta", "gamma"]);
  });

  test("a word wider than the measure hard-splits instead of overflowing", () => {
    const lines = wrapText(mockCtx as any, "supercalifragilistic", 100);
    expect(lines.length).toBeGreaterThan(1);
    for (const line of lines) {
      expect(line.length).toBeLessThanOrEqual(10);
    }
  });

  test("mixed CJK-Latin: the Latin word rides to the next line whole", () => {
    const lines = wrapText(mockCtx as any, "中中中中中中中中 attention", 100);
    expect(lines).toEqual(["中中中中中中中中", "attention"]);
  });

  test("ASCII period obeys kinsoku — never starts a line", () => {
    const lines = wrapText(mockCtx as any, "中中中中中中中中中中.", 100);
    expect(lines.length).toBe(1);
    for (const line of lines.slice(1)) {
      expect(line.startsWith(".")).toBe(false);
    }
    expect(lines[0].endsWith(".")).toBe(true);
  });

  test("wrapSegments keeps Latin words whole across the highlight boundary", () => {
    const segments: CardSegment[] = [
      { text: "中中中中中中中中 atte", highlighted: false },
      { text: "ntion", highlighted: true },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 100);
    const lineTexts = lines.map((l) => l.map((sp) => sp.text).join(""));
    expect(lineTexts).toEqual(["中中中中中中中中", "attention"]);
  });
});

// ── Title formatting ─────────────────────────────────────────────────────

describe("formatCardTitle", () => {
  test("wraps Han titles in 《》", () => {
    expect(formatCardTitle("线性注意力背后的视角转换")).toEqual({
      text: "《线性注意力背后的视角转换》",
      italic: false,
    });
  });

  test("mixed CJK-Latin titles still take 《》", () => {
    expect(formatCardTitle("从 PC 到生成式 AI")).toEqual({
      text: "《从 PC 到生成式 AI》",
      italic: false,
    });
  });

  test("never double-brackets a pre-bracketed title", () => {
    expect(formatCardTitle("《红楼梦》读后").text).toBe("《红楼梦》读后");
    expect(formatCardTitle("「引号开头」的题目").text).toBe("「引号开头」的题目");
  });

  test("Latin titles go italic, unwrapped", () => {
    expect(formatCardTitle("On Writing Well")).toEqual({
      text: "On Writing Well",
      italic: true,
    });
  });

  test("empty title stays empty and upright", () => {
    expect(formatCardTitle("")).toEqual({ text: "", italic: false });
  });
});

// ── Selection image in card ──────────────────────────────────────────────

describe("wrapSegments", () => {
  // Mock measureText: each char = 10px width.
  // maxWidth = 50 means 5 chars per line.

  test("single highlighted segment wraps same as wrapText", () => {
    const segments: CardSegment[] = [
      { text: "一二三四五六七八", highlighted: true },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    // 8 chars, 5 per line -> 2 lines
    expect(lines).toHaveLength(2);
    // Each line should contain spans that are all highlighted
    for (const line of lines) {
      for (const span of line) {
        expect(span.highlighted).toBe(true);
      }
    }
    // First line should have 5 chars, second 3
    const line1text = lines[0].map((s) => s.text).join("");
    const line2text = lines[1].map((s) => s.text).join("");
    expect(line1text).toBe("一二三四五");
    expect(line2text).toBe("六七八");
  });

  test("two segments split correctly across line wrap", () => {
    // "abc" (non-highlighted) + "defgh" (highlighted) = "abcdefgh" = 8 chars
    // At 5 chars per line: line1 = "abcde", line2 = "fgh"
    // line1 should have: {abc, false}, {de, true}
    // line2 should have: {fgh, true}
    const segments: CardSegment[] = [
      { text: "abc", highlighted: false },
      { text: "defgh", highlighted: true },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    expect(lines).toHaveLength(2);

    // Line 1: two spans
    expect(lines[0]).toHaveLength(2);
    expect(lines[0][0]).toEqual({ text: "abc", highlighted: false });
    expect(lines[0][1]).toEqual({ text: "de", highlighted: true });

    // Line 2: one span
    expect(lines[1]).toHaveLength(1);
    expect(lines[1][0]).toEqual({ text: "fgh", highlighted: true });
  });

  test("segment boundary mid-line produces two spans on same line", () => {
    // "ab" (false) + "cd" (true) = 4 chars, fits in one line (maxWidth=50)
    const segments: CardSegment[] = [
      { text: "ab", highlighted: false },
      { text: "cd", highlighted: true },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toHaveLength(2);
    expect(lines[0][0]).toEqual({ text: "ab", highlighted: false });
    expect(lines[0][1]).toEqual({ text: "cd", highlighted: true });
  });

  test("kinsoku rules: no-start character kept on current line across segment boundary", () => {
    // "一二三四五" (false) + "。六七八" (true)
    // At 5 chars per line, "一二三四五" fills line 1. Next char is "。" which
    // is a no-start char, so it must be pulled onto line 1: "一二三四五。"
    // Then line 2: "六七八"
    const segments: CardSegment[] = [
      { text: "一二三四五", highlighted: false },
      { text: "。六七八", highlighted: true },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);

    // "。" should NOT start any line
    for (let i = 1; i < lines.length; i++) {
      expect(lines[i][0].text[0]).not.toBe("。");
    }
    // First line should contain "一二三四五。" (the 。 pulled from the highlighted segment)
    const line1text = lines[0].map((s) => s.text).join("");
    expect(line1text).toContain("。");
  });

  test("empty segments are filtered out", () => {
    const segments: CardSegment[] = [
      { text: "", highlighted: false },
      { text: "abc", highlighted: true },
      { text: "", highlighted: false },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    expect(lines).toHaveLength(1);
    // The empty segments should not produce spans
    for (const line of lines) {
      for (const span of line) {
        expect(span.text.length).toBeGreaterThan(0);
      }
    }
  });

  test("returns empty array for all-empty segments", () => {
    const segments: CardSegment[] = [
      { text: "", highlighted: false },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    expect(lines).toHaveLength(0);
  });

  test("multiple segments produce correct span structure across lines", () => {
    // "12" (false) + "345" (true) + "67890" (false) = "1234567890" = 10 chars
    // Line 1: "12345" -> {12, false}, {345, true}
    // Line 2: "67890" -> {67890, false}
    const segments: CardSegment[] = [
      { text: "12", highlighted: false },
      { text: "345", highlighted: true },
      { text: "67890", highlighted: false },
    ];
    const lines = wrapSegments(mockCtx as any, segments, 50);
    expect(lines).toHaveLength(2);

    expect(lines[0]).toHaveLength(2);
    expect(lines[0][0]).toEqual({ text: "12", highlighted: false });
    expect(lines[0][1]).toEqual({ text: "345", highlighted: true });

    expect(lines[1]).toHaveLength(1);
    expect(lines[1][0]).toEqual({ text: "67890", highlighted: false });
  });
});

// ── Block-aware rendering in buildCardCanvas ──────────────────────────────

describe("block-aware rendering", () => {
  const longHighlightedText =
    "This is a longer piece of text that exceeds forty characters for testing block mode";

  function makeBlocks(opts?: {
    beforeSibling?: string;
    afterSibling?: string;
    selectionPrefix?: string;
    selectionSuffix?: string;
  }): CardBlock[] {
    const blocks: CardBlock[] = [];

    if (opts?.beforeSibling) {
      blocks.push({
        lines: [[{ text: opts.beforeSibling, highlighted: false }]],
        tag: "P",
      });
    }

    const selSegments: CardSegment[] = [];
    if (opts?.selectionPrefix) {
      selSegments.push({ text: opts.selectionPrefix, highlighted: false });
    }
    selSegments.push({ text: longHighlightedText, highlighted: true });
    if (opts?.selectionSuffix) {
      selSegments.push({ text: opts.selectionSuffix, highlighted: false });
    }
    blocks.push({ lines: [selSegments], tag: "P" });

    if (opts?.afterSibling) {
      blocks.push({
        lines: [[{ text: opts.afterSibling, highlighted: false }]],
        tag: "P",
      });
    }

    return blocks;
  }

  test("two blocks produce paragraph gap visible in canvas height", () => {
    const singleBlock: CardBlock[] = [
      {
        lines: [[{ text: longHighlightedText, highlighted: true }]],
        tag: "P",
      },
    ];

    const twoBlocks: CardBlock[] = [
      {
        lines: [[{ text: longHighlightedText, highlighted: true }]],
        tag: "P",
      },
      {
        lines: [[
          { text: "Second paragraph for extra height testing and verification", highlighted: false },
        ]],
        tag: "P",
      },
    ];

    const canvas1 = buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks: singleBlock,
    });
    const canvas2 = buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks: twoBlocks,
    });

    // Two blocks should result in taller canvas due to paragraph gap + more lines
    expect(canvas2.height).toBeGreaterThan(canvas1.height);
  });

  test("sibling block lines fade in the 0.18–0.30 band", () => {
    const blocks = makeBlocks({
      beforeSibling: "Before paragraph context text for opacity check.",
    });

    // Track globalAlpha values set during fillText calls
    const alphaLog: number[] = [];
    mockCtx.fillText.mockImplementation(() => {
      alphaLog.push(mockCtx.globalAlpha);
    });

    buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks,
    });

    // Sibling context sits in the raised 0.18–0.30 fade band — the old
    // 0.1 floor vanished under chat-app recompression
    const lowAlphaValues = alphaLog.filter((a) => a >= 0.17 && a <= 0.31);
    expect(lowAlphaValues.length).toBeGreaterThan(0);
  });

  test("selection block non-highlighted segments at 0.35 alpha", () => {
    const blocks: CardBlock[] = [
      {
        lines: [[
          { text: "Non-highlighted prefix in same block ", highlighted: false },
          { text: longHighlightedText, highlighted: true },
        ]],
        tag: "P",
      },
    ];

    const alphaLog: { alpha: number; text: string }[] = [];
    mockCtx.fillText.mockImplementation((text: string) => {
      alphaLog.push({ alpha: mockCtx.globalAlpha, text });
    });

    buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks,
    });

    // With per-character justification, fillText is called per character on
    // non-last lines. Non-highlighted chars should be drawn at 0.35 alpha.
    const at03 = alphaLog.filter((e) => Math.abs(e.alpha - 0.35) < 0.02);
    expect(at03.length).toBeGreaterThan(0);
  });

  test("highlighted text rendered at 1.0 alpha", () => {
    const blocks: CardBlock[] = [
      {
        lines: [[
          { text: "dim ", highlighted: false },
          { text: longHighlightedText, highlighted: true },
        ]],
        tag: "P",
      },
    ];

    const alphaLog: { alpha: number; text: string }[] = [];
    mockCtx.fillText.mockImplementation((text: string) => {
      alphaLog.push({ alpha: mockCtx.globalAlpha, text });
    });

    buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks,
    });

    // With per-character justification, highlighted text is drawn character-by-character
    // on non-last lines and as whole spans on last lines. All should be at alpha 1.0.
    // Check that the majority of body content draws are at 1.0 (highlighted dominates).
    const at1 = alphaLog.filter((e) => e.alpha === 1.0);
    const at03 = alphaLog.filter((e) => Math.abs(e.alpha - 0.35) < 0.02);
    // There should be many more 1.0 entries (highlighted text) than 0.35 entries (dim prefix)
    expect(at1.length).toBeGreaterThan(at03.length);
    // And there should be some 0.35 entries for the "dim " prefix
    expect(at03.length).toBeGreaterThan(0);
  });

  test("after-sibling lines fade 0.30 -> 0.18", () => {
    const blocks = makeBlocks({
      afterSibling: "After paragraph context text that should fade out gradually for opacity testing.",
    });

    const alphaLog: { alpha: number; text: string }[] = [];
    mockCtx.fillText.mockImplementation((text: string) => {
      alphaLog.push({ alpha: mockCtx.globalAlpha, text });
    });

    buildCardCanvas(longHighlightedText, "Title", "Author", {
      blocks,
    });

    // After-sibling alphas should be in the 0.18-0.30 band and decreasing
    const afterEntries = alphaLog.filter(
      (e) => e.text.includes("After") || e.text.includes("fade") || e.text.includes("opacity")
    );
    expect(afterEntries.length).toBeGreaterThan(0);
    for (const entry of afterEntries) {
      expect(entry.alpha).toBeGreaterThanOrEqual(0.17);
      expect(entry.alpha).toBeLessThanOrEqual(0.31);
    }
  });

  test("a blockquote's rule stands beside its words, not above them", () => {
    const blocks: CardBlock[] = [
      {
        lines: [[{ text: longHighlightedText, highlighted: true }]],
        tag: "BLOCKQUOTE",
      },
    ];

    const textTops: number[] = [];
    mockCtx.fillText.mockImplementation((_t: string, _x: number, y: number) => {
      textTops.push(y);
    });
    const rules: { y: number; h: number }[] = [];
    mockCtx.fillRect.mockImplementation(
      (_x: number, y: number, w: number, h: number) => {
        // The rule is the tall 2px-wide bar; the bottom bar and plate are wide.
        if (w === 2) rules.push({ y, h });
      }
    );

    buildCardCanvas(longHighlightedText, "Title", "Author", { blocks });

    expect(rules).toHaveLength(1);
    const firstTop = Math.min(...textTops);
    // The canvas draws with textBaseline "top", so the rule starts no higher
    // than the first glyph top. It used to start a whole font-size above it,
    // poking into the card's padding.
    expect(rules[0].y).toBeGreaterThanOrEqual(firstTop - 1);
    expect(rules[0].y).toBeLessThanOrEqual(firstTop + 1);
    expect(rules[0].h).toBeGreaterThan(0);
  });

  test("short text still uses centered mode even when blocks provided", () => {
    const shortText = "Short quote";
    const blocks: CardBlock[] = [
      {
        lines: [[{ text: shortText, highlighted: true }]],
        tag: "P",
      },
    ];

    buildCardCanvas(shortText, "Title", "Author", {
      blocks,
    });

    // Short mode should add corner brackets and center text
    const textCalls = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    const bodyText = textCalls.find(
      (t: string) => t.includes("\u300C") && t.includes(shortText)
    );
    expect(bodyText).toBeDefined();
  });
});

// ── The author's line breaks, on the card ────────────────────────────────

describe("hard line breaks are drawn as lines", () => {
  /** The y each fillText landed on, in order, deduplicated. */
  function drawnRows(): number[] {
    const rows: number[] = [];
    for (const c of mockCtx.fillText.mock.calls) {
      const y = c[2] as number;
      if (rows[rows.length - 1] !== y) rows.push(y);
    }
    return rows;
  }

  test("a four-line poem occupies four rows, not one", () => {
    const verse = ["一行诗句", "二行诗句", "三行诗句", "四行诗句"];
    const blocks: CardBlock[] = [
      { lines: verse.map((t) => [{ text: t, highlighted: true }]), tag: "P" },
    ];

    buildCardCanvas(verse.join("\n"), "Title", "Author", {
      blocks,
    });

    // Body rows come before the bottom bar; four distinct ones is the poem.
    const drawn = mockCtx.fillText.mock.calls
      .map((c: any[]) => ({ text: c[0] as string, y: c[2] as number }))
      .filter((d) => verse.some((v) => v.includes(d.text) || d.text.includes(v)));
    const rows = new Set(drawn.map((d) => d.y));
    expect(rows.size).toBe(4);
  });

  test("a selection with a break is never the centred short treatment", () => {
    // 8 characters — comfortably under the short-mode threshold, and still a
    // poem. Short mode would print it as one centred 「run」 and lose the break.
    const text = "一行诗句\n二行诗句";
    const blocks: CardBlock[] = [
      {
        lines: [
          [{ text: "一行诗句", highlighted: true }],
          [{ text: "二行诗句", highlighted: true }],
        ],
        tag: "P",
      },
    ];

    buildCardCanvas(text, "Title", "Author", { blocks });

    const bracketed = mockCtx.fillText.mock.calls.some((c: any[]) =>
      String(c[0]).includes("「")
    );
    expect(bracketed).toBe(false);
    expect(drawnRows().length).toBeGreaterThan(1);
  });

  test("a heading block is set larger than the prose around it", () => {
    const blocks: CardBlock[] = [
      { lines: [[{ text: "The Heading", highlighted: false }]], tag: "H2" },
      {
        lines: [[{ text: "A long enough paragraph of body prose to render.", highlighted: true }]],
        tag: "P",
      },
    ];

    const fonts: string[] = [];
    mockCtx.fillText.mockImplementation(() => fonts.push(mockCtx.font));

    buildCardCanvas(
      "A long enough paragraph of body prose to render.",
      "Title",
      "Author",
      { blocks }
    );

    const sizes = fonts
      .map((f) => Number(/(\d+(?:\.\d+)?)px/.exec(f)?.[1]))
      .filter((n) => !Number.isNaN(n));
    expect(Math.max(...sizes)).toBeGreaterThan(16);
  });

  test("a list item draws its marker in the gutter", () => {
    const blocks: CardBlock[] = [
      {
        lines: [[{ text: "An item long enough to need a full line of prose.", highlighted: true }]],
        tag: "LI",
        marker: "•",
      },
    ];

    buildCardCanvas(
      "An item long enough to need a full line of prose.",
      "Title",
      "Author",
      { blocks }
    );

    const marker = mockCtx.fillText.mock.calls.find((c: any[]) => c[0] === "•");
    expect(marker).toBeDefined();
    // Gutter: the marker sits at the padding, the text is indented past it.
    expect(marker![1]).toBe(36);
  });
});

// ── shareAsImage forwards blocks ──────────────────────────────────────────

describe("shareAsImage forwards blocks", () => {
  test("blocks from ShareOptions are forwarded to buildCardCanvas", async () => {
    // Suppress share/clipboard so shareAsImage completes without errors
    Object.defineProperty(navigator, "canShare", {
      value: undefined,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(navigator, "clipboard", {
      value: { write: vi.fn().mockRejectedValue(new Error("blocked")) },
      writable: true,
      configurable: true,
    });

    const blocks: CardBlock[] = [
      {
        lines: [[
          { text: "prefix ", highlighted: false },
          {
            text: "highlighted text that is longer than forty characters for testing",
            highlighted: true,
          },
        ]],
        tag: "P",
      },
    ];

    // Track globalAlpha values during fillText to detect block-aware rendering
    const alphaLog: { alpha: number; text: string }[] = [];
    mockCtx.fillText.mockImplementation((text: string) => {
      alphaLog.push({ alpha: mockCtx.globalAlpha, text });
    });

    await shareAsImage(
      "highlighted text that is longer than forty characters for testing",
      "Title",
      "Author",
      { blocks }
    );

    // Block-aware rendering draws per-character with justification.
    // Non-highlighted "prefix " chars should be at 0.3 alpha,
    // highlighted text chars should be at 1.0 alpha.
    const at03 = alphaLog.filter((e) => Math.abs(e.alpha - 0.35) < 0.02);
    expect(at03.length).toBeGreaterThan(0);

    const at1 = alphaLog.filter((e) => e.alpha === 1.0);
    expect(at1.length).toBeGreaterThan(0);
    // Highlighted text dominates, so more 1.0 entries than 0.3 entries
    expect(at1.length).toBeGreaterThan(at03.length);
  });
});

// ── buildPalette ──────────────────────────────────────────────────────────

describe("buildPalette", () => {
  test("light palette returns all 6 fields", () => {
    const palette = buildPalette(false);
    expect(palette).toHaveProperty("bg");
    expect(palette).toHaveProperty("text");
    expect(palette).toHaveProperty("faded");
    expect(palette).toHaveProperty("rule");
    expect(palette).toHaveProperty("meta");
    expect(palette).toHaveProperty("muted");
    // All values should be non-empty strings
    for (const value of Object.values(palette)) {
      expect(typeof value).toBe("string");
      expect(value.length).toBeGreaterThan(0);
    }
  });

  test("dark palette returns all 6 fields", () => {
    const palette = buildPalette(true);
    expect(palette).toHaveProperty("bg");
    expect(palette).toHaveProperty("text");
    expect(palette).toHaveProperty("faded");
    expect(palette).toHaveProperty("rule");
    expect(palette).toHaveProperty("meta");
    expect(palette).toHaveProperty("muted");
    for (const value of Object.values(palette)) {
      expect(typeof value).toBe("string");
      expect(value.length).toBeGreaterThan(0);
    }
  });

  test("uses fallback values when CSS custom properties are empty (jsdom)", () => {
    // In jsdom, getComputedStyle returns empty strings for custom properties
    const light = buildPalette(false);
    expect(light.bg).toBe("#f5f0e6"); // Card override: warm paper

    const dark = buildPalette(true);
    expect(dark.text).toBe("#d4ccb8"); // Card override: brighter for contrast
    // Muted lifted for AA at 11px: old #6a6050 was ~2.8:1 on the dark card bg
    expect(dark.muted).toBe("#9a9080");
  });

  test("reads CSS custom properties when set", () => {
    const customTextColor = "#112233";
    // Mock getComputedStyle to return a value for --moss-color-text
    const originalGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el) => {
      const real = originalGetComputedStyle(el);
      return new Proxy(real, {
        get(target, prop) {
          if (prop === "getPropertyValue") {
            return (name: string) => {
              if (name === "--moss-color-text") return customTextColor;
              return target.getPropertyValue(name);
            };
          }
          return (target as any)[prop];
        },
      });
    });

    const palette = buildPalette(false);
    expect(palette.text).toBe(customTextColor);

    vi.restoreAllMocks();
  });
});

// ── Serif font CSS token ──────────────────────────────────────────────────

describe("serif font CSS token", () => {
  test("body text uses serif font from CSS token (falls back to 'serif')", () => {
    // In jsdom, --moss-font-heading is empty, so the fallback "serif" is used.
    // Capture all ctx.font assignments by tracking property sets on mockCtx.
    const fontAssignments: string[] = [];
    Object.defineProperty(mockCtx, "font", {
      configurable: true,
      get() {
        return fontAssignments[fontAssignments.length - 1] ?? "";
      },
      set(value: string) {
        fontAssignments.push(value);
      },
    });

    buildCardCanvas("Some text to test font", "Title", "Author");

    // At least one body font assignment should contain "serif" (the fallback)
    const serifAssignments = fontAssignments.filter((f) => f.includes("serif"));
    expect(serifAssignments.length).toBeGreaterThan(0);

    // None of the body font assignments should be the hardcoded literal "px serif"
    // — they should all go through the serifFont variable (which falls back to "serif")
    // so the value is the same but the mechanism is dynamic
    const bodyFontAssignments = fontAssignments.filter((f) => /^\d+px /.test(f) && !f.includes("sans-serif"));
    expect(bodyFontAssignments.length).toBeGreaterThan(0);
    for (const font of bodyFontAssignments) {
      expect(font).toContain("serif");
      expect(font).not.toContain("sans-serif");
    }
  });

  test("body text uses custom serif font when CSS token is set", () => {
    const customFont = "Iowan Old Style, Palatino, serif";
    const fontAssignments: string[] = [];
    Object.defineProperty(mockCtx, "font", {
      configurable: true,
      get() {
        return fontAssignments[fontAssignments.length - 1] ?? "";
      },
      set(value: string) {
        fontAssignments.push(value);
      },
    });

    // Mock getComputedStyle to return a value for --moss-font-heading
    const originalGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el) => {
      const real = originalGetComputedStyle(el);
      return new Proxy(real, {
        get(target, prop) {
          if (prop === "getPropertyValue") {
            return (name: string) => {
              if (name === "--moss-font-heading") return customFont;
              return target.getPropertyValue(name);
            };
          }
          return (target as any)[prop];
        },
      });
    });

    buildCardCanvas("Some text to test font", "Title", "Author");

    const bodyFontAssignments = fontAssignments.filter((f) => /^\d+px /.test(f) && !f.includes("sans-serif"));
    expect(bodyFontAssignments.length).toBeGreaterThan(0);
    for (const font of bodyFontAssignments) {
      expect(font).toContain(customFont);
    }

    vi.restoreAllMocks();
  });
});

// ── drawJustifiedLine ─────────────────────────────────────────────────────

describe("drawJustifiedLine", () => {
  beforeEach(() => {
    mockCtx.fillText.mockClear();
  });

  test("last line draws with single fillText", () => {
    drawJustifiedLine(mockCtx as any, "Hello", 10, 20, 300, true);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(1);
    expect(mockCtx.fillText).toHaveBeenCalledWith("Hello", 10, 20);
  });

  test("Latin-only line stays ragged — one fillText, no letter-spacing", () => {
    drawJustifiedLine(mockCtx as any, "Hi there", 10, 20, 300, false);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(1);
    expect(mockCtx.fillText).toHaveBeenCalledWith("Hi there", 10, 20);
  });

  test("CJK line distributes at every character gap", () => {
    drawJustifiedLine(mockCtx as any, "你好世界", 10, 20, 300, false);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(4);
  });

  test("mixed line keeps Latin words whole between token gaps", () => {
    drawJustifiedLine(mockCtx as any, "线性 attention 机制", 10, 20, 300, false);
    const texts = mockCtx.fillText.mock.calls.map((c: any[]) => c[0]);
    expect(texts).toContain("attention");
    expect(texts).toContain("线");
    expect(texts).toContain("制");
  });

  test("single character uses fillText directly", () => {
    drawJustifiedLine(mockCtx as any, "A", 10, 20, 300, false);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(1);
  });
});

// ── drawJustifiedWrappedLine ──────────────────────────────────────────────

describe("drawJustifiedWrappedLine", () => {
  beforeEach(() => {
    mockCtx.fillText.mockClear();
  });

  test("last line draws spans without justification", () => {
    const spans: LineSpan[] = [
      { text: "Hello", highlighted: true },
      { text: " world", highlighted: false },
    ];
    const getAlpha = (s: LineSpan) => s.highlighted ? 1.0 : 0.5;
    drawJustifiedWrappedLine(mockCtx as any, spans, 10, 20, 300, true, getAlpha);
    // Last line: one fillText per span
    expect(mockCtx.fillText).toHaveBeenCalledTimes(2);
    expect(mockCtx.fillText).toHaveBeenCalledWith("Hello", expect.any(Number), 20);
  });

  test("Latin-only non-last line stays ragged — one fillText per span", () => {
    const spans: LineSpan[] = [
      { text: "AB", highlighted: true },
      { text: "CD", highlighted: false },
    ];
    const getAlpha = (s: LineSpan) => s.highlighted ? 1.0 : 0.5;
    drawJustifiedWrappedLine(mockCtx as any, spans, 10, 20, 300, false, getAlpha);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(2);
  });

  test("CJK non-last line draws per character with alpha mapping", () => {
    const spans: LineSpan[] = [
      { text: "线性", highlighted: true },
      { text: "注意", highlighted: false },
    ];
    const getAlpha = (s: LineSpan) => s.highlighted ? 1.0 : 0.5;
    drawJustifiedWrappedLine(mockCtx as any, spans, 10, 20, 300, false, getAlpha);
    expect(mockCtx.fillText).toHaveBeenCalledTimes(4);
  });
});

// ── buildVerticalCardCanvas — the transposed two-column meta ─────────────
//
// The vertical card's reading-end meta used to be title/author/url, three
// columns; horizontal's bottom bar has only ever had two registers (a title
// block, then one "author · domain" line — bar.ts:108). These pin the
// transposed shape: one merged meta column, no separate URL run, and — once
// a real selection supplies `blocks` — the same context-dimming horizontal's
// own prose gets from `blocks.ts`'s `fadeAlpha`.

describe("buildVerticalCardCanvas — the transposed meta column", () => {
  const verticalCtx = () => ({
    palette: buildPalette(false),
    serifFont: "serif",
    isDark: false,
  });

  test("author and domain share one column — no separate URL run", () => {
    mockCtx.fillText.mockClear();
    const longQuote =
      "此段引言刻意寫得足夠長，超過四十個字符，以避免觸發短引置中模式，用於測試向量分享卡片的版面配置。";
    buildVerticalCardCanvas(longQuote, "標題", "作者", verticalCtx());

    // `drawUprightColumn` never calls `fillText` for a space token (it just
    // advances y), so the drawn-glyph stream skips the literal spaces in
    // `${author} · ${domain}` — everything else appears in the order
    // `tokenize` walks the string in.
    const drawn = mockCtx.fillText.mock.calls.map((c: any[]) => String(c[0])).join("");
    expect(drawn).toContain(`作者·${extractDomain()}`);
    // No path ever reaches the card now that `urlText`/`decodedPath` are
    // gone — a slash could only get here through the deleted URL column.
    expect(drawn).not.toContain("/");
  });

  test("no author falls back to the bare domain, still one column", () => {
    mockCtx.fillText.mockClear();
    const longQuote =
      "此段引言刻意寫得足夠長，超過四十個字符，以避免觸發短引置中模式，用於測試向量分享卡片的版面配置。";
    buildVerticalCardCanvas(longQuote, "標題", "", verticalCtx());

    const drawn = mockCtx.fillText.mock.calls.map((c: any[]) => String(c[0])).join("");
    expect(drawn).toContain(extractDomain());
    expect(drawn).not.toContain("·");
  });
});

describe("buildVerticalCardCanvas — blocks carry the quote's context fade", () => {
  test("a line outside the highlight paints at the same reduced alpha blocks.ts gives horizontal's context prose", () => {
    const alphaLog: number[] = [];
    mockCtx.fillText.mockClear();
    mockCtx.fillText.mockImplementation(() => {
      alphaLog.push(mockCtx.globalAlpha);
    });

    const before: CardSegment = {
      text: "前情提要的襯字用來把整段引文撐得足夠長",
      highlighted: false,
    };
    const selected: CardSegment = { text: "被選取的重點句子", highlighted: true };
    const after: CardSegment = {
      text: "後續補充的襯字同樣把整段引文撐得足夠長",
      highlighted: false,
    };
    const blocks: CardBlock[] = [{ lines: [[before, selected, after]], tag: "P" }];
    const fullText = before.text + selected.text + after.text;

    buildVerticalCardCanvas(fullText, "標題", "作者", {
      palette: buildPalette(false),
      serifFont: "serif",
      isDark: false,
      blocks,
    });

    // Without the fix nothing dims: every draw call happens at whatever
    // `ctx.globalAlpha` the quote loop left at 1. With it, the selected
    // run draws full ink and its unselected siblings in the same block
    // draw at 0.35 — `fadeAlpha`'s row-holds-the-highlight branch.
    expect(alphaLog).toContain(1);
    expect(alphaLog).toContain(0.35);
  });
});
