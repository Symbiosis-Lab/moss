/**
 * Render gate: a bare inline video (`![[clip.mp4]]`, not in a figure) takes
 * its own decoded shape, not a box forced to 16:9. A 1280x1080 clip used to
 * render at ~65% of the column's width — exactly (32/27)/(16/9) — because
 * `article video`'s `aspect-ratio: 16 / 9` plus `object-fit: contain`
 * pillarboxed it. This is a real-engine question: only a browser that has
 * decoded the video's `loadedmetadata` knows its natural ratio, which is what
 * `aspect-ratio: auto 16 / 9` reads once available.
 *
 *   npx playwright test -c playwright/video-embed-shape.config.ts
 *
 * Both engines. `syntheticTestClip`'s own doc comment explains a real trap
 * here: Chromium resolves a `<video>`'s `aspect-ratio: auto` from its
 * `poster` image's natural size whenever `poster` is present, not from the
 * decoded frame — so a fixture clip too short for moss's thumbnail-generator
 * seek left a 404 poster, and Chromium silently fell back to the CSS
 * placeholder ratio even with the video itself fully decoded. That looked
 * like a Chromium engine gap until isolated launches against a longer clip
 * (a working poster) showed both engines agreeing, every time.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli
 * (or MOSS_BIN), and `ffmpeg` must be on PATH to generate the fixture clip.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { videoEmbedShapeGate } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8814;
const serveDir = buildScratchSite(videoEmbedShapeGate());

export default defineGateConfig({
  gate: 'video-embed-shape',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
