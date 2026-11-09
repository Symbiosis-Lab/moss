import { describe, expect, it } from "vitest";
import { ScreenHold } from "../screen-hold";
import { OverlaySlot } from "../overlay-slot";

describe("ScreenHold", () => {
  it("reads the overlay lazily, since PreviewManager only has one after initialize()", () => {
    let container: HTMLElement | null = null;
    const hold = new ScreenHold(() => ({ container, slot: new OverlaySlot() }));
    expect(hold.active).toBe(false);
    expect(hold.overlay().container).toBeNull();
    container = document.createElement("div");
    expect(hold.overlay().container).toBe(container);
  });
});
