/**
 * Render gate for the media-collection lightbox's close/prev/next focus ring
 * and the GitHub link's hover/focus shape.
 *
 * Runs tests/render-gates/site/lightbox-github-shapes.spec.ts in BOTH
 * chromium and webkit against a scratch site with one video-collection item
 * (for the lightbox) and one raw-HTML `.github-link`.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/lightbox-github-shapes.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { lightboxGithubShapesGate } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('lightbox-github-shapes');
const serveDir = buildScratchSite(lightboxGithubShapesGate());

export default defineGateConfig({
  gate: 'lightbox-github-shapes',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
