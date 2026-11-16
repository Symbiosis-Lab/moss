/**
 * A file the page names that is not in the folder is a box naming it.
 *
 * The box reads `Picture not found: <name>` (`File not found` for other
 * media), a block where the embed stood alone and inline within a sentence.
 * An empty frame of the size asked for, a broken `<img>` or a bare link would
 * not say what is wrong.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → MISSING_FILE_BOX_GATE.
 * `GATE_SHOTS=<dir>` keeps a picture of the page in each colour scheme.
 */
import { test, expect } from "@playwright/test";
import { join } from "node:path";

for (const scheme of ["light", "dark"] as const) {
  test(`a missing file is a box naming it (${scheme})`, async ({ page, browserName }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");

    const boxes = page.locator("article .moss-embed-missing");
    await expect(boxes).toHaveText([
      "Picture not found: slide-06.jpg",
      "Picture not found: charts/gone.png",
      "Picture not found: mark.png",
      "File not found: talk.mp4",
    ]);

    // No image or player to fail, no caption made of the size suffix, no
    // link to nowhere.
    await expect(page.locator("article img, article video, article figure, article figcaption")).toHaveCount(0);
    await expect(page.locator('article a[href*="moss-unresolved"], article a[href*="slide-06"]')).toHaveCount(0);
    await expect(page.locator("article")).not.toContainText("800x450");

    const facts = await boxes.evaluateAll((els) => {
      const article = document.querySelector("article")!.getBoundingClientRect();
      // The colour the box is seen against: the nearest painted background.
      const backdrop = (el: Element): string => {
        for (let e: Element | null = el; e; e = e.parentElement) {
          const bg = getComputedStyle(e).backgroundColor;
          if (bg !== "rgba(0, 0, 0, 0)" && bg !== "transparent") return bg;
        }
        return "rgb(255, 255, 255)";
      };
      const luminance = (rgb: string): number => {
        const [r, g, b] = rgb.match(/[\d.]+/g)!.slice(0, 3).map((v) => {
          const c = Number(v) / 255;
          return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
        });
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      };
      return els.map((el) => {
        const cs = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        const [hi, lo] = [luminance(cs.color), luminance(backdrop(el))].sort((a, b) => b - a);
        return {
          display: cs.display,
          inside: r.left >= article.left - 1 && r.right <= article.right + 1,
          height: r.height,
          top: r.top,
          bottom: r.bottom,
          contrast: (hi + 0.05) / (lo + 0.05),
        };
      });
    });

    expect(facts.map((f) => f.display)).toEqual(["block", "block", "inline", "block"]);
    for (const f of facts) {
      expect(f.inside, `the box leaves the article: ${JSON.stringify(f)}`).toBe(true);
      // One line of small text, not the 450px the picture asked for.
      expect(f.height, JSON.stringify(f)).toBeLessThan(80);
      expect(f.contrast, `the words are too faint to read: ${JSON.stringify(f)}`).toBeGreaterThanOrEqual(4.5);
    }

    // Two boxes in a row, and the text after one, stand apart like paragraphs.
    expect(facts[1].top - facts[0].bottom, "two boxes in a row touch").toBeGreaterThanOrEqual(8);
    const after = await page.locator("article p", { hasText: "After." }).boundingBox();
    expect(after!.y - facts[3].bottom, "the text after a box touches it").toBeGreaterThanOrEqual(8);

    // The inline one stays in its sentence.
    const sentence = page.locator("article p", { hasText: "A sentence with" });
    await expect(sentence).toHaveText("A sentence with Picture not found: mark.png inside it.");

    const dir = process.env.GATE_SHOTS;
    if (dir) await page.screenshot({ path: join(dir, `preview-box-${browserName}-${scheme}.png`) });
  });
}
