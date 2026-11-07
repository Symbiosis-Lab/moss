/**
 * Render gate: a country map previews on hover, frames a chosen country
 * beside the panel, becomes a sideways box and a bottom sheet on a phone, and
 * its search field and legend work from the keyboard.
 *
 *   npx playwright test -c playwright/country-map-interaction.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { COUNTRY_MAP_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('country-map-interaction');
const serveDir = buildScratchSite(COUNTRY_MAP_GATE);

export default defineGateConfig({
  gate: 'country-map-interaction',
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
