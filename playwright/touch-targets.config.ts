/**
 * Render gate: nav, footer and subscribe controls clear the 44x44 CSS px
 * touch-target floor at phone width, and stay unchanged at desktop widths.
 *
 * Bounding-box measurement against a real cascade (padding/min-block-size
 * resolving through custom properties and media queries, `::after` hit-area
 * geometry) is exactly what a Rust test reading emitted CSS cannot do.
 *
 * Two real scratch sites are built and served — one with an authored
 * footer.md, one relying on the generated fallback footer — because the
 * footer fix has to reach whatever moss actually emits for each, not a
 * hand-written lookalike. Nav and subscribe stay stylesheet-injection: their
 * markup shape (plain `<a>`, `.active`, `.moss-btn`) carries no analogous
 * "a hand-built fixture would miss this" risk, so `page.setContent()` is
 * enough and does not need a binary.
 *
 *   npx playwright test -c playwright/touch-targets.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli
 * (or set MOSS_BIN) — needed for the two footer scratch sites; nav and
 * subscribe tests run regardless.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import {
  TOUCH_TARGETS_FOOTER_MD_GATE,
  TOUCH_TARGETS_FOOTER_FALLBACK_GATE,
} from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const footerMdServeDir = buildScratchSite(TOUCH_TARGETS_FOOTER_MD_GATE);
const footerFallbackServeDir = buildScratchSite(TOUCH_TARGETS_FOOTER_FALLBACK_GATE);

export default defineGateConfig({
  gate: 'touch-targets',
  use: { colorScheme: 'light' },
  webServer: [
    { serveDir: footerMdServeDir, port: gatePort('touch-targets:footer-md') },
    { serveDir: footerFallbackServeDir, port: gatePort('touch-targets:footer-fallback') },
  ],
});
