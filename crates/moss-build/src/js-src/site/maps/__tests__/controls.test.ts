import { expect, test } from "vitest";
import { buildControls } from "../controls";

test("builds one labelled group: the zoom capsule over the reset circle", () => {
  const c = buildControls({ group: "Map controls", zoomIn: "Zoom in", zoomOut: "Zoom out", reset: "Show the whole map" });
  expect(c.element.getAttribute("aria-label")).toBe("Map controls");
  expect(c.element.getAttribute("role")).toBe("group");
  expect([...c.element.children].map((el) => el.className)).toEqual(["moss-places-capsule", "moss-places-control"]);
  expect(c.zoomIn.parentElement!.className).toBe("moss-places-capsule");
  expect([c.zoomIn, c.zoomOut, c.reset].map((b) => [b.dataset.control, b.getAttribute("aria-label"), b.type])).toEqual([
    ["zoom-in", "Zoom in", "button"],
    ["zoom-out", "Zoom out", "button"],
    ["reset", "Show the whole map", "button"],
  ]);
});
