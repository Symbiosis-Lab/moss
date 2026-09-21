/**
 * Render gate for notebook loading: the built site's JupyterLite must be able
 * to open the site's notebook.
 *
 * Runs tests/render-gates/site/notebook-loads.spec.ts against a scratch site
 * containing one .ipynb. Building it downloads the pinned JupyterLite bundle
 * (network, ~6 MB, cached under ~/.moss/assets/), so the first run on a
 * machine is slower than the rest.
 *
 * Chromium only, deliberately: the claim is "the app fetches moss's contents
 * index and renders the cells", which is bundle+config behaviour, not a
 * cascade — one engine answers it, and JupyterLite's boot is the expensive
 * part of the gate.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/notebook-loads.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NOTEBOOK_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 9378;
const serveDir = buildScratchSite(NOTEBOOK_GATE);

export default defineGateConfig({
  gate: 'notebook-loads',
  engines: ['chromium'],
  timeout: 120_000,
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
