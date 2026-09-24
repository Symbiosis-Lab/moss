import { test, expect, type Page } from "@playwright/test";

// Verifies the fix for: clicking a footnote backlink scrolled the marker
// under the floating nav island with no visual indication of which note
// was landed on.
//
// The fixture puts its footnotes on /notes/, one level under a
// `breadcrumb: true` home — `generate_nav_island` (island.rs) emits nothing
// for a page with no breadcrumb trail, and a homepage never gets one, so
// /notes/ is the shallowest page that actually carries the island markup.

/**
 * Scroll to `y` and wait two animation frames before returning — the island
 * reads scroll direction from a running total, so a step it never saw reads
 * as the opposite direction on the next one. Mirrors nav-island.spec.ts's
 * own `scrollAndSettle`.
 */
async function scrollAndSettle(page: Page, y: number): Promise<void> {
  await page.evaluate(
    (target) =>
      new Promise<void>((resolve) => {
        window.scrollTo(0, target);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
    y,
  );
}

test.describe("footnote :target landing", () => {
  test("a backref from the expanded sheet clears the island on the upward jump home", async ({
    page,
  }) => {
    // The original reported bug, carried into the sheet era: land at the
    // top, read DOWN the article (nav-island.ts reads that as downward and
    // HIDES the island), then travel back UP to a marker near the top. The
    // upward travel reveals the island again AFTER the jump has landed, so
    // a clearance rule conditioned on the island's click-time state arrives
    // one step too late. The route to the backref changed — the endnote
    // list no longer sits at the page's foot; it opens as the expanded
    // sheet — but the upward jump it launches is the same jump.
    //
    // Below the 76rem threshold, the only width where the sheet exists.
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto("/notes/");

    await scrollAndSettle(page, 300);
    await scrollAndSettle(page, 900);
    await scrollAndSettle(
      page,
      await page.evaluate(() => document.body.scrollHeight),
    );

    // Precondition: the island must actually be hidden here, or the jump
    // below would prove nothing about the reported bug.
    await expect(
      page.locator('.moss-nav-island[data-shown="true"]'),
      "setup failed to hide the island via downward scroll",
    ).toHaveCount(0);

    // Reader's route: tap the last marker (the fixture cites it deep in the
    // document, so the tap happens at real scroll depth), take the grabber
    // to the full glossary, then follow that note's backref home — an
    // upward travel long enough to reveal the island, landing on a marker
    // deep enough that the masthead is behind us. (The three markers near
    // the head of the document land ABOVE the masthead, where the island is
    // rightly absent — they cannot reproduce the overlap.)
    // Tapped in the page, not through the harness: Playwright scrolls a
    // target into view before clicking, and that scroll would carry the
    // reader up to the marker — destroying the very depth this test is
    // about. The reader's own thumb reaches a marker that is off screen
    // only by scrolling; here the scroll already happened.
    await page.evaluate(() => {
      const refs = document.querySelectorAll<HTMLElement>(
        ".moss-footnote-ref a",
      );
      refs[refs.length - 1].click();
    });
    await page.locator(".moss-footnote-grab").click();
    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(1);
    await page.locator("#fn-4 .moss-footnote-backref").click();

    // The departure releases the sheet; the (smooth) travel carries the
    // reader.
    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(0);
    const marker = page.locator("#fnref-4");
    await expect(marker).toBeInViewport();
    // The arrival cue waits for the scroll to come to rest, then washes the
    // marker the backref returned to.
    await expect(marker).toHaveClass(/moss-footnote-landed/);

    // Let the jump's own scroll event reach onScroll and the island settle.
    await page.waitForTimeout(50);
    await scrollAndSettle(page, await page.evaluate(() => window.scrollY));

    // Asserted, not guarded: the whole point is that the backward hop DOES
    // reveal the island. An `if (count === 0) return` here would turn the one
    // test that reproduces the bug into a no-op the moment it regressed.
    const island = page.locator('.moss-nav-island[data-shown="true"]');
    await expect(
      island,
      "the backward jump did not reveal the island — this test no longer reproduces the reported sequence",
    ).toHaveCount(1);

    // Both boxes read AFTER that retrying assertion settles. The island
    // transitions in from translateY(-12px) over 0.16s, so a box captured
    // before the reveal completes sits 12px high — which can only ever make
    // this assertion pass when it should fail.
    await page.waitForFunction(() => {
      const el = document.querySelector('.moss-nav-island[data-shown="true"]');
      return !!el && getComputedStyle(el).transform === "none";
    });
    const markerBox = await marker.boundingBox();
    expect(markerBox).not.toBeNull();
    const islandBox = await island.boundingBox();
    expect(islandBox).not.toBeNull();

    expect(markerBox!.y).toBeGreaterThan(islandBox!.y + islandBox!.height);
  });

  test("marker gets a visible background wash on backward :target navigation", async ({
    page,
  }) => {
    await page.goto("/notes/#fnref-1");
    const marker = page.locator("#fnref-1");
    // The wash is a 2.4s animation with the default fill-mode (`none`), so
    // its computed background color reverts to transparent once the
    // animation completes — asserting on that color here would go red on
    // any run slower than 2.4s (a cold webkit start under xvfb, a traced CI
    // run, a contended box) with nothing wrong in the implementation. The
    // animation NAME is the deterministic, time-invariant fact: it proves
    // the wash fired at all, which is what this test exists to check.
    const animName = await marker.evaluate(
      (el) => getComputedStyle(el).animationName,
    );
    expect(animName).toBe("moss-footnote-target-wash");
  });

  test("reduced motion holds a static wash instead of animating", async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/notes/#fnref-1");
    const marker = page.locator("#fnref-1");
    const animName = await marker.evaluate(
      (el) => getComputedStyle(el).animationName,
    );
    expect(animName).toBe("none");
    // Reduced motion holds a static (non-animating) wash, so unlike the
    // animated case above, the computed color here is time-invariant —
    // asserting on it is safe. Assert a minimum alpha rather than merely
    // "not fully transparent": `not.toBe("rgba(0, 0, 0, 0)")` would still
    // pass at alpha 0.001, which is indistinguishable from no wash at all.
    const bg = await marker.evaluate(
      (el) => getComputedStyle(el).backgroundColor,
    );
    const alphaMatch = bg.match(
      /^rgba?\(\s*[\d.]+\s*,\s*[\d.]+\s*,\s*[\d.]+\s*(?:,\s*([\d.]+)\s*)?\)$/,
    );
    const alpha = alphaMatch?.[1] === undefined ? 1 : Number(alphaMatch[1]);
    expect(alpha).toBeGreaterThan(0.05);
  });
});

// Margin sidenotes. Every claim below is a live-layout question: whether a
// note sits beside the column rather than over it, whether two notes cited a
// few words apart overlap, and whether the aside is rendered at all at a
// given width. jsdom can answer none of them — it has no layout, so
// `getBoundingClientRect` there is whatever the test mocked.
//
// Both engines matter here specifically. The placement is `float: right` +
// `clear: right` rather than measured arithmetic, so what is under test IS
// the engine's float model, and chromium and webkit are separate
// implementations of it.
test.describe("margin sidenotes", () => {
  /** Every `.moss-sidenote` box, in document order. */
  async function noteBoxes(page: Page) {
    return page.evaluate(() =>
      [...document.querySelectorAll(".moss-sidenote")].map((el) => {
        const r = el.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
      }),
    );
  }

  test("notes sit in the margin, beside the column and never over it", async ({
    page,
  }) => {
    await page.goto("/notes/");
    const boxes = await noteBoxes(page);
    expect(
      boxes.length,
      "fixture should carry four footnotes, so four sidenotes",
    ).toBe(4);

    // The content column's right edge. A note whose left edge is inside it is
    // sitting ON the prose, which is the failure this whole approach exists
    // to avoid.
    const columnRight = await page.evaluate(() => {
      const p = document.querySelector("main article p");
      if (!p) throw new Error("fixture has no article paragraph");
      return p.getBoundingClientRect().right;
    });
    for (const box of boxes) {
      expect(box.left).toBeGreaterThanOrEqual(columnRight);
    }
    // Containment inside the window is asserted by "the column centres on its
    // own, and the notes still fit beside it" below, at three widths and
    // against clientWidth — which is the right instrument, because the note is
    // clipped by the scrollbar that `viewportSize().width` counts and
    // `clientWidth` does not.
  });

  test("notes cited a few words apart stack instead of overlapping", async ({
    page,
  }) => {
    await page.goto("/notes/");
    const boxes = await noteBoxes(page);
    // The fixture's third and fourth markers are one line apart — as plain
    // floats they would land on the same line and overlap. Asserted over
    // every pair rather than just that one so a future note added to the
    // fixture is covered by construction.
    for (let i = 1; i < boxes.length; i++) {
      expect(
        boxes[i].top,
        `sidenote ${i + 1} overlaps sidenote ${i}`,
      ).toBeGreaterThanOrEqual(boxes[i - 1].bottom);
    }
  });

  test("each note starts at or below the marker that cites it", async ({
    page,
  }) => {
    await page.goto("/notes/");
    const rows = await page.evaluate(() =>
      [...document.querySelectorAll(".moss-footnote-ref")].map((ref) => {
        const note = ref.nextElementSibling;
        return {
          markerTop: ref.getBoundingClientRect().top,
          noteTop: note?.classList.contains("moss-sidenote")
            ? note.getBoundingClientRect().top
            : null,
          // The float is placed against the LINE BOX the marker sits in, not
          // against the marker's own box — and a superscript's box is raised
          // within that line, so the note legitimately starts a fraction
          // above it (measured: 1.25px, in both engines). One line-height of
          // slack absorbs that without weakening the claim, which is that a
          // note never floats up past the line that cites it.
          lineHeight: parseFloat(
            getComputedStyle(ref.closest("p") ?? ref).lineHeight,
          ),
        };
      }),
    );
    expect(rows.length).toBe(4);
    for (const row of rows) {
      expect(row.noteTop, "marker has no sidenote after it").not.toBeNull();
      expect(row.lineHeight).toBeGreaterThan(0);
      expect(row.noteTop!).toBeGreaterThanOrEqual(
        row.markerTop - row.lineHeight,
      );
    }
  });

  test("below the gutter threshold no sidenote is rendered", async ({
    page,
  }) => {
    // 1100px is under the 76rem (1216px) threshold in site.css. The asides
    // are still in the DOM — the script builds them unconditionally, since
    // deciding visibility in JS would mean re-deciding it on every resize —
    // so the assertion is on `display`, not on presence.
    await page.setViewportSize({ width: 1100, height: 900 });
    await page.goto("/notes/");
    const displays = await page.evaluate(() =>
      [...document.querySelectorAll(".moss-sidenote")].map(
        (el) => getComputedStyle(el).display,
      ),
    );
    expect(displays.length).toBe(4);
    expect(displays.every((d) => d === "none")).toBe(true);

    // The endnote section is the interface here: the bottom-of-page list a
    // marker tap lifts as the sheet. In flow, visible, ids intact.
    await expect(page.locator("#fn-1")).toHaveCount(1);
    const sectionStyle = await page
      .locator(".moss-footnotes")
      .evaluate((el) => {
        const cs = getComputedStyle(el);
        return { display: cs.display, position: cs.position };
      });
    expect(sectionStyle.display).not.toBe("none");
    expect(sectionStyle.position).toBe("static");
  });

  test("a marker tap lifts the sheet, peeking exactly the note, no wash", async ({
    page,
  }) => {
    // The sheet replaces the old jump-to-endnote: the reader must NOT be
    // scrolled anywhere, and the peek clips to exactly the tapped note —
    // the clip alone names it, so no arrival wash fires here.
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto("/notes/");
    // Park the first marker comfortably inside the viewport (it sits ~300px
    // down the document) so the click itself needs no auto-scroll — the
    // baseline below must measure the sheet, not Playwright's positioning.
    await scrollAndSettle(page, 100);
    const before = await page.evaluate(() => window.scrollY);

    await page.locator(".moss-footnote-ref a").first().click();
    const sheet = page.locator(".moss-footnotes-lifted");
    await expect(sheet).toHaveCount(1);
    expect(await page.evaluate(() => window.scrollY)).toBe(before);

    // Pinned to the bottom edge, and — once the settle completes — clipped
    // to exactly the asked-for note: fn-1 fully inside, its neighbour's
    // text starting at the sheet's bottom edge.
    const viewport = page.viewportSize()!;
    await expect
      .poll(async () => {
        const g = await page.evaluate(() => {
          const s = document
            .querySelector(".moss-footnotes-lifted")!
            .getBoundingClientRect();
          const n1 = document.getElementById("fn-1")!.getBoundingClientRect();
          const n2 = document.getElementById("fn-2")!.getBoundingClientRect();
          return { s, n1, n2 };
        });
        return (
          g.s.bottom > viewport.height - 2 &&
          g.n1.top >= g.s.top - 1 &&
          g.n1.bottom <= g.s.bottom + 1 &&
          g.n2.top + 8 >= g.s.bottom - 1
        );
      })
      .toBe(true);
    await expect(page.locator(".moss-footnotes .moss-footnote-landed")).toHaveCount(0);

    // The grabber is a button; tapping it names the drag's destination —
    // the glossary at the expanded detent. Still no wash: the open sheet
    // shows the note, and a cue for something already visible is one
    // animation too many.
    await page.locator(".moss-footnote-grab").click();
    // Expanded stops at 62% of the viewport: a readable band of article
    // stays visible above the sheet.
    await expect
      .poll(async () => (await sheet.boundingBox())!.height)
      .toBeGreaterThan(0.6 * viewport.height);
    expect((await sheet.boundingBox())!.height).toBeLessThan(
      0.66 * viewport.height,
    );

    // Esc sinks the sheet back into flow and hands focus home.
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.id))
      .toBe("fnref-1");
  });

  test("lifting the sheet does not change the page's length", async ({
    page,
  }) => {
    // A sticky element keeps its flow box, so the height clip that opens
    // the sheet is also the DOCUMENT's height: unheld, the page shortens
    // by the whole endnote list the instant a marker is tapped and grows
    // back over the settle — a scrollbar resizing every frame, and a
    // clamped scroll for anyone reading near the foot. A spacer holds the
    // difference. Sampled across the settle, not just at its ends.
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto("/notes/");
    await scrollAndSettle(page, 100);
    const before = await page.evaluate(
      () => document.documentElement.scrollHeight,
    );

    const samples: number[] = await page.evaluate(() => {
      const out: number[] = [];
      const t0 = performance.now();
      const tick = () => {
        out.push(document.documentElement.scrollHeight);
        if (performance.now() - t0 < 900) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
      (
        document.querySelector(".moss-footnote-ref a") as HTMLElement
      ).click();
      return new Promise((r) => setTimeout(() => r(out), 1000));
    });

    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(1);
    // Sub-pixel rounding of the clip is the only allowance.
    for (const h of samples) expect(Math.abs(h - before)).toBeLessThanOrEqual(2);
  });

  test("dragging the sheet up expands it; down dismisses; a nudge does neither", async ({
    page,
  }) => {
    // The design's primary entry to the expanded detent: the drag follows
    // the finger the whole way — one surface, its clip height riding the
    // pointer. Mouse events drive the same pointer pipeline the touch
    // gesture uses (slop → window capture → continuous follow → detent).
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto("/notes/");

    const sheet = page.locator(".moss-footnotes-lifted");
    const openPeek = async () => {
      await page.locator(".moss-footnote-ref a").first().click();
      await expect(sheet).toHaveCount(1);
      // Let the lift settle before measuring the drag origin.
      await expect
        .poll(async () => (await sheet.boundingBox())!.height)
        .toBeGreaterThan(40);
      const box = await sheet.boundingBox();
      return { x: box!.x + box!.width / 2, y: box!.y + 10 };
    };
    const drag = async (from: { x: number; y: number }, dy: number) => {
      await page.mouse.move(from.x, from.y);
      await page.mouse.down();
      await page.mouse.move(from.x, from.y + dy, { steps: 4 });
      await page.mouse.up();
    };

    // A nudge inside the slop band leaves the peek alone.
    let mid = await openPeek();
    const peekH = (await sheet.boundingBox())!.height;
    await drag(mid, 6);
    await expect(sheet).toHaveCount(1);

    // Upward the sheet follows the finger: mid-gesture its top edge rides
    // the pointer — the continuous morph, not a commit-on-release.
    await page.mouse.move(mid.x, mid.y);
    await page.mouse.down();
    await page.mouse.move(mid.x, mid.y - 60, { steps: 4 });
    const partway = (await sheet.boundingBox())!;
    expect(partway.height).toBeGreaterThan(peekH + 30);
    await page.mouse.move(mid.x, mid.y - 250, { steps: 4 });
    const higher = (await sheet.boundingBox())!;
    expect(higher.y).toBeLessThan(partway.y);
    await page.mouse.up();
    // Released past the midpoint: the expanded detent, and no wash there
    // either — the sheet reaching 62% is the whole of the feedback.
    await expect
      .poll(async () => (await sheet.boundingBox())!.height)
      .toBeGreaterThan(0.6 * page.viewportSize()!.height);
    await expect(
      page.locator(".moss-footnotes .moss-footnote-landed"),
    ).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);

    // Down past the threshold: swipe-down dismiss.
    mid = await openPeek();
    await drag(mid, 120);
    await expect(sheet).toHaveCount(0);
  });

  test("scrolling the page to the end docks the sheet back into the list, chrome fading on approach", async ({
    page,
  }) => {
    // The sheet IS the bottom list dragged up, so reaching the list's true
    // place makes them one again: position: sticky is the mechanism, the
    // release is a layout identity, and the chrome (grabber opacity via
    // --moss-sheet-away) fades continuously over the approach.
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto("/notes/");
    await scrollAndSettle(page, 100);
    await page.locator(".moss-footnote-ref a").first().click();
    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(1);

    // Mid-page, the sheet stays stuck while the page scrolls freely behind
    // it — non-modal by design.
    await scrollAndSettle(page, 600);
    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(1);

    // At the end, the sheet docks: the lifted class is gone and the section
    // is ordinary flow content again, ids intact. The travel must be a REAL
    // gesture (keyboard here) — docking listens only to the reader's own
    // scrolling, never to programmatic or browser-initiated scrolls.
    await page.keyboard.press("End");
    await expect
      .poll(async () => {
        await page.keyboard.press("End");
        return page.evaluate(
          () =>
            window.scrollY + window.innerHeight >=
            document.body.scrollHeight - 1,
        );
      })
      .toBe(true);
    await expect(page.locator(".moss-footnotes-lifted")).toHaveCount(0);
    const docked = await page.locator(".moss-footnotes").evaluate((el) => {
      const cs = getComputedStyle(el);
      return { position: cs.position, display: cs.display };
    });
    expect(docked.position).toBe("static");
    expect(docked.display).not.toBe("none");
    await expect(page.locator("#fn-1")).toBeInViewport();
  });

  test("the note sets as a hanging indent, its figure in the gutter", async ({
    page,
  }) => {
    // 悬挂缩进: the figure sits left of the note and every wrapped line
    // returns to where the first line's text began. Only an engine can
    // answer this — it is a question about line boxes, and the figure is an
    // absolutely positioned ::before that no markup assertion can see.
    await page.setViewportSize({ width: 414, height: 780 });
    await page.goto("/notes/");
    const m = await page.evaluate(() => {
      const p = document.querySelector("#fn-1 p") as HTMLElement;
      const range = document.createRange();
      range.selectNodeContents(p);
      const lines = [...range.getClientRects()]
        .filter((r) => r.width > 1)
        .map((r) => Math.round(r.left));
      const li = document.querySelector("#fn-1") as HTMLElement;
      return { lines, liLeft: Math.round(li.getBoundingClientRect().left) };
    });
    expect(m.lines.length).toBeGreaterThan(1);
    expect(m.lines[1]).toBe(m.lines[0]);
    // The gutter is real: the note's box starts well left of its text.
    expect(m.lines[0]).toBeGreaterThan(m.liLeft + 8);
  });

  test("the endnote list keeps the ids and back-links; the margin copy has neither", async ({
    page,
  }) => {
    await page.goto("/notes/");
    // Duplicate ids would break every `#fn-N` jump on the page — the exact
    // navigation the rest of this file proves works.
    const duplicateIds = await page.evaluate(() => {
      const seen = new Set<string>();
      const dupes: string[] = [];
      for (const el of document.querySelectorAll("[id]")) {
        if (seen.has(el.id)) dupes.push(el.id);
        seen.add(el.id);
      }
      return dupes;
    });
    expect(duplicateIds).toEqual([]);

    await expect(
      page.locator(".moss-sidenote .moss-footnote-backref"),
      "the return arrow belongs to the endnote list, not the margin",
    ).toHaveCount(0);
    await expect(
      page.locator(".moss-footnotes .moss-footnote-backref"),
      "the endnote list keeps its return arrows",
    ).toHaveCount(4);
  });

  test("a marker click at wide width lands on the margin note, and the list stays retired", async ({
    page,
  }) => {
    await page.goto("/notes/");
    // The retirement is class-shaped and atomic: `moss-footnotes-cloned`
    // lands only after every note was cloned, and under the margin's own
    // media scope it removes the section from flow, tab order and the
    // accessibility tree.
    await expect(page.locator(".moss-footnotes")).toHaveClass(
      /moss-footnotes-cloned/,
    );
    expect(
      await page
        .locator(".moss-footnotes")
        .evaluate((el) => getComputedStyle(el).display),
    ).toBe("none");

    // The first note is short by fixture design: its margin copy sits
    // beside the marker and fits on screen, so "no travel" is the honest
    // claim. (A note taller than the viewport is scrolled to centre by
    // land() — that is the deep-link test's territory, not this one's.)
    const before = await page.evaluate(() => window.scrollY);
    await page.locator(".moss-footnote-ref a").first().click();

    // No travel: the note is already beside the line. The wash names it.
    expect(await page.evaluate(() => window.scrollY)).toBe(before);
    await expect(
      page.locator(".moss-footnote-ref").first().locator("+ .moss-sidenote"),
    ).toHaveClass(/moss-footnote-landed/);
    expect(
      await page
        .locator(".moss-footnotes")
        .evaluate((el) => getComputedStyle(el).display),
    ).toBe("none");
  });

  test("a shared #fn-N deep link lands on the margin note, in view and washed", async ({
    page,
  }) => {
    // The engine cannot land this one itself — the `#fn-2` target sits
    // inside the retired (display:none) endnote list — so sidenotes.ts
    // carries the arrival to the margin copy: washed, and scrolled into
    // view when off-screen.
    await page.goto("/notes/#fn-2");
    const aside = page.locator(".moss-footnote-landed");
    await expect(aside).toHaveCount(1);
    await expect(aside).toBeInViewport();
    expect(
      await page
        .locator(".moss-footnotes")
        .evaluate((el) => getComputedStyle(el).display),
    ).toBe("none");
  });

  // Every width below is 1358 or 1440, never 1216. The gutter's gate is
  // `min-width: 76rem` = 1216px, and webkit reckons that query against a
  // client width that excludes the scrollbar (measured: 1210 for a 1216px
  // window) while chromium does not — so at exactly the threshold the two
  // engines honestly disagree about whether a reserve exists at all. 1358 and
  // 1440 are above it in both; the sub-threshold assertions use 1100, which is
  // below it in both.
  const WIDE = [1358, 1440];

  test("a hero on a footnoted site reaches the viewport's right edge", async ({
    page,
  }) => {
    // /banner/, NOT the home page. The gutter is a SITE-wide stylesheet gate,
    // so this page carries a full reserve without carrying a single note —
    // and its banner is a body-level flex item outside <main>, whose
    // containing block body's padding-right has narrowed by exactly that
    // reserve. The home page would prove nothing here: it is scoped out of the
    // reserve entirely, so its banner reaches the edge whether the escape
    // works or not.
    //
    // This failed on develop by exactly the reserve — 1078 against a 1358
    // viewport, 280px short, in both engines — because the escape was written
    // as a negative margin, which widens a flex item only while its cross size
    // comes from `align-items: stretch`, and the `width: 100%` fifteen lines
    // above turned stretch off. A margin cannot be asserted on directly here:
    // the failure was that the margin did nothing, so only the painted box
    // says whether the banner escaped.
    // 1280 leads the list because it is the only width here where the page
    // actually moves. The shift is demand-driven since 2026-09-01 and is 0px
    // at 1358 and 1440, so a precondition written as "body has a non-zero
    // padding-right" — which is what this test guarded itself with before —
    // would now be false at exactly the widths it runs. The precondition that
    // still means something is that a gutter is reserved on this page at all.
    for (const width of [1280, ...WIDE]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/banner/");
      // Precondition: without a reserve on this page the assertion below is
      // vacuous, which is the trap the home page fell into.
      expect(
        await page.evaluate(() =>
          getComputedStyle(document.body)
            .getPropertyValue("--moss-sidenote-reserve")
            .trim(),
        ),
        `/banner/ must carry a reserve at ${width}px or this test proves nothing`,
      ).not.toBe("");
      const geom = await page.evaluate(() => {
        const hero = document.querySelector(".moss-hero");
        if (!hero) throw new Error("fixture /banner/ page has no hero");
        return {
          right: hero.getBoundingClientRect().right,
          left: hero.getBoundingClientRect().left,
          clientWidth: document.documentElement.clientWidth,
        };
      });
      // Both edges, not just the right one: a hero that overshot on the left
      // would also reach the right edge, and that is a different bug.
      expect(
        geom.left,
        `hero left edge at ${width}px`,
      ).toBeLessThanOrEqual(1);
      // Two-sided. A hero that adds back the RESERVE instead of the inset —
      // the mistake .moss-hero's comment warns about — overshoots by up to
      // 280px, and `left <= 1` plus a one-sided `right >= clientWidth - 1`
      // would call that a pass while `overflow-x: clip` hid the damage and
      // `object-fit: cover` cropped against a box wider than the screen.
      expect(
        Math.abs(geom.right - geom.clientWidth),
        `hero right edge at ${width}px viewport (client ${geom.clientWidth})`,
      ).toBeLessThanOrEqual(1);
    }
  });

  test("the front page keeps its column centred — no gutter is reserved there", async ({
    page,
  }) => {
    // A front page is a composition (hero, card grids), not a reading page:
    // there is no column for the gutter to hold notes beside, so it pays the
    // 140px shift for nothing. On develop this page's column sat 140px left of
    // the viewport centre with a 280px empty band on the right, at every wide
    // width, in both engines.
    //
    // Asserted as "the reserve is not declared" AND as geometry, because
    // either alone can pass for the wrong reason: a page could be centred with
    // a reserve declared if something else compensated, and a property can be
    // absent while some other rule still shifts the column.
    for (const width of WIDE) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      const geom = await page.evaluate(() => {
        const cs = getComputedStyle(document.body);
        const col = document.querySelector("main > article.container")!;
        const nav = document.querySelector(".main-nav")!;
        const c = col.getBoundingClientRect();
        const n = nav.getBoundingClientRect();
        return {
          reserve: cs.getPropertyValue("--moss-sidenote-reserve").trim(),
          columnMid: (c.left + c.right) / 2,
          navMid: (n.left + n.right) / 2,
          half: document.documentElement.clientWidth / 2,
        };
      });
      // The reserve, not the padding. Since the inset is demand-driven the
      // padding is 0px at these widths on EVERY page, scoped out or not, so
      // asserting it would leave this test green if the scope-out were deleted
      // — measuring nothing while claiming to guard the front page.
      expect(
        geom.reserve,
        `no gutter may be reserved on the front page at ${width}px`,
      ).toBe("");
      expect(
        Math.abs(geom.columnMid - geom.half),
        `front-page column centre offset at ${width}px`,
      ).toBeLessThan(1);
      expect(
        Math.abs(geom.navMid - geom.half),
        `front-page nav centre offset at ${width}px`,
      ).toBeLessThan(1);
    }
  });

  test("the column centres on its own, and the notes still fit beside it", async ({
    page,
  }) => {
    // What replaced layout (C)'s "column + notes centre as one assembly". The
    // assembly no longer centres — deliberately: centring it costs a 140px
    // shift at every width, which most pages of a real footnoted site paid
    // for a gutter they had nothing to put in. The shift is now only what
    // the window is short of fitting the notes, so at these widths it is 0px
    // and the column sits where it would on any other page.
    //
    // 1280 is in the list because it is the one width here where the shift is
    // non-zero (36px): the fit assertion is vacuous at widths with room to
    // spare, and this is where a wrong formula would clip a note.
    for (const width of [1280, ...WIDE]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/notes/");
      const geom = await page.evaluate(() => {
        const article = document.querySelector("main > article.container")!;
        const nav = document.querySelector(".main-nav")!;
        const asides = [...document.querySelectorAll(".moss-sidenote")];
        const a = article.getBoundingClientRect();
        return {
          articleLeft: a.left,
          articleMid: (a.left + a.right) / 2,
          navLeft: nav.getBoundingClientRect().left,
          noteRight: Math.max(
            ...asides.map((el) => el.getBoundingClientRect().right),
          ),
          inset: getComputedStyle(document.body)
            .getPropertyValue("--moss-sidenote-inset")
            .trim(),
          clientWidth: document.documentElement.clientWidth,
        };
      });
      // The invariant layout (C) was actually chosen for, and the one that
      // survives the change: nav, column and footer share one left edge.
      expect(
        Math.abs(geom.navLeft - geom.articleLeft),
        `nav and column left edges at ${width}px`,
      ).toBeLessThan(1);
      // The notes must clear the window with room to spare. 24px, not 0:
      // 100vw counts a scrollbar that clientWidth does not, so a formula
      // solving for an exact fit passes in chromium and clips by 3px in
      // webkit — silently, because body is `overflow-x: clip`.
      expect(
        geom.noteRight,
        `rightmost note's right edge at ${width}px (client ${geom.clientWidth})`,
      ).toBeLessThanOrEqual(geom.clientWidth - 24);
      // Conditioned on the measured inset, not on a copy of WIDE's contents:
      // the claim is "wherever the page gives up nothing, the column is dead
      // centre", and tying it to a width literal would silently change meaning
      // the day WIDE does.
      if (geom.inset === "0px") {
        expect(
          Math.abs(geom.articleMid - geom.clientWidth / 2),
          `column centre offset at ${width}px, where the page gives up no width at all`,
        ).toBeLessThan(2);
      }
    }
  });

  test("a page with no notes on a footnoted site is not shifted at all", async ({
    page,
  }) => {
    // The reported defect, and the case no gate could see before this fixture
    // page existed. The stylesheet gate is site-wide, so /plain/ carries a
    // reserved gutter because some other page cites a source; on develop it
    // paid a 140px shift and showed a 280px empty band for it, at every wide
    // width, in both engines. Nearly every page of a real footnoted site is
    // this page.
    //
    // Asserted as both the shift and the geometry: the property could be 0px
    // while some other rule still moved the column, and the column could be
    // centred by compensation rather than by not moving.
    for (const width of WIDE) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/plain/");
      const geom = await page.evaluate(() => {
        const cs = getComputedStyle(document.body);
        const col = document.querySelector("main > article.container")!;
        const nav = document.querySelector(".main-nav")!;
        const c = col.getBoundingClientRect();
        const n = nav.getBoundingClientRect();
        return {
          reserve: cs.getPropertyValue("--moss-sidenote-reserve").trim(),
          padRight: cs.paddingRight,
          columnMid: (c.left + c.right) / 2,
          navLeft: n.left,
          columnLeft: c.left,
          half: document.documentElement.clientWidth / 2,
        };
      });
      // Precondition, the same one /banner/ carries: this page must actually
      // be inside the gutter's scope. Scoping no-notes pages out of the reserve
      // is a live option the design doc weighs and declines, and if a later
      // change took it, everything below would pass while measuring a page the
      // stylesheet never touched.
      expect(
        geom.reserve,
        `/plain/ must carry a reserve at ${width}px or this test proves nothing`,
      ).not.toBe("");
      expect(geom.padRight, `body padding-right at ${width}px`).toBe("0px");
      expect(
        Math.abs(geom.columnMid - geom.half),
        `column centre offset on a page with no notes at ${width}px`,
      ).toBeLessThan(2);
      expect(
        Math.abs(geom.navLeft - geom.columnLeft),
        `nav and column left edges at ${width}px`,
      ).toBeLessThan(1);
    }
  });
});



