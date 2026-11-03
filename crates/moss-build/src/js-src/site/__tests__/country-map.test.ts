/**
 * Choosing a country opens one panel from three entry points (a tap on the
 * map, a list row, the address hash); Esc, the close button and empty sea
 * clear it; disputed land is inert.
 */
import { beforeEach, describe, expect, test } from "vitest";

const HEAD = (name: string) =>
  `<div class="moss-country-panel-head"><h3 class="moss-country-panel-name">${name}</h3><button class="moss-country-panel-close" type="button">x</button></div>`;

function page(): void {
  document.body.innerHTML = `<figure data-moss-country-map>
    <svg><path class="moss-country-graticule" id="sea"/><g>
      <path class="moss-country-disputed" id="disputed"/>
      <path class="moss-country" id="japan" data-slug="japan" data-name="Japan" data-level="1"/>
      <path class="moss-country" id="mexico" data-slug="mexico" data-name="Mexico" data-level="2"/>
      <path class="moss-country" id="chad" data-slug="chad" data-name="Chad"/>
    </g></svg>
    <div class="moss-country-panel" hidden></div>
    <div class="moss-country-list"><a id="row" href="#mexico">Mexico</a></div>
    <template data-slug="japan">${HEAD("Japan")}<p class="moss-country-panel-level">Low</p><p>Take normal precautions.</p><p><a href="/ask/?to=Japan">Ask us</a></p></template>
    <template data-slug="mexico">${HEAD("Mexico")}<p class="moss-country-panel-level">High</p><dl><dt>Note</dt><dd>Night travel.</dd></dl></template>
    <template data-not-rated>${HEAD("")}<p class="moss-country-panel-level">Not rated</p><p>Not assessed.</p><p><a data-link="/ask/?to={country}">Ask us</a></p></template>
  </figure>`;
}

const panel = () => document.querySelector<HTMLElement>(".moss-country-panel")!;
const click = (id: string) => document.getElementById(id)!.dispatchEvent(new MouseEvent("click", { bubbles: true }));

describe("country map panel", () => {
  beforeEach(async () => {
    page();
    location.hash = "";
    await import("../maps/country-map");
    document.dispatchEvent(new Event("moss-morph-patched"));
  });

  test("a tap shows the name, level and the row's own columns", () => {
    click("mexico");
    expect(panel().hidden).toBe(false);
    expect(panel().textContent).toContain("Mexico");
    expect(panel().textContent).toContain("High");
    expect(panel().textContent).toContain("Night travel.");
    expect(panel().textContent).not.toContain("Avoid travel.");
    expect(document.getElementById("mexico")!.classList.contains("is-chosen")).toBe(true);
  });

  test("a row with no extra columns shows its level's text and the action", () => {
    click("japan");
    expect(panel().textContent).toContain("Take normal precautions.");
    expect(panel().querySelector("a")!.getAttribute("href")).toBe("/ask/?to=Japan");
  });

  test("an unrated country shows Not rated, the unrated text and the action", () => {
    click("chad");
    expect(panel().textContent).toContain("Chad");
    expect(panel().textContent).toContain("Not rated");
    expect(panel().textContent).toContain("Not assessed.");
    expect(panel().querySelector("a")!.getAttribute("href")).toBe("/ask/?to=Chad");
  });

  test("disputed land is inert and the sea clears", () => {
    click("japan");
    click("disputed");
    expect(panel().hidden).toBe(false);
    click("sea");
    expect(panel().hidden).toBe(true);
  });

  test("Esc and the close button clear", () => {
    click("japan");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(panel().hidden).toBe(true);
    click("japan");
    panel().querySelector<HTMLElement>(".moss-country-panel-close")!.click();
    expect(panel().hidden).toBe(true);
  });

  test("a list row opens the same panel", () => {
    click("row");
    expect(panel().textContent).toContain("Mexico");
  });
});
