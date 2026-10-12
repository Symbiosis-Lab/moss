/**
 * Short hero clips played through hls.js (the path Chromium takes: Media Source
 * Extensions, no native HLS). Four claims a jsdom test cannot make:
 *
 *  - on a fast link the playing clip climbs the ladder to its top rung, even
 *    though the clip is short enough to be buffered whole at the first rung
 *    hls.js guesses;
 *  - the clip waiting its turn (`preload="none"`) fetches no media until the
 *    sequence warms it, 3 s before its turn;
 *  - when it does play, it starts from what the first clip measured (the top
 *    rung on a fast link), not from hls.js's default guess;
 *  - on a slow link the clip stays on a low rung and is never refetched higher.
 *
 * Unit tests pin the decisions (`hls-player.test.ts`); this pins that they are
 * wired to a real hls.js and a real network stack.
 *
 * Site: tests/e2e/helpers/gate-sites.ts -> hlsShortClipGate.
 */
import { test, expect, type Page } from "@playwright/test";

interface Rung {
  bandwidth: number;
  height: number;
  /** The rung's media file, e.g. "v5.m4s". */
  media: string;
}

/** The rungs of a clip's master playlist, lowest bandwidth first. */
async function rungs(page: Page, clip: string): Promise<Rung[]> {
  const text = await (await page.request.get(`/${clip}.hls/master.m3u8`)).text();
  const lines = text.split("\n");
  const out: Rung[] = [];
  lines.forEach((line, i) => {
    const m = /^#EXT-X-STREAM-INF:.*?\bBANDWIDTH=(\d+).*?RESOLUTION=\d+x(\d+)/.exec(line);
    if (m) {
      out.push({
        bandwidth: Number(m[1]),
        height: Number(m[2]),
        media: lines[i + 1].trim().replace(/\.m3u8$/, ".m4s"),
      });
    }
  });
  return out.sort((a, b) => a.bandwidth - b.bandwidth);
}

/** Media (segment-file) requests for one clip, in order, with wall-clock times. */
function watchMedia(page: Page, clip: string) {
  const seen: { media: string; at: number }[] = [];
  page.on("request", (req) => {
    const m = new RegExp(`/${clip}\\.hls/([^/?]+\\.m4s)`).exec(req.url());
    if (m) seen.push({ media: m[1], at: Date.now() });
  });
  return seen;
}

const heightOf = (page: Page, index: number) =>
  page.evaluate((i) => document.querySelectorAll("video")[i].videoHeight, index);

test.describe("short hero clips through hls.js", () => {
  test("the playing clip climbs to the top rung on a fast link", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("/");
    const top = (await rungs(page, "a")).at(-1)!;
    const heights: number[] = [];
    await expect
      .poll(async () => {
        heights.push(await heightOf(page, 0));
        return heights.at(-1);
      }, { timeout: 10_000, intervals: [250] })
      .toBe(top.height);
    console.log(`[hls-short-clip] videoHeight over time: ${heights.join(" ")}`);
  });

  test("the waiting clip fetches no media until it is warmed, and then starts on the top rung", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const media = watchMedia(page, "b");
    const startedAt = Date.now();
    await page.goto("/");
    const ladder = await rungs(page, "b");

    // Clip a is 8 s; b is warmed with 3 s left, so nothing for b in the first 4 s.
    await page.waitForTimeout(4_000);
    expect(media, "b's media was fetched long before its turn").toEqual([]);

    await expect(page.locator('.moss-hero-slide[data-slide="1"][data-current]')).toBeAttached({
      timeout: 20_000,
    });
    await expect.poll(() => media.length, { timeout: 10_000 }).toBeGreaterThan(0);
    expect(media[0].at, "b's media began before the warm-up window").toBeGreaterThan(startedAt + 4_000);
    // The first clip measured a fast local link; hls.js's default estimate
    // would start b on a middle rung instead.
    expect(media[0].media, "b did not start from what the first clip measured").toBe(ladder.at(-1)!.media);
  });

  test("a slow link keeps the low rung: nothing is refetched higher", async ({ page }) => {
    test.setTimeout(60_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    const kbps = 300;
    // Media only, and only for the byte range asked for, delivered at `kbps`.
    // Playwright's route is used rather than CDP throttling because the
    // browser cache would answer a throttled reload without touching the
    // network, and the static server ignores `Range`, which would double
    // every rung's download.
    await page.route("**/*.m4s", async (route) => {
      const res = await route.fetch();
      const full = await res.body();
      const range = /bytes=(\d+)-(\d*)/.exec(route.request().headers()["range"] ?? "");
      const start = range ? Number(range[1]) : 0;
      const end = range && range[2] ? Number(range[2]) + 1 : full.length;
      const body = full.subarray(start, end);
      await new Promise((resolve) => setTimeout(resolve, (body.length * 8) / kbps));
      await route.fulfill({
        status: range ? 206 : 200,
        headers: {
          ...res.headers(),
          "content-length": String(body.length),
          ...(range ? { "content-range": `bytes ${start}-${end - 1}/${full.length}` } : {}),
        },
        body,
      });
    });
    const media = watchMedia(page, "a");
    await page.goto("/");
    const ladder = await rungs(page, "a");
    const rank = (file: string) => ladder.findIndex((r) => r.media === file);

    const played = () => page.evaluate(() => document.querySelectorAll("video")[0].currentTime);
    await expect.poll(played, { timeout: 30_000 }).toBeGreaterThan(0);
    const heights = new Set<number>();
    for (let i = 0; i < 6; i++) {
      heights.add(await heightOf(page, 0));
      await page.waitForTimeout(1_000);
    }
    console.log(`[hls-short-clip] ${kbps} kbps: rungs requested ${media.map((m) => m.media).join(" ")}, heights ${[...heights].join(" ")}`);

    const first = rank(media[0].media);
    expect(
      media.filter((m) => rank(m.media) > first).map((m) => m.media),
      "a rung above the one the clip started on was fetched on a slow link",
    ).toEqual([]);
    expect(Math.max(...heights), "the clip sharpened past what 300 kbps carries").toBeLessThanOrEqual(
      ladder[first].height,
    );
  });
});
