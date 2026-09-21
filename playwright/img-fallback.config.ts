/**
 * Playwright config for the blueprint image placeholder
 * (crates/moss-build/src/js-src/bridge/asset-placeholder.ts).
 *
 * Runs in BOTH chromium and webkit, and both engines are load-bearing here:
 *
 *   - Chromium paints its native broken-image icon OVER any CSS background on a
 *     failed <img>, WebKit does not. That difference is the whole reason the
 *     placeholder writes into `img.src` instead of styling the element, so the
 *     assertion has to see both.
 *   - Recovery leans on source-set re-selection when a `<source srcset>` is
 *     restored, which has engine-specific history. The preview itself is a
 *     WKWebView, so webkit is not the exotic case — it is the shipping one.
 *
 * No webServer: the spec serves both the page and the images through
 * `page.route()` on a fake origin, which is how it can hold an asset 404 and
 * then let the same URL succeed — a pending encode, without a real encoder.
 * That also means this gate needs no `moss build` and no MOSS_BIN; it costs
 * seconds. It still lives with the render gates because it answers their
 * question: what does the engine actually paint.
 *
 * Run via: pnpm run test:render-gates img-fallback
 */
import './localhost-no-proxy';
import { defineGateConfig } from './define-gate-config';

export default defineGateConfig({
  gate: 'img-fallback',
  use: { reducedMotion: 'reduce' },
});
