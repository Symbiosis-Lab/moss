/**
 * Playwright config for the article date-line alignment gate
 * (tests/render-gates/site/article-date-alignment.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines the real site.css + vertical.css
 * into `page.setContent` and reads real boxes. No webServer, no `moss build`,
 * no MOSS_BIN. Both engines, because the shipping preview is a WKWebView and
 * vertical writing modes are where engines historically disagree.
 *
 * Run via: pnpm run test:render-gates article-date-alignment
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'article-date-alignment',
  fullyParallel: true,
  use: { reducedMotion: 'reduce' },
});
