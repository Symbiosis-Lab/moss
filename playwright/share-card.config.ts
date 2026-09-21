/**
 * Render gate: the quote card a reader shares really has the article's cover
 * photograph painted across its top.
 *
 * Every step of that sentence needs an engine. The card is drawn on a
 * `<canvas>` from an image the browser fetched and decoded, and the only way
 * to know what it contains is to read its pixels back. jsdom has no canvas, no
 * decoder and no network, so the previous test of this code could only build a
 * fake DOM and assert against it — which is how a cover strip that never
 * painted shipped green for two months.
 *
 *   npx playwright test -c playwright/share-card.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 *
 * Because this is the first gate whose subject is a site script rather than a
 * stylesheet, editing anything under `crates/moss-build/src/js-src/site/`
 * needs `node scripts/build-backend-scripts.mjs` before the gate sees it —
 * that is what writes `crates/moss-build/src/assets/js/share-card.js`, which
 * a dev moss reads off disk. No `cargo build` is needed for a script-only
 * change, and no stamp to delete by hand: the scratch-site stamp hashes those
 * scripts, so a rebuilt bundle invalidates the cached site on its own.
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { SHARE_CARD_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(SHARE_CARD_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /share-card\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-share-card",
  use: {
    baseURL: "http://localhost:8757/",
    // The card's paper background is the light palette's `#f5f0e6`; the
    // "no strip" assertion samples for exactly that, so the scheme is pinned.
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8757`,
    cwd: serveDir,
    url: "http://localhost:8757/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
