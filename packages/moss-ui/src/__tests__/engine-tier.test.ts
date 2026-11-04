import { describe, expect, it } from "vitest";
import { engineTier, isEngineCommand } from "../index.ts";

describe("engineTier", () => {
  it("answers each engine command's tier", () => {
    expect(engineTier("scan_shortcodes")).toBe("public_read");
    expect(engineTier("editor_bootstrap")).toBe("session_read");
    expect(engineTier("save_editor_content")).toBe("write");
  });

  it("answers undefined for a host's own command, so it is never sent to the engine", () => {
    expect(isEngineCommand("reveal_entry")).toBe(false);
    expect(engineTier("reveal_entry")).toBeUndefined();
    expect(engineTier("toString")).toBeUndefined();
  });
});
