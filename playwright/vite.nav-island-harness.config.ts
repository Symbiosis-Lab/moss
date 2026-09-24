// Vite server for the floating nav island's layout gate.
//
// Root is the repo root, not the fixture dir, so the harness page can load the
// production stylesheet and the production module by absolute path — the point
// of this gate is that nothing about the island is re-declared in the fixture.
// Vite is what makes the second half possible: `nav-island.ts` is TypeScript,
// and a plain static server cannot serve it.
//
// See vite-harness-plugins.ts for the token-block and build-assets plugins
// this shares with the other two harness configs.
import { defineHarnessConfig } from './define-harness-config';

export default defineHarnessConfig({ port: 5404 });
