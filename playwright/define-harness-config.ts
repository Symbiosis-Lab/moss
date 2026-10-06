// Shared wrapper for the three Vite dev-server configs that back a render
// gate's harness fixture (nav-island, edge-clamp, vertical-nav-chrome). Each
// caller differs only in its port and, for edge-clamp, one extra route
// plugin — everything else (root, the token/build-assets plugins, strict
// port binding) is identical and lives here once.
import { defineConfig, type Plugin } from 'vite';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { mossBuildAssetsPlugin, tokensPlugin } from './vite-harness-plugins';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function defineHarnessConfig(options: { port: number; extraPlugins?: Plugin[] }) {
  return defineConfig({
    root: repoRoot,
    plugins: [tokensPlugin(), ...(options.extraPlugins ?? []), mossBuildAssetsPlugin()],
    server: {
      port: options.port,
      strictPort: true,
    },
  });
}
