import { describe, expect, it } from "vitest";
import { parsePortInput } from "../src/settings";

describe("parsePortInput", () => {
  it("accepts valid TCP ports", () => {
    expect(parsePortInput("8080")).toBe(8080);
    expect(parsePortInput(" 1 ")).toBe(1);
    expect(parsePortInput("65535")).toBe(65535);
  });

  it("rejects junk", () => {
    expect(parsePortInput("")).toBeNull();
    expect(parsePortInput("0")).toBeNull();
    expect(parsePortInput("65536")).toBeNull();
    expect(parsePortInput("80a")).toBeNull();
    expect(parsePortInput("-1")).toBeNull();
  });
});
