/**
 * Behavioural tests for the site-search client runtime (frontend/site/search.ts).
 *
 * Pagefind itself is never available here — the dynamic import of
 * `/_moss/pagefind/pagefind.js` fails under jsdom, which search.ts handles by
 * falling through to the "no matches" state. That failure is useful: it makes
 * "did a query actually fire?" observable as a status-text change, which is
 * exactly what the IME-composition test needs to assert.
 *
 * The bug class under test (flagged in
 * docs/archive/2026-07-30-search-feature-plan.md): a naive `input` listener
 * ships a garbage pinyin-fragment query on every IME keystroke.
 *
 * The module is imported ONCE. It binds delegated listeners to `document`, so
 * re-importing under `vi.resetModules()` would leave the previous instance's
 * listeners live and open two overlays per click. Instead each test resets by
 * wiping the body and firing `moss-morph-patched` — the same signal the preview
 * bridge sends after idiomorph strips JS-appended nodes.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";

import "../search";
import { renderExcerpt, joinSegmentedHan } from "../search";

/** Nav markup as `build/components/nav.rs` emits it when search is enabled. */
function setupPage(): HTMLButtonElement {
  document.body.innerHTML = `
    <nav><div class="nav-icons">
      <button class="nav-search-btn" type="button" aria-label="Search"></button>
    </div></nav>
    <article><p>Auguries of Innocence</p></article>
    <input id="page-input" />
  `;
  return document.querySelector<HTMLButtonElement>(".nav-search-btn")!;
}

const overlay = () => document.querySelector<HTMLElement>(".moss-search");
const searchInput = () => document.querySelector<HTMLInputElement>(".moss-search__input");
const status = () => document.querySelector<HTMLElement>(".moss-search__status");

/** Let the debounce fire and any pending microtasks settle. */
async function settle(ms = 200): Promise<void> {
  await new Promise((r) => setTimeout(r, ms));
}

/**
 * Spy on the "index unavailable" warning, which is one line per attempted
 * `import()` of the Pagefind bundle — the only observable proof that a load was
 * *attempted* rather than served from a memoized result. Returns the attempted
 * URLs in order.
 *
 * The leading `settle` is not decoration: earlier tests click the trigger
 * without awaiting, so their load rejections can land after this spy is
 * installed and be miscounted as this test's.
 */
async function watchIndexLoads(): Promise<{ attempts: () => string[]; restore: () => void }> {
  await settle(20);
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  return {
    attempts: () =>
      warn.mock.calls
        .filter((c) => c[0] === "[moss] search index unavailable")
        .map((c) => String(c[1])),
    restore: () => warn.mockRestore(),
  };
}

/** Return the runtime to a cold state without re-importing it. */
function reset(): void {
  overlay()?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  document.body.innerHTML = "";
  document.dispatchEvent(new CustomEvent("moss-morph-patched"));
  document.documentElement.style.overflow = "";
}

