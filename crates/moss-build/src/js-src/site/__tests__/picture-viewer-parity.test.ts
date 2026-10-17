/**
 * The build puts the viewer's markup on a page that might have a picture and
 * leaves the exact rules to fullscreen.js. This runs the shipped script over
 * the real build output of each case and checks which pictures open.
 *
 * The pages are `<main>` fixtures written by the moss-cli test
 * `picture_viewer_test`, which fails when they go stale; regenerate with
 * `SNAPSHOTS=overwrite cargo test -p moss-cli --test picture_viewer_test`.
 */

import { describe, test, expect, afterAll } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadViewer, unloadViewer } from "./fixtures/load-viewer";

const OPENS: Record<string, string[]> = {
  figure: ["/a.png"],
  "caption-only": ["/a.png"],
  "lone-alt": ["/a.png"],
  "grid-cell": ["/a.png", "/b.png"],
  gallery: ["/a.png", "/b.png"],
  // Inside a link, in a hero, decorative, or marked `no-viewer`.
  linked: [],
  hero: [],
  decorative: [],
  "opted-out": [],
  "opted-out-grid": [],
};

afterAll(unloadViewer);

describe("which pictures of a built page open in the viewer", () => {
  test.each(Object.entries(OPENS))("%s", async (name, expected) => {
    const main = readFileSync(resolve(__dirname, `fixtures/picture-viewer/${name}.html`), "utf8");
    await loadViewer(main);
    const stayOnPage = (e: Event) => e.preventDefault();
    document.addEventListener("click", stayOnPage);
    const opened: string[] = [];
    for (const img of Array.from(document.querySelectorAll("main img"))) {
      img.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
      const lightbox = document.getElementById("lightbox");
      if (lightbox && !lightbox.hidden) {
        opened.push(img.getAttribute("src")!);
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      }
    }
    document.removeEventListener("click", stayOnPage);
    expect(opened).toEqual(expected);
  });
});
