/**
 * Two dark-mode cascade facts about a built site, sharing one scratch build.
 *
 * The first is layer order. The second is the colophon mark's two states: grey
 * with the wording at rest, inked when the credit is reached for. Both readings
 * need a browser. The resting one is an inheritance chain three elements long
 * that ends in a `currentColor` on an SVG path, and the inked one is a rule
 * keyed on the literal the mark is painted with — a shape nothing else in
 * site.css uses, and one no Rust test can check: those see the stylesheet as
 * text, and text cannot say whether a rule beat a presentation attribute. Both
 * arms of the dark rule are exercised, the media-query one and the
 * `[data-theme]` one, because they are written out twice and a typo in either
 * is invisible from the other.
 *
 * Fact one: dark tokens are beaten by layer order, not specificity.
 *
 * The author's override and moss's tokens-layer dark block both use a bare
 * `[data-theme="dark"]` selector, with no `:root` prefix. Their specificity is
 * therefore identical, so the author win asserted here can only come from layer
 * order (`tokens` < `themes`). Add `:root` to either and the gate stops proving
 * anything.
 *
 * A third test used to fetch the built stylesheet and regex it to confirm the
 * tokens-layer selector is bare. That is a fact about emitted text, already
 * asserted by exact selector match in a Rust unit test — downloading the
 * sheet inside two browsers to re-check it added cost, not confidence.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts, built by the
 * playwright config at parse time and served by its webServer.
 *
 * Run via:
 *   npx playwright test -c playwright/dark-layer-order.config.ts
 */
import { test, expect } from "@playwright/test";

const EXPECTED_BG = "rgb(17, 17, 17)"; // #111111 — author override wins via layer order

// The mark's ink, which it takes only while the credit is hovered or focused.
const LINK = ".moss-colophon a";
const ICON = ".moss-colophon-icon";
const DROP = '.moss-colophon-icon svg path[fill="#4f7031"]';
const DROP_LIGHT = "rgb(79, 112, 49)"; // #4f7031, the drop on paper
const DROP_DARK = "rgb(163, 212, 131)"; // #a3d483, lifted for a dark ground
// The black half on a dark ground: #ccc, not the #fff it reads as on paper's
// mirror. The mark is a solid mass beside a name written in strokes, so full
// white out-weighed the wording it is supposed to stand level with; mark.css's
// dark block carries the measurements. Held here as a literal because that is
// the point of the assertion — a token indirection would pass while painting
// anything.
const INK_DARK = "rgb(204, 204, 204)";

/**
 * The three colours that decide the mark, read together so a failure says
 * which of them broke: the wording's colour, the mark's black half, the drop.
 * @param page the page to read
 */
async function markInks(page: import("@playwright/test").Page) {
  await page.locator(DROP).waitFor({ state: "attached" });
  return page.evaluate(
    ([link, icon, drop]) => {
      const at = (sel: string) =>
        window.getComputedStyle(document.querySelector(sel)!);
      return {
        word: at(link).color,
        ink: at(icon).color,
        drop: at(drop).fill,
      };
    },
    [LINK, ICON, DROP],
  );
}

// One render, both readings. The data-theme attribute is strictly implied by
// the background assertion — the override cannot apply unless the pre-paint
// script set it — but it is free to read here, and it says *which half* broke.
test("themes-layer author override beats the tokens-layer dark block", async ({
  page,
}) => {
  // Set the explicit toggle, then reload so the pre-paint script reads it
  // before the stylesheet is parsed.
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await page.evaluate(() => localStorage.setItem("moss-theme", "dark"));
  await page.reload({ waitUntil: "domcontentloaded" });

  const seen = await page.evaluate(() => ({
    dataTheme: document.documentElement.getAttribute("data-theme"),
    bg: window.getComputedStyle(document.body).backgroundColor,
  }));

  expect(
    seen.dataTheme,
    'pre-paint script must set <html data-theme="dark"> from localStorage["moss-theme"]',
  ).toBe("dark");
  expect(
    seen.bg,
    `body background must be ${EXPECTED_BG}: both [data-theme="dark"] blocks have ` +
      "equal specificity, so only themes > tokens can produce the author's value",
  ).toBe(EXPECTED_BG);
});

test("the colophon mark is grey at rest and inks when reached for, on both dark arms", async ({
  page,
}) => {
  // The pre-paint script stamps data-theme from the system preference as well
  // as from the toggle, so a dark context arrives with the attribute already
  // set: this first reading is the [data-theme="dark"] arm.
  await page.goto("./", { waitUntil: "domcontentloaded" });
  expect(
    await page.evaluate(() =>
      document.documentElement.getAttribute("data-theme"),
    ),
  ).toBe("dark");

  // At rest the mark is the wording's own colour, both halves of it. Asserted
  // against the wording rather than against a literal because that is the
  // design — the mark joins the words — and it stays true under a theme that
  // moves --moss-color-muted.
  const rest = await markInks(page);
  expect(
    rest.ink,
    "at rest the mark's black half must take the colour it inherits from the link",
  ).toBe(rest.word);
  expect(
    rest.drop,
    "the drop must go quiet with it; a drop still green at rest means its rule " +
      "was left unscoped to hover, and the mark reads two-tone when nothing is " +
      "asking it to",
  ).toBe(rest.word);
  expect(
    rest.drop,
    "the control: if the wording were somehow the drop's own green, the two " +
      "assertions above would pass while proving nothing",
  ).not.toBe(DROP_DARK);

  // Reached for. The drop's rule has to beat the path's own fill attribute,
  // which is the half no Rust test can see.
  await page.hover(LINK);
  await expect(
    page.locator(DROP),
    `the [data-theme] arm must ink the drop to ${DROP_DARK}; a fill of ` +
      `${DROP_LIGHT} means the rule did not beat the path's own fill attribute`,
  ).toHaveCSS("fill", DROP_DARK);
  await expect(
    page.locator(ICON),
    "and the black half takes the dark ground's ink with it",
  ).toHaveCSS("color", INK_DARK);

  // Same rule, written a second time under @media. Stripping the attribute is
  // the only way to reach it on a page that stamps one — and the two are
  // written out separately, so a typo in either is invisible from the other.
  await page.evaluate(() =>
    document.documentElement.removeAttribute("data-theme"),
  );
  await expect(
    page.locator(DROP),
    "with data-theme removed only the media-query arm can match, and it must " +
      "ink the drop the same way",
  ).toHaveCSS("fill", DROP_DARK);

  // The control. Without it this test passes just as well with the drop painted
  // #a3d483 everywhere, which would lose the two-tone reading on paper.
  await page.emulateMedia({ colorScheme: "light" });
  await page.evaluate(() => localStorage.setItem("moss-theme", "light"));
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.hover(LINK);
  await expect(
    page.locator(DROP),
    `on paper the drop must ink to ${DROP_LIGHT} — the lift is for dark grounds only`,
  ).toHaveCSS("fill", DROP_LIGHT);
});
