/**
 * Unit tests for StateMessage primitive (shared/ui/state-message.ts).
 *
 * Tests behaviour, not implementation details:
 * - Title always rendered
 * - Optional description, icon, and action button render when provided
 * - Action button fires onClick
 * - Variant class applied to root
 * - Default variant is "empty"
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { createStateMessage } from "../state-message";

describe("createStateMessage", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  // --- Title ---

  it("renders the title text", () => {
    const el = createStateMessage({ title: "No data found" });
    const titleEl = el.querySelector(".moss-state__title");
    expect(titleEl?.textContent).toBe("No data found");
  });

  // --- Optional elements ---

  it("renders description when provided", () => {
    const el = createStateMessage({ title: "X", description: "Try again later." });
    const descEl = el.querySelector(".moss-state__description");
    expect(descEl?.textContent).toBe("Try again later.");
  });

  it("does not render description when omitted", () => {
    const el = createStateMessage({ title: "X" });
    expect(el.querySelector(".moss-state__description")).toBeNull();
  });

  it("renders icon when provided", () => {
    const el = createStateMessage({ title: "X", icon: "📭" });
    const iconEl = el.querySelector(".moss-state__icon");
    expect(iconEl?.textContent).toBe("📭");
  });

  it("does not render icon when omitted", () => {
    const el = createStateMessage({ title: "X" });
    expect(el.querySelector(".moss-state__icon")).toBeNull();
  });

  it("icon has aria-hidden='true'", () => {
    const el = createStateMessage({ title: "X", icon: "⚠" });
    const iconEl = el.querySelector(".moss-state__icon");
    expect(iconEl?.getAttribute("aria-hidden")).toBe("true");
  });

  // --- Action button ---

  it("renders action button with correct label", () => {
    const el = createStateMessage({ title: "X", action: { label: "Retry", onClick: vi.fn() } });
    const btn = el.querySelector(".moss-state__action button");
    expect(btn?.textContent).toBe("Retry");
  });

  it("action button fires onClick when clicked", () => {
    const onClick = vi.fn();
    const el = createStateMessage({ title: "X", action: { label: "Go", onClick } });
    const btn = el.querySelector<HTMLButtonElement>(".moss-state__action button")!;
    btn.click();
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("does not render action section when omitted", () => {
    const el = createStateMessage({ title: "X" });
    expect(el.querySelector(".moss-state__action")).toBeNull();
  });

  // --- Variants ---

  it("applies moss-state--empty class by default", () => {
    const el = createStateMessage({ title: "X" });
    expect(el.classList.contains("moss-state--empty")).toBe(true);
  });

  it("applies moss-state--loading class for loading variant", () => {
    const el = createStateMessage({ title: "X", variant: "loading" });
    expect(el.classList.contains("moss-state--loading")).toBe(true);
  });

  it("applies moss-state--error class for error variant", () => {
    const el = createStateMessage({ title: "X", variant: "error" });
    expect(el.classList.contains("moss-state--error")).toBe(true);
  });

  it("root element always has .moss-state class", () => {
    const el = createStateMessage({ title: "X", variant: "error" });
    expect(el.classList.contains("moss-state")).toBe(true);
  });

  // --- DOM structure ---

  it("title comes before description in DOM order", () => {
    const el = createStateMessage({ title: "Title", description: "Desc" });
    const children = Array.from(el.children);
    const titleIdx = children.findIndex(c => c.classList.contains("moss-state__title"));
    const descIdx = children.findIndex(c => c.classList.contains("moss-state__description"));
    expect(titleIdx).toBeLessThan(descIdx);
  });

  it("icon comes before title in DOM order when both present", () => {
    const el = createStateMessage({ title: "Title", icon: "X" });
    const children = Array.from(el.children);
    const iconIdx = children.findIndex(c => c.classList.contains("moss-state__icon"));
    const titleIdx = children.findIndex(c => c.classList.contains("moss-state__title"));
    expect(iconIdx).toBeLessThan(titleIdx);
  });
});
