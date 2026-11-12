/**
 * Playwright config for the page colour-scheme gate
 * (tests/render-gates/site/color-scheme.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css into `page.setContent`
 * and reads what the engine paints. No webServer, no `moss build`.
 *
 * Run via: pnpm run test:render-gates color-scheme
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'color-scheme',
  fullyParallel: true,
});
