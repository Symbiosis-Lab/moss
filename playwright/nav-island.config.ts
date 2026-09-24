/**
 * Playwright config for the floating nav island's LAYOUT guard.
 *
 * Two of the island's promises are geometry, and geometry is the one thing
 * neither a Rust test nor jsdom can answer:
 *
 *   1. The trail is ONE line at any width — it never wraps and never overflows.
 *      That is what the measured fold buys, and a breakpoint would only appear
 *      to buy.
 *   2. Every label in the sections panel starts at the same x, current row or
 *      not. The first attempt marked the current row with an in-flow `content:
 *      "●"`, which shifted its own label sideways; the gutter rule is the fix
 *      and this is the assertion that would have caught the bug.
 *
 * Runs in BOTH chromium and webkit: moss sites are read on Safari and on iOS,
 * and the trail leans on `:has()`, `min()` inside `calc()`, and flex baseline
 * alignment, all of which have engine-specific history.
 *
 * Harness: playwright/fixtures/nav-island/live-css.html — production site.css
 * plus the production `nav-island.ts`, with the island markup the Rust emitter
 * produces (pinned by `nav_island_harness_matches_the_emitter`).
 *
 * Run via: pnpm run test:render-gates nav-island
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'nav-island',
  use: { baseURL: 'http://localhost:5404', reducedMotion: 'reduce' },
  webServer: { configFile: 'playwright/vite.nav-island-harness.config.ts', port: 5404 },
});
