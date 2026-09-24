/**
 * Render gate for Task 2.5: the --moss-color-ui-accent seam (quiet-chrome opt-in).
 *
 * Runs tests/render-gates/cascade/ui-accent-seam.spec.ts in BOTH chromium and
 * webkit, against two sites — because the seam is only visible as a difference:
 *
 *   SITE A (default):  no user theme CSS. --moss-color-ui-accent
 *     falls back to var(--moss-color-accent), so #chrome-probe and
 *     #content-probe both resolve to rgb(45, 90, 45) (#2d5a2d, moss accent).
 *
 *   SITE B (override): theme CSS points ui-accent at
 *     var(--moss-color-text). #chrome-probe moves to rgb(44, 40, 37) while
 *     #content-probe stays rgb(45, 90, 45) — chrome and content accents are
 *     independently overridable, and only chrome follows ui-accent.
 *
 * Each site's port comes from gate-ports.ts (`ui-accent-seam:default` /
 * `:override`), not a literal — see that file for why, and
 * tests/render-gates/cascade/ui-accent-seam.spec.ts for the matching URLs.
 *
 * These assertions are only provable by a real browser render — jsdom is blind
 * to custom-property resolution through var() chaining and @layer cascade.
 *
 * Both sites are scaffolded and built by `buildScratchSite` below, at
 * config-parse time. That is deliberate and not a `globalSetup`: playwright
 * starts `webServer` BEFORE `globalSetup`, so a site built in `globalSetup`
 * does not exist yet when `webServer.cwd` is needed. This config used to
 * work around that by carrying its own inline copy of the fixture and the
 * build — a third copy, which had already drifted from the other two.
 * See tests/e2e/helpers/scratch-site.ts.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/ui-accent-seam.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import {
  UI_ACCENT_SEAM_DEFAULT,
  UI_ACCENT_SEAM_OVERRIDE,
} from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const defaultServeDir = buildScratchSite(UI_ACCENT_SEAM_DEFAULT);
const overrideServeDir = buildScratchSite(UI_ACCENT_SEAM_OVERRIDE);

export default defineGateConfig({
  gate: 'ui-accent-seam',
  testDir: 'cascade',
  use: {
    colorScheme: 'light', // pin light so dark-mode vars do not interfere
  },
  webServer: [
    { serveDir: defaultServeDir, port: gatePort('ui-accent-seam:default') },
    { serveDir: overrideServeDir, port: gatePort('ui-accent-seam:override') },
  ],
});
