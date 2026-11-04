import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { brotliDecompressSync } from "node:zlib";
import { BUDGET_BYTES, INDEX_BUDGET_BYTES, cutCities, equalEarth, HALF_WIDTH, simplifyRings } from "./generate-countries.mjs";

const DATA = resolve(import.meta.dirname, "../../crates/moss-build/data/place-map");

test("Equal Earth matches the published projection", () => {
  assert.deepEqual(equalEarth(0, 0), [0, 0]);
  assert.ok(Math.abs(HALF_WIDTH - 2.7066) < 1e-3, `half width ${HALF_WIDTH}`);
  const [, pole] = equalEarth(0, 90);
  assert.ok(Math.abs(pole - 1.3173) < 1e-3, `pole ${pole}`);
  assert.ok(Math.abs(HALF_WIDTH / pole - 2.05) < 0.01, "aspect ratio is about 2.05");
});

test("an edge two countries share is simplified identically in both", () => {
  const edge = [[10.4, 1], [10.1, 2.5], [10.5, 4], [10.05, 5.5], [10.6, 7], [10.2, 8]];
  const west = [[0, 0], [10, 0], ...edge, [10, 9], [0, 9]];
  const east = [[10, 0], [20, 0], [20, 9], [10, 9], ...[...edge].reverse()];
  const result = simplifyRings(new Map([["w", [west]], ["e", [east]]]), 0.6);
  const onEdge = (ring) => ring.filter(([x, y]) => x > 10 && y > 0 && y < 9).map((p) => p.join(",")).sort();
  assert.ok(onEdge(result.get("w")[0]).length > 0);
  assert.deepEqual(onEdge(result.get("w")[0]), onEdge(result.get("e")[0]));
});

test("the checked-in asset is within its budget and says what it holds", async () => {
  const packed = await readFile(resolve(DATA, "countries-v1.bin.br"));
  const report = JSON.parse(await readFile(resolve(DATA, "countries-size.json"), "utf8"));
  assert.ok(packed.length <= BUDGET_BYTES, `${packed.length} > ${BUDGET_BYTES}`);
  assert.equal(report.brotli_bytes, packed.length);
  assert.equal(report.budget_bytes, BUDGET_BYTES);
  assert.equal(brotliDecompressSync(packed).subarray(0, 8).toString("ascii"), "MOSSCTY1");
  assert.ok(!report.disputed_names.includes("Antarctica"));
});

const place = (name, adm0_a3, scalerank, pop_max, adm0cap = 0, sov_a3 = adm0_a3) => ({ properties: { name, adm0_a3, sov_a3, scalerank, pop_max, adm0cap } });

test("the city cut keeps big cities and capitals, largest first, one per name and country", () => {
  const cities = cutCities(
    [place("Small Town", "NGA", 7, 900), place("Abuja", "NGA", 5, 2000000, 1), place("Lagos", "NGA", 1, 9000000), place("Kano", "NGA", 3, 3000000), place("Lagos", "NGA", 2, 100), place("Hamlet", "NGA", 8, 10, 1), place("Nowhere", "XXX", 0, 5)],
    new Set(["NGA"]),
  );
  assert.deepEqual(cities, [["Lagos", "NGA"], ["Kano", "NGA"], ["Abuja", "NGA"]]);
});

test("a place whose own admin code has no shape falls back to its sovereign state, and a place on disputed land is dropped", () => {
  const cities = cutCities([place("Hargeisa", "SOL", 4, 1000, 1, "SOL"), place("Stanley", "FLK", 4, 2000, 1, "GBR")], new Set(["GBR", "FLK"]));
  assert.deepEqual(cities, [["Stanley", "FLK"]]);
});

test("the city list, the largest part of the search index, fits the index's budget, which the report records", async () => {
  const report = JSON.parse(await readFile(resolve(DATA, "countries-size.json"), "utf8"));
  const cities = await readFile(resolve(DATA, "countries-cities-v1.json"));
  assert.equal(report.index_budget_bytes, INDEX_BUDGET_BYTES);
  assert.ok(cities.length < INDEX_BUDGET_BYTES, `${cities.length} bytes of cities leave no room for the names`);
});
