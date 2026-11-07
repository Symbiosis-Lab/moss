/**
 * Unit tests for the Toggle primitive (shared/ui/toggle.ts).
 *
 * Tests behaviour, not implementation details:
 * - Rendering: label, description, role, aria-checked
 * - Initial state reflected on switch element
 * - Click toggles checked state and fires onChange
 * - Space key toggles
 * - Enter key toggles
 * - Disabled prevents toggle and keeps aria state unchanged
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { createToggle, setToggleChecked } from "../toggle";

describe("createToggle", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  // --- Rendering ---

  it("renders the label text", () => {
    const el = createToggle({ label: "Enable notifications", checked: false, onChange: vi.fn() });
    const labelEl = el.querySelector(".moss-toggle__label");
    expect(labelEl?.textContent).toBe("Enable notifications");
  });

  it("renders description when provided", () => {
    const el = createToggle({
      label: "Dark mode",
      description: "Switch to dark theme",
      checked: false,
      onChange: vi.fn(),
    });
    const descEl = el.querySelector(".moss-toggle__description");
    expect(descEl?.textContent).toBe("Switch to dark theme");
  });

  it("does not render description element when omitted", () => {
    const el = createToggle({ label: "Feature flag", checked: false, onChange: vi.fn() });
    expect(el.querySelector(".moss-toggle__description")).toBeNull();
  });

  it("renders no .moss-toggle__body when neither label nor description is given, and still toggles", () => {
    const onChange = vi.fn();
    const el = createToggle({ checked: false, onChange });
    expect(el.querySelector(".moss-toggle__body")).toBeNull();
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("has role='switch' on the switch element", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn() });
    const sw = el.querySelector(".moss-toggle__switch");
    expect(sw?.getAttribute("role")).toBe("switch");
  });

  // --- Initial state ---

  it("reflects checked=false with aria-checked='false' and no active class", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn() });
    const sw = el.querySelector(".moss-toggle__switch")!;
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.classList.contains("active")).toBe(false);
  });

  it("reflects checked=true with aria-checked='true' and active class", () => {
    const el = createToggle({ label: "X", checked: true, onChange: vi.fn() });
    const sw = el.querySelector(".moss-toggle__switch")!;
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(sw.classList.contains("active")).toBe(true);
  });

  // --- Click interaction ---

  it("click toggles from false to true and calls onChange(true)", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("click toggles from true to false and calls onChange(false)", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: true, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("consecutive clicks toggle state correctly", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenLastCalledWith(false);
  });

  // --- Keyboard ---

  it("Space key toggles the switch and calls onChange", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("Enter key toggles the switch and calls onChange", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("unrelated key does not toggle", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(onChange).not.toHaveBeenCalled();
  });

  // --- Disabled ---

  it("disabled toggle has .moss-toggle--disabled on root", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn(), disabled: true });
    expect(el.classList.contains("moss-toggle--disabled")).toBe(true);
  });

  it("disabled toggle does not fire onChange on click", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange, disabled: true });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onChange).not.toHaveBeenCalled();
    expect(sw.getAttribute("aria-checked")).toBe("false");
  });

  it("disabled toggle does not fire onChange on Space key", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange, disabled: true });
    const sw = el.querySelector(".moss-toggle__switch")!;
    sw.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("disabled switch has aria-disabled='true'", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn(), disabled: true });
    const sw = el.querySelector(".moss-toggle__switch")!;
    expect(sw.getAttribute("aria-disabled")).toBe("true");
  });

  it("disabled switch has tabindex='-1'", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn(), disabled: true });
    const sw = el.querySelector(".moss-toggle__switch")!;
    expect(sw.getAttribute("tabindex")).toBe("-1");
  });

  it("enabled switch has tabindex='0'", () => {
    const el = createToggle({ label: "X", checked: false, onChange: vi.fn() });
    const sw = el.querySelector(".moss-toggle__switch")!;
    expect(sw.getAttribute("tabindex")).toBe("0");
  });
});

describe("setToggleChecked", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  // PROVEN regression: reverting by repainting the switch DOM from outside
  // left the toggle's internal isChecked on the failed value, so the NEXT
  // click computed from stale state — a visual no-op that re-persisted the
  // value already on disk. The setter is the one place state + DOM move
  // together.
  it("updates state AND DOM together — the next click computes from the set value", () => {
    const onChange = vi.fn();
    const el = createToggle({ label: "X", checked: false, onChange });
    document.body.appendChild(el);
    const sw = el.querySelector(".moss-toggle__switch") as HTMLElement;

    sw.click(); // user turns it ON
    expect(onChange).toHaveBeenLastCalledWith(true);

    setToggleChecked(el, false); // programmatic revert (failed save)
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.classList.contains("active")).toBe(false);
    expect(onChange).toHaveBeenCalledTimes(1); // a revert is not a user action

    sw.click(); // user tries again — must compute from the REVERTED state
    expect(onChange).toHaveBeenLastCalledWith(true);
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(sw.classList.contains("active")).toBe(true);
  });

  it("is a safe no-op on an element no toggle was created for", () => {
    expect(() => setToggleChecked(document.createElement("div"), true)).not.toThrow();
  });
});
