// Vite server for the edge-clamp gate — the guard on `crates/moss-build/src/js-src/site/viewport.ts`.
//
// Same shape as `vite.nav-island-harness.config.ts`, and for the same reason:
// root is the repo root so the fixture can load the production stylesheet and
// the production TypeScript modules by absolute path. See
// vite-harness-plugins.ts for the token-block and build-assets plugins this
// shares with the other two harness configs.
//
// One route this harness needs that the island's does not: `/_moss/previews.json`.
// `link-preview.ts` will not render a card at all without a matching entry, so
// without it the gate would pass by never testing anything.
import type { Plugin } from 'vite';
import { defineHarnessConfig } from './define-harness-config';
import { gatePort } from './gate-ports';

/** The one fixture datum: a preview entry long enough to need clamping. */
function previewsPlugin(): Plugin {
  return {
    name: 'moss-previews',
    configureServer(server) {
      server.middlewares.use('/_moss/previews.json', (_req, res) => {
        res.setHeader('Content-Type', 'application/json');
        res.end(
          JSON.stringify({
            '/docs/example-import/': {
              title: 'Example import source',
              description: 'A placeholder preview entry long enough to need edge clamping.',
              preview: 'Body text for the preview card, long enough to exercise the clamp logic near a viewport edge.',
            },
          }),
        );
      });
    },
  };
}

export default defineHarnessConfig({ port: gatePort('edge-clamp'), extraPlugins: [previewsPlugin()] });
