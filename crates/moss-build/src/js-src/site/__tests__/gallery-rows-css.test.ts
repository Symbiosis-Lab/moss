/**
 * The justified-rows layout crops each photo to its ratio (`aspect-ratio` plus
 * `object-fit: cover`). That is right only inside `:::gallery rows`; a column
 * gallery lets each photo keep its own shape.
 */

import { test, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const css = readFileSync(resolve(__dirname, "../../../assets/css/site.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

test("the ratio crop on gallery images applies to the rows layout only", () => {
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*aspect-ratio:\s*var\(--ar[^{}]*)\}/g)];
  const gallery = rules.filter(([, selector]) => selector.includes(".moss-gallery-item"));
  expect(gallery.length).toBeGreaterThan(0);
  for (const [, selector] of gallery) {
    expect(selector.trim(), selector).toMatch(/^\.moss-gallery\[data-layout="rows"\]/);
  }
});
