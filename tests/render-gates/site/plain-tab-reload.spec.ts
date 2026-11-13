/**
 * The preview server puts its bridge script into every page it serves. Inside
 * moss the page is framed and its parent refreshes it, opens its links and
 * draws its menu. In a plain browser tab there is no parent, so the page must
 * refresh itself and must not hold the keys, the menu or the links for one.
 *
 * Server: the real `moss-cli build --serve --watch` over a scratch site this
 * spec edits. Config: playwright/plain-tab-reload.config.ts.
 *
 *   npx playwright test -c playwright/plain-tab-reload.config.ts
 */
import { test, expect, type Frame, type Page } from "@playwright/test";
import { writeFileSync } from "node:fs";
import path from "node:path";
import { gatePort } from "../../../playwright/gate-ports";
import { scratchSiteDir } from "../../e2e/helpers/scratch-site";
import { gotoFramed } from "../../support/framed-page";
import { PLAIN_TAB_RELOAD } from "../../e2e/helpers/gate-sites";

const BASE = `http://localhost:${gatePort("plain-tab-reload")}/`;
const SOURCE = path.join(scratchSiteDir(PLAIN_TAB_RELOAD.name), "index.md");
const INTERNAL_LINK = 'a[href*="other"]';

/** Whether the page's own script kept each gesture from the browser. */
async function gesturesHeld(target: Page | Frame) {
  return target.evaluate((INTERNAL_LINK) => {
    const held = (event: Event, on: EventTarget) => {
      // The bridge listens on the document as the event goes down. It holds a
      // gesture by cancelling it, by stopping it there, or both; the window
      // hears on the way back up only what was not stopped.
      let kept = true;
      const record = (e: Event) => {
        kept = e.defaultPrevented;
        e.preventDefault();
      };
      window.addEventListener(event.type, record, { once: true });
      on.dispatchEvent(event);
      window.removeEventListener(event.type, record);
      return kept;
    };
    return {
      reloadKey: held(new KeyboardEvent("keydown", { key: "r", metaKey: true, ctrlKey: true, bubbles: true, cancelable: true }), document.body),
      menu: held(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }), document.querySelector("p")!),
      externalLink: held(new MouseEvent("click", { bubbles: true, cancelable: true }), document.querySelector('a[href^="https://example.com"]')!),
      internalLink: held(new MouseEvent("click", { bubbles: true, cancelable: true }), document.querySelector(INTERNAL_LINK)!),
    };
  }, INTERNAL_LINK);
}

/** Replace the page's first paragraph in the source and return the new text. */
function edit(label: string): string {
  const text = `Edited in ${label} at ${Date.now()}.`;
  writeFileSync(SOURCE, PLAIN_TAB_RELOAD.files["index.md"].replace("First text.", text));
  return text;
}

test("a plain tab shows an edit without being reloaded by hand", async ({ page }, testInfo) => {
  const stream = page.waitForRequest((r) => r.url().endsWith("/__moss/reload"));
  await page.goto(BASE);
  await stream;
  const text = edit(testInfo.project.name);
  await expect(page.locator("body")).toContainText(text, { timeout: 30_000 });
});

test("a plain tab keeps the browser's reload key, menu and links", async ({ page }) => {
  await page.goto(BASE);
  expect(await gesturesHeld(page)).toEqual({ reloadKey: false, menu: false, externalLink: false, internalLink: false });
  await page.click("[data-viewer]");
  await expect(page.locator("#lightbox")).toBeVisible();
});

test("a plain tab opens an internal link in a new tab when asked to", async ({ page, context }) => {
  await page.goto(BASE);
  const opened = context.waitForEvent("page", { timeout: 15_000 });
  await page.click(INTERNAL_LINK, { modifiers: ["ControlOrMeta"] });
  const other = await opened;
  await expect(other.locator("body")).toContainText("Second page.");
  expect(page.url()).toBe(BASE);
});

// A browser gives one origin six connections over plain HTTP. A stream held
// by every tab would use them up, and the seventh tab would wait for a page.
test("seven tabs of one browser all load, and each follows an edit by reloading once", async ({ context }, testInfo) => {
  const pages: Page[] = [];
  const loads: number[] = [];
  for (let tab = 0; tab < 7; tab++) {
    const page = await context.newPage();
    loads.push(0);
    page.on("load", () => loads[tab]++);
    await page.goto(BASE, { timeout: 10_000 });
    pages.push(page);
  }
  const settled = async (open: number[], expected: number) => {
    await pages[open[0]].waitForTimeout(3_000);
    expect(open.map((tab) => loads[tab])).toEqual(open.map(() => expected));
  };
  const all = pages.map((_, tab) => tab);
  /** Which of the `open` tabs holds the lock the stream goes with, or -1. */
  const holder = async (open: number[]) => {
    for (const tab of open) {
      const holds = await pages[tab].evaluate(
        (probe) =>
          navigator.locks.request(probe, async () => {
            const { held = [] } = await navigator.locks.query();
            const me = held.find((lock) => lock.name === probe)?.clientId;
            return held.some((lock) => lock.name === "moss-reload" && lock.clientId === me);
          }),
        `probe-${tab}`,
      );
      if (holds) return tab;
    }
    return -1;
  };

  let text = edit(`${testInfo.project.name}, seven tabs`);
  for (const page of pages) await expect(page.locator("body")).toContainText(text, { timeout: 30_000 });
  await settled(all, 2);

  // The tab holding the stream goes away: another takes it up, and none is left behind.
  const closed = await holder(all);
  expect(closed).not.toBe(-1);
  await pages[closed].close();
  const rest = all.filter((tab) => tab !== closed);
  await expect.poll(() => holder(rest)).not.toBe(-1);
  await settled(rest, 2);
  text = edit(`${testInfo.project.name}, six tabs`);
  for (const tab of rest) await expect(pages[tab].locator("body")).toContainText(text, { timeout: 30_000 });
  await settled(rest, 3);
});

test("a framed page opens no stream and hands its gestures to its parent", async ({ page }) => {
  const streams: string[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith("/__moss/reload")) streams.push(r.frame() === page.mainFrame() ? "parent" : "frame");
  });
  const frame = await gotoFramed(page, BASE);
  // Inside moss a click on a picture is the editor's, so the page's viewer stays shut.
  await frame.click("[data-viewer]");
  await expect(frame.locator("#lightbox")).not.toBeVisible();
  // Last, since a held internal link takes the frame to the other page.
  expect(await gesturesHeld(frame)).toEqual({ reloadKey: true, menu: true, externalLink: true, internalLink: true });
  expect(streams).toEqual([]);
});
