/**
 * Tests for cards.ts's pure half — row order and row membership. The
 * DOM-building half (CardRow, the expand-in-place grid track) is covered
 * by the render gates.
 */
import { describe, test, expect } from "vitest";
import { byDateDescThenTitle, worksForRow } from "../cards";
import type { Work } from "../types";

function work(id: string, title: string, date?: string): Work {
  return { id, title, url: `/${id}/`, date, byline: [], places: [], companions: [] };
}

describe("byDateDescThenTitle", () => {
  test("sorts by date descending", () => {
    const works = [work("old", "Old", "2020-01-01"), work("new", "New", "2024-06-01")];
    expect([...works].sort(byDateDescThenTitle).map((w) => w.id)).toEqual(["new", "old"]);
  });

  test("breaks a date tie by title", () => {
    const works = [work("b", "Beta", "2024-01-01"), work("a", "Alpha", "2024-01-01")];
    expect([...works].sort(byDateDescThenTitle).map((w) => w.id)).toEqual(["a", "b"]);
  });

  test("an undated work always sorts after every dated one, not as the oldest", () => {
    const works = [work("undated", "Undated"), work("ancient", "Ancient", "1900-01-01")];
    expect([...works].sort(byDateDescThenTitle).map((w) => w.id)).toEqual(["ancient", "undated"]);
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
