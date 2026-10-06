/**
 * Playwright config for the grid block-rhythm gate
 * (tests/render-gates/site/grid-block-rhythm.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css into `page.setContent`
 * and reads real computed margins. No webServer, no `moss build`, no MOSS_BIN.
 * Both engines: margin-collapse and @layer precedence are invisible to jsdom.
 *
 * Run via: pnpm run test:render-gates grid-block-rhythm
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'grid-block-rhythm',
  fullyParallel: true,
  use: { reducedMotion: 'reduce' },
});
