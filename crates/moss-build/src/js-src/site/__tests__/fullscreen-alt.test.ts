/**
 * The full-screen viewer's image alt and caption, for a gallery item.
 *
 * - An alt-only item (`![Description|](a.jpg)`) has no data-title, so the
 *   caption is hidden, but the viewer's <img> keeps the thumbnail's alt.
 * - A plain item keeps its title as the caption, and the alt is the same text.
 */

import { describe, test, expect, beforeEach, afterAll } from "vitest";
import { loadViewer, unloadViewer } from "./fixtures/load-viewer";

function buildPage(linkAttrs: string, thumbAlt: string): void {
  document.body.innerHTML = `
    <div id="lightbox" hidden>
      <div class="lightbox-content">
        <img class="lightbox-image" hidden>
      </div>
      <p class="lightbox-title"></p>
    </div>
    <div class="moss-gallery">
      <div class="moss-gallery-item">
        <a class="moss-gallery-link" href="a.jpg" ${linkAttrs}>
          <img src="a.jpg" alt="${thumbAlt}">
        </a>
      </div>
    </div>`;
}

async function openFirstItem(): Promise<{
  image: HTMLImageElement;
  title: HTMLElement;
}> {
  await loadViewer(document.body.innerHTML);
  (document.querySelector(".moss-gallery-link") as HTMLElement).click();
  return {
    image: document.querySelector(".lightbox-image") as HTMLImageElement,
    title: document.querySelector(".lightbox-title") as HTMLElement,
  };
}

afterAll(unloadViewer);

describe("fullscreen viewer alt and caption", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  test("an alt-only item shows no caption and keeps its alt on the viewer image", async () => {
    buildPage("", "Two dancers on a pink stage");
    const { image, title } = await openFirstItem();
    expect(image.alt).toBe("Two dancers on a pink stage");
    expect(title.textContent).toBe("");
    expect(title.hidden).toBe(true);
  });

  test("a plain item keeps its title as the caption and as the alt", async () => {
    buildPage('data-title="Two dancers"', "Two dancers");
    const { image, title } = await openFirstItem();
    expect(image.alt).toBe("Two dancers");
    expect(title.textContent).toBe("Two dancers");
    expect(title.hidden).toBe(false);
  });
});
