import { describe, it, expect } from "vitest";

import { pageUrl } from "../page-url";

describe("pageUrl", () => {
  it("joins a bare host and a slash-less path with exactly one slash", () => {
    expect(pageUrl("example.mosspub.com", "community/blog/slug/")).toBe(
      "https://example.mosspub.com/community/blog/slug/",
    );
  });

  it("does not double the slash when the host already ends with one", () => {
    expect(pageUrl("example.mosspub.com/", "blog/")).toBe("https://example.mosspub.com/blog/");
  });

  it("does not double the slash when the path already starts with one", () => {
    expect(pageUrl("example.mosspub.com", "/blog/")).toBe("https://example.mosspub.com/blog/");
  });
});
