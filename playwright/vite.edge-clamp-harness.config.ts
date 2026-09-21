// Vite server for the edge-clamp gate — the guard on `crates/moss-build/src/js-src/site/viewport.ts`.
//
// Same shape as `vite.nav-island-harness.config.ts`, and for the same reason:
// root is the repo root so the fixture can load the production stylesheet and
// the production TypeScript modules by absolute path, and `/__moss-tokens.css`
// closes the gap left by site.css being rules-only (the `:root` block is
// generated from tokens.json at build time and prepended, so a browser pointed
// straight at the stylesheet gets a sheet where every `var(--moss-*)` is
// undefined).
//
// One route this harness needs that the island's does not: `/_moss/previews.json`.
// `link-preview.ts` will not render a card at all without a matching entry, so
// without it the gate would pass by never testing anything.
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

export default defineConfig({
  root: repoRoot,
  plugins: [tokensPlugin(), previewsPlugin(), mossBuildAssetsPlugin()],
  server: {
    port: 5405,
    strictPort: true,
  },
});
