/**
 * Playwright config for the subscribe-bodyless gate
 * (tests/render-gates/site/subscribe-bodyless.spec.ts).
 *
 * Builds a scratch site with MOSS_BIN; the spec serves it at a virtual https
 * origin through request routing, as the subscribed-state gate does. No
 * webServer. Both engines.
 *
 * Run via: pnpm run test:render-gates subscribe-bodyless
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { SUBSCRIBE_BODYLESS_GATE } from '../tests/e2e/helpers/subscribe-bodyless-site';
import { defineGateConfig } from './define-gate-config';

buildScratchSite(SUBSCRIBE_BODYLESS_GATE);

export default defineGateConfig({ gate: 'subscribe-bodyless' });
