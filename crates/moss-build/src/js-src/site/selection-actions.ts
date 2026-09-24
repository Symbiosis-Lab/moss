/**
 * Selection Actions — desktop popover and mobile mini-bar.
 *
 * When a reader selects text within an article body, action buttons appear:
 * - Desktop: popover above selection with 分享 / 评论 / 复制 buttons
 * - Mobile: bottom bar with 分享 / 评论 (no copy — iOS native menu handles it)
 *
 * Copy with attribution wraps text in 「」 with author, title, and URL.
 */

import { dispatchQuoteComment } from "./comments/quote-events";
import { langBucket, type Lang } from "./subscribe/i18n";
import { showToast } from "./toast";
import { clampLeft, verticalSlot } from "./viewport";

// ── Lazy share-card chunk ────────────────────────────────────────────────

/**
 * Load the quote-card renderer on first use.
 *
 * `share-card.ts` and its text engine are ~12.4 kb of the theme bundle
 * (~5.1 kb gzipped, ~31% of a page's JS) and run only when a reader selects
 * text and taps Share. They ship as a separate `format: "esm"` bundle so this
 * `import()` is a real network boundary.
 *
 * **The specifier must be computed, not literal.** Everything else in this
 * bundle is `format: "iife"` with no `splitting`, so a
 * literal `import("./share-card")` is resolved and *inlined* by esbuild —
 * which is precisely how this code ended up eager in the first place. A
 * runtime-computed URL is opaque to the bundler; the same trick loads pagefind
 * in `search.ts`.
 *
 * **The URL comes from the build**, on a `data-share-card` attribute of the
 * theme `<script>` tag, because the emitted filename is content-hashed
 * (`share-card.<hash>.js`) and this source cannot know the hash. The attribute
 * is build-global — identical on every page — so it does not trip the preview
 * morph-guard's `scriptsDiffer` full reload.
 */
let shareCardLoad: Promise<typeof import("./share-card")> | null = null;

function loadShareCard(): Promise<typeof import("./share-card")> {
  if (shareCardLoad) return shareCardLoad;
  const src = document.querySelector<HTMLScriptElement>("script[data-share-card]")
    ?.dataset.shareCard;
  if (!src) {
    // No attribute means a build that did not emit the chunk. Reject rather
    // than import a bogus URL, and drop the memo so a later morph that brings
    // the attribute in can retry.
    return Promise.reject(new Error("[moss] share-card chunk URL missing"));
  }
  const url = new URL(src, location.href).href;
  shareCardLoad = (import(/* @vite-ignore */ url) as Promise<typeof import("./share-card")>).catch(
    (err) => {
      // Retryable: a failed fetch must not permanently disable Share.
      shareCardLoad = null;
      throw err;
    },
  );
  return shareCardLoad;
}

/** Shown when the chunk cannot be fetched or the card cannot be drawn. */
const SHARE_FAILED: Record<Lang, string> = {
  en: "Could not create the image",
  "zh-hans": "无法生成图片",
  "zh-hant": "無法產生圖片",
};

// ── Types ────────────────────────────────────────────────────────────────

// Shared capture→render contract lives in share-card/text.ts (module-graph
// leaf); re-exported here so existing importers keep their path.
export type { CardSegment, CardBlock, CardLine } from "./share-card/text";
import type { CardSegment, CardBlock } from "./share-card/text";
// Value import — this one IS a runtime edge, so it must not reach
// share-card/text.ts or the whole canvas engine lands in theme.js. See
// block-text.ts's header.
import { cleanBlockText, splitIntoLines } from "./block-text";

const COPY: Record<Lang, { share: string; comment: string; copy: string }> = {
  en: {
    share: "Share",
    comment: "Comment",
    copy: "Copy",
  },
  "zh-hans": {
    share: "分享",
    comment: "评论",
    copy: "复制",
  },
  "zh-hant": {
    share: "分享",
    comment: "留言",
    copy: "複製",
  },
};

