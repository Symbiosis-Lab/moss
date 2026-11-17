// The pieces every waiting screen is drawn from.
import { describe, it, expect } from "vitest";
import { locationLineOf, meter } from "../shell";

describe("locationLineOf", () => {
  it("shows the path once, with no tooltip repeating it", () => {
    const line = locationLineOf("Google Drive › blog");
    expect(line).toContain("Google Drive › blog");
    expect(line).not.toContain("data-tooltip");
  });
});

describe("meter", () => {
  it("draws nothing across the meter at 0 of M, only an empty fill", () => {
    const host = document.createElement("div");
    host.innerHTML = meter("live", { value: 0, max: 165 });
    const lines = [...host.firstElementChild!.children] as HTMLElement[];
    expect(lines.map((line) => [line.className, line.style.width])).toEqual([["moss-waiting-screen__meter-fill", "0%"]]);
  });
});
