/**
 * A coverless `:::grid` cell beside covered siblings must read as a card.
 *
 * `render_list_with_typesetting` (the auto-generated listing grid) already
 * upgrades a coverless card in a mixed row to a `data-cover="quote"` slot —
 * the page's description, or its title with none — printed on the same
 * surface-filled box every covered sibling's cover occupies, instead of the
 * bare empty `.moss-card-cover.moss-card-no-cover` placeholder
 * (`grid_card.rs::render_item`'s `list_has_covers` doc). Before this fix
 * `apply_collection_cards` (grid_cells.rs) rendered every `:::grid` cell in
 * isolation and never computed that flag, so a hand-picked grid's coverless
 * cell always fell to the bare placeholder — a pale, chromeless box beside
 * its siblings' photos.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → GRID_CARD_NO_COVER_GATE — three
 * `[[wikilink]]` cells in one `:::grid 3`, "Card Two" the only one with no
 * `cover:` of its own.
 */
import { test, expect } from "@playwright/test";

test("a coverless cell in a mixed :::grid gets the quote slot, not the bare placeholder", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");

  const cards = await page.$$eval(".moss-grid > a.moss-card", (els) =>
    els.map((el) => {
      const cover = el.querySelector(".moss-card-cover") as HTMLElement;
      const coverRect = cover.getBoundingClientRect();
      const cardRect = el.getBoundingClientRect();
      const quote = cover.querySelector("p");
      return {
        title: el.querySelector(".moss-card-title")?.textContent?.trim() ?? "",
        cardHeight: Math.round(cardRect.height),
        isPlaceholder: cover.classList.contains("moss-card-no-cover"),
        isQuote: cover.getAttribute("data-cover") === "quote",
        coverWidth: Math.round(coverRect.width),
        coverHeight: Math.round(coverRect.height),
        quoteText: quote?.textContent?.trim() ?? null,
        coverBackground: getComputedStyle(cover).backgroundColor,
      };
    }),
  );

  expect(cards).toHaveLength(3);

  // Equal heights: a coverless card in the row must not collapse or grow
  // against its covered siblings.
  const heights = cards.map((c) => c.cardHeight);
  expect(
    Math.max(...heights) - Math.min(...heights),
    `cards in the row are not equal height: ${JSON.stringify(cards)}`,
  ).toBeLessThanOrEqual(1);

  const two = cards.find((c) => c.title === "Card Two");
  expect(two, `"Card Two" not found among rendered cards: ${JSON.stringify(cards)}`).toBeTruthy();

  // The bare placeholder must be gone — replaced by the quote slot.
  expect(two!.isPlaceholder, "the coverless card still carries the bare placeholder").toBe(false);
  expect(two!.isQuote, 'the coverless card must carry data-cover="quote"').toBe(true);

  // The box is genuinely visible: real, positive dimensions (not collapsed
  // to zero like the auto-listing's subgrid-collapsed track), a printed
  // fallback — the page's title, since this fixture sets no description —
  // and a real fill distinguishable from a fully transparent hole.
  expect(two!.coverWidth, "the quote box collapsed to zero width").toBeGreaterThan(0);
  expect(two!.coverHeight, "the quote box collapsed to zero height").toBeGreaterThan(0);
  expect(two!.quoteText).toBe("Card Two");
  expect(
    two!.coverBackground,
    "the quote box has no visible fill against the page background",
  ).not.toBe("rgba(0, 0, 0, 0)");

  // The two covered siblings are unaffected: still a real image, no quote
  // slot painted over it.
  for (const title of ["Card One", "Card Three"]) {
    const covered = cards.find((c) => c.title === title);
    expect(covered, `"${title}" not found: ${JSON.stringify(cards)}`).toBeTruthy();
    expect(covered!.isQuote, `${title} must not carry the quote slot`).toBe(false);
    expect(covered!.isPlaceholder, `${title} must not carry the bare placeholder`).toBe(false);
  }
});
