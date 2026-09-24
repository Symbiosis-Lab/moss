/**
 * Tests for asset-urls.ts — the URL conversions shared by the blueprint
 * placeholder and the preview's asset swap.
 *
 * These replace the app's own old iframe-bridge-image-handler test,
 * which declared its own private copy of an image handler keyed on
 * `data-placeholder-src` — an attribute iframe-bridge stopped using for images in
 * 2026-05. It tested its own fixture, so no regression in the real swap could
 * ever turn it red. The functions here ARE the production code.
 */

import { describe, test, expect } from "vitest";
import {
  bustSrcset,
  normPath,
  srcsetMatches,
  srcsetUrls,
  stripBust,
} from "../asset-urls";

describe("normPath", () => {
  test("resolves a document-relative attribute to an absolute pathname", () => {
    expect(normPath("./assets/photo.webp")).toBe("/assets/photo.webp");
  });

  test("decodes percent-encoded segments so non-ASCII paths compare equal", () => {
    // moss percent-encodes emitted URLs per segment; AssetReady paths arrive
    // decoded. A site whose filenames are all CJK depends entirely on this.
    expect(normPath("/%E9%97%9C%E6%96%BC/%E9%A0%AD%E5%83%8F.webp")).toBe(
      "/關於/頭像.webp",
    );
  });

  test("drops the query string — a cache-bust must not change identity", () => {
    expect(normPath("/assets/photo.webp?_t=123")).toBe("/assets/photo.webp");
  });

  test("returns empty for empty input rather than resolving to the page URL", () => {
    expect(normPath("")).toBe("");
  });
});

describe("stripBust", () => {
  test("removes a bust without touching the rest of the URL", () => {
    expect(stripBust("/a/photo.webp?_t=1712345")).toBe("/a/photo.webp");
  });

  test("does not accumulate across repeated swaps", () => {
    expect(stripBust("/a.webp?_t=1?_t=2")).toBe("/a.webp");
  });
});

describe("srcsetUrls", () => {
  test("splits a ladder and drops width descriptors", () => {
    expect(srcsetUrls("/a.w800.webp 800w, /a.w1600.webp 1600w")).toEqual([
      "/a.w800.webp",
      "/a.w1600.webp",
    ]);
  });

  test("handles a single candidate with no descriptor", () => {
    expect(srcsetUrls("/a.webp")).toEqual(["/a.webp"]);
  });

  test("handles density descriptors", () => {
    expect(srcsetUrls("/a.webp 1x, /a2.webp 2x")).toEqual(["/a.webp", "/a2.webp"]);
  });

  test("tolerates trailing commas and extra whitespace", () => {
    expect(srcsetUrls("  /a.webp 800w ,  , /b.webp 1600w,")).toEqual([
      "/a.webp",
      "/b.webp",
    ]);
  });

  test("returns nothing for an empty srcset", () => {
    expect(srcsetUrls("")).toEqual([]);
  });
});

describe("srcsetMatches", () => {
  const LADDER = "/assets/a.w800.webp 800w, /assets/a.w1600.webp 1600w";

  test("matches a middle candidate, not just the first", () => {
    // The defect this function exists to fix: comparing the whole attribute
    // string against one event path never matched a laddered image.
    expect(srcsetMatches(LADDER, "/assets/a.w1600.webp")).toBe(true);
  });

  test("matches the first candidate", () => {
    expect(srcsetMatches(LADDER, "/assets/a.w800.webp")).toBe(true);
  });

  test("does not match a rung the ladder does not contain", () => {
    expect(srcsetMatches(LADDER, "/assets/a.w400.webp")).toBe(false);
  });

  test("matches through an existing cache-bust", () => {
    expect(srcsetMatches("/assets/a.w800.webp?_t=99 800w", "/assets/a.w800.webp")).toBe(
      true,
    );
  });
});

describe("bustSrcset", () => {
  test("busts every candidate and preserves descriptors", () => {
    expect(bustSrcset("/a.w800.webp 800w, /a.w1600.webp 1600w", "?_t=7")).toBe(
      "/a.w800.webp?_t=7 800w, /a.w1600.webp?_t=7 1600w",
    );
  });

  test("busts a descriptorless single candidate", () => {
    expect(bustSrcset("/a.webp", "?_t=7")).toBe("/a.webp?_t=7");
  });

  test("replaces a prior bust instead of stacking one", () => {
    expect(bustSrcset("/a.webp?_t=1 800w", "?_t=2")).toBe("/a.webp?_t=2 800w");
  });
});
