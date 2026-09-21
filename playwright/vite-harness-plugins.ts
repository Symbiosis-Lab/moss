// Vite plugins shared by the edge-clamp, nav-island and vertical-nav-chrome
// harness configs (playwright/vite.*-harness.config.ts).
import type { Plugin } from 'vite';
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { openCrateDir } from '../tests/support/crate-paths';
import { resolveMossBin } from '../tests/support/moss-bin';

// Their `live-css.html` fixtures load the production stylesheet and
// production TypeScript modules straight from `moss-build`'s `src/`, through
// a stable `/moss-build-assets/<rel>` route rather than a hardcoded relative
// path, so the fixture keeps working if `moss-build`'s location ever moves.
//
// This plugin resolves that route to `crates/moss-build/src/<rel>` and
// rewrites the request onto Vite's own `/@fs/<absolute path>` convention,
// which is what lets Vite serve (and, for `.ts` modules, transform) the file.
export function mossBuildAssetsPlugin(): Plugin {
  const crateSrcDir = join(openCrateDir('moss-build'), 'src');
  return {
    name: 'moss-build-assets',
    enforce: 'pre',
    resolveId(source) {
      // A fixture's inline `<script type="module">` block (as opposed to a
      // `<script type="module" src="...">` tag) has its bare import
      // specifiers resolved through this hook, not through an HTTP request —
      // `configureServer`'s middleware below never runs for it, since no
      // request is ever made for the specifier itself.
      if (source.startsWith('/moss-build-assets/')) {
        return join(crateSrcDir, source.slice('/moss-build-assets/'.length));
      }
    },
    configureServer(server) {
      // Mounting with a path prefix (`.use('/moss-build-assets', fn)`) does
      // NOT work here: connect restores `req.url` to its original value once
      // a mounted layer calls `next()`, so a rewrite made inside one is
      // silently discarded before the next middleware (Vite's own `/@fs`
      // handler) ever sees it. Matching on the unmounted middleware stack
      // and rewriting in place is what makes the rewrite stick.
      server.middlewares.use((req, _res, next) => {
        if (req.url?.startsWith('/moss-build-assets/')) {
          req.url = `/@fs${crateSrcDir}${req.url.slice('/moss-build-assets'.length)}`;
        }
        next();
      });
    },
  };
}

interface DescribedToken {
  name: string;
  value: string;
  dark_value?: string | null;
}

// The one thing a harness fixture cannot simply link is the token block.
// site.css is rules-only by design (see the note at its top): the `:root`
// custom properties are generated from tokens.json at BUILD time and
// prepended, so a browser pointed straight at site.css gets a sheet where
// every `var(--moss-*)` is undefined. This plugin closes that gap by asking
// the moss-cli binary — `moss-cli describe --json` is the shipped, documented
// way to read the token set — rather than by copying values into a fixture,
// where they would silently rot the first time a token changed.

/** Serve `:root { … }` built from the binary's own token table. */
export function tokensPlugin(): Plugin {
  const mossBin = resolveMossBin();
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
