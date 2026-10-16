/**
 * The viewer finds pictures only when the reader acts on one, and only
 * re-scans when the page gains content other than the viewer's own caption.
 */

import { describe, test, expect, beforeEach, afterEach, vi } from "vitest";
import { loadViewer, unloadViewer } from "./fixtures/load-viewer";

const PAGE = `
  <main>
    <input id="search" type="text">
    <figure class="moss-image"><img src="/a.jpg" alt="One"><figcaption>First</figcaption></figure>
    <figure class="moss-image"><img src="/b.jpg" alt="Two"><figcaption>Second</figcaption></figure>
  </main>`;

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const key = (target: Element, k: string) =>
  target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));

let scans: ReturnType<typeof vi.spyOn>;

beforeEach(async () => {
  await loadViewer(PAGE);
  await tick();
  scans = vi.spyOn(document, "querySelectorAll");
});

afterEach(() => {
  scans.mockRestore();
  unloadViewer();
});

describe("the viewer's idle cost", () => {
  test("typing Space or Enter in a field does not scan the document", () => {
    const field = document.getElementById("search")!;
    key(field, " ");
    key(field, "Enter");
    expect(scans).not.toHaveBeenCalled();
  });

  test("Enter on a picture still opens it", () => {
    key(document.querySelector('img[src="/a.jpg"]')!, "Enter");
    expect((document.getElementById("lightbox") as HTMLElement).hidden).toBe(false);
  });

  test("stepping through pictures does not re-scan the page", async () => {
    document.querySelector<HTMLElement>('img[src="/a.jpg"]')!.click();
    await tick();
    scans.mockClear();
    key(document.body, "ArrowRight");
    await tick();
    expect(scans).not.toHaveBeenCalled();
  });

  test("a picture added later is made reachable", async () => {
    const figure = document.createElement("figure");
    figure.className = "moss-image";
    figure.innerHTML = '<img src="/c.jpg" alt="Three">';
    document.querySelector("main")!.append(figure);
    await tick();
    expect(figure.querySelector("img")!.getAttribute("role")).toBe("button");
  });
});
