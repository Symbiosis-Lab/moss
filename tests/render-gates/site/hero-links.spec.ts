// In a hero's words, a link inside a sentence stays a text link in the words'
// colour, and a paragraph of nothing but links is the call to action, drawn as
// buttons: on a page's hero and on the closing picture, on a phone and wide.
import { test, expect } from "@playwright/test";

const HEROES = [
  { name: "a page's hero", root: ".moss-hero:not(.moss-hero-closing)" },
  { name: "the closing picture", root: "footer .moss-hero-closing" },
];

for (const width of [390, 1280]) {
  for (const { name, root } of HEROES) {
    test(`${name} at ${width}px: a link in a sentence is text, a lone link a button`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      const look = (text: string) =>
        page.locator(root).first().getByRole("link", { name: text }).evaluate((a) => {
          const s = getComputedStyle(a);
          const p = getComputedStyle(a.parentElement!);
          return { display: s.display, border: s.borderTopWidth, underline: s.textDecorationLine, sameInk: s.color === p.color };
        });
      expect(await look("A text link")).toEqual({ display: "inline", border: "0px", underline: "underline", sameInk: true });
      const button = await look("Plan a visit");
      expect(button.display).toBe("inline-block");
      expect(button.border).toBe("1px");
      expect(button.underline).toBe("none");
    });
  }
}

test("a stacked phone hero's lead heading takes --moss-hero-title-size, else 1.5rem", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 });
  await page.goto("/");
  const size = () => page.locator(".moss-hero:not(.moss-hero-closing) .moss-hero-content h1").first().evaluate((h) => getComputedStyle(h).fontSize);
  expect(await size(), "no token").toBe("24px");
  await page.addStyleTag({ content: ":root { --moss-hero-title-size: 40px; }" });
  expect(await size(), "token set").toBe("40px");
});

for (const width of [390, 1280]) {
  test(`the closing picture at ${width}px: an image in the words stays an inline mark, not the picture`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    const hero = page.locator("footer .moss-hero-closing").first();
    const mark = hero.locator(".moss-hero-content a img");
    const words = (await hero.locator(".moss-hero-content").boundingBox())!;
    const box = (await mark.boundingBox())!;
    expect(box.width, "mark width").toBeLessThan(40);
    expect(box.y, "mark top is inside the words").toBeGreaterThanOrEqual(words.y);
    expect(box.y + box.height, "mark bottom is inside the words").toBeLessThanOrEqual(words.y + words.height);
    expect(await mark.evaluate((img) => getComputedStyle(img).display)).toBe("inline");
    // The hero's own picture is untouched: it still fills the closing picture.
    const picture = (await hero.locator(":scope > img, :scope > picture img").first().boundingBox())!;
    expect(picture.width).toBeGreaterThan(200);
  });
}