describe("site search runtime", () => {
  beforeEach(() => {
    reset();
    setupPage();
  });
  afterEach(reset);

  test("nothing is added to the page until the panel is first opened", () => {
    expect(overlay()).toBeNull();
  });

  test("clicking the nav trigger opens the panel and locks page scroll", () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    expect(overlay()!.hidden).toBe(false);
    expect(document.activeElement).toBe(searchInput());
    expect(document.documentElement.style.overflow).toBe("hidden");
  });

  test("`/` opens the panel, but not while focus is in a text field", () => {
    const pageInput = document.querySelector<HTMLInputElement>("#page-input")!;
    pageInput.focus();
    pageInput.dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true }));
    expect(overlay()).toBeNull();

    pageInput.blur();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true }));
    expect(overlay()!.hidden).toBe(false);
  });

  test("Cmd/Ctrl+K opens the panel", () => {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }),
    );
    expect(overlay()!.hidden).toBe(false);
  });

  test("Esc closes, restores scroll, and returns focus to the trigger", () => {
    const trigger = document.querySelector<HTMLButtonElement>(".nav-search-btn")!;
    trigger.click();

    overlay()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(overlay()!.hidden).toBe(true);
    expect(document.documentElement.style.overflow).toBe("");
    expect(document.activeElement).toBe(trigger);
  });

  test("no query fires mid-IME-composition; one fires on compositionend", async () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    const el = searchInput()!;
    const idle = status()!.textContent;

    // Typing "zai" as pinyin: every keystroke emits `input`, but the value is
    // an unresolved fragment, not a query.
    el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    for (const fragment of ["z", "za", "zai"]) {
      el.value = fragment;
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }
    await settle();
    expect(status()!.textContent).toBe(idle);

    // The IME resolves to 在 — now, and only now, one query runs.
    el.value = "在";
    el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
    await settle();
    expect(status()!.textContent).not.toBe(idle);
    expect(status()!.textContent).toContain("在");
  });

  test("closing mid-composition doesn't permanently gate the next open's queries", async () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    let el = searchInput()!;
    const idle = status()!.textContent;

    // Start composing, then close via backdrop click before compositionend
    // ever fires. (Escape itself is deferred to the IME's own candidate
    // picker while composing — this must go through a close path that
    // isn't gated on composition state, like the backdrop.)
    el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    const backdrop = overlay()!.querySelector<HTMLElement>(".moss-search__backdrop")!;
    backdrop.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(overlay()!.hidden).toBe(true);

    // Reopen and type a plain (non-IME) query. If the composition flag were
    // still stuck true, this `input` event would be silently swallowed.
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    el = searchInput()!;
    el.value = "garden";
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(status()!.textContent).not.toBe(idle);
  });

  test("a morph mid-composition also clears the gate, not just Esc-close", async () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    const idle = status()!.textContent;
    searchInput()!.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));

    // idiomorph replaces the body mid-composition (no compositionend, no Esc).
    setupPage();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    const el = searchInput()!;
    el.value = "garden";
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(status()!.textContent).not.toBe(idle);
  });

  test("an emptied query returns to the idle state, not to a no-matches line", async () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    const el = searchInput()!;
    // Idle shows nothing at all — the input placeholder is the only prompt.
    expect(status()!.hidden).toBe(true);
    expect(status()!.textContent).toBe("");

    el.value = "garden";
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(status()!.hidden).toBe(false);
    expect(status()!.textContent).not.toBe("");

    el.value = "";
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(status()!.hidden).toBe(true);
    expect(status()!.textContent).toBe("");
  });

  test("panel strings come from the resolved <html lang> table", () => {
    // No `lang` on this fixture, so the runtime falls back to English. (The
    // table itself is resolved once at module init, which is why the CJK
    // variants are exercised by the render-gate build rather than here.)
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    expect(searchInput()!.placeholder).toBe("Search this site");
  });

  test("a morph that strips the overlay leaves the trigger still working", () => {
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    expect(overlay()).not.toBeNull();

    // idiomorph replaces the body and drops JS-appended nodes.
    setupPage();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));
    expect(overlay()).toBeNull();
    expect(document.documentElement.style.overflow).toBe("");

    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    expect(overlay()!.hidden).toBe(false);
  });

  /**
   * In preview the index can appear *after* the page is already live: the
   * author flips `[site].search` on, or the background reindex lands a moment
   * behind the rebuild that morphed the page. `open()` warms the index, so the
   * routine case is "palette opened before the first index exists" — and a
   * memoized `null` there would leave search dead for the whole preview
   * session with no reload to recover it.
   *
   * Pagefind never loads under jsdom, so every attempt fails and each one is
   * observable as a `console.warn`. Counting the warnings counts the attempts:
   * one per open is the fix, exactly one ever is the bug.
   */
  test("a failed index load is retried on the next open, not cached forever", async () => {
    const { attempts, restore } = await watchIndexLoads();

    const trigger = () => document.querySelector<HTMLButtonElement>(".nav-search-btn")!;
    trigger().click();
    await settle(20);
    expect(attempts()).toHaveLength(1);

    overlay()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    trigger().click();
    await settle(20);
    expect(attempts()).toHaveLength(2);

    restore();
  });

  /**
   * A rebuild rewrites the bundle, and Pagefind's chunk filenames are
   * content-hashed — so a page holding the previously-loaded module asks for
   * chunks that no longer exist. The morph signal must drop the module even
   * though the overlay itself is untouched by a reindex.
   */
  test("a morph re-imports the index under a fresh cache-busting version", async () => {
    const { attempts, restore } = await watchIndexLoads();

    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    await settle(20);

    // The overlay survives — a reindex is not a DOM change — so the reset must
    // not be conditional on the overlay having been stripped.
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));
    expect(overlay()).not.toBeNull();

    overlay()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    document.querySelector<HTMLButtonElement>(".nav-search-btn")!.click();
    await settle(20);

    const tried = attempts();
    expect(tried).toHaveLength(2);
    expect(tried[1]).toContain("?v=");
    expect(tried[1]).not.toBe(tried[0]);

    restore();
  });
});

