import { defineConfig } from "tsdown";

export default defineConfig([
  {
    entry: { index: "src/index.ts", element: "src/element.ts" },
    format: ["esm"],
    dts: true,
    sourcemap: false,
  },
  // One self-contained script that defines <blueprint-grid>, for a page without a bundler.
  {
    entry: { "blueprint-grid": "src/element.ts" },
    format: ["iife"],
    minify: true,
    dts: false,
    clean: false,
    sourcemap: false,
  },
]);