export function initSelectionActions(): void {
  const article = document.querySelector("article.container");
  if (!article) return;

  // Extract metadata from page
  // og:title carries the exact article title; the <title> tag is
  // "article - site" and the split amputates titles containing " - ".
  const ogTitle = document
    .querySelector<HTMLMetaElement>('meta[property="og:title"]')
    ?.getAttribute("content");
  const rawTitle = document.querySelector("title")?.textContent ?? "";
  const title = ogTitle || rawTitle.split(" - ")[0] || "";
  const author =
    document.querySelector('meta[name="author"]')?.getAttribute("content") ?? "";

  // Comments are enabled when the builder injected a .moss-comments section
  // and the page didn't explicitly opt out via data-comments="false".
  const hasComments = !!document.querySelector(".moss-comments")
    && article.getAttribute("data-comments") !== "false";

  // Detect language bucket for button labels
  const lang = langBucket(document.documentElement.lang);
  const labels = COPY[lang];
  const isZh = lang !== "en";

  // Create UI elements
  const popover = createPopover(labels, hasComments);
  document.body.appendChild(popover);

  const mobileBar = createMobileBar(labels, hasComments);
  document.body.appendChild(mobileBar);

  const isMobile =
    "ontouchstart" in window ||
    window.matchMedia("(pointer: coarse)").matches;

  // Desktop: mouseup to show, mousedown to hide
  if (!isMobile) {
    document.addEventListener("mouseup", () => {
      requestAnimationFrame(() => handleDesktopSelection(article, popover));
    });

    document.addEventListener("mousedown", (e) => {
      // Right-click must not dismiss: the reader is opening a context menu on
      // the selection (or a moss menu, once one exists here), and hiding the
      // popover the instant the menu appears made the two look mutually
      // exclusive. Only a primary-button press outside dismisses.
      if (e.button === 2) return;
      if (!popover.contains(e.target as Node)) {
        hideElement(popover);
      }
    });
  }

  // Mobile: selectionchange with debounce
  if (isMobile) {
    let debounceTimer: ReturnType<typeof setTimeout>;
    document.addEventListener("selectionchange", () => {
      clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        handleMobileSelection(article, mobileBar);
      }, 300);
    });
  }

  // Scroll dismiss — matches Medium, iOS, and Google Docs behavior
  document.addEventListener(
    "scroll",
    () => {
      hideElement(popover);
      hideElement(mobileBar);
    },
    { passive: true }
  );

  // Wire up button actions
  setupActions(popover, mobileBar, title, author, isZh);
}

// ── DOM creation ─────────────────────────────────────────────────────────

function createPopover(labels: { share: string; comment: string; copy: string }, hasComments: boolean): HTMLDivElement {
  const el = document.createElement("div");
  el.className = "sel-popover";
  const commentHtml = hasComments
    ? `<span class="sel-sep"></span>
    <button class="sel-comment" type="button">${labels.comment}</button>`
    : '';
  el.innerHTML = `
    <button class="sel-share" type="button">${labels.share}</button>
    ${commentHtml}
    <span class="sel-sep"></span>
    <button class="sel-copy" type="button">${labels.copy}</button>
  `;
  return el;
}

function createMobileBar(labels: { share: string; comment: string }, hasComments: boolean): HTMLDivElement {
  const el = document.createElement("div");
  el.className = "mobile-bar";
  const commentHtml = hasComments
    ? `<button class="pill-bar-btn sel-comment" type="button">${labels.comment}</button>`
    : '';
  el.innerHTML = `
    <div class="pill-bar">
      <button class="pill-bar-btn sel-share" type="button">${labels.share}</button>
      ${commentHtml}
    </div>
  `;
  return el;
}

// ── Selection handlers ───────────────────────────────────────────────────

// Capture selection data when the popover/bar is shown, because clicking
// a button clears the browser selection before the click event fires.
let capturedText = "";
let capturedBlocks: CardBlock[] = [];

/**
 * The text the share card and copy actions receive. Prefer the cleaned
 * highlighted segments (junk like heading-anchor "#" already stripped);
 * fall back to the raw selection when block capture found nothing.
 */
function cleanSelectedText(sel: Selection, blocks: CardBlock[]): string {
  const highlighted = blocks
    .map((b) =>
      b.lines
        .map((line) =>
          line
            .filter((seg) => seg.highlighted)
            .map((seg) => seg.text)
            .join("")
        )
        .join("\n")
        // A blank line the author typed inside a stanza is part of the text and
        // survives; empty lines at either end are just the unselected remainder
        // of a partly-selected block. Dropping every empty line — which is what
        // filtering line by line does — silently closed up stanza breaks that
        // the card itself draws.
        .replace(/^\n+|\n+$/g, "")
    )
    .filter((text) => text !== "")
    .join("\n");
  return highlighted || sel.toString();
}

