/**
 * Regression tests for the site i18n bucket fix.
 *
 * zh-TW / zh-Hant must stay on Traditional Chinese copy in the fixed site
 * modules, not fall back to Simplified Chinese.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { initSelectionActions } from "../selection-actions";
import { shareAsImage } from "../share-card";
import { showToast } from "../toast";

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
    globalAlpha: 1.0,
  };
}

function createArticlePage(): void {
  document.head.innerHTML = "";

  const titleEl = document.createElement("title");
  titleEl.textContent = "Test Article - Test Site";
  document.head.appendChild(titleEl);

  const metaAuthor = document.createElement("meta");
  metaAuthor.setAttribute("name", "author");
  metaAuthor.setAttribute("content", "Test Author");
  document.head.appendChild(metaAuthor);

  const main = document.createElement("main");
  const article = document.createElement("article");
  article.className = "container";

  const h1 = document.createElement("h1");
  h1.textContent = "Test Article";
  article.appendChild(h1);

  const p = document.createElement("p");
  p.textContent = "This is some article body text that can be selected.";
  article.appendChild(p);

  main.appendChild(article);

  const commentsSection = document.createElement("section");
  commentsSection.className = "moss-comments";
  commentsSection.id = "moss-comments";
  main.appendChild(commentsSection);

  document.body.appendChild(main);
}

beforeEach(() => {
  document.body.innerHTML = "";
  document.body.className = "";
  document.head.innerHTML = "";
  document.documentElement.lang = "";

  Object.defineProperty(window, "requestAnimationFrame", {
    writable: true,
    configurable: true,
    value: (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    },
  });

  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });

  const mockCtx = createMockContext();
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

  // The lightbox previews the card from its blob rather than re-encoding it,
  // and jsdom implements neither object-URL function.
  if (typeof URL.createObjectURL !== "function") {
    URL.createObjectURL = vi.fn().mockReturnValue("blob:fake");
    URL.revokeObjectURL = vi.fn();
  }

  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    new Response("", { status: 404 })
  );

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
    value: {
      write: vi.fn().mockRejectedValue(new Error("blocked")),
    },
    writable: true,
    configurable: true,
  });

  if (typeof globalThis.ClipboardItem === "undefined") {
    (globalThis as any).ClipboardItem = class ClipboardItem {
      constructor(public items: Record<string, Blob>) {}
    };
  }
});

afterEach(() => {
  document.body.innerHTML = "";
  document.body.className = "";
  document.head.innerHTML = "";
  document.documentElement.lang = "";
  vi.restoreAllMocks();
});

describe("showToast", () => {
  test.each(["zh-TW", "zh-Hant"])(
    "uses Traditional Chinese copy for %s",
    (lang) => {
      document.documentElement.lang = lang;
      showToast();

      const toast = document.querySelector<HTMLElement>(".share-toast");
      expect(toast).not.toBeNull();
      expect(toast!.textContent).toBe("已複製到剪貼簿");
      expect(toast!.textContent).not.toBe("已复制到剪贴板");
    }
  );
});

describe("initSelectionActions", () => {
  test.each(["zh-TW", "zh-Hant"])(
    "uses Traditional Chinese labels for %s",
    (lang) => {
      document.documentElement.lang = lang;
      createArticlePage();
      initSelectionActions();

      const popover = document.querySelector<HTMLElement>(".sel-popover");
      expect(popover).not.toBeNull();

      const shareBtn = popover!.querySelector(".sel-share");
      const commentBtn = popover!.querySelector(".sel-comment");
      const copyBtn = popover!.querySelector(".sel-copy");

      expect(shareBtn?.textContent).toBe("分享");
      expect(commentBtn?.textContent).toBe("留言");
      expect(copyBtn?.textContent).toBe("複製");
      expect(commentBtn?.textContent).not.toBe("评论");
      expect(copyBtn?.textContent).not.toBe("复制");
    }
  );
});

describe("shareAsImage", () => {
  test.each(["zh-TW", "zh-Hant"])(
    "uses Traditional Chinese lightbox labels for %s",
    async (lang) => {
      document.documentElement.lang = lang;

      await shareAsImage(
        "Quote text",
        "Title",
        "Author",
        "https://example.com/article"
      );

      const overlay = document.querySelector<HTMLElement>(".share-card-overlay");
      expect(overlay).not.toBeNull();

      const buttons = overlay!.querySelectorAll<HTMLButtonElement>(
        ".pill-bar-btn"
      );
      expect(buttons).toHaveLength(2);
      expect(buttons[0].textContent).toBe("分享");
      expect(buttons[1].textContent).toBe("下載");
      expect(buttons[1].textContent).not.toBe("下载");
    }
  );
});
