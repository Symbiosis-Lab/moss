/**
 * The picture at the top of a shared quote card.
 *
 * A reader highlights a sentence, taps Share, and moss draws a PNG: the
 * article's own cover photograph across the top 140pt, the quote below it, the
 * site's name at the foot. For two months the photograph was simply missing —
 * `findCoverSource()` searched for `.article-cover img`, a class moss has never
 * emitted in any version — and the suite stayed green the whole time, because
 * the one test covering it hand-built a `div.article-cover` in jsdom. It
 * asserted the author's belief about moss's markup instead of moss's markup.
 *
 * So this gate refuses to look at markup at all. It drives the real flow —
 * select, click Share, wait for the chunk to load over the network — and then
 * reads the PNG the browser actually produced back through a canvas and
 * samples its pixels. Every one of those steps needs an engine: a real 2D
 * context, a real image decode, a real `drawImage`. jsdom's canvas draws
 * nothing and its `fetch` reaches nowhere, so no test in that suite could have
 * caught this however it was written.
 *
 * What is NOT here: whether `data-share-cover` is emitted, and with what value.
 * That is text, so it is a Rust test — `share_cover_*` in `build/page/cover.rs`.
 * Re-checking the attribute here would reproduce the original blind spot one
 * level up: the attribute can be perfect and the strip still blank.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → SHARE_CARD_GATE. Six pages: three,
 * one per rung of `share_cover_url`, with identical body text so their cards
 * differ only by the strip; a poem and the same words as prose; and a page set
 * vertically, whose card is the vertical layout (portrait, cover as a
 * landscape band across the top, columns below it, a reading-end meta block,
 * a corner QR seal) and is sampled for exactly that.
 */
import { test, expect, type Page } from "@playwright/test";

/** The card's warm paper background, light palette (`buildPalette`). */
const PAPER: RGB = [0xf5, 0xf0, 0xe6];
/** The two fixture covers, deliberately nothing like paper or like each other. */
const MAGENTA: RGB = [0xff, 0x00, 0xff];
const CYAN: RGB = [0x00, 0xff, 0xff];

type RGB = [number, number, number];

interface CardReadout {
  /** Device pixels; the card is drawn at `scale = 2`. */
  width: number;
  height: number;
  /** Three samples across the middle of where the cover strip belongs. */
  strip: RGB[];
  /** How many pixels in the bottom-right QR plate are ink rather than paper. */
  qrInk: number;
  /**
   * Three samples down the left gutter, `x = 12` CSS px, all below where the
   * horizontal card's cover strip ends. On a horizontal card they sit inside
   * the 36pt padding, so they are paper whatever the text wraps to; on a
   * vertical card they are inside the cover band.
   */
  gutter: RGB[];
  /**
   * Ink bounding box of the quote area — everything right of a possible cover
   * band, between the top padding and the bar — in device pixels. `null` when
   * the area holds no ink.
   */
  quoteInk: InkBox | null;
  /** Ink bounding box of the rightmost column's window, same units. */
  firstColumnInk: InkBox | null;
}

