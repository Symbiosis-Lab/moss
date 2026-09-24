// Vite server for the vertical-writing nav chrome gate — the font selector
// morph and the masthead breadcrumb fold, both driven by production theme.ts.
//
// Same pattern as vite.nav-island-harness.config.ts (repo root as server
// root so the fixture can load site.css + theme.ts by absolute path — see
// vite-harness-plugins.ts for the token-block and build-assets plugins this
// shares with the other two harness configs). Its own slot in gate-ports.ts,
// distinct from nav-island's and edge-clamp's, so all three can run
// concurrently within one checkout — and never collide with another
// checkout's ports either, since gate-ports.ts derives them per worktree.
import { defineHarnessConfig } from './define-harness-config';
import { gatePort } from './gate-ports';

export default defineHarnessConfig({ port: gatePort('vertical-nav-chrome') });
