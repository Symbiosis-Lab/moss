/**
 * Video card covers autoplay in view. Real engines, at a phone and a desktop
 * width, because autoplay policy and inline muted playback differ per engine
 * and jsdom plays nothing.
 */

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect, type Page } from "../../support/browser-test";
import { syntheticTestClip } from "../../e2e/helpers/scratch-site";

const HERE = dirname(fileURLToPath(import.meta.url));
const THEME = readFileSync(join(HERE, "../../../crates/moss-build/src/assets/js/theme.js"), "utf8");
const MP4 = syntheticTestClip(320, 180);
const WEBM = (() => {
  const dir = join(HERE, "../../../target/test-tmp/fixtures");
  const file = join(dir, "cover-clip.webm");
  if (!existsSync(file)) {
    mkdirSync(dir, { recursive: true });
    execFileSync("ffmpeg", ["-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=15", "-t", "2", "-c:v", "libvpx", "-b:v", "200k", file]);
  }
  return readFileSync(file);
})();

const CARDS = 6;
const PAGE = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<style>body{margin:0;display:flex;flex-wrap:wrap;gap:20px}body::after{content:"";flex-basis:100%;height:1500px}.moss-card{display:block;height:200px}.moss-card-cover{position:relative;height:180px;width:320px}
.moss-card-cover video,.cover-thumb{position:absolute;inset:0;width:100%;height:100%}.cover-thumb.is-playing{opacity:0}</style></head><body>
${Array.from({ length: CARDS }, (_, i) => `<a class="moss-card" href="/p${i}"><div class="moss-card-cover">
<video muted loop playsinline preload="metadata"><source src="/clip.webm" type="video/webm"><source src="/clip.mp4" type="video/mp4"></video>
<img class="cover-thumb" alt="" src="data:image/gif;base64,R0lGODlhAQABAAAAACw="></div></a>`).join("\n")}
</body></html>`;

async function open(page: Page) {
  const fetched: number[] = [];
  await page.route("http://gate.test/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/clip.mp4") { fetched.push(MP4.length); return route.fulfill({ body: MP4, contentType: "video/mp4" }); }
    if (path === "/clip.webm") { fetched.push(WEBM.length); return route.fulfill({ body: WEBM, contentType: "video/webm" }); }
    return route.fulfill({ body: PAGE, contentType: "text/html" });
  });
  await page.goto("http://gate.test/");
  await page.addScriptTag({ content: THEME });
  return fetched;
}

const playing = (page: Page) => page.evaluate(() => [...document.querySelectorAll("video")].filter((v) => !v.paused && !v.ended).length);

for (const [label, size] of [["phone", { width: 390, height: 720 }], ["desktop", { width: 1280, height: 800 }]] as const) {
  test.describe(label, () => {
    test.use({ viewport: size });

    test("covers in view play muted, at most three at once, and pause out of view", async ({ page }, info) => {
      const fetched = await open(page);
      await expect.poll(() => playing(page), { timeout: 10_000 }).toBeGreaterThan(0);
      await page.waitForTimeout(1500);
      const n = await playing(page);
      // Desktop shows all six covers, so the cap alone holds the count at three.
      if (label === "desktop") expect(n).toBe(3);
      else expect(n).toBeLessThanOrEqual(3);
      expect(await page.evaluate(() => [...document.querySelectorAll("video")].every((v) => v.muted))).toBe(true);
      info.annotations.push({ type: "measure", description: `${label}: ${n} playing of ${CARDS}, ${fetched.reduce((a, b) => a + b, 0)} bytes fetched` });
      console.log(`MEASURE ${info.project.name} ${label}: ${n} playing of ${CARDS}, ${fetched.reduce((a, b) => a + b, 0)} bytes fetched`);

      await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
      await expect.poll(() => page.evaluate(() => { const v = document.querySelector("video")!; return v.paused; }), { timeout: 10_000 }).toBe(true);
    });

    test("a hover preview ending while the cover is in view leaves it playing", async ({ page }, info) => {
      test.skip(info.project.name !== "chromium" || label !== "desktop", "hover is a desktop pointer");
      await open(page);
      await expect.poll(() => playing(page), { timeout: 10_000 }).toBeGreaterThan(0);
      // Which three covers take the places varies by run; hover one that has one.
      const i = await page.evaluate(() => [...document.querySelectorAll("video")].findIndex((v) => !v.paused));
      const card = page.locator(".moss-card").nth(i);
      await card.hover();
      await page.waitForTimeout(400);
      await page.mouse.move(5, 790);
      await page.waitForTimeout(400);
      expect(await card.locator("video").evaluate((v: HTMLVideoElement) => !v.paused)).toBe(true);
    });

    test("under reduced motion no cover plays", async ({ page }) => {
      await page.emulateMedia({ reducedMotion: "reduce" });
      await open(page);
      await page.waitForTimeout(1500);
      expect(await playing(page)).toBe(0);
    });
  });
}
