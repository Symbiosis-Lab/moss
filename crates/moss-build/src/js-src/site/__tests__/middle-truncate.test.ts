import { describe, expect, test } from "vitest";

import { truncateMiddle } from "../nav/middle-truncate";

/** Every code point is 10 wide, "…" included: width is a count, so the
 *  expected strings below can be worked out by hand. */
const measure = (s: string): number => [...s].length * 10;

describe("truncateMiddle", () => {
  test("a title that fits is returned untouched", () => {
    expect(truncateMiddle("短標題", 30, measure)).toBe("短標題");
  });

  test("a long CJK title keeps its start and its end", () => {
    const title = "第一章：從一座小島出發，穿過三個季節，最後回到海邊的老屋（上）";
    // 9 slots of 10px: eight characters plus the ellipsis; the start gets the
    // odd one.
    expect(truncateMiddle(title, 90, measure)).toBe("第一章：…屋（上）");
    const out = truncateMiddle(title, 90, measure);
    expect([...out]).toHaveLength(9);
    expect(out.startsWith("第一章")).toBe(true);
    expect(out.endsWith("（上）")).toBe(true);
    expect(out).toContain("…");
  });

  test("it never cuts an astral character in half", () => {
    // 𠮷 is one code point, two UTF-16 units. A slice by unit would leave a
    // lone surrogate beside the ellipsis.
    const title = "𠮷野家的𠮷祥物語𠮷";
    for (let width = 20; width < 90; width += 10) {
      const out = truncateMiddle(title, width, measure);
      expect(out).not.toMatch(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/);
    }
    const out = truncateMiddle(title, 50, measure);
    expect(out.startsWith("𠮷野")).toBe(true);
    expect(out.endsWith("𠮷")).toBe(true);
  });

  test("the result is the longest form that fits, one character wider does not", () => {
    const title = "abcdefghijklmnopqrstuvwxyz";
    for (const width of [30, 55, 100, 200]) {
      const out = truncateMiddle(title, width, measure);
      expect(measure(out)).toBeLessThanOrEqual(width);
      // One more kept character would have overflowed.
      const kept = [...out].length - 1;
      const bigger =
        [...title].slice(0, Math.ceil((kept + 1) / 2)).join("") +
        "…" +
        [...title].slice(title.length - Math.floor((kept + 1) / 2)).join("");
      expect(measure(bigger)).toBeGreaterThan(width);
    }
  });

  test("an odd count of kept characters gives the extra one to the start", () => {
    // Six slots: five characters and the ellipsis.
    expect(truncateMiddle("abcdefghij", 60, measure)).toBe("abc…ij");
  });

  test("spaces beside the ellipsis are trimmed on both sides", () => {
    // Ten kept characters would be "aaaa " + "…" + " cccc": a space on each
    // side of the cut, which would read "aaaa … cccc".
    expect(truncateMiddle("aaaa bbbbbbbb cccc", 100, measure)).toBe("aaaa…cccc");
  });

  test("when nothing fits it still returns the smallest form, not an empty label", () => {
    expect(truncateMiddle("標題很長很長", 5, measure)).toBe("標…");
  });
});
