// Shared by the edge-clamp, nav-island and vertical-nav-chrome harness
// configs. Their `live-css.html` fixtures load the production stylesheet and
// production TypeScript modules straight from `moss-build`'s `src/`, through
// a stable `/moss-build-assets/<rel>` route rather than a hardcoded relative
// path, so the fixture keeps working if `moss-build`'s location ever moves.
//
// This plugin resolves that route to `crates/moss-build/src/<rel>` and
// rewrites the request onto Vite's own `/@fs/<absolute path>` convention,
// which is what lets Vite serve (and, for `.ts` modules, transform) the file.
import type { Plugin } from 'vite';
import { join } from 'node:path';
import { openCrateDir } from '../tests/support/crate-paths';

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
