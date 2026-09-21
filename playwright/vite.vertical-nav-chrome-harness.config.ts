// Vite server for the vertical-writing nav chrome gate — the font selector
// morph and the masthead breadcrumb fold, both driven by production theme.ts.
//
// Same pattern as vite.nav-island-harness.config.ts (repo root as server
// root so the fixture can load site.css + theme.ts by absolute path — see
// vite-harness-plugins.ts for the token-block and build-assets plugins this
// shares with the other two harness configs). A separate port from
// nav-island's so the two gates can run concurrently.
import { defineHarnessConfig } from './define-harness-config';

export default defineHarnessConfig({ port: 5405 });
