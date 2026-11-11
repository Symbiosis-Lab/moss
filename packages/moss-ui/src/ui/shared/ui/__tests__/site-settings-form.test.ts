import { describe, it, expect, beforeEach } from "vitest";
import { drawSiteSettings, translateFields, type SettingTable } from "../site-settings-form";
import type { SiteSettingField as SettingField, SettingValue } from "../setting-field-input";

const SERVICE: SettingField = {
  key: "services",
  type: "map",
  label: "Services",
  add_label: "Add service",
  placeholder: "guided-walk",
  key_pattern: "^[a-z0-9-]{1,40}$",
  items: {
    key: "",
    type: "object",
    fields: [
      { key: "name", type: "string", label: "Name" },
      {
        key: "charge",
        type: "string",
        label: "Charge",
        default: "hourly",
        options: [
          { value: "hourly", label: "By the hour" },
          { value: "fixed", label: "Fixed price" },
        ],
      },
      { key: "per_hour", type: "number", label: "Rate per hour", scale: 100, when: { charge: "hourly" } },
      {
        key: "hours",
        type: "object",
        label: "Hours",
        when: { charge: "hourly" },
        fields: [
          { key: "min", type: "number", label: "From", default: 1 },
          { key: "max", type: "number", label: "To", default: 4 },
        ],
      },
      { key: "minutes", type: "number", label: "Length", unit: "minutes", default: 60, when: { charge: "fixed" } },
      { key: "price", type: "number", label: "Price", scale: 100, default: 0, when: { charge: "fixed" } },
      {
        key: "deposit",
        type: "object",
        label: "Deposit",
        optional: true,
        fields: [{ key: "percent", type: "number", label: "Share", unit: "%", default: 50 }],
      },
    ],
  },
};

const OPEN: SettingField = {
  key: "open",
  type: "object",
  label: "Opening hours",
  fields: [
    {
      key: "mon",
      type: "array",
      label: "Monday",
      items: { key: "", type: "string", pattern: "^\\d\\d:\\d\\d-\\d\\d:\\d\\d$", pattern_message: "Write times as 09:00-17:00." },
    },
  ],
};

let writes: [string[], SettingValue | null][];
let saves: number;
let ran: string[];
let container: HTMLElement;

function draw(values: SettingTable, fields: SettingField[] = [SERVICE, OPEN]) {
  writes = [];
  saves = 0;
  ran = [];
  container = document.createElement("div");
  document.body.replaceChildren(container);
  drawSiteSettings(container, fields, values, {
    write: async (changes) => {
      saves += 1;
      writes.push(...changes);
      return true;
    },
    actions: [{ id: "connect", label: "Connect payments", group: "Payments" }],
    run: async (action) => {
      ran.push(action.id);
    },
  });
}

const flush = () => new Promise((r) => setTimeout(r, 0));
const entry = (id: string) => container.querySelector<HTMLElement>(`[data-entry="services:${id}"]`)!;
const row = (scope: HTMLElement, key: string) => scope.querySelector<HTMLElement>(`[data-field="${key}"]`)!;
const input = (scope: HTMLElement, key: string) => row(scope, key).querySelector<HTMLInputElement>("input")!;
const select = (scope: HTMLElement, key: string) => row(scope, key).querySelector<HTMLSelectElement>("select")!;
const commit = async (el: HTMLInputElement, value: string) => {
  el.value = value;
  el.dispatchEvent(new Event("blur"));
  await flush();
};

