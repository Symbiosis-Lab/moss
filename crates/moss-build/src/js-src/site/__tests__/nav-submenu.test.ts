import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

type Registered = [EventTarget, string, EventListenerOrEventListenerObject];
const registered: Registered[] = [];

function render(): { group: HTMLElement; button: HTMLElement; panel: HTMLElement; other: HTMLElement } {
  document.body.innerHTML = `
    <div class="nav-links">
      <span class="nav-group"><a href="/services/">Services</a><button class="nav-sub-toggle" type="button" hidden aria-expanded="false" aria-controls="nav-sub-1"></button><ul class="nav-sub" id="nav-sub-1" hidden><li><a href="/services/patrol/">Patrol</a></li></ul></span>
    </div>
    <p id="other">elsewhere</p>`;
  const q = <T extends HTMLElement>(s: string) => document.querySelector<T>(s)!;
  return { group: q(".nav-group"), button: q(".nav-sub-toggle"), panel: q(".nav-sub"), other: q("#other") };
}

async function load(): Promise<void> {
  vi.resetModules();
  for (const target of [document, window]) {
    const add = target.addEventListener.bind(target);
    vi.spyOn(target, "addEventListener").mockImplementation((type: string, fn: EventListenerOrEventListenerObject) => {
      registered.push([target, type, fn]);
      add(type, fn);
    });
  }
  await import("../nav/submenu");
}

function rectAt(el: HTMLElement, left: number, width: number): void {
  el.getBoundingClientRect = () => ({ left, width, right: left + width, top: 0, bottom: 0, height: 0, x: left, y: 0, toJSON() {} }) as DOMRect;
}

beforeEach(() => {
  Object.defineProperty(document.documentElement, "clientWidth", { configurable: true, value: 321 });
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const [target, type, fn] of registered.splice(0)) target.removeEventListener(type, fn);
});

describe("nav submenu", () => {
  test("the toggle is revealed on load and the list stays closed", async () => {
    const { button, panel } = render();
    await load();
    expect(button.hidden).toBe(false);
    expect(panel.hidden).toBe(true);
  });

  test("click opens, a second click closes, aria-expanded follows", async () => {
    const { group, button, panel } = render();
    await load();
    button.click();
    expect(panel.hidden).toBe(false);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(group.hasAttribute("data-open")).toBe(true);
    button.click();
    expect(panel.hidden).toBe(true);
    expect(button.getAttribute("aria-expanded")).toBe("false");
  });

  test("Escape closes and returns focus to the button", async () => {
    const { button, panel } = render();
    await load();
    button.click();
    panel.querySelector("a")!.focus();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(panel.hidden).toBe(true);
    expect(document.activeElement).toBe(button);
  });

  test("focus leaving the group closes it; focus moving inside does not", async () => {
    const { group, button, panel, other } = render();
    other.tabIndex = 0;
    await load();
    button.click();
    const link = panel.querySelector("a")!;
    link.focus();
    button.focus();
    expect(panel.hidden).toBe(false);
    button.dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: link }));
    expect(group.hasAttribute("data-open")).toBe(true);
    link.dispatchEvent(new FocusEvent("focusout", { bubbles: true, relatedTarget: other }));
    expect(panel.hidden).toBe(true);
  });

  test("a page with no group gets no listeners until a morph adds one", async () => {
    document.body.innerHTML = "<p>none</p>";
    await load();
    expect(registered.some(([, type]) => type === "click")).toBe(false);
    const { button, panel } = render();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));
    expect(button.hidden).toBe(false);
    button.click();
    expect(panel.hidden).toBe(false);
  });

  test("a click outside closes it", async () => {
    const { button, panel, other } = render();
    await load();
    button.click();
    other.click();
    expect(panel.hidden).toBe(true);
  });

  test("opening again clamps a panel that overflows either edge", async () => {
    const { button, panel } = render();
    await load();
    rectAt(panel, 300, 100);
    button.click();
    expect(panel.style.getPropertyValue("--nav-sub-shift")).toBe("-87px");
    button.click();
    rectAt(panel, -20, 100);
    button.click();
    expect(panel.style.getPropertyValue("--nav-sub-shift")).toBe("28px");
  });
});
