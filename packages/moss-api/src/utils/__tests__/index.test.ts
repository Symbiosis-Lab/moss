import { describe, it, expect } from "vitest";
import * as utils from "../index.js";

describe("Utils Exports", () => {
  it("should not export deprecated logger functions", () => {
    // These functions are deprecated and should not be exported from the main utils module
    expect(utils).not.toHaveProperty("log");
    expect(utils).not.toHaveProperty("warn");
    expect(utils).not.toHaveProperty("error");
  });

  it("should still export core utilities", () => {
    // Verify that other exports are still present
    expect(utils).toHaveProperty("sendMessage");
    expect(utils).toHaveProperty("openBrowser");
    expect(utils).toHaveProperty("readFile");
  });
});