function handleDesktopSelection(
  article: Element,
  popover: HTMLDivElement
): void {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || !sel.toString().trim()) {
    hideElement(popover);
    return;
  }

  if (!selectionTouchesArticle(sel, article)) {
    hideElement(popover);
    return;
  }

  // Capture while the selection is alive — it is gone by the time Share runs.
  const range = sel.getRangeAt(0);
  capturedBlocks = captureBlocks(sel, article);
  capturedText = cleanSelectedText(sel, capturedBlocks);

  positionPopover(popover, range.getBoundingClientRect());
}

/**
 * Put the popover above the selection, without letting it leave the screen.
 *
 * `position: fixed`, so the rect's viewport coordinates are already the right
 * frame. Two things here are less obvious than they look:
 *
 * **Show before measuring.** `.sel-popover` is `display: none` until `.visible`
 * lands, and a `display: none` box measures 0 in both axes. The original code
 * read `offsetHeight` first and fell back to a hard-coded 36 — which meant the
 * fallback was not a jsdom convenience, it was the only branch that ever ran.
 * Adding the class first forces a real layout, so both dimensions are the
 * popover's actual size. Nothing flashes: the reads and the writes are the same
 * task, so the browser paints once, already positioned.
 *
 * **The left edge is computed here, not by CSS.** `.sel-popover` used to carry
 * `transform: translateX(-50%)`, which turned this `left` into a centre point —
 * so a selection near the left margin put half the bar off-screen and no clamp
 * written here could have seen it. Centring is now this function's arithmetic,
 * which is also the only way `clampLeft` can know where the bar really starts.
 */
function positionPopover(popover: HTMLDivElement, rect: DOMRect): void {
  showElement(popover);

  const width = popover.offsetWidth;
  const height = popover.offsetHeight;

  popover.style.left = `${clampLeft(rect.left + rect.width / 2 - width / 2, width)}px`;
  // Above by default — the popover would otherwise cover the words the reader
  // just highlighted. `verticalSlot` flips it below for a selection on the
  // first visible line, where "above" is off the top of the screen.
  popover.style.top = `${verticalSlot(rect, height, "above").top}px`;
}

function handleMobileSelection(
  article: Element,
  mobileBar: HTMLDivElement
): void {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || !sel.toString().trim()) {
    hideElement(mobileBar);
    return;
  }

  if (!selectionTouchesArticle(sel, article)) {
    hideElement(mobileBar);
    return;
  }

  capturedBlocks = captureBlocks(sel, article);
  capturedText = cleanSelectedText(sel, capturedBlocks);
  showElement(mobileBar);
}

/**
 * Does this selection cover any of the article's prose?
 *
 * The anchor alone does not answer that. The anchor is where the reader put the
 * cursor DOWN, so a drag that starts just above the first paragraph — in the
 * title, in the byline, in the padding above them — anchors outside the article
 * and used to offer no Share button at all, however much prose it went on to
 * cover. Selecting upward from inside the article has the same shape with the
 * roles swapped: the anchor is at the END of what the reader sees highlighted.
 *
 * So either endpoint being inside is enough, plus the case where the selection
 * swallows the article whole (Select All) and neither endpoint is in it.
 * `containsNode` is optional-chained because jsdom does not implement it; the
 * endpoint checks are what the tests hold.
 */
function selectionTouchesArticle(sel: Selection, article: Element): boolean {
  if (sel.anchorNode && article.contains(sel.anchorNode)) return true;
  if (sel.focusNode && article.contains(sel.focusNode)) return true;
  return sel.containsNode?.(article, true) ?? false;
}

// ── Context capture ──────────────────────────────────────────────────────

// The elements a reader would call "a block of text". FIGCAPTION, PRE, DT and
// DD are here because a selection inside one used to climb past it to the
// nearest DIV — sometimes a layout wrapper — and take the entire section as one
// block. DIV stays last in spirit: it is the fallback, not a prose element.
const BLOCK_TAGS = new Set([
  "P", "LI", "BLOCKQUOTE", "H1", "H2", "H3", "H4", "H5", "H6",
  "PRE", "FIGCAPTION", "DT", "DD", "DIV",
]);
const BLOCK_SELECTOR = [...BLOCK_TAGS].join(",");

function findBlock(node: Node): Element | null {
  let n: Node | null = node;
  while (n && !(n instanceof Element && BLOCK_TAGS.has(n.nodeName))) {
    n = n.parentNode;
  }
  return n as Element | null;
}

// ── Block-aware context capture ─────────────────────────────────────────

const CONTEXT_BUDGET = 200;
const CUT_PUNCT = /[,.;:!?。，；：！？]/;

