import path from "node:path";
import { build } from "esbuild";

/**
 * Every bundle esbuild emits for the Rust side, as one table.
 *
 * `out` decides which crate embeds the bundle, so it is the load-bearing
 * column: a bundle must land in the crate whose `include_str!` reads it, and
 * writing one to the wrong crate fails no build — it leaves that crate
 * embedding a stale copy that no longer tracks its TS source (moss, 2026-08-28).
 *
 * The two destinations are two runtimes, not two folders that drifted apart:
 * `js-src/site/` is the published site a visitor loads (moss-build compiles
 * it), `js-src/bridge/` is preview-only JS the app injects into its iframe
 * and never deploys. See docs/reference/target/02-target-architecture.md L2,
 * rows 8 and 9 — the entry path and the outfile crate always agree.
 *
 * Ported from the desktop repo's scripts/build-backend-scripts.mjs (landing
 * order step 7, docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md):
 * these 15 outputs (plus the hand-written comment-owner-controls.js) already
 * live in this repo, so their TS sources and the bundler that produces them
 * belong here too. Desktop's own build.rs stops invoking this script once
 * it lands and builds green here.
 *
 * Exported because it is the single owner of that mapping: check-embedded-js.mjs
 * derives from it rather than restating it.
 */
export const BUNDLES = [
  { entry: "crates/moss-build/src/js-src/bridge/iframe-bridge.ts", out: "crates/moss-build/src/ops/serve/js/iframe-bridge.js" },
  // The blueprint placeholder for any <img> that fails to load. NOT a hashed
  // <script src> like its neighbours: template.rs `include_str!`s this output
  // and inlines it into shell.html's <head>, because the capture-phase `error`
  // listener has to be registered before the first image load can fail.
  // Absorbed the old thumb-swap.js, whose two jobs (placeholder a missing
  // .thumb.jpg, restore it when ffmpeg lands) are this module's general case.
  { entry: "crates/moss-build/src/js-src/bridge/asset-placeholder.ts", out: "crates/moss-build/src/ops/serve/js/asset-placeholder.js" },
  { entry: "crates/moss-build/src/js-src/bridge/preview-shim.ts", out: "crates/moss-build/src/ops/serve/js/comment-preview-shim.js" },

  { entry: "crates/moss-build/src/js-src/site/theme.ts", out: "crates/moss-build/src/assets/js/theme.js" },
  { entry: "crates/moss-build/src/js-src/site/fullscreen.ts", out: "crates/moss-build/src/assets/js/fullscreen.js" },
  { entry: "crates/moss-build/src/js-src/site/link-preview.ts", out: "crates/moss-build/src/assets/js/preview.js" },
  { entry: "crates/moss-build/src/js-src/site/comments/artalk.ts", out: "crates/moss-build/src/assets/js/comments-artalk.js" },
  { entry: "crates/moss-build/src/js-src/site/subscribe/subscribe.ts", out: "crates/moss-build/src/assets/js/subscribe.js" },
  { entry: "crates/moss-build/src/js-src/site/heading-anchor.ts", out: "crates/moss-build/src/assets/js/heading-anchor.js" },
  // Site search runtime. `external: ["/_moss/pagefind/*"]` is not needed — the
  // pagefind.js specifier is computed at runtime, so esbuild leaves the dynamic
  // import in place instead of trying to resolve a module that only exists in
  // the published build output.
  { entry: "crates/moss-build/src/js-src/site/search.ts", out: "crates/moss-build/src/assets/js/search.js" },
  // Quote-card renderer. The ONE `esm` output here, because it is the one
  // bundle nothing loads eagerly: `selection-actions.ts` `import()`s it the
  // first time a reader taps Share, and an `iife` build would have no export
  // for that import to destructure. It is a separate entry point rather than a
  // code-split chunk because every other bundle is `iife` with no `splitting` —
  // esbuild inlines a dynamic import back into an iife parent, which is exactly
  // how 12.4 kb of canvas rendering ended up in theme.js on every page.
  { entry: "crates/moss-build/src/js-src/site/share-card.ts", out: "crates/moss-build/src/assets/js/share-card.js", format: "esm" },
  // Adaptive video. The boot half is iife and eager on any page with a
  // ladder; the player half is the second `esm` output, carrying hls.js, and
  // is fetched only where MSE exists and native HLS does not.
  { entry: "crates/moss-build/src/js-src/site/hls-boot.ts", out: "crates/moss-build/src/assets/js/hls-boot.js" },
  { entry: "crates/moss-build/src/js-src/site/hls-player.ts", out: "crates/moss-build/src/assets/js/hls.js", format: "esm" },
  { entry: "crates/moss-build/src/js-src/site/sidenotes.ts", out: "crates/moss-build/src/assets/js/sidenotes.js" },
  { entry: "crates/moss-build/src/js-src/site/math-copy.ts", out: "crates/moss-build/src/assets/js/math-copy.js" },
  { entry: "crates/moss-build/src/js-src/site/scroll-row.ts", out: "crates/moss-build/src/assets/js/scroll-row.js" },
];

/** The directories BUNDLES writes into — derived, never restated. */
export const EMBEDDED_DIRS = [...new Set(BUNDLES.map((b) => path.dirname(b.out)))].sort();

export async function buildBackendScripts() {
  await Promise.all(
    BUNDLES.map(({ entry, out, format = "iife" }) =>
      build({ entryPoints: [entry], bundle: true, minify: true, format, outfile: out }),
    ),
  );
}

if (import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  buildBackendScripts().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