describe("site settings form", () => {
  beforeEach(() => {
    document.body.replaceChildren();
  });

  it("shows an amount kept in cents in its units and writes it back in cents, at its own path", async () => {
    draw({ services: { walk: { name: "Walk", per_hour: 12000, hours: { min: 2, max: 6 } } } });
    const rate = input(entry("walk"), "per_hour");
    expect(rate.value).toBe("120.00");
    await commit(rate, "125.5");
    expect(writes).toEqual([[["services", "walk", "per_hour"], 12550]]);
  });

  it("reads the kind of a service saved without one from the keys it holds", () => {
    draw({ services: { call: { name: "Call", minutes: 30, price: 0 } } });
    const card = entry("call");
    expect(select(card, "charge").value).toBe("fixed");
    expect(row(card, "per_hour").hidden).toBe(true);
    expect(row(card, "minutes").hidden).toBe(false);
    expect(row(card, "minutes").textContent).toContain("minutes");
  });

  it("switching the kind writes it, removes the other kind's keys and starts the new ones at their defaults", async () => {
    draw({ services: { call: { name: "Call", minutes: 30, price: 900 } } });
    const choice = select(entry("call"), "charge");
    choice.value = "hourly";
    choice.dispatchEvent(new Event("change"));
    await flush();
    expect(writes).toEqual([
      [["services", "call", "charge"], "hourly"],
      [["services", "call", "minutes"], null],
      [["services", "call", "price"], null],
      [["services", "call", "hours"], { min: 1, max: 4 }],
    ]);
    expect(saves).toBe(1);
    expect(input(entry("call"), "min").value).toBe("1");
    expect(row(entry("call"), "minutes").hidden).toBe(true);
  });

  it("adds a service under a valid new id with its defaults, hinting the id's form, and refuses a bad or taken one", async () => {
    draw({ services: { walk: { name: "Walk", per_hour: 100, hours: { min: 1, max: 2 } } } });
    const add = container.querySelector<HTMLElement>(".moss-site-settings-add")!;
    const id = add.querySelector<HTMLInputElement>("input")!;
    const button = add.querySelector<HTMLButtonElement>("button")!;
    expect(id.placeholder).toBe("guided-walk");
    for (const bad of ["Bad Id", "walk"]) {
      id.value = bad;
      button.click();
      await flush();
    }
    expect(writes).toEqual([]);
    expect(add.textContent).toContain("walk");
    id.value = "tour";
    button.click();
    await flush();
    expect(writes).toEqual([[["services", "tour"], { charge: "hourly", hours: { min: 1, max: 4 } }]]);
    expect(entry("tour")).toBeTruthy();
  });

  it("draws every heading and switch above the rows it introduces, inside a service's card too", () => {
    draw({ services: { walk: { name: "Walk", per_hour: 100, hours: { min: 1, max: 2 }, deposit: { percent: 50 } } } }, [
      { ...SERVICE, group: "Offer" },
    ]);
    const order = (scope: Element) =>
      [...scope.children].map((el) => (el as HTMLElement).dataset.field ?? (el as HTMLElement).className.split(" ")[0] + ":" + el.textContent);
    const card = order(entry("walk").querySelector(".moss-info-grid")!);
    expect(card.indexOf("moss-settings-section-title:Hours")).toBe(card.indexOf("min") - 1);
    expect(card.indexOf("deposit")).toBe(card.indexOf("percent") - 1);
    const top = order(container.querySelector('[data-slot="site-settings-fields"]') ?? container);
    expect(top.indexOf("moss-settings-section-title:Offer")).toBeLessThan(top.indexOf("moss-settings-section-title:Services"));
  });

  it("removes a service by its path", async () => {
    draw({ services: { walk: { name: "Walk", per_hour: 100, hours: { min: 1, max: 2 } } } });
    entry("walk").querySelector<HTMLButtonElement>(".moss-site-settings-entry__head button")!.click();
    await flush();
    expect(writes).toEqual([[["services", "walk"], null]]);
    expect(container.querySelector('[data-entry="services:walk"]')).toBeNull();
  });

  it("an optional table is added with its defaults and removed whole", async () => {
    draw({ services: { walk: { name: "Walk", per_hour: 100, hours: { min: 1, max: 2 } } } });
    expect(row(entry("walk"), "percent").hidden).toBe(true);
    const sw = () => row(entry("walk"), "deposit").querySelector<HTMLElement>('[role="switch"]')!;
    sw().click();
    await flush();
    expect(writes).toEqual([[["services", "walk", "deposit"], { percent: 50 }]]);
    expect(row(entry("walk"), "percent").hidden).toBe(false);
    sw().click();
    await flush();
    expect(writes[1]).toEqual([["services", "walk", "deposit"], null]);
  });

  it("a list of times is written as a list, and a malformed one is refused with the manifest's words", async () => {
    draw({ open: { mon: ["09:00-17:00"] } });
    const monday = input(container, "mon");
    expect(monday.value).toBe("09:00-17:00");
    await commit(monday, "10:00-12:00, 9-5");
    expect(writes).toEqual([]);
    expect(row(container, "mon").textContent).toContain("Write times as 09:00-17:00.");
    await commit(monday, "10:00-12:00, 13:00-18:00");
    expect(writes).toEqual([[["open", "mon"], ["10:00-12:00", "13:00-18:00"]]]);
    await commit(monday, "");
    expect(writes[1]).toEqual([["open", "mon"], []]);
  });
});

describe("site settings actions", () => {
  it("draws each action under its group, after the fields, and runs it on click", async () => {
    draw({});
    const button = container.querySelector<HTMLButtonElement>('[data-action="connect"]')!;
    expect(button.textContent).toBe("Connect payments");
    expect(container.lastElementChild!.contains(button)).toBe(true);
    button.click();
    await flush();
    expect(ran).toEqual(["connect"]);
  });
});

describe("translateFields", () => {
  it("draws the manifest's words in the reader's language, falling back to the base locale and then English", () => {
    const fields = translateFields([SERVICE], { zh: { Services: "服务", "By the hour": "按小时" } }, "zh-hans");
    expect(fields[0]!.label).toBe("服务");
    expect(fields[0]!.items!.fields![1]!.options![0]!.label).toBe("按小时");
    expect(fields[0]!.items!.fields![0]!.label).toBe("Name");
  });
});
