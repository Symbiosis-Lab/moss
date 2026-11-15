/**
 * Playwright config for the subscribed-state gate
 * (tests/render-gates/site/subscribed-state.spec.ts).
 *
 * Builds a scratch site with MOSS_BIN; the spec serves it at a virtual https
 * origin through request routing, since a `__Host-` cookie needs a secure
 * origin in every engine. No webServer. Both engines.
 *
 * Run via: pnpm run test:render-gates subscribed-state
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { SUBSCRIBED_STATE_GATE } from '../tests/e2e/helpers/subscribed-state-site';
import { defineGateConfig } from './define-gate-config';

buildScratchSite(SUBSCRIBED_STATE_GATE);

export default defineGateConfig({ gate: 'subscribed-state' });