/**
 * The part of `range` that lies inside `scope`.
 *
 * A reader's drag does not respect the article's boundaries — it can begin in
 * the title and end three paragraphs down, or start in the last paragraph and
 * run into the footer. The card is a card of the ARTICLE, so an endpoint
 * outside it is pulled to the article's own edge. Without this, relaxing the
 * gate to "either endpoint is inside" would put nav links and footer boilerplate
 * on the card as prose.
 */
function clampToScope(range: Range, scope?: Element): Range {
  if (!scope) return range;
  const inside = document.createRange();
  inside.selectNodeContents(scope);

  const clamped = range.cloneRange();
  if (!scope.contains(clamped.startContainer)) {
    clamped.setStart(inside.startContainer, inside.startOffset);
  }
  if (!scope.contains(clamped.endContainer)) {
    clamped.setEnd(inside.endContainer, inside.endOffset);
  }
  return clamped;
}

/**
 * Capture block-aware context around a selection. Returns an ordered array of
 * CardBlock objects: optional before-sibling, selection block(s), optional
 * after-sibling.
 *
 * `scope` — when given, the article the card belongs to. See `clampToScope`.
 */
export function captureBlocks(sel: Selection, scope?: Element): CardBlock[] {
  if (!sel.rangeCount) return [];
  const range = clampToScope(sel.getRangeAt(0), scope);
  if (range.collapsed) return [];

  const startBlock = findBlock(range.startContainer);
  const endBlock = findBlock(range.endContainer);
  if (!startBlock) return [];

  const effectiveEndBlock = endBlock ?? startBlock;

  // Build selection blocks
  const selBlocks: CardBlock[] = [];

  if (startBlock === effectiveEndBlock) {
    // Single-block selection
    selBlocks.push(buildSelectionBlock(startBlock, range));
  } else {
    // Multi-block: collect all blocks between start and end (inclusive)
    const blocks = collectBlocksBetween(startBlock, effectiveEndBlock);
    for (const block of blocks) {
      selBlocks.push(buildSelectionBlock(block, range));
    }
  }

  // Calculate budget consumed by same-block (non-highlighted) text
  let budgetUsed = 0;
  for (const b of selBlocks) {
    for (const line of b.lines) {
      for (const seg of line) {
        if (!seg.highlighted) budgetUsed += seg.text.length;
      }
    }
  }

  const remainingBudget = Math.max(0, CONTEXT_BUDGET - budgetUsed);

  // Expand to siblings (max 1 per side)
  const result: CardBlock[] = [];

  const beforeSibling = findPreviousBlockSibling(startBlock);
  const afterSibling = findNextBlockSibling(effectiveEndBlock);

  // Split remaining budget between before and after siblings
  let beforeBudget: number;
  let afterBudget: number;
  if (beforeSibling && afterSibling) {
    beforeBudget = Math.floor(remainingBudget / 2);
    afterBudget = remainingBudget - beforeBudget;
  } else if (beforeSibling) {
    beforeBudget = remainingBudget;
    afterBudget = 0;
  } else {
    beforeBudget = 0;
    afterBudget = remainingBudget;
  }

  // Before-sibling: take clauses from the END
  if (beforeSibling && beforeBudget > 0) {
    const sibText = cleanBlockText(beforeSibling).text;
    const truncated = truncateFromEnd(sibText, beforeBudget);
    if (truncated) {
      result.push({
        lines: splitIntoLines([{ text: truncated, highlighted: false }]),
        tag: beforeSibling.nodeName,
        marker: listMarker(beforeSibling),
      });
    }
  }

  // Selection blocks
  result.push(...selBlocks);

  // After-sibling: take clauses from the START
  if (afterSibling && afterBudget > 0) {
    const sibText = cleanBlockText(afterSibling).text;
    const truncated = truncateFromStart(sibText, afterBudget);
    if (truncated) {
      result.push({
        lines: splitIntoLines([{ text: truncated, highlighted: false }]),
        tag: afterSibling.nodeName,
        marker: listMarker(afterSibling),
      });
    }
  }

  return result;
}

/**
 * Build a CardBlock for a single block element, splitting text into
 * highlighted and non-highlighted segments based on the selection range.
 *
 * The selection range may start before or end after this block (in multi-block
 * selections). We clamp to the block boundaries using compareBoundaryPoints.
 */
