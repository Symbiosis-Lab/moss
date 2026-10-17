/**
 * Render gate: a hero's slide captions, overlay content and pause control
 * never overlap, at any width, with or without align=end / mobile=overlay.
 *
 *   npx playwright test -c playwright/hero-caption-stack.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { heroCaptionStackGate } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hero-caption-stack');
const serveDir = buildScratchSite(heroCaptionStackGate());

export default defineGateConfig({
  gate: 'hero-caption-stack',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
