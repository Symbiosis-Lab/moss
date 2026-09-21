// Vite server for the floating nav island's layout gate (ADR-049).
//
// Root is the repo root, not the fixture dir, so the harness page can load the
// production stylesheet and the production module by absolute path — the point
// of this gate is that nothing about the island is re-declared in the fixture.
// Vite is what makes the second half possible: `nav-island.ts` is TypeScript,
// and a plain static server cannot serve it.
//
// The one thing the fixture cannot simply link is the token block. site.css is
// rules-only by design (see the note at its top): the `:root` custom properties
// are generated from tokens.json at BUILD time and prepended, so a browser
// pointed straight at site.css gets a sheet where every `var(--moss-*)` is
// undefined. `/__moss-tokens.css` below closes that gap by asking the moss-cli
// binary — `moss-cli describe --json` is the shipped, documented way to read
// the token set — rather than by copying values into the fixture, where they
// would silently rot the first time a token changed.
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
    port: 5404,
    strictPort: true,
  },
});