function buildSelectionBlock(block: Element, range: Range): CardBlock {
  const { text: fullText, toClean } = cleanBlockText(block);
  const tag = block.nodeName;

  // Create a range covering the entire block
  const blockRange = document.createRange();
  blockRange.selectNodeContents(block);

  // Determine effective start: is selection start before or within this block?
  // compareBoundaryPoints(START_TO_START) < 0 means range starts before blockRange
  const selStartsBeforeBlock =
    range.compareBoundaryPoints(Range.START_TO_START, blockRange) <= 0;

  // Determine effective end: does selection end after or within this block?
  // compareBoundaryPoints(END_TO_END) > 0 means range ends after blockRange
  const selEndsAfterBlock =
    range.compareBoundaryPoints(Range.END_TO_END, blockRange) >= 0;

  // Compute the text offset where the highlight starts within this block
  let startOffset: number;
  if (selStartsBeforeBlock) {
    startOffset = 0;
  } else {
    const preRange = document.createRange();
    preRange.setStart(block, 0);
    preRange.setEnd(range.startContainer, range.startOffset);
    startOffset = toClean(preRange.toString().length);
  }

  // Compute the text offset where the highlight ends within this block
  let endOffset: number;
  if (selEndsAfterBlock) {
    endOffset = fullText.length;
  } else {
    const preEndRange = document.createRange();
    preEndRange.setStart(block, 0);
    preEndRange.setEnd(range.endContainer, range.endOffset);
    endOffset = toClean(preEndRange.toString().length);
  }

  const segments: CardSegment[] = [];

  const prefix = fullText.slice(0, startOffset);
  if (prefix) {
    segments.push({ text: prefix, highlighted: false });
  }

  const highlighted = fullText.slice(startOffset, endOffset);
  if (highlighted) {
    segments.push({ text: highlighted, highlighted: true });
  }

  const suffix = fullText.slice(endOffset);
  if (suffix) {
    segments.push({ text: suffix, highlighted: false });
  }

  return { lines: splitIntoLines(segments), tag, marker: listMarker(block) };
}

/**
 * The marker the page shows in front of a list item — "•" for a bullet,
 * "3." for the third item of an ordered list (honouring the list's `start`).
 *
 * The marker is drawn by CSS, so it is in no text node and no selection can
 * contain it. The card has to reconstruct it, or a shared list item arrives as
 * a bare sentence that no longer reads as part of a list.
 */
function listMarker(el: Element): string | undefined {
  if (el.nodeName !== "LI") return undefined;
  const list = el.parentElement;
  if (!list || list.nodeName !== "OL") return "•";
  const start = Number(list.getAttribute("start") ?? "1");
  const index = Array.from(list.children).indexOf(el);
  const n = (Number.isFinite(start) ? start : 1) + Math.max(0, index);
  return `${n}.`;
}

/**
 * Every block the selection passes through, in document order.
 *
 * Walks the document, not the sibling chain. The sibling walk could only see a
 * selection that began and ended among children of one parent; drag from a
 * paragraph into a list, or out of a blockquote, and it hit `null`, gave up, and
 * returned `[start, end]` — silently dropping every block in between with no
 * error and no gap in the card to hint that anything was missing.
 *
 * Containers are dropped: when the span contains both a `BLOCKQUOTE` and the
 * paragraphs inside it, only the paragraphs are prose. Keeping both would draw
 * the same words twice.
 */
function collectBlocksBetween(start: Element, end: Element): Element[] {
  if (start === end) return [start];

  let root: Element | null = start;
  while (root && !root.contains(end)) root = root.parentElement;
  if (!root) return [start, end];

  const all = Array.from(root.querySelectorAll(BLOCK_SELECTOR));
  const i = all.indexOf(start);
  const j = all.indexOf(end);
  if (i < 0 || j < 0 || j < i) return [start, end];

  const span = all.slice(i, j + 1);
  return span.filter((el) => !span.some((other) => other !== el && el.contains(other)));
}

/**
 * Find the previous sibling that is a block element.
 */
function findPreviousBlockSibling(el: Element): Element | null {
  let sib = el.previousElementSibling;
  while (sib) {
    if (BLOCK_TAGS.has(sib.nodeName)) return sib;
    sib = sib.previousElementSibling;
  }
  return null;
}

/**
 * Find the next sibling that is a block element.
 */
function findNextBlockSibling(el: Element): Element | null {
  let sib = el.nextElementSibling;
  while (sib) {
    if (BLOCK_TAGS.has(sib.nodeName)) return sib;
    sib = sib.nextElementSibling;
  }
  return null;
}

