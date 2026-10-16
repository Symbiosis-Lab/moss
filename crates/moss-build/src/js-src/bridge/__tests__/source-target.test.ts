/**
 * Source-target resolution for the preview bridge: which annotations an
 * element maps to, nearest first, and the owning file each one carries.
 */
import { describe, it, expect, afterEach } from "vitest";
import { resolveSourceTargets, themeSelectors } from "../source-target";

describe("resolveSourceTargets", () => {
  it("stops at data-source-none, so an annotated ancestor above chrome is not reported", () => {
    document.body.innerHTML =
      '<header data-source-line="1"><nav data-source-none>' +
      '<a id="t" data-source-file="about.md" data-source-fm="nav">About</a>' +
      "</nav></header>";
    expect(resolveSourceTargets(document.getElementById("t"))).toEqual([
      { kind: "frontmatter", field: "nav", path: "about.md", element: "a" },
    ]);
  });

  it("lists a shortcode range and the body line inside it, nearest first", () => {
    document.body.innerHTML =
      '<section class="moss-hero x" data-source-range="2-7"><h1 data-source-line="4">' +
      '<span id="x">Hi</span></h1></section>';
    expect(resolveSourceTargets(document.getElementById("x"))).toEqual([
      { kind: "body-line", line: 4, element: "h1" },
      { kind: "body-range", startLine: 2, endLine: 7, element: "section.moss-hero" },
    ]);
  });

  it("lists the field, the owning page, then the outer body line", () => {
    document.body.innerHTML =
      '<div data-source-line="9"><article data-source-file="posts/a.md">' +
      '<h3 data-source-fm="title"><b id="t">T</b></h3></article></div>';
    expect(resolveSourceTargets(document.getElementById("t"))).toEqual([
      { kind: "frontmatter", field: "title", path: "posts/a.md", element: "h3" },
      { kind: "page", path: "posts/a.md", element: "article" },
      { kind: "body-line", line: 9, element: "div" },
    ]);
  });

  it("lists a line once when nested elements carry the same one", () => {
    document.body.innerHTML = '<li data-source-line="3"><p id="t" data-source-line="3">x</p></li>';
    expect(resolveSourceTargets(document.getElementById("t"))).toEqual([
      { kind: "body-line", line: 3, element: "p" },
    ]);
  });

  it("returns an empty list outside any annotation", () => {
    document.body.innerHTML = '<p id="t">plain</p>';
    expect(resolveSourceTargets(document.getElementById("t"))).toEqual([]);
    expect(resolveSourceTargets(null)).toEqual([]);
  });
});

function themeSheet(css: string): CSSStyleSheet {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.appendChild(style);
  return style.sheet as CSSStyleSheet;
}

describe("themeSelectors", () => {
  afterEach(() => {
    document.head.innerHTML = "";
    document.body.innerHTML = "";
  });

  it("returns the selectors of the nearest element a rule matches, in sheet order", () => {
    const sheet = themeSheet(
      ".moss-hero { color: green } .moss-hero h1 { color: red } " +
        "@layer themes { h1 { color: blue } }",
    );
    document.body.innerHTML =
      '<section class="moss-hero"><h1><span id="t">x</span></h1></section>';
    expect(themeSelectors(document.getElementById("t"), sheet)).toEqual([
      ".moss-hero h1",
      "h1",
    ]);
  });

  it("descends into @media blocks", () => {
    const sheet = themeSheet("@media (min-width: 1px) { h1 { color: red } }");
    document.body.innerHTML = '<h1 id="t">x</h1>';
    expect(themeSelectors(document.getElementById("t"), sheet)).toEqual(["h1"]);
  });

  it("returns nothing when no rule matches the element or its ancestors", () => {
    const sheet = themeSheet(".moss-hero h1 { color: red }");
    document.body.innerHTML = '<p id="t">x</p>';
    expect(themeSelectors(document.getElementById("t"), sheet)).toEqual([]);
  });

  it("skips a selector the engine rejects instead of throwing", () => {
    const sheet = themeSheet("p:bogus-pseudo { color: red } h1 { color: blue }");
    document.body.innerHTML = '<h1 id="t">x</h1>';
    expect(themeSelectors(document.getElementById("t"), sheet)).toEqual(["h1"]);
  });

  it("reads a style rule that also has cssRules, as engines with CSS nesting give it", () => {
    const rule = { selectorText: "h1", cssRules: [] };
    const sheet = { cssRules: [{ cssRules: [rule] }] } as unknown as CSSStyleSheet;
    document.body.innerHTML = '<h1 id="t">x</h1>';
    expect(themeSelectors(document.getElementById("t"), sheet)).toEqual(["h1"]);
  });

  it("returns nothing when the page has no theme sheet", () => {
    themeSheet("h1 { color: red }");
    document.body.innerHTML = '<h1 id="t">x</h1>';
    expect(themeSelectors(document.getElementById("t"))).toEqual([]);
  });
});
