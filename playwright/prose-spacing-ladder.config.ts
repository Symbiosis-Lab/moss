/**
 * Playwright config for the prose spacing-ladder gate
 * (tests/render-gates/site/prose-spacing-ladder.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines the real tokens block and
 * site.css into `page.setContent` and reads real computed gaps (margin
 * collapse included). No webServer, no `moss build`, no MOSS_BIN — the
 * reader's Aa classes and the `lang` attribute are plain CSS selectors here,
 * so setting them directly in the fixture's markup exercises the real rules
 * without needing the site JS that normally writes them. Both engines:
 * margin-collapse and @layer precedence are invisible to jsdom.
 *
 * Run via: npx playwright test -c playwright/prose-spacing-ladder.config.ts
 */
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'prose-spacing-ladder',
  fullyParallel: true,
  use: { reducedMotion: 'reduce' },
});
