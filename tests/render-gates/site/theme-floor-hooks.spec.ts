/**
 * Tokens alone state a theme's header band, its card text and a hero-only home.
 *
 * Painted colours and laid-out boxes: the rules involved tie on specificity or
 * source order against moss's own `header=over` and cover-tint rules, which
 * jsdom does not resolve.
 *
 * See tests/e2e/helpers/gate-sites.ts → THEME_FLOOR_HOOKS_GATE.
 */
import { test, expect, Page } from "@playwright/test";

const css = (page: Page, sel: string, prop: "color" | "backgroundColor" | "borderBlockEndColor") =>
  page.evaluate(
    ([s, p]) => getComputedStyle(document.querySelector(s)!)[p as "color"],
    [sel, prop] as const,
  );

test.describe("theme floor hooks", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
  });

  test("a normal page paints the header band and link colour from the tokens", async ({ page }) => {
    await page.goto("/about/");
    expect(await css(page, "body > header", "backgroundColor")).toBe("rgb(200, 0, 0)");
    expect(await css(page, "body > header .site-name", "color")).toBe("rgb(0, 0, 255)");
    expect(await css(page, ".nav-content", "borderBlockEndColor")).toBe("rgba(0, 0, 0, 0)");
  });

  test("a footer takes its band, ink and rule from the tokens", async ({ page }) => {
    await page.goto("/about/");
    const m = await page.evaluate(() => {
      const style = (s: string, pseudo?: string) => getComputedStyle(document.querySelector(s)!, pseudo);
      const footer = style("footer.container");
      return {
        outset: footer.borderImageOutset,
        text: footer.color,
        link: style("footer.container a").color,
        heading: style("footer.container h2").color,
        rule: style("footer.container hr").borderBlockStartColor,
        ornament: style("footer.container hr", "::before").content,
        divider: style("footer.container", "::before").borderBlockStartColor,
        marks: document.querySelectorAll("footer .moss-heading-anchor").length,
      };
    });
    expect(m.text).toBe("rgb(240, 240, 240)");
    expect(m.link).toBe("rgb(240, 240, 240)");
    expect(m.heading).toBe("rgb(240, 240, 240)");
    expect(m.rule).toBe("rgb(90, 90, 90)");
    expect(m.divider).toBe("rgb(90, 90, 90)");
    expect(m.ornament).toBe("none");
    expect(m.outset).toMatch(/1280px$/);
    expect(m.marks).toBe(0);
  });

  // Whether the band paints is a pixel fact: the same 8x8 clip, taken outside
  // the footer's column, equals a clip of a probe known to be the band colour.
  const paintsBand = async (page: Page) => {
    const y = await page.evaluate(() => {
      const r = document.querySelector("footer.container")!.getBoundingClientRect();
      return Math.round(r.top + window.scrollY + 20);
    });
    const clip = { x: 0, y, width: 8, height: 8 };
    const painted = await page.screenshot({ fullPage: true, clip });
    await page.evaluate((top) => {
      const d = document.createElement("div");
      d.id = "band-probe";
      d.style.cssText = `position:absolute;left:0;top:${top}px;width:8px;height:8px;background:rgb(20, 20, 20);z-index:99999`;
      document.body.appendChild(d);
    }, y);
    const expected = await page.screenshot({ fullPage: true, clip });
    await page.evaluate(() => document.getElementById("band-probe")!.remove());
    return painted.equals(expected);
  };

  test("the footer band paints past the column and adds no horizontal scroll", async ({ page }) => {
    await page.goto("/about/");
    expect(await paintsBand(page)).toBe(true);
    await page.setViewportSize({ width: 390, height: 800 });
    expect(await paintsBand(page)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  });

  test("with none of the footer properties set the footer keeps its own colours", async ({ page }) => {
    await page.goto("/about/");
    const m = await page.evaluate(() => {
      const root = document.documentElement.style;
      for (const p of ["bg", "ink", "rule"]) root.setProperty(`--moss-footer-${p}`, "initial");
      const color = (token: string, prop: "color" | "borderTopColor") => {
        const probe = document.createElement("i");
        probe.style.cssText = `position:absolute;color:var(${token});border:1px solid var(${token})`;
        document.body.appendChild(probe);
        const v = getComputedStyle(probe)[prop];
        probe.remove();
        return v;
      };
      const style = (s: string, pseudo?: string) => getComputedStyle(document.querySelector(s)!, pseudo);
      return {
        text: style("footer.container").color,
        link: style("footer.container p a").color,
        heading: style("footer.container h2").color,
        divider: style("footer.container", "::before").borderBlockStartColor,
        band: style("footer.container").borderImageSource,
        muted: color("--moss-color-muted", "color"),
        accent: color("--moss-color-accent", "color"),
        strong: color("--moss-color-text", "color"),
        light: color("--moss-border-light", "borderTopColor"),
      };
    });
    expect(m.text).toBe(m.muted);
    expect(m.link).toBe(m.accent);
    expect(m.heading).toBe(m.strong);
    expect(m.divider).toBe(m.light);
    expect(m.band).toContain("rgba(0, 0, 0, 0)");
  });

  test("a header=over page stays transparent with light links", async ({ page }) => {
    await page.goto("/over/");
    expect(await css(page, "body > header", "backgroundColor")).toBe("rgba(0, 0, 0, 0)");
    expect(await css(page, "body > header .site-name", "color")).toBe("rgb(255, 255, 255)");
  });

  test("a hero-only home has no empty article and no blank page above the footer", async ({ page }) => {
    await page.goto("/");
    expect(await page.locator("main#main-content").count()).toBe(1);
    expect(await page.locator("main > article").count()).toBe(0);

    const m = await page.evaluate(() => {
      const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
      const hero = document.querySelector(".moss-hero")!;
      return {
        main: box("main").height,
        gap: box("footer").top - box(".moss-hero").bottom,
        heroMargin: parseFloat(getComputedStyle(hero).marginBlockEnd),
        footerMargin: parseFloat(getComputedStyle(document.querySelector("footer")!).marginBlockStart),
      };
    });
    // Only the two margins moss gives the hero and the footer remain between them.
    expect(m.main).toBe(0);
    expect(m.gap).toBe(m.heroMargin + m.footerMargin);
  });

  test("a page with body text keeps its article", async ({ page }) => {
    await page.goto("/about/");
    expect(await page.locator("main > article.container").count()).toBe(1);
  });

  test("clearing the cover tint with --moss-cover-ink gives readable plain cards", async ({ page }) => {
    await page.goto("/cards/");
    const bodyText = await css(page, "body", "color");
    expect(await css(page, ".moss-card-content", "backgroundColor")).toBe("rgba(0, 0, 0, 0)");
    expect(await css(page, ".moss-card-title", "color")).toBe(bodyText);
    // The muted description derives from the ink, so it is not the white it was.
    expect(await css(page, ".moss-card-description", "color")).not.toMatch(/255, 255, 255|srgb 1 1 1/);
  });
});
