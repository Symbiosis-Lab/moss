import { describe, it, expect } from "vitest";

import {
  DRAG_REGION_ATTR,
  markDragRegion,
  createDragSpacer,
} from "../window-drag";

describe("window-drag", () => {
  it("DRAG_REGION_ATTR is Tauri's drag-region attribute name", () => {
    expect(DRAG_REGION_ATTR).toBe("data-tauri-drag-region");
  });

  it("markDragRegion opts an element INTO window dragging (allowlist)", () => {
    const el = document.createElement("div");
    markDragRegion(el);
    expect(el.hasAttribute(DRAG_REGION_ATTR)).toBe(true);
    // Value is the empty string, matching Tauri's convention.
    expect(el.getAttribute(DRAG_REGION_ATTR)).toBe("");
  });

  it("createDragSpacer builds a draggable flex spacer with the given class", () => {
    const spacer = createDragSpacer("moss-x__titlebar-spacer");
    expect(spacer.tagName).toBe("DIV");
    expect(spacer.className).toBe("moss-x__titlebar-spacer");
    expect(spacer.hasAttribute(DRAG_REGION_ATTR)).toBe(true);
  });
});
