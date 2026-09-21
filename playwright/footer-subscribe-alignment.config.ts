/**
 * Playwright config for the footer subscribe-form alignment gate
 * (tests/render-gates/site/footer-subscribe-alignment.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css, vertical.css and
 * email.css into `page.setContent` and reads real boxes. No webServer, no
 * `moss build`, no MOSS_BIN. Both engines, because the shipping preview is a
 * WKWebView and vertical writing modes are where engines historically
 * disagree.
 *
 * Run via: pnpm run test:render-gates footer-subscribe-alignment
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'footer-subscribe-alignment',
  fullyParallel: true,
  use: { reducedMotion: 'reduce' },
});