interface InkBox {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/**
 * Drive the reader's actual path to a card and read the result back.
 *
 * The selection is made with a `Range` rather than a mouse drag — a drag over
 * wrapped text lands on different words in the two engines, and the words are
 * not what this gate is about — but everything after it is the real thing: the
 * real `mouseup` handler decides whether to show the popover, a real click on
 * the real Share button runs the real handler, and the share-card module is
 * fetched over the network (it is a content-hashed chunk named by a
 * `data-share-card` attribute, so the `import()` is a genuine round trip).
 */
async function produceCard(
  page: Page,
  path: string,
  paraIndex = 1
): Promise<CardReadout> {
  await page.goto(path);

  // `initSelectionActions` appends the popover; until it has, there is nothing
  // for a selection to reveal.
  await page.waitForSelector(".sel-popover", { state: "attached" });

  // Read down to the paragraph first. The popover is `position: fixed` and
  // placed from the selection's viewport rect, so a selection below the fold
  // puts it off-screen — which is exactly what happens on the hero page, where
  // a full-width photograph sits above the text. moss also dismisses the
  // popover on scroll, so the scrolling has to finish before the selection.
  // `inline` too: a vertical page scrolls along x.
  await page.evaluate((i) => {
    document
      .querySelectorAll("article.container p")[i]
      .scrollIntoView({ block: "center", inline: "center" });
  }, paraIndex);
  await page.waitForTimeout(300);

  await page.evaluate((i) => {
    const para = document.querySelectorAll("article.container p")[i];
    const range = document.createRange();
    range.selectNodeContents(para);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    // What a finished drag dispatches. The handler reads the live selection.
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  }, paraIndex);

  const share = page.locator(".sel-popover.visible .sel-share");
  await expect(share).toBeVisible();
  await share.click();

  // The overlay appears as soon as the canvas has been drawn and encoded; the
  // `<img>` inside it holds the finished PNG as a data URL. No Web Share API in
  // either headless engine, but that branch is irrelevant here — the lightbox
  // is shown BEFORE moss tries to share or copy, so this is the same image the
  // reader would receive either way.
  await page.waitForSelector(".share-card-overlay img");
  await page.waitForFunction(() => {
    const el = document.querySelector<HTMLImageElement>(".share-card-overlay img");
    return !!el && el.complete && el.naturalWidth > 0;
  });

  return page.evaluate(() => {
    const el = document.querySelector<HTMLImageElement>(".share-card-overlay img")!;
    // The card arrives as a `data:` URL, which never taints a canvas, so the
    // pixels can be read straight back. (Nor did the cover taint the card moss
    // drew: `data-share-cover` is same-origin by construction — `share_cover_url`
    // drops anything else — which is half of why the attribute exists.)
    const canvas = document.createElement("canvas");
    canvas.width = el.naturalWidth;
    canvas.height = el.naturalHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(el, 0, 0);
    // Mid-strip vertically (the strip is 140pt tall at scale 2 = 280px), and
    // three points across, because a cover is drawn edge to edge.
    const y = 140;
    const at = (x: number): [number, number, number] => {
      const d = ctx.getImageData(x, y, 1, 1).data;
      return [d[0], d[1], d[2]];
    };
    // The QR plate: 64pt square at the bar's right edge, 36pt in from it, at
    // scale 2. Sampling the region rather than a point, because which modules
    // are dark depends on what the code encodes.
    const plate = ctx.getImageData(
      canvas.width - (36 + 64) * 2,
      canvas.height - (36 + 64) * 2,
      64 * 2,
      64 * 2
    ).data;
    let qrInk = 0;
    for (let i = 0; i < plate.length; i += 4) {
      if (plate[i] < 128 && plate[i + 1] < 128 && plate[i + 2] < 128) qrInk++;
    }

    // Where the ink is inside a window, in device pixels. Ink is anything
    // darker than mid-grey; the paper and both fixture covers are lighter.
    const inkBox = (x0: number, y0: number, x1: number, y1: number) => {
      const d = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data;
      const box = { left: Infinity, right: -1, top: Infinity, bottom: -1 };
      for (let i = 0; i < d.length; i += 4) {
        if (d[i] + d[i + 1] + d[i + 2] >= 3 * 128) continue;
        const px = (i / 4) % (x1 - x0);
        const py = Math.floor(i / 4 / (x1 - x0));
        box.left = Math.min(box.left, x0 + px);
        box.right = Math.max(box.right, x0 + px);
        box.top = Math.min(box.top, y0 + py);
        box.bottom = Math.max(box.bottom, y0 + py);
      }
      return box.right < 0 ? null : box;
    };
    // The quote area: right of the widest possible cover band (140pt), below
    // the top padding, above the tallest possible bar (a two-line title with a
    // QR is ~110pt). The bar's plate would otherwise count as quote ink.
    const barTop = canvas.height - 110 * 2;
    const quoteInk = inkBox((140 + 36) * 2, 36 * 2, canvas.width - 36 * 2 + 4, barTop);
    // The rightmost column's window: one 16pt em plus 4pt of slack, hung at
    // the right padding.
    const firstColumnInk = inkBox(
      canvas.width - (36 + 16 + 4) * 2,
      36 * 2,
      canvas.width - (36 - 4) * 2,
      barTop
    );

    return {
      width: canvas.width,
      height: canvas.height,
      strip: [at(20), at(Math.floor(canvas.width / 2)), at(canvas.width - 20)],
      qrInk,
      gutter: [300, 500, 700].map((gy) => {
        const d = ctx.getImageData(12 * 2, gy, 1, 1).data;
        return [d[0], d[1], d[2]] as RGB;
      }),
      quoteInk,
      firstColumnInk,
    };
  });
}

/** PNG is lossless and the readback is 1:1, so this only absorbs decode rounding. */
function expectColour(actual: RGB[], expected: RGB): void {
  for (const px of actual) {
    for (let c = 0; c < 3; c++) {
      expect(Math.abs(px[c] - expected[c])).toBeLessThanOrEqual(6);
    }
  }
}

test.describe("share card cover strip", () => {
  test("a page with a :::hero image paints that image across the card's top", async ({
    page,
  }) => {
    const card = await produceCard(page, "/hero/");
    expectColour(card.strip, MAGENTA);
  });

  test("a page whose cover is frontmatter-only paints that image too", async ({ page }) => {
    // Rung 2 of `share_cover_url`. Cyan, not magenta: if the lookup ever
    // reached past this page for a cover, the wrong fixture would show up.
    const card = await produceCard(page, "/covered/");
    expectColour(card.strip, CYAN);
    // Pin the horizontal composition by geometry and pixels, not a PNG hash:
    // system font rasterization and PNG encoding differ by OS and engine.
    expect(card.width).toBe(750);
    expect(card.quoteInk).not.toBeNull();
    expect(card.quoteInk!.top).toBeGreaterThanOrEqual(280);
    expectColour(card.gutter, PAPER);
  });

  test("a page with no cover of its own gets no strip, and a shorter card", async ({
    page,
  }) => {
    // This page HAS an og:image — the auto-generated 1200×630 title card moss
    // makes for every page without a cover. Feeding that into the cover strip
    // was a real regression once (fixed in 05b1d85cb): a card whose photograph
    // was a picture of its own title, in tofu for CJK titles. The card must be
    // bare paper here, and this assertion is what keeps that shut, since the
    // og tag is the most tempting thing on the page to reach for.
    await page.goto("/plain/");
    const og = await page
      .locator('meta[property="og:image"]')
      .getAttribute("content");
    // Absolute, because this gate builds the site as deployed (`siteUrl`) so
    // that it has QR codes at all.
    expect(og).toMatch(/^https:\/\/gate\.test\/_moss\/og\/.*\.png$/);

    const plain = await produceCard(page, "/plain/");
    expectColour(plain.strip, PAPER);

    // And the absence is structural, not just pale: with no strip to reserve,
    // the whole card is 140pt (280px at scale 2) shorter. Same body text on
    // both pages, so the strip is the only difference between them.
    const hero = await produceCard(page, "/hero/");
    expect(hero.width).toBe(plain.width);
    // The card has a 300pt floor. Both fixtures clear it by a wide margin —
    // asserted, because a clamped card would make the difference below
    // arbitrary rather than the strip's height.
    expect(plain.height).toBeGreaterThan(600);
    expect(hero.height - plain.height).toBe(280);
  });
});

test.describe("share card QR code", () => {
  test("the card draws the code the page names, fetched from where it says", async ({
    page,
  }) => {
    const fetched: string[] = [];
    page.on("request", (r) => {
      if (r.url().includes("/qr/")) fetched.push(new URL(r.url()).pathname);
    });

    await page.goto("/plain/");
    const attr = await page
      .locator("article.container")
      .getAttribute("data-share-qr");
    expect(attr).toBe("/qr/plain.svg");

    const card = await produceCard(page, "/plain/");

    // Fetched exactly what the page named — no derived filename in between.
    expect(fetched).toContain(attr!);
    // And it is on the card: a plate of ink where paper would otherwise be.
    // A v2–v4 code at 56pt fills a few thousand device pixels; anything above
    // a few hundred is a code rather than stray anti-aliasing.
    expect(card.qrInk).toBeGreaterThan(500);
  });

  test("a card whose QR 404s still renders, without a plate", async ({ page }) => {
    // The corner is the last thing drawn and the least important; a missing
    // code must not cost the reader the card.
    await page.route("**/qr/**", (route) => route.fulfill({ status: 404 }));
    const card = await produceCard(page, "/plain/");
    expect(card.height).toBeGreaterThan(600);
    expect(card.qrInk).toBeLessThan(100);
  });
});

test.describe("share card line breaks", () => {
  test("a poem is set on the poet's lines, not reflowed into a paragraph", async ({
    page,
  }) => {
    // Same words, same page furniture; the only difference is that one page
    // wrote them on eight lines and the other on one. Eight lines at 16pt with
    // moss's 1.85 line-height is ~150pt taller than the three the prose needs
    // — so if the card ever reflows the breaks away again, the two cards
    // become nearly the same height and this fails.
    const poem = await produceCard(page, "/poem/", 0);
    const prose = await produceCard(page, "/prose/", 0);
    expect(poem.height).toBeGreaterThan(prose.height + 200);
  });
});

/**
 * The v2 vertical layout: a landscape cover band across the full top, the
 * quote below it in columns, a reading-end meta block (title/author/url)
 * past a hairline at the far left, and a QR seal in the bottom-left corner.
 * `produceCard()`'s `CardReadout` was built for the v1 geometry (cover as a
 * left band, citation in a horizontal foot bar) and samples the wrong
 * coordinates for this one, so this drives the same real flow — select,
 * click Share, wait for the PNG — through its own readout instead of
 * stretching `CardReadout` to mean two different things.
 *
 * Geometry mirrors the constants in share-card/vertical.ts at scale=2:
 * PADDING=36, BAND_HEIGHT=150, BAND_GAP=20, QUOTE_ZONE_HEIGHT=250,
 * FOOT_GAP=20, QR_SIZE=72, QR_FRAME=6.
 */
interface VerticalReadout {
  width: number;
  height: number;
  /** Three samples across the cover band, mid-height of it. */
  bandRow: RGB[];
  /** Ink bounding box of the rightmost quote column's own window. */
  firstColumnInk: InkBox | null;
  /** Ink bounding box of just the topmost glyph in the rightmost quote
   * column — a fixed-height slice from the column's own top, one cell
   * (`topCellHeightPx`) tall, in a window wide enough for either quote font
   * size (16 or 22px). Lets a test read one glyph's own shape (a bracket's
   * rotated width-over-height, say) instead of the whole column's aggregate
   * box, which is tall by construction once more than one character is
   * stacked in it. */
  firstGlyphInk: InkBox | null;
  /** Ink bounding box of each of the column's first 10 cells, same fixed
   * slicing as `firstGlyphInk` — lets a test compare one cell's own ink
   * (is it centred, like an ordinary CJK character, or corner/edge-shifted)
   * against another's, within the same render. Empty unless
   * `topCellHeightPx` is given. */
  cellInk: (InkBox | null)[];
  /** Ink bounding box of everything left of the quote's own column window,
   * down to the card's left padding — the reading-end meta columns. */
  metaInk: InkBox | null;
  /** Dark-pixel count inside the QR seal's content box. */
  qrInk: number;
  /** Just inside the seal's hairline frame, outside the QR image itself —
   * the quiet zone the frame reinforces. */
  qrQuietZone: RGB[];
  /** Average `r+g+b` over `firstColumnInk`'s own box — the selection
   * column's real ink strength, for comparing against `contextInkAvg`. */
  selectionInkAvg: number | null;
  /** Average `r+g+b` over a faded context column's own box, when
   * `captureBlocks` pulled a before/after sibling paragraph in as grey
   * context (it draws to the right of the selection column) — `null` when
   * no such column painted. Sits strictly between paper's own sum (~715)
   * and `selectionInkAvg` when the fade is real. */
  contextInkAvg: number | null;
}

async function produceVerticalCard(
  page: Page,
  path: string,
  paraIndex: number,
  // Device-px height of the column's own first cell (fontSize * scale), for
  // `firstGlyphInk` below — undefined when a test has no need of it.
  topCellHeightPx?: number
): Promise<VerticalReadout> {
  await page.goto(path);
  await page.waitForSelector(".sel-popover", { state: "attached" });
  await page.evaluate((i) => {
    document
      .querySelectorAll("article.container p")[i]
      .scrollIntoView({ block: "center", inline: "center" });
  }, paraIndex);
  await page.waitForTimeout(300);
  await page.evaluate((i) => {
    const para = document.querySelectorAll("article.container p")[i];
    const range = document.createRange();
    range.selectNodeContents(para);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  }, paraIndex);

  const share = page.locator(".sel-popover.visible .sel-share");
  await expect(share).toBeVisible();
  await share.click();

  await page.waitForSelector(".share-card-overlay img");
  await page.waitForFunction(() => {
    const el = document.querySelector<HTMLImageElement>(".share-card-overlay img");
    return !!el && el.complete && el.naturalWidth > 0;
  });

  return page.evaluate((cellH) => {
    const el = document.querySelector<HTMLImageElement>(".share-card-overlay img")!;
    const canvas = document.createElement("canvas");
    canvas.width = el.naturalWidth;
    canvas.height = el.naturalHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(el, 0, 0);

    const scale = 2;
    const at = (x: number, y: number): [number, number, number] => {
      const d = ctx.getImageData(x, y, 1, 1).data;
      return [d[0], d[1], d[2]];
    };

    const bandMidY = Math.round(75 * scale);
    // Nine points, not three: a band that is only partly filled (object-fit:
    // cover math applied to the wrong destination width, or a partial-width
    // `drawImage`) reads as paper past whatever fraction it stopped at, and a
    // sparse sample can land entirely inside the painted fraction and miss
    // it. This spacing catches a cutoff anywhere from 10% to 90% of the way
    // across.
    const bandRow: [number, number, number][] = Array.from({ length: 9 }, (_, i) => {
      const frac = (i + 1) / 10;
      return at(Math.round(frac * canvas.width), bandMidY);
    });

    // `cutoff` defaults to the strict "is this basically full-strength ink"
    // reading every other caller wants; a looser cutoff (below) finds a
    // fainter, context-faded box instead, cropped tight to its own real ink
    // rather than the tall, mostly-paper window it was searched inside —
    // averaging that uncropped window would read as "faded" for a short
    // full-strength column too, just for having more blank rows than ink.
    const inkBox = (x0: number, y0: number, x1: number, y1: number, cutoff = 3 * 128) => {
      const d = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data;
      const box = { left: Infinity, right: -1, top: Infinity, bottom: -1 };
      for (let i = 0; i < d.length; i += 4) {
        if (d[i] + d[i + 1] + d[i + 2] >= cutoff) continue;
        const px = (i / 4) % (x1 - x0);
        const py = Math.floor(i / 4 / (x1 - x0));
        box.left = Math.min(box.left, x0 + px);
        box.right = Math.max(box.right, x0 + px);
        box.top = Math.min(box.top, y0 + py);
        box.bottom = Math.max(box.bottom, y0 + py);
      }
      return box.right < 0 ? null : box;
    };

    const topReserve = Math.round((150 + 20) * scale);
    const footReserve = Math.round(140 * scale);
    // The meta block's own geometry (`HAIRLINE_GAP + META_GAP +
    // metaCount*META_COL_STEP + 8`, vertical.ts:337, metaCount=2 for this
    // fixture's title+author·domain) — the true boundary between the meta
    // columns and the quote area, reused below to keep both `metaInk` and
    // the quote scan out of each other's territory.
    const metaWidthCss = 22 + 22 + 2 * 24 + 8;
    const quoteAreaLeft = Math.round((36 + metaWidthCss) * scale);
    const quoteAreaRight = Math.round((36 - 4) * scale);
    // The quote column carrying the actual selection — not necessarily the
    // rightmost slot. `captureBlocks` (selection-actions.ts) always tries to
    // pull in a before/after sibling paragraph as grey context, up to a
    // shared budget, and vertical draws context columns first (rightmost),
    // so a full-paragraph selection with a short neighbour puts that
    // neighbour's own (faded) column at the right edge instead of the
    // selection. Rather than assume a fixed column pitch (which the extra
    // gap between a context block and the highlighted one would throw off),
    // scan the whole quote area's ink column-by-column and take the first
    // contiguous run — reading right to left — wide enough to be a real
    // column (a CJK glyph is nearly a full em) rather than a stray
    // antialiased pixel off a faded context column. That fade should stay
    // under the strict threshold above (paper ~715, faded context ~500-560
    // against a >=384 cutoff) and mostly does, but not every pixel of it.
    const fontSizeCss = cellH ? cellH / scale : 16;
    const minColumnInkWidth = Math.round(fontSizeCss * 0.5 * scale);
    const quoteAreaW = canvas.width - quoteAreaRight - quoteAreaLeft;
    const quoteAreaData = ctx.getImageData(
      quoteAreaLeft,
      topReserve,
      quoteAreaW,
      canvas.height - footReserve - topReserve
    ).data;
    const colHasInk = new Array(quoteAreaW).fill(false);
    for (let i = 0; i < quoteAreaData.length; i += 4) {
      if (quoteAreaData[i] + quoteAreaData[i + 1] + quoteAreaData[i + 2] >= 3 * 128) continue;
      colHasInk[(i / 4) % quoteAreaW] = true;
    }
    let runLeft = -1;
    let runRight = -1;
    let firstColumnInk: ReturnType<typeof inkBox> = null;
    for (let x = quoteAreaW - 1; x >= 0; x--) {
      if (colHasInk[x]) {
        if (runRight < 0) runRight = x;
        runLeft = x;
        continue;
      }
      if (runRight >= 0) {
        if (runRight - runLeft + 1 >= minColumnInkWidth) {
          firstColumnInk = inkBox(
            quoteAreaLeft + runLeft,
            topReserve,
            quoteAreaLeft + runRight + 1,
            canvas.height - footReserve
          );
          break;
        }
        runRight = -1;
        runLeft = -1;
      }
    }
    // A faded context column, if `captureBlocks` pulled one in, sits to the
    // right of the selection's own column (vertical draws context columns
    // first) and fails the strict `colHasInk` cutoff above by construction —
    // that is what let the strict scan skip past it to find the real
    // selection column. Rescan just that stretch (right of the selection
    // column found above, out to the quote area's own right edge) with a
    // cutoff loose enough to catch the faded ink (sum ~500-560) without
    // catching paper (~715), and report its average ink strength alongside
    // the selection column's own, so a test can compare the two directly
    // instead of only checking presence.
    const avgInkSum = (x0: number, y0: number, x1: number, y1: number): number => {
      const d = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data;
      let sum = 0;
      let n = 0;
      for (let i = 0; i < d.length; i += 4) {
        sum += d[i] + d[i + 1] + d[i + 2];
        n++;
      }
      return n ? sum / n : NaN;
    };
    const LENIENT_INK_CUTOFF = 650;
    // `inkBox` (above) applies the same strict `>= 3*128` cutoff as
    // `colHasInk` — the faded context ink this rescan is looking for reads
    // as "not ink" to it by the same construction that let the strict scan
    // skip past it above, so the box has to be built from the lenient run's
    // own coordinates directly rather than by re-running it through `inkBox`.
    const lenientColHasInk = new Array(quoteAreaW).fill(false);
    for (let i = 0; i < quoteAreaData.length; i += 4) {
      if (quoteAreaData[i] + quoteAreaData[i + 1] + quoteAreaData[i + 2] >= LENIENT_INK_CUTOFF) continue;
      lenientColHasInk[(i / 4) % quoteAreaW] = true;
    }
    let contextRunLeft = -1;
    let contextRunRight = -1;
    let contextColumnInk: ReturnType<typeof inkBox> = null;
    const searchFrom = runRight >= 0 ? runRight + 1 : quoteAreaW;
    for (let x = quoteAreaW - 1; x >= searchFrom; x--) {
      if (lenientColHasInk[x]) {
        if (contextRunRight < 0) contextRunRight = x;
        contextRunLeft = x;
        continue;
      }
      if (contextRunRight >= 0) {
        if (contextRunRight - contextRunLeft + 1 >= minColumnInkWidth) {
          // Crop tight to the faded ink's own extent (lenient cutoff), not
          // the full quote-zone height the run search scanned — an
          // uncropped box would average in blank rows above/below a short
          // context column and read as faded regardless of alpha.
          contextColumnInk = inkBox(
            quoteAreaLeft + contextRunLeft,
            topReserve,
            quoteAreaLeft + contextRunRight + 1,
            canvas.height - footReserve,
            LENIENT_INK_CUTOFF
          );
          break;
        }
        contextRunRight = -1;
        contextRunLeft = -1;
      }
    }
    const selectionInkAvg = firstColumnInk
      ? avgInkSum(firstColumnInk.left, firstColumnInk.top, firstColumnInk.right + 1, firstColumnInk.bottom + 1)
      : null;
    const contextInkAvg = contextColumnInk
      ? avgInkSum(
          contextColumnInk.left,
          contextColumnInk.top,
          contextColumnInk.right + 1,
          contextColumnInk.bottom + 1
        )
      : null;
    // The topmost cell's own box: a fixed-height slice from the column's own
    // top, one cell (`cellH` = fontSize * scale) tall, in a window wide
    // enough for either quote font size (16 or 22px) — not a gap-scan
    // hunting for "the first blob of ink", which a font's own hinting can
    // fragment into more than one row-gap before the real glyph body
    // starts. The invariant this reads for ("every cell advances one em")
    // fixes the slice height directly, so this samples exactly the cell the
    // layout promises rather than whatever ink happens to be contiguous.
    const cellWindowX0 = canvas.width - Math.round((36 + 24 + 8) * scale);
    const cellWindowX1 = canvas.width - Math.round((36 - 8) * scale);
    const cellInk: (ReturnType<typeof inkBox>)[] =
      cellH && firstColumnInk
        ? Array.from({ length: 10 }, (_, i) =>
            inkBox(
              cellWindowX0,
              firstColumnInk.top + i * cellH,
              cellWindowX1,
              firstColumnInk.top + (i + 1) * cellH
            )
          )
        : [];
    const firstGlyph = cellInk[0] ?? null;
    // Everything left of the quote's own column area, down to the left
    // padding — the reading-end meta columns, past the hairline. The right
    // bound is `quoteAreaLeft` above (the meta block's own geometry,
    // vertical.ts:337), not a fixed one-column offset from the canvas edge
    // — the quote area can hold more than one column (a long selection, or
    // a captured before/after context column hung to its right) and a
    // right-edge-relative window would then reach past the hairline into
    // that real, full-strength quote ink.
    const metaInk = inkBox(Math.round(4 * scale), topReserve, quoteAreaLeft, canvas.height - footReserve);

    // QR seal: 72pt content box inset 6pt inside a hairline frame that is
    // itself 36pt from the left and bottom padding.
    const qrX = Math.round((36 + 6) * scale);
    const qrSize = Math.round(72 * scale);
    const qrY = canvas.height - Math.round((36 + 72 + 12) * scale) + Math.round(6 * scale);
    const plate = ctx.getImageData(qrX, qrY, qrSize, qrSize).data;
    let qrInk = 0;
    for (let i = 0; i < plate.length; i += 4) {
      if (plate[i] < 128 && plate[i + 1] < 128 && plate[i + 2] < 128) qrInk++;
    }
    const qrQuietZone: [number, number, number][] = [
      at(qrX - Math.round(2 * scale), qrY + Math.floor(qrSize / 2)),
      at(qrX + Math.floor(qrSize / 2), qrY - Math.round(2 * scale)),
    ];

    return {
      width: canvas.width,
      height: canvas.height,
      bandRow,
      firstColumnInk,
      firstGlyphInk: firstGlyph,
      cellInk,
      metaInk,
      qrInk,
      qrQuietZone,
      selectionInkAvg,
      contextInkAvg,
    };
  }, topCellHeightPx);
}

test.describe("share card on a vertical page", () => {
  // The vertical fixture carries the magenta cover, so the band is telling
  // against the paper the horizontal card has at the same coordinates —
  // asserted by the unchanged horizontal tests above, which never see it.
  test("the cover is a landscape band across the full top", async ({ page }) => {
    const card = await produceVerticalCard(page, "/vertical/", 1);
    expectColour(card.bandRow, MAGENTA);
  });

  test("a short quote is compact; a long one reaches the cap and grows wider, never taller", async ({
    page,
  }) => {
    // Paragraph 1 is ~75 characters (several columns, reaching the
    // QUOTE_ZONE_HEIGHT cap); paragraph 2 is a one-column quote in short
    // mode's larger 22px font. The zone's actual height is `min(cap, tallest
    // column needed)` — the tallest of the quote's own columns *or* the meta
    // block's (title/author/url, identical strings for both cards), so a
    // short quote at the bigger short-mode font isn't guaranteed to land
    // below a long quote's height rendered at the smaller long-mode font.
    // The two independently-true claims are: the long quote actually reaches
    // the cap, and the short one does not need to.
    const long = await produceVerticalCard(page, "/vertical/", 1);
    const short = await produceVerticalCard(page, "/vertical/", 2);
    // topPad (BAND_HEIGHT + BAND_GAP = 170) + QUOTE_ZONE_HEIGHT (250) +
    // footHeight (FOOT_GAP + QR_SIZE + QR_FRAME*2 + PADDING = 140), at
    // scale=2 — a ceiling, not an exact value: font hinting differs by
    // engine, so the tallest column can land a few px under the cap.
    const capHeight = (170 + 250 + 140) * 2;
    expect(long.height).toBeLessThanOrEqual(capHeight);
    expect(long.height).toBeGreaterThan(capHeight - 40);
    expect(short.height).toBeLessThan(capHeight);
    expect(short.width).toBeLessThan(long.width);
  });

  test("the quote stands in columns hung at the right edge", async ({ page }) => {
    const card = await produceVerticalCard(page, "/vertical/", 1);
    const first = card.firstColumnInk!;
    expect(first).not.toBeNull();
    // The columns run tall: the rightmost column carries most of the fixed
    // 250pt column-height cap, not one 16pt line of text.
    expect(first.bottom - first.top).toBeGreaterThan(180 * 2);
    // Upright glyphs: one column is one em wide.
    expect(first.right - first.left).toBeLessThanOrEqual((16 + 2) * 2);
  });

  test("a CJK glyph draws upright, not rotated onto its side", async ({ page }) => {
    // 一 ("one") is a single horizontal stroke — nearly the full em wide and
    // only a hairline tall. Upright, its column reads as that wide mark
    // repeated down the column; rotated a quarter turn (the failure this
    // gate exists for: a browser that doesn't lay `fillText` out vertically,
    // so the whole line drew flat and the *rotate* stood it up glyph and
    // all), that same stroke turns into a narrow vertical sliver a few
    // device pixels wide. Column geometry alone ("the quote stands in
    // columns hung at the right edge", above) can't tell the two apart —
    // both are one column, one em wide, many lines tall — so this samples
    // the one glyph shape that visibly differs between them.
    const card = await produceVerticalCard(page, "/vertical/", 3);
    const first = card.firstColumnInk!;
    expect(first).not.toBeNull();
    expect(first.right - first.left).toBeGreaterThan(12);
  });

  test("a bracket rotates onto its side instead of drawing upright", async ({
    page,
  }) => {
    // Paragraph 2's short quote is "「石痕，墨跡也。」"; short mode wraps it
    // in another 「…」 pair, so the column's own top glyph is 「. Unrotated,
    // a bracket's ink is a tall narrow stroke (upright, like any Latin
    // punctuation mark); vertical typesetting rotates brackets and
    // quote-corners a quarter turn, like a Latin run, which turns that same
    // stroke wide and short. `firstGlyphInk` isolates the one glyph's own
    // cell — the whole column's box is tall by construction once more than
    // one character is stacked in it, so it can't tell rotated from upright.
    const card = await produceVerticalCard(page, "/vertical/", 2, 22 * 2);
    const glyph = card.firstGlyphInk!;
    expect(glyph).not.toBeNull();
    expect(glyph.right - glyph.left).toBeGreaterThan(glyph.bottom - glyph.top);
  });

  test("a full-width colon draws centred in its cell, not overlapping the glyph above it", async ({
    page,
  }) => {
    // Paragraph 4, short mode: "印：「字曰年」。" wrapped in 「…」 is
    // 「印：「字曰年」。」 — the column's cells in order are 「(rotate),
    // 印(upright), ：(upright), 「(rotate), 字/曰/年(upright), 」(rotate),
    // 。(corner), 」(rotate). The reported bug: ： took the same top-right
    // corner offset as ，。、, whose `y - advance * 0.15` nudge pulls it up
    // toward the cell above — which reads as the colon overlapping 印. The
    // gap between 印's own ink (cell 1) and ：'s (cell 2) is the direct
    // read on that: corner-shifted, the nudge nearly halves it (measured
    // ~5-6 device px against ~13-14 fixed, ablating the punctuation-
    // classification fix); centred, ：'s natural glyph shape (small, high
    // in its own cell) leaves the gap intact.
    const card = await produceVerticalCard(page, "/vertical/", 4, 22 * 2);
    const yin = card.cellInk[1]!; // 印
    const colon = card.cellInk[2]!; // ：
    expect(yin).not.toBeNull();
    expect(colon).not.toBeNull();
    expect(colon.top - yin.bottom).toBeGreaterThan(9);
  });

  test("the reading-end meta sits past a hairline, left of the quote", async ({
    page,
  }) => {
    const card = await produceVerticalCard(page, "/vertical/", 1);
    const meta = card.metaInk!;
    const quote = card.firstColumnInk!;
    expect(meta).not.toBeNull();
    // The meta block (title, then one author·domain column) is genuinely to
    // the left of the quote's own column window, with the hairline's gap
    // between them — not the same ink counted twice.
    expect(meta.right).toBeLessThan(quote.left);
  });

  test("the meta block is two columns, title then author·domain — no separate URL column", async ({
    page,
  }) => {
    // Transposed from bar.ts's own two registers (a title block, then one
    // combined "author · domain" line): the vertical meta used to be three
    // columns (title, author, a domain+path URL) at one `META_COL_STEP` (24
    // CSS px) apart. Two columns' ink spans at most ~1.5 steps once the
    // widest glyph in each is counted; three spans past 2 full steps. This
    // fails on the pre-transposition layout and passes on the two-column one.
    const card = await produceVerticalCard(page, "/vertical/", 1);
    const meta = card.metaInk!;
    expect(meta).not.toBeNull();
    const scale = 2;
    expect(meta.right - meta.left).toBeLessThan(24 * 1.5 * scale + 40);
  });

  test("a captured context sentence around the selection fades instead of drawing at full strength", async ({
    page,
  }) => {
    // Paragraph 1 has a short paragraph on each side (0 and 2), so selecting
    // it whole makes `captureBlocks` (selection-actions.ts) pull both in as
    // grey context — this is the same fixture and selection the geometry
    // tests above use, not a new one. A real rotate+alpha canvas draw is the
    // point: the mocked jsdom unit test (`buildVerticalCardCanvas — blocks
    // carry the quote's context fade`, share-card.test.ts) only checks the
    // `ctx.globalAlpha` value a fake `fillText` recorded, which can't see
    // whether `drawSidewaysRun`'s save/rotate/fillText/restore sequence
    // actually carries that alpha through onto a real canvas's rotated ink —
    // exactly the risk a per-character alpha threaded through a
    // coordinate-rotating draw call introduces.
    const card = await produceVerticalCard(page, "/vertical/", 1);
    expect(card.selectionInkAvg).not.toBeNull();
    expect(card.contextInkAvg).not.toBeNull();
    // Full-strength ink is near-black; paper is ~715 (0xf5+0xf0+0xe6). Measured
    // on this fixture: selection ~605-609, context ~691-692 — the context
    // column must read as visibly faded relative to the selection, not the
    // same ink counted twice, while still short of paper.
    expect(card.contextInkAvg!).toBeGreaterThan(card.selectionInkAvg! + 60);
    expect(card.contextInkAvg!).toBeLessThan(705);
  });

  test("the QR seal is a scannable corner code, not a decoration", async ({ page }) => {
    const card = await produceVerticalCard(page, "/vertical/", 1);
    // A v2-v4 code at 72pt content-box fills thousands of device pixels;
    // this floor is the same order of magnitude as the horizontal card's.
    expect(card.qrInk).toBeGreaterThan(500);
    // The frame's padding is a real quiet zone, not more code: uniformly
    // paper-coloured, never a stray dark module.
    expectColour(card.qrQuietZone, PAPER);
  });
});
