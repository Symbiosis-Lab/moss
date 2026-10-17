/**
 * Render gate: the footer's end space survives a hidden colophon.
 *
 * Two scratch sites with an authored footer.md, one with the theme rule
 * `.moss-colophon { display: none }` and one without, are built and served;
 * tests/render-gates/site/footer-end-space.spec.ts measures the space below
 * the footer's last content in each, at desktop and phone width.
 *
 *   npx playwright test -c playwright/footer-end-space.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli
 * (or set MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import {
  FOOTER_END_SPACE_HIDDEN,
  FOOTER_END_SPACE_DEFAULT,
} from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const hiddenServeDir = buildScratchSite(FOOTER_END_SPACE_HIDDEN);
const defaultServeDir = buildScratchSite(FOOTER_END_SPACE_DEFAULT);

export default defineGateConfig({
  gate: 'footer-end-space',
  use: { colorScheme: 'light' },
  webServer: [
    { serveDir: hiddenServeDir, port: gatePort('footer-end-space:hidden') },
    { serveDir: defaultServeDir, port: gatePort('footer-end-space:default') },
  ],
});
