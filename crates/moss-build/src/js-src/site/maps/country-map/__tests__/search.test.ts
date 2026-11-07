/** The search field is an ARIA combobox that chooses a country the way a tap does. */
import { beforeEach, describe, expect, test, vi } from "vitest";
import { $, boot, INDEX, media, page, panel } from "./fixture";

const input = () => $<HTMLInputElement>(".moss-country-search input");
const listbox = () => $(".moss-country-search [role=listbox]");
const options = () => [...document.querySelectorAll<HTMLElement>(".moss-country-search [role=option]")];
const status = () => $(".moss-country-search [role=status]");
const type = (text: string) => {
  input().value = text;
  input().dispatchEvent(new Event("input", { bubbles: true }));
};
const key = (k: string) => input().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));

let fetched: string[];

beforeEach(async () => {
  page();
  location.hash = "";
  media({});
  fetched = [];
  globalThis.fetch = vi.fn(async (url: string) => {
    fetched.push(url);
    return { ok: true, json: async () => INDEX } as Response;
  }) as unknown as typeof fetch;
  await boot();
});

describe("the field", () => {
  test("replaces the no-script link with a combobox wired to a listbox", () => {
    expect(document.querySelector("a.moss-country-search")).toBeNull();
    expect(input().getAttribute("role")).toBe("combobox");
    expect(input().getAttribute("aria-expanded")).toBe("false");
    expect(input().getAttribute("aria-controls")).toBe(listbox().id);
    expect(input().placeholder).toBe("Search a country or city");
    expect(input().getAttribute("aria-label")).toBe("Search a country or city");
  });
  test("fetches the index on first focus only", async () => {
    expect(fetched).toEqual([]);
    input().dispatchEvent(new FocusEvent("focus"));
    input().dispatchEvent(new FocusEvent("focus"));
    await vi.waitFor(() => expect(fetched).toEqual(["/_moss/map.x/country-search.json"]));
  });
  test("a page without search has no field", async () => {
    page();
    document.querySelector("a.moss-country-search")!.remove();
    await boot();
    expect(document.querySelector(".moss-country-search")).toBeNull();
  });
});

describe("a failed index fetch", () => {
  test("is tried again on the next focus", async () => {
    let calls = 0;
    globalThis.fetch = vi.fn(async () => {
      calls++;
      throw new Error("offline");
    }) as unknown as typeof fetch;
    page();
    await boot();
    input().dispatchEvent(new FocusEvent("focus"));
    await vi.waitFor(() => expect(calls).toBe(1));
    await new Promise((r) => setTimeout(r));
    input().dispatchEvent(new FocusEvent("focus"));
    await vi.waitFor(() => expect(calls).toBe(2));
  });
});

describe("focus", () => {
  const heading = () => document.querySelector<HTMLElement>(".moss-country-panel-name");
  beforeEach(() => {
    input().focus();
  });
  test("a pick by Enter moves focus to the panel heading, and clearing returns it to the field", () => {
    type("mex");
    key("Enter");
    expect(document.activeElement).toBe(heading());
    expect(heading()!.getAttribute("tabindex")).toBe("-1");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.activeElement).toBe(input());
  });
  test("a pick by click leaves focus alone", () => {
    type("japan");
    options()[0].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(document.activeElement).not.toBe(heading());
    expect(panel().hidden).toBe(false);
  });
});

describe("results", () => {
  beforeEach(async () => {
    input().dispatchEvent(new FocusEvent("focus"));
    await vi.waitFor(() => expect(fetched).toHaveLength(1));
    await Promise.resolve();
  });
  test("show swatch, name and level, with the first row highlighted", () => {
    type("mex");
    expect(options().map((o) => o.textContent)).toEqual(["MexicoHigh", "Mexico City, MexicoHigh"]);
    expect(options()[0].querySelector("[data-level]")!.getAttribute("data-level")).toBe("2");
    expect(options()[0].getAttribute("aria-selected")).toBe("true");
    expect(input().getAttribute("aria-activedescendant")).toBe(options()[0].id);
    expect(input().getAttribute("aria-expanded")).toBe("true");
    expect(listbox().hidden).toBe(false);
    expect(status().textContent).toBe("2 results");
  });
  test("an unrated country says so, and a city names its country", () => {
    type("chad");
    expect(options()[0].textContent).toBe("ChadNot rated");
    type("tokyo");
    expect(options()[0].textContent).toBe("Tokyo, JapanLow");
    expect(status().textContent).toBe("1 result");
  });
  test("arrows move the highlight and wrap", () => {
    type("m");
    expect(options().map((o) => o.textContent)).toEqual(["MexicoHigh", "Mexico City, MexicoHigh"]);
    const active = () => options().findIndex((o) => o.getAttribute("aria-selected") === "true");
    key("ArrowDown");
    expect(active()).toBe(1);
    key("ArrowDown");
    expect(active()).toBe(0);
    key("ArrowUp");
    expect(active()).toBe(1);
    expect(input().getAttribute("aria-activedescendant")).toBe(options()[1].id);
  });
  test("Enter chooses the highlighted row exactly as a tap does", () => {
    type("mex");
    key("Enter");
    expect(panel().hidden).toBe(false);
    expect(panel().textContent).toContain("Night travel.");
    expect(document.getElementById("mexico")!.classList.contains("is-chosen")).toBe(true);
    expect(location.hash).toBe("#mexico");
    expect(listbox().hidden).toBe(true);
    expect(input().getAttribute("aria-expanded")).toBe("false");
  });
  test("a click on a row chooses it too", () => {
    type("japan");
    options()[0].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(panel().textContent).toContain("Take normal precautions.");
    expect(location.hash).toBe("#japan");
  });
  test("Esc closes the list first and clears the country only the next time", () => {
    document.getElementById("japan")!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    type("mex");
    key("Escape");
    expect(listbox().hidden).toBe(true);
    expect(panel().hidden).toBe(false);
    key("Escape");
    expect(panel().hidden).toBe(true);
  });
  test("no match is said in words and links to the list", () => {
    type("zzz");
    const none = $(".moss-country-search-none");
    expect(none.hidden).toBe(false);
    expect(none.textContent).toBe("Nothing matches “zzz”. See the list.");
    expect(none.querySelector("a")!.getAttribute("href")).toBe("#moss-country-rows");
    expect(listbox().hidden).toBe(true);
    expect(input().getAttribute("aria-expanded")).toBe("false");
    key("Enter");
    expect(panel().hidden).toBe(true);
  });
  test("an empty field closes everything", () => {
    type("mex");
    type("");
    expect(listbox().hidden).toBe(true);
    expect($(".moss-country-search-none").hidden).toBe(true);
  });
});