describe("renderExcerpt", () => {
  // Pagefind's excerpt is decoded plain text, not sanitized HTML: any literal
  // "<"/">" visible on the source page (a code sample, a stray tag) survives
  // into it unescaped, so innerHTML-rendering it is a stored-XSS vector.
  test("literal markup in the excerpt is rendered as text, never parsed", () => {
    const container = document.createElement("span");
    renderExcerpt(container, 'see <img src=x onerror=alert(1)> and <script>alert(2)</script>');
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(container.textContent).toBe(
      "see <img src=x onerror=alert(1)> and <script>alert(2)</script>",
    );
  });

  test("Pagefind's own <mark> wrapper still highlights, as the one real element", () => {
    const container = document.createElement("span");
    renderExcerpt(container, "the quick <mark>fox</mark> jumps");
    const mark = container.querySelector("mark");
    expect(mark).not.toBeNull();
    expect(mark!.textContent).toBe("fox");
    expect(container.textContent).toBe("the quick fox jumps");
  });

  test("markup nested inside a <mark> match is also just text", () => {
    const container = document.createElement("span");
    renderExcerpt(container, "<mark><img src=x onerror=alert(1)></mark>");
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("mark")!.textContent).toBe(
      "<img src=x onerror=alert(1)>",
    );
  });
});

/**
 * The index is built from a jieba-segmented copy of each page ("這是一段文字" →
 * "這 是 一段 文字" — build/feeds/search.rs), and Pagefind's excerpts and titles
 * are slices of that segmented text. Display must invert it: remove exactly the
 * spaces segmentation inserted, and no others.
 */
describe("CJK de-segmentation for display", () => {
  test("spaces between Han characters are joined", () => {
    expect(joinSegmentedHan("這 是 一段 文字 的 預覽")).toBe("這是一段文字的預覽");
  });

  test("Latin text and Han-Latin boundaries are untouched", () => {
    expect(joinSegmentedHan("the quick fox")).toBe("the quick fox");
    expect(joinSegmentedHan("moss 是 一個 tool 的 名字")).toBe("moss 是一個 tool 的名字");
  });

  test("a removable space needs Han on one side — authored spaces elsewhere survive", () => {
    // Fullwidth alphanumerics space like Latin; segmentation never inserted this.
    expect(joinSegmentedHan("ＡＢＣ ＤＥＦ")).toBe("ＡＢＣ ＤＥＦ");
    // Kana↔kana likewise: only Han-run boundaries ever got a space.
    expect(joinSegmentedHan("カタカナ ひらがな")).toBe("カタカナ ひらがな");
    // Han↔ASCII punctuation keeps its space — "文字 (note)" is authored-legit.
    expect(joinSegmentedHan("文字 (note)")).toBe("文字 (note)");
    // Han-flanked exclusions (mutation-verified): re-broadening HAN_NEIGHBOR to
    // all of FF01-FF60 would eat these authored spaces and only these catch it.
    expect(joinSegmentedHan("文字 ＡＢＣ")).toBe("文字 ＡＢＣ");
    expect(joinSegmentedHan("１２３ 文")).toBe("１２３ 文");
  });

  test("kanji↔kana boundary spaces are joined — never authored Japanese", () => {
    expect(joinSegmentedHan("日本語 のテキスト")).toBe("日本語のテキスト");
    // Halfwidth katakana too — the segmenter spaces that boundary as well.
    expect(joinSegmentedHan("文 ｶﾞ")).toBe("文ｶﾞ");
  });

  test("boundary spaces around CJK punctuation are joined too", () => {
    expect(joinSegmentedHan("文字 。 下一 句")).toBe("文字。下一句");
    expect(joinSegmentedHan("（ 括號 ） 之外")).toBe("（括號）之外");
  });

  test("astral-plane Han (Ext-B+) joins — the indexer segments it too", () => {
    // 𠮷 is U+20BB7; drops silently if the `u` flag or the \u{20000} range goes.
    expect(joinSegmentedHan("𠮷 野家 的 招牌")).toBe("𠮷野家的招牌");
  });

  test("adjacent marks join — Pagefind wraps each matched word separately", () => {
    const container = document.createElement("span");
    renderExcerpt(container, "前文 <mark>潮汐</mark> <mark>紀實</mark> 後文");
    expect(container.textContent).toBe("前文潮汐紀實後文");
    expect(container.querySelectorAll("mark")).toHaveLength(2);
  });

  test("a segmentation space straddling a <mark> boundary is joined", () => {
    const container = document.createElement("span");
    renderExcerpt(container, "一段 <mark>文 字</mark> 更多");
    expect(container.textContent).toBe("一段文字更多");
    expect(container.querySelector("mark")!.textContent).toBe("文字");
  });

  test("de-segmentation never re-opens the excerpt XSS surface", () => {
    const container = document.createElement("span");
    renderExcerpt(container, "程式 <mark>碼</mark> <img src=x onerror=alert(1)> 之後");
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("<img src=x onerror=alert(1)>");
  });
});
