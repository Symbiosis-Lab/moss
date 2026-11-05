/**
 * Unit tests for the pill-select / pill-input factories
 */

import { describe, it, expect, vi } from "vitest";
import { createPillSelect, createPillInput } from "../pill-select";

describe("createPillSelect", () => {
  it("emits the base pill classes plus any extraClass", () => {
    const select = createPillSelect({ options: [], extraClass: "moss-attachments__mode" });
    expect(select.tagName).toBe("SELECT");
    expect(select.className).toBe("moss-select moss-select--pill moss-attachments__mode");
  });

  it("builds one <option> per entry and selects the given value", () => {
    const select = createPillSelect({
      options: [
        { value: "a", label: "Option A" },
        { value: "b", label: "Option B" },
      ],
      value: "b",
    });
    expect(select.options.length).toBe(2);
    expect(select.options[1].textContent).toBe("Option B");
    expect(select.value).toBe("b");
  });

  it("fires onChange with the new value on 'change'", () => {
    const onChange = vi.fn();
    const select = createPillSelect({
      options: [
        { value: "a", label: "A" },
        { value: "b", label: "B" },
      ],
      value: "a",
      onChange,
    });
    select.value = "b";
    select.dispatchEvent(new Event("change"));
    expect(onChange).toHaveBeenCalledWith("b");
  });
});

describe("createPillInput", () => {
  it("emits .moss-pill-input plus any extraClass", () => {
    const input = createPillInput({ extraClass: "moss-domain-pill-input" });
    expect(input.tagName).toBe("INPUT");
    expect(input.className).toBe("moss-pill-input moss-domain-pill-input");
  });

  it("defaults to type=text and accepts type=password", () => {
    expect(createPillInput({}).type).toBe("text");
    expect(createPillInput({ type: "password" }).type).toBe("password");
  });
});
