/**
 * Render gate: built pages meet WCAG 2.1 AA where only an engine can tell.
 *
 * Builds the WCAG_OUTPUT scratch site (its surfaces are listed beside it in
 * tests/e2e/helpers/gate-sites.ts) and runs axe-core plus the checks axe
 * cannot make over it in both themes; tests/render-gates/site/wcag-output.spec.ts.
 *
 *   npx playwright test -c playwright/wcag-output.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli
 * (or set MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { WCAG_OUTPUT } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

export default defineGateConfig({
  gate: 'wcag-output',
  webServer: { serveDir: buildScratchSite(WCAG_OUTPUT), port: gatePort('wcag-output') },
});