/**
 * Truncate text from the END to fit within budget, cutting at clause boundaries.
 * Returns the trailing portion of the text (nearest to the quote).
 * Returns null if no complete clause fits.
 */
export function truncateFromEnd(text: string, budget: number): string | null {
  if (text.length <= budget) return text;

  // Take the last `budget` chars, then find the first clause-ending punctuation
  // to get a clean start.
  const tail = text.slice(-budget);

  // Find the first clause-ending punctuation in the tail — everything after it
  // is a complete clause sequence.
  let cutIdx = -1;
  for (let i = 0; i < tail.length; i++) {
    if (CUT_PUNCT.test(tail[i])) {
      cutIdx = i;
      break;
    }
  }

  if (cutIdx < 0) return null; // no clause boundary found

  // Return from after the cut point to the end
  const result = tail.slice(cutIdx + 1);
  return result.length > 0 ? result : null;
}

/**
 * Truncate text from the START to fit within budget, cutting at clause boundaries.
 * Returns the leading portion of the text (nearest to the quote).
 * Returns null if no complete clause fits.
 */
export function truncateFromStart(text: string, budget: number): string | null {
  if (text.length <= budget) return text;

  // Take the first `budget` chars, then find the last clause-ending punctuation
  // to get a clean end.
  const head = text.slice(0, budget);

  // Find the last clause-ending punctuation in the head
  let cutIdx = -1;
  for (let i = head.length - 1; i >= 0; i--) {
    if (CUT_PUNCT.test(head[i])) {
      cutIdx = i;
      break;
    }
  }

  if (cutIdx < 0) return null; // no clause boundary found

  // Return from start to and including the punctuation
  const result = head.slice(0, cutIdx + 1);
  return result.length > 0 ? result : null;
}

// ── Visibility helpers ───────────────────────────────────────────────────

function showElement(el: HTMLElement): void {
  el.classList.add("visible");
}

function hideElement(el: HTMLElement): void {
  el.classList.remove("visible");
}

// ── Actions ──────────────────────────────────────────────────────────────

function setupActions(
  popover: HTMLDivElement,
  mobileBar: HTMLDivElement,
  title: string,
  author: string,
  isZh: boolean
): void {
  const url = window.location.href;

  // Copy with attribution (desktop popover only)
  const copyBtn = popover.querySelector(".sel-copy");
  if (copyBtn) {
    copyBtn.addEventListener("click", () => {
      if (capturedText) {
        copyWithAttribution(capturedText, title, author, url, isZh);
      }
    });
  }

  // Share buttons — generate quote card image
  const shareBtns = [
    popover.querySelector(".sel-share"),
    mobileBar.querySelector(".sel-share"),
  ];
  for (const btn of shareBtns) {
    if (!btn) continue;
    btn.addEventListener("click", async () => {
      if (!capturedText) return;
      // The chunk is a real network fetch since 2026-08-04, so it can fail —
      // flaky connection, a host that 404s it, a CSP without `script-src
      // 'self'`. Before the split esbuild inlined it and this could not throw.
      // Without the catch the rejection is unhandled and Share does nothing
      // visible at all, which reads to the reader as a broken button.
      try {
        const { shareAsImage } = await loadShareCard();
        await shareAsImage(capturedText, title, author, {
          blocks: capturedBlocks,
        });
      } catch (err) {
        console.error("[moss] share card failed", err);
        showToast(SHARE_FAILED[langBucket(document.documentElement.lang)]);
      }
    });
  }

  // Comment buttons — dispatch quote-comment event; comment provider listens.
  const commentBtns = [
    popover.querySelector(".sel-comment"),
    mobileBar.querySelector(".sel-comment"),
  ];
  for (const btn of commentBtns) {
    if (!btn) continue;
    btn.addEventListener("click", () => {
      if (capturedText) dispatchQuoteComment(capturedText);
      hideElement(popover);
      hideElement(mobileBar);
    });
  }
}

function copyWithAttribution(
  text: string,
  title: string,
  author: string,
  url: string,
  isZh: boolean
): void {
  const attributed = isZh
    ? `\u300C${text}\u300D\n\u2014\u2014 ${author}\uFF0C${title}\n${url}`
    : `\u201C${text}\u201D\n\u2014 ${author}, ${title}\n${url}`;
  navigator.clipboard.writeText(attributed).catch(() => {
    // Silent fail — clipboard might be blocked
  });
}
