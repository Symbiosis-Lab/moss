/**
 * Open a page the way moss's preview shows it: in a frame that fills the
 * viewport, under a parent on the same origin with no script of its own.
 *
 * The preview bridge works only in a framed page; in a plain tab it does
 * nothing but reload on a rebuild. A gate that tests the bridge opens its page
 * here. The test serves the parent itself, so this works against any server.
 */
import { test, type Frame, type Page } from "@playwright/test";

export async function gotoFramed(
  page: Page,
  url: string,
  waitUntil: "load" | "domcontentloaded" = "load",
): Promise<Frame> {
  const target = new URL(url, test.info().project.use.baseURL);
  const parent = new URL("/__framed-parent__", target);
  await page.route(parent.href, (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><style>body{margin:0}iframe{display:block;border:0;width:100vw;height:100vh}</style><iframe src="${target.href}"></iframe>`,
    }),
  );
  await page.goto(parent.href, { waitUntil: "commit" });
  const frame = (await (await page.waitForSelector("iframe", { state: "attached" })).contentFrame())!;
  await frame.waitForURL(target.href, { waitUntil });
  return frame;
}
