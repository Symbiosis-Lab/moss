/**
 * Source-target resolution for the preview bridge: which annotations an
 * element maps to, nearest first, and the owning file each one carries.
 */
import { describe, it, expect } from "vitest";
import { resolveSourceTargets } from "../source-target";

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
