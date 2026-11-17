// The pieces every waiting screen is drawn from.
import { describe, it, expect } from "vitest";
import { locationLineOf } from "../shell";

describe("locationLineOf", () => {
  it("shows the path once, with no tooltip repeating it", () => {
    const line = locationLineOf("Google Drive › blog");
    expect(line).toContain("Google Drive › blog");
    expect(line).not.toContain("data-tooltip");
  });
});
