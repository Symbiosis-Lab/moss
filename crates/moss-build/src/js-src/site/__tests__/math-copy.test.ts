/**
 * Tests for math-copy.ts — click-to-copy LaTeX source.
 *
 * The payload contract: svg.moss-math carries RAW TeX in aria-label
 * (verified against a real vault build — NO delimiters), so the script
 * restores `$…$` / `$$…$$` per data-moss-math; the P1 code chip already
 * renders the delimited source as text and is copied verbatim. Entities
 * must round-trip: the build serializes `a < b` as `a &lt; b` in the
 * attribute, getAttribute un-escapes it back.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { mathCopyPayload } from "../math-copy";

const SVG_NS = "http://www.w3.org/2000/svg";

function mathSvg(kind: "inline" | "display", tex: string): SVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("class", "moss-math");
  svg.setAttribute("data-moss-math", kind);
  svg.setAttribute("aria-label", tex);
  const path = document.createElementNS(SVG_NS, "path");
  svg.appendChild(path);
  return svg;
}

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("mathCopyPayload", () => {
  test("inline svg wraps raw TeX in single dollars", () => {
    expect(mathCopyPayload(mathSvg("inline", "E = mc^2"))).toBe("$E = mc^2$");
  });

  test("display svg wraps raw TeX in double dollars, source spacing preserved", () => {
    // Real builds keep the author's padding inside $$ … $$ — the payload
    // must reproduce the source spelling, not a trimmed variant.
    expect(mathCopyPayload(mathSvg("display", " o_t = \\frac{a}{b} "))).toBe(
      "$$ o_t = \\frac{a}{b} $$",
    );
  });

  test("entities round-trip through the attribute", () => {
    // Built HTML serializes aria-label="a &lt; b \&amp; c &gt; d"; the DOM
    // hands back the unescaped source via getAttribute.
    const holder = document.createElement("div");
    holder.innerHTML =
      '<svg class="moss-math" data-moss-math="inline" aria-label="a &lt; b \\&amp; c &gt; d"></svg>';
    const svg = holder.querySelector("svg.moss-math")!;
    expect(mathCopyPayload(svg)).toBe("$a < b \\& c > d$");
  });

  test("code fallback chip is copied verbatim (delimiters already present)", () => {
    const code = document.createElement("code");
    code.className = "moss-math";
    code.setAttribute("data-moss-math", "inline");
    code.textContent = "$一个5，两个10$";
    expect(mathCopyPayload(code)).toBe("$一个5，两个10$");
  });

  test("display code chip keeps its double-dollar source", () => {
    const code = document.createElement("code");
    code.className = "moss-math";
    code.setAttribute("data-moss-math", "display");
    code.textContent = "$$x^2$$";
    expect(mathCopyPayload(code)).toBe("$$x^2$$");
  });

  test("svg without aria-label yields null", () => {
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("class", "moss-math");
    expect(mathCopyPayload(svg)).toBeNull();
  });
});

describe("delegated click handler", () => {
  let writeText: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
  });

  test("click on a child of the math svg copies the wrapped TeX", async () => {
    const svg = mathSvg("inline", "x^2");
    document.body.appendChild(svg);
    svg.querySelector("path")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(writeText).toHaveBeenCalledWith("$x^2$");
  });

  test("click elsewhere copies nothing", () => {
    const p = document.createElement("p");
    p.textContent = "prose";
    document.body.appendChild(p);
    p.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(writeText).not.toHaveBeenCalled();
  });

  test("shows the toast after a successful copy", async () => {
    document.documentElement.lang = "en";
    const svg = mathSvg("display", "y");
    document.body.appendChild(svg);
    svg.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await vi.waitFor(() => {
      expect(document.querySelector(".share-toast")).not.toBeNull();
    });
    expect(writeText).toHaveBeenCalledWith("$$y$$");
  });

  test("degrades gracefully when navigator.clipboard is absent", () => {
    Object.defineProperty(navigator, "clipboard", {
      value: undefined,
      configurable: true,
    });
    const svg = mathSvg("inline", "z");
    document.body.appendChild(svg);
    expect(() =>
      svg.dispatchEvent(new MouseEvent("click", { bubbles: true })),
    ).not.toThrow();
    expect(document.querySelector(".share-toast")).toBeNull();
  });
});
