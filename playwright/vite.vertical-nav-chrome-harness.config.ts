// Vite server for the vertical-writing nav chrome gate — the font selector
// morph and the masthead breadcrumb fold, both driven by production theme.ts.
//
// Same pattern as vite.nav-island-harness.config.ts (repo root as server
// root so the fixture can load site.css + theme.ts by absolute path, plus a
// `/__moss-tokens.css` endpoint since site.css itself carries no `:root`
// tokens — see that file's comment for why). A separate port from
// nav-island's so the two gates can run concurrently.
import { defineConfig, type Plugin } from 'vite';
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { mossBuildAssetsPlugin } from './vite-moss-build-assets-plugin';
import { resolveMossBin } from '../tests/support/moss-bin';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const mossBin = resolveMossBin();

interface DescribedToken {
  name: string;
  value: string;
  dark_value?: string | null;
}

/** Serve `:root { … }` built from the binary's own token table. */
function tokensPlugin(): Plugin {
  let css: string | null = null;
  return {
    name: 'moss-tokens',
    configureServer(server) {
      server.middlewares.use('/__moss-tokens.css', (_req, res) => {
        if (css === null) {
          if (!existsSync(mossBin)) {
            throw new Error(
              `no moss-cli binary at ${mossBin} — run \`cargo build -p moss-cli\`, or set MOSS_BIN`,
            );
          }
          const described = JSON.parse(
            execFileSync(mossBin, ['describe', '--json'], {
              encoding: 'utf8',
              maxBuffer: 32 * 1024 * 1024,
            }),
          ) as { tokens: Record<string, DescribedToken[]> };
          const groups = Object.values(described.tokens).flat();
          const light = groups.map((t) => `  --${t.name}: ${t.value};`).join('\n');
          const dark = groups
            .filter((t) => t.dark_value)
            .map((t) => `  --${t.name}: ${t.dark_value};`)
            .join('\n');
          css = `@layer tokens {\n:root {\n${light}\n}\n:root[data-theme="dark"] {\n${dark}\n}\n}\n`;
        }
        res.setHeader('Content-Type', 'text/css');
        res.end(css);
      });
    },
  };
}

export default defineConfig({
  root: repoRoot,
  plugins: [tokensPlugin(), mossBuildAssetsPlugin()],
  server: {
    port: 5405,
    strictPort: true,
  },
});
