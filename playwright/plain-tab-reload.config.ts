/**
 * Render gate: a page from `moss-cli build <srcDir> --serve --watch`, open in
 * a plain browser tab, reloads when the site is rebuilt and leaves the
 * browser's own keys, menu and links alone. See the spec header.
 *
 *   npx playwright test -c playwright/plain-tab-reload.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite, scratchSiteDir } from '../tests/e2e/helpers/scratch-site';
import { PLAIN_TAB_RELOAD } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

buildScratchSite(PLAIN_TAB_RELOAD);

export default defineGateConfig({
  gate: 'plain-tab-reload',
  timeout: 120_000,
  webServer: { srcDir: scratchSiteDir(PLAIN_TAB_RELOAD.name), port: gatePort('plain-tab-reload'), watch: true },
});
