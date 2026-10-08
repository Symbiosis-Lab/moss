/**
 * Tests for cards.ts's pure half — row order and row membership. The
 * DOM-building half (CardRow, the expand-in-place grid track) is covered
 * by the render gates.
 */
import { describe, test, expect } from "vitest";
import { byWeightThenDateThenTitle, CardRow, worksForRow } from "../cards";
import { copyFor, type PlacesStrings } from "../strings";
import type { Work } from "../types";

function work(id: string, title: string, date?: string): Work {
  return { id, title, url: `/${id}/`, date, byline: [], authors: [], places: [], companions: [] };
}

describe("byWeightThenDateThenTitle", () => {
  test("sorts by date descending", () => {
    const works = [work("old", "Old", "2020-01-01"), work("new", "New", "2024-06-01")];
    expect([...works].sort(byWeightThenDateThenTitle).map((w) => w.id)).toEqual(["new", "old"]);
  });

  test("breaks a date tie by title", () => {
    const works = [work("b", "Beta", "2024-01-01"), work("a", "Alpha", "2024-01-01")];
    expect([...works].sort(byWeightThenDateThenTitle).map((w) => w.id)).toEqual(["a", "b"]);
  });

  test("an undated work always sorts after every dated one, not as the oldest", () => {
    const works = [work("undated", "Undated"), work("ancient", "Ancient", "1900-01-01")];
    expect([...works].sort(byWeightThenDateThenTitle).map((w) => w.id)).toEqual(["ancient", "undated"]);
  });
});

describe("byWeightThenDateThenTitle with weights", () => {
  test("weight ascending beats title, and weighted works come before unweighted ones", () => {
    const works = [{ ...work("zeta", "Zeta"), weight: 1 }, work("alpha", "Alpha"), { ...work("mid", "Mid"), weight: 2 }];
    expect([...works].sort(byWeightThenDateThenTitle).map((w) => w.id)).toEqual(["zeta", "mid", "alpha"]);
  });
});

describe("worksForRow", () => {
  const works = [work("a", "Alpha", "2024-03-01"), work("b", "Beta", "2024-01-01"), work("c", "Gamma", "2024-02-01")];

  test("only visible works, sorted date descending", () => {
    const row = worksForRow(works, new Set(["a", "b"]), null, null);
    expect(row.map((w) => w.id)).toEqual(["a", "b"]);
  });

  test("a ring scope further narrows the visible set", () => {
    const row = worksForRow(works, new Set(["a", "b", "c"]), new Set(["b"]), null);
    expect(row.map((w) => w.id)).toEqual(["b"]);
  });

  test("the selected work stays in the row even once it scrolls out of view", () => {
    const row = worksForRow(works, new Set(["a"]), null, "c");
    // Re-admitted, then sorted like any other row member — not pinned first:
    // "a" (2024-03-01) is newer than "c" (2024-02-01).
    expect(row.map((w) => w.id)).toEqual(["a", "c"]);
  });

  test("a scoped-out selection is not added back — scope wins", () => {
    const row = worksForRow(works, new Set(["a", "b", "c"]), new Set(["b"]), "c");
    expect(row.map((w) => w.id)).toEqual(["b"]);
  });
});

describe("the collapsed card's meta line", () => {
  function metaOf(w: Work, sep = ", "): string[] {
    const container = document.createElement("div");
    const row = new CardRow(container, { selectWork() {} }, { untitled: "Untitled", readArticle: "Read", listSeparator: sep } as PlacesStrings);
    row.render([w], [], null);
    return [...container.querySelectorAll(".moss-places-card-select .moss-card-meta > span")].map((el) => el.textContent ?? "");
  }

  test("the author names come first, joined, then the date carrying its own separator", () => {
    expect(metaOf({ ...work("a", "A", "2024-06-10"), authors: ["Ana", "Bo"], byline: ["Photographs: Cy"] })).toEqual([
      "Ana, Bo",
      "\u00a0\u00b7\u00a0" + "2024-06-10",
    ]);
  });

  test("several authors are joined with the language's list separator, not the author-to-date dot", () => {
    const w = { ...work("a", "A", "2024-06-10"), authors: ["Ana", "Bo", "Cy"] };
    expect(metaOf(w, copyFor("en").listSeparator)[0]).toBe("Ana, Bo, Cy");
    expect(metaOf(w, copyFor("zh-Hant").listSeparator)[0]).toBe("Ana、Bo、Cy");
    expect(metaOf(w, copyFor("zh-CN").listSeparator)[0]).toBe("Ana、Bo、Cy");
  });

  test("a page with no author falls back to the first byline entry", () => {
    expect(metaOf({ ...work("a", "A", "2024-06-10"), byline: ["Ana", "Bo"] })).toEqual(["Ana", "\u00a0\u00b7\u00a0" + "2024-06-10"]);
  });

  test("a lone date or a lone author is the whole line, with no separator", () => {
    expect(metaOf(work("a", "A", "2024-06-10"))).toEqual(["2024-06-10"]);
    expect(metaOf({ ...work("a", "A"), byline: ["Ana"] })).toEqual(["Ana"]);
  });
});

describe("the expanded card's places", () => {
  test("each place is a link to its own page, in work order", () => {
    const container = document.createElement("div");
    const row = new CardRow(container, { selectWork() {} }, { untitled: "Untitled", readArticle: "Read", listSeparator: ", " } as PlacesStrings);
    const places = [
      { id: "places/cambridge", name: "Cambridge" },
      { id: "places/england", name: "England" },
    ];
    row.render([{ ...work("a", "A"), places: ["places/cambridge", "places/england"] }], places, null);
    const links = [...container.querySelectorAll<HTMLAnchorElement>(".moss-places-card-detail p a")];
    expect(links.map((a) => [a.textContent, a.getAttribute("href")])).toEqual([
      ["Cambridge", "/places/cambridge/"],
      ["England", "/places/england/"],
    ]);
  });
});
