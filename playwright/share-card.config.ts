/**
 * Render gate: the quote card a reader shares really has the article's cover
 * photograph painted across its top.
 *
 * Every step of that sentence needs an engine. The card is drawn on a
 * `<canvas>` from an image the browser fetched and decoded, and the only way
 * to know what it contains is to read its pixels back. jsdom has no canvas, no
 * decoder and no network, so the previous test of this code could only build a
 * fake DOM and assert against it — which is how a cover strip that never
 * painted shipped green for two months.
 *
 *   npx playwright test -c playwright/share-card.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 *
 * Because this is the first gate whose subject is a site script rather than a
 * stylesheet, editing anything under `crates/moss-build/src/js-src/site/`
 * needs `node scripts/build-backend-scripts.mjs` before the gate sees it —
 * that is what writes `crates/moss-build/src/assets/js/share-card.js`, which
 * a dev moss reads off disk. No `cargo build` is needed for a script-only
 * change, and no stamp to delete by hand: the scratch-site stamp hashes those
 * scripts, so a rebuilt bundle invalidates the cached site on its own.
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { SHARE_CARD_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';

const PORT = 8757;
const serveDir = buildScratchSite(SHARE_CARD_GATE);

export default defineGateConfig({
  gate: 'share-card',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    // The card's paper background is the light palette's `#f5f0e6`; the
    // "no strip" assertion samples for exactly that, so the scheme is pinned.
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
