/**
 * Render gate: short background clips played through hls.js sharpen to the top
 * rung on a fast link, and a clip that is not playing fetches no media.
 *
 *   npx playwright test -c playwright/hls-short-clip.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN), and ffmpeg on PATH.
 *
 * Chromium only: it has no native HLS before v142 and runs hls.js on Media
 * Source Extensions; WebKit plays the ladder natively and never loads the
 * script this gate is about.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { hlsShortClipGate } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('hls-short-clip');
const serveDir = buildScratchSite(hlsShortClipGate());

export default defineGateConfig({
  gate: 'hls-short-clip',
  engines: ['chromium'],
  use: { baseURL: `http://localhost:${PORT}/` },
  webServer: { serveDir, port: PORT },
});
