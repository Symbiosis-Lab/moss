/** Search matching: accents and case fold away, prefixes rank before word starts, cities point to their country. */
import { describe, expect, test } from "vitest";
import { fold, rank, type Row } from "../match";

const ROWS: Row[] = [
  ["nigeria", "Nigeria", ["Federal Republic of Nigeria", "NGA"], ["Lagos", "Abuja"]],
  ["niger", "Niger", [], ["Niamey"]],
  ["south-africa", "South Africa", ["RSA"], ["Cape Town"]],
  ["central-african-republic", "Central African Republic", [], []],
  ["afghanistan", "Afghanistan", [], ["Kabul"]],
  ["cote-d-ivoire", "Côte d'Ivoire", ["ivory coast"], ["Abidjan"]],
  ["singapore", "Singapore", [], ["Singapore"]],
];
const texts = (q: string) => rank(ROWS, q).map((h) => h.text);

describe("fold", () => {
  test("drops accents, case and punctuation", () => {
    expect(fold("Côte d'Ivoire")).toBe("cote d ivoire");
    expect(fold("  SÃO  Paulo ")).toBe("sao paulo");
  });
});

describe("rank", () => {
  test("an empty query finds nothing", () => {
    expect(rank(ROWS, "  ")).toEqual([]);
  });
  test("prefix matches come before word-start matches", () => {
    expect(texts("af")).toEqual(["Afghanistan", "South Africa", "Central African Republic"]);
  });
  test("an exact name beats a longer name it begins", () => {
    expect(texts("niger").slice(0, 2)).toEqual(["Niger", "Nigeria"]);
  });
  test("a city points to its country", () => {
    expect(rank(ROWS, "lagos")).toEqual([{ slug: "nigeria", text: "Lagos, Nigeria" }]);
  });
  test("accents and case do not matter, in the query or the name", () => {
    expect(texts("COTE D'IV")).toEqual(["Côte d'Ivoire"]);
    expect(texts("abidjan")).toEqual(["Abidjan, Côte d'Ivoire"]);
  });
  test("a short or formal name finds its country under the country's name", () => {
    expect(texts("ivory")).toEqual(["Côte d'Ivoire"]);
    expect(texts("rsa")).toEqual(["South Africa"]);
  });
  test("a city named like its country is not listed twice", () => {
    expect(texts("sing")).toEqual(["Singapore"]);
  });
  test("at most six rows", () => {
    const many: Row[] = Array.from({ length: 20 }, (_, i) => [`land-${i}`, `Land ${i}`, [], []]);
    expect(rank(many, "land")).toHaveLength(6);
  });
});
