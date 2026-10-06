/**
 * Playwright config for the vertical-rl measure gate
 * (tests/render-gates/site/vertical-measure.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css and vertical.css into
 * `page.setContent` and reads real boxes. No webServer, no `moss build`, no
 * MOSS_BIN. Both engines, because the shipping preview is a WKWebView and
 * vertical writing modes are where engines historically disagree.
 *
 * Run via: pnpm run test:render-gates vertical-measure
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'vertical-measure',
  fullyParallel: true,
  use: { reducedMotion: 'reduce' },
});
