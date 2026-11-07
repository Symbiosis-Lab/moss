import { describe, test, expect } from "vitest";
import { isRootUrlPath } from "../url-path";

describe("isRootUrlPath (shared root rule for preview cache + Edit-open resolve)", () => {
  test.each(["", "/", "///", "//"])("root spelling %j → true", (p) => {
    expect(isRootUrlPath(p)).toBe(true);
  });

  test.each(["/about", "/tags/rust/", "/文档/", "/index"])(
    "non-root %j → false",
    (p) => {
      expect(isRootUrlPath(p)).toBe(false);
    }
  );
});
