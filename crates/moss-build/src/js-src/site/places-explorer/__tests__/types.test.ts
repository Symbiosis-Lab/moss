/**
 * Tests for the one wire-to-in-memory normalization boundary this
 * directory has: `normalizePlacesData`. `places_data.rs` omits `byline`
 * from the wire entirely when a work has no author
 * (`#[serde(skip_serializing_if = "Vec::is_empty")]`), the same way it
 * already omits `date`/`description`/`cover` when absent — a work with no
 * byline at all is the realistic case this guards (an unauthored field
 * note, an imported post with no byline frontmatter), not a hypothetical.
 */
import { describe, test, expect } from "vitest";
import { normalizePlacesData } from "../types";
import type { PlacesDataWire } from "../types";

describe("normalizePlacesData", () => {
  test("a work with only the always-present wire fields gets byline defaulted to []", () => {
    const wire: PlacesDataWire = {
      works: [{ id: "a", title: "A", url: "/a/", places: ["places/kyoto"], companions: [] }],
      places: [],
    };
    const data = normalizePlacesData(wire);
    expect(data.works[0].byline).toEqual([]);
  });

  test("a work that already carries byline keeps it, not re-defaulted", () => {
    const wire: PlacesDataWire = {
      works: [{ id: "a", title: "A", url: "/a/", byline: ["Jane Doe"], places: [], companions: [] }],
      places: [],
    };
    const data = normalizePlacesData(wire);
    expect(data.works[0].byline).toEqual(["Jane Doe"]);
  });

  test("date/description/cover pass through unchanged, present or absent — only byline needed defaulting", () => {
    const wire: PlacesDataWire = {
      works: [{ id: "a", title: "A", url: "/a/", places: [], companions: [] }],
      places: [],
    };
    const data = normalizePlacesData(wire);
    expect(data.works[0].date).toBeUndefined();
    expect(data.works[0].description).toBeUndefined();
    expect(data.works[0].cover).toBeUndefined();
  });

  test("places passes through unchanged — every one of its own optional fields (parent) was already correctly optional in the normalized type", () => {
    const wire: PlacesDataWire = {
      works: [],
      places: [{ id: "places/kyoto", name: "Kyoto", precision: "city", lat: 35.01, lng: 135.77 }],
    };
    const data = normalizePlacesData(wire);
    expect(data.places[0].parent).toBeUndefined();
  });
});
