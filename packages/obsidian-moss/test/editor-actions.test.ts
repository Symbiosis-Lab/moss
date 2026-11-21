import { describe, expect, it } from "vitest";
import { relativePath } from "../src/editor-actions";
import { publishLabel } from "../src/desktop/publish-state";

describe("relativePath", () => {
  it("writes the image as seen from the page's folder", () => {
    expect(relativePath("posts/heron.md", "posts/img/heron.png")).toBe("img/heron.png");
    expect(relativePath("posts/heron.md", "img/heron.png")).toBe("../img/heron.png");
    expect(relativePath("heron.md", "img/heron.png")).toBe("img/heron.png");
    expect(relativePath("a/b/p.md", "a/c/x.png")).toBe("../c/x.png");
  });
});

describe("publishLabel", () => {
  const page = { path: "/walk/", draft: false, published: true, changed: true, channels: [], copies: {} };
  it("says Draft before anything else", () => expect(publishLabel({ ...page, draft: true, published: false })).toBe("Draft"));
  it("says Not published for a page that never went out", () => expect(publishLabel({ ...page, published: false })).toBe("Not published"));
  it("says Changed since publish only when the source moved on", () => {
    expect(publishLabel(page)).toBe("Changed since publish");
    expect(publishLabel({ ...page, changed: false })).toBe("");
  });
  it("says nothing when moss cannot tell", () => {
    expect(publishLabel({ ...page, published: undefined, changed: undefined })).toBe("");
    expect(publishLabel({ ...page, changed: undefined })).toBe("");
  });
});
