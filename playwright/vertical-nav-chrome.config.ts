/**
 * Playwright config for the nav chrome under vertical writing — the font
 * selector morph (theme.ts `initFontPanel`) and the masthead breadcrumb fold
 * (nav/masthead-fold.ts + nav/breadcrumb-fold.ts), both of which measure or
 * position along a physical axis that vertical-rl rotates.
 *
 * Runs in BOTH chromium and webkit for the same reason nav-island's config
 * does: vertical-writing sites are read on Safari, and getComputedStyle's
 * writing-mode resolution plus flex alignment for out-of-flow children have
 * engine-specific history.
 *
 * Harness: playwright/fixtures/vertical-nav-chrome/live-css.html —
 * production site.css + vertical.css + theme.ts, under
 * body[data-typesetting="vertical"].
 *
 * Run via: pnpm run test:render-gates vertical-nav-chrome
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'vertical-nav-chrome',
  use: { baseURL: 'http://localhost:5405', reducedMotion: 'reduce' },
  webServer: { configFile: 'playwright/vite.vertical-nav-chrome-harness.config.ts', port: 5405 },
});
