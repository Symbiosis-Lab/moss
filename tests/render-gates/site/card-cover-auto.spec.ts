/**
 * A card cover's box takes `--moss-card-cover-ratio`. With a ratio, every
 * cover is that box and its image is cropped to fill it, captioned listing
 * included (`article figure img` must not shorten the image). With `auto`,
 * the box has no ratio of its own and each cover is its image's shape: a 3:2
 * print and a 2:3 print side by side in a grid listing, at a desktop and a
 * phone width, and in `:::grid {rows=equal}` cards, whose titles still line
 * up. A summary list's thumbnail is 120px wide: with a ratio it is that tall
 * at least, taller when its row's words are; with `auto` it is its picture's
 * shape. A cover always spans its card, on a phone too. Each card carries
 * its picture's shape as `--item-cover-ratio`, and the guide's flex row laid
 * out by it stands every cover at one height with no script.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → CARD_COVER_AUTO_GATE.
 */
import { test, expect, type Page, type Locator } from "@playwright/test";

const DESKTOP = { width: 1280, height: 900 };
const PHONE = { width: 390, height: 844 };

async function open(page: Page, viewport: { width: number; height: number }, ratio?: string) {
  await page.setViewportSize(viewport);
  await page.goto("/");
  if (ratio) await page.addStyleTag({ content: `:root { --moss-card-cover-ratio: ${ratio}; }` });
}

async function shape(cover: Locator) {
  const box = (await cover.boundingBox())!;
  const img = (await cover.locator("img").first().boundingBox())!;
  const card = (await cover.locator("xpath=ancestor::*[contains(concat(' ', @class, ' '), ' moss-card ')][1]").boundingBox())!;
  return { ratio: box.width / box.height, box, img, card };
}

function cover(page: Page, section: string, title: string) {
  return page
    .locator(`h2:has-text("${section}") + * .moss-card, h2:has-text("${section}") + * + * .moss-card`)
    .filter({ hasText: title })
    .locator(".moss-card-cover")
    .first();
}

for (const [label, viewport] of [["desktop", DESKTOP], ["phone", PHONE]] as const) {
  test(`auto keeps each grid-listing cover's own shape at ${label} width`, async ({ page }) => {
    await open(page, viewport, "auto");
    const wide = await shape(cover(page, "Grid", "Wide print"));
    const tall = await shape(cover(page, "Grid", "Tall print"));
    expect(wide.ratio).toBeCloseTo(3 / 2, 1);
    expect(tall.ratio).toBeCloseTo(2 / 3, 1);
    expect(wide.img.height, "the image fills its box").toBeCloseTo(wide.box.height, 0);
    expect(tall.img.height, "the image fills its box").toBeCloseTo(tall.box.height, 0);
    expect(wide.box.width, "the cover spans its card").toBeCloseTo(wide.card.width, 0);
    expect(tall.box.width, "the cover spans its card").toBeCloseTo(tall.card.width, 0);
  });
}

for (const [label, viewport] of [["desktop", DESKTOP], ["phone", PHONE]] as const) {
  test(`a fixed ratio crops both covers to one box at ${label} width, in a captioned listing too`, async ({ page }) => {
    await open(page, viewport, "1 / 1");
    for (const section of ["Grid", "Captioned"]) {
      for (const title of ["Wide print", "Tall print"]) {
        const s = await shape(cover(page, section, title));
        expect(s.ratio, `${section} ${title}`).toBeCloseTo(1, 1);
        expect(s.img.height, `${section} ${title}: the image fills the box`).toBeCloseTo(s.box.height, 0);
        expect(s.box.width, `${section} ${title}: the cover spans its card`).toBeCloseTo(s.card.width, 0);
      }
    }
  });
}

test("a flex row sized by --item-cover-ratio stands every cover at one height", async ({ page }) => {
  await open(page, DESKTOP, "auto");
  await page.addStyleTag({
    content: `.moss-cards { display: flex; }
.moss-card { display: block; flex: calc(var(--item-cover-ratio, 1)) 1 0; min-inline-size: 0; }`,
  });
  const wide = await shape(cover(page, "Grid", "Wide print"));
  const tall = await shape(cover(page, "Grid", "Tall print"));
  expect(wide.ratio).toBeCloseTo(3 / 2, 1);
  expect(tall.ratio).toBeCloseTo(2 / 3, 1);
  expect(Math.abs(wide.box.height - tall.box.height), "one height").toBeLessThan(2);
  expect(wide.box.width / tall.box.width, "widths in the pictures' proportion").toBeCloseTo(9 / 4, 1);
});

test("auto keeps each shape in equal-row cards and their titles still line up", async ({ page }) => {
  await open(page, DESKTOP, "auto");
  const wide = await shape(cover(page, "Shelf", "Wide print"));
  const tall = await shape(cover(page, "Shelf", "Tall print"));
  expect(wide.ratio).toBeCloseTo(3 / 2, 1);
  expect(tall.ratio).toBeCloseTo(2 / 3, 1);
  const titles = page.locator('h2:has-text("Shelf") + * .moss-card-title');
  const [a, b] = [(await titles.nth(0).boundingBox())!, (await titles.nth(1).boundingBox())!];
  expect(a.y).toBeCloseTo(b.y, 0);
});

test("a summary list's thumbnail is the ratio at least, and its picture's shape with auto", async ({ page }) => {
  await open(page, DESKTOP);
  for (const title of ["Wide print", "Tall print"]) {
    const s = await shape(cover(page, "Summary", title));
    expect(s.box.width).toBeCloseTo(120, 0);
    expect(s.box.height, `${title}: at least 4:3`).toBeGreaterThanOrEqual(89.5);
    expect(s.img.height, `${title}: the image fills the thumbnail`).toBeCloseTo(s.box.height, 0);
  }
  await open(page, DESKTOP, "auto");
  const tall = await shape(cover(page, "Summary", "Tall print"));
  expect(tall.box.width).toBeCloseTo(120, 0);
  expect(tall.ratio, "a 2:3 picture keeps its shape").toBeCloseTo(2 / 3, 1);
  expect(tall.img.height).toBeCloseTo(tall.box.height, 0);
});
