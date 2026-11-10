import { expect, type Locator, type Page } from "@playwright/test";

/** Pixels on a column through the field's middle: 6px and 3px above its top edge, and the six rows across that edge. */
async function probeTopEdge(page: Page, field: Locator) {
  // A full-page clip is in document coordinates in every engine, whatever the scroll position.
  const box = await field.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height };
  });
  const ox = Math.floor(box.x) - 6;
  const oy = Math.floor(box.y) - 6;
  const clipWidth = Math.ceil(box.width) + 12;
  const shot = (await page.screenshot({ fullPage: true, clip: { x: ox, y: oy, width: clipWidth, height: Math.ceil(box.height) + 12 } })).toString("base64");
  return page.evaluate(async ({ shot, col, row, clipWidth }) => {
    const img = new Image();
    img.src = `data:image/png;base64,${shot}`;
    await img.decode();
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.canvas.width = img.width;
    ctx.canvas.height = img.height;
    ctx.drawImage(img, 0, 0);
    // The screenshot is in device pixels; the probes are in CSS pixels.
    const scale = img.width / clipWidth;
    const at = (dy: number) => Array.from(ctx.getImageData(Math.round((col + 0.5) * scale), Math.round((row + dy + 0.5) * scale), 1, 1).data.slice(0, 3));
    return { far: at(-6), near: at(-3), edge: [-2, -1, 0, 1, 2, 3].map(at) };
  }, { shot, col: Math.round(box.width / 2) + 6, row: Math.round(box.y) - oy, clipWidth });
}

/** A focused field paints nothing just outside its box: no theme ring drawn as a second box. */
export async function expectNothingPaintedOutside(page: Page, field: Locator, what: string) {
  const px = await probeTopEdge(page, field);
  expect(px.near, `${what}: nothing is painted just outside the field`).toEqual(px.far);
}

/** Click into a boxed field and check that focus shows on the field's own edge and paints nothing outside it. */
export async function expectFocusOnOwnEdge(page: Page, field: Locator, what: string) {
  await field.scrollIntoViewIfNeeded();
  await field.click();
  const accent = await page.evaluate(() => {
    const probe = document.createElement("i");
    probe.style.color = "var(--moss-color-ui-accent)";
    document.body.append(probe);
    const colour = getComputedStyle(probe).color;
    probe.remove();
    return colour;
  });
  expect(await field.evaluate((el) => getComputedStyle(el).outlineStyle), `${what}: no outline, whatever the theme says`).toBe("none");
  // The border fades to the accent; wait for it to land.
  await expect.poll(() => field.evaluate((el) => getComputedStyle(el).borderTopColor), `${what}: the border takes the accent`).toBe(accent);
  const px = await probeTopEdge(page, field);
  expect(px.near, `${what}: nothing is painted just outside the border`).toEqual(px.far);
  const [r, g, b] = accent.match(/\d+/g)!.map(Number);
  const away = (p: number[]) => Math.abs(p[0] - r) + Math.abs(p[1] - g) + Math.abs(p[2] - b);
  const off = Math.min(...px.edge.map(away));
  expect(off, `${what}: the accent is painted on the border`).toBeLessThan(60);
}
