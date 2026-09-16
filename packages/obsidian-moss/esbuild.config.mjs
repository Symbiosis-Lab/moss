// Standard obsidian-sample-plugin toolchain: bundle src/main.ts → main.js
// (CommonJS — Obsidian loads plugins with require()), everything Obsidian
// provides at runtime marked external.
import esbuild from "esbuild";
import process from "node:process";

const production = process.argv[2] === "production";

const context = await esbuild.context({
  entryPoints: ["src/main.ts"],
  bundle: true,
  external: [
    "obsidian",
    "electron",
    "@codemirror/autocomplete",
    "@codemirror/collab",
    "@codemirror/commands",
    "@codemirror/language",
    "@codemirror/lint",
    "@codemirror/search",
    "@codemirror/state",
    "@codemirror/view",
    "@lezer/common",
    "@lezer/highlight",
    "@lezer/lr",
    ...["assert", "buffer", "child_process", "constants", "crypto", "events", "fs", "http", "https", "net", "os", "path", "process", "stream", "url", "util", "zlib"].flatMap((m) => [m, `node:${m}`]),
  ],
  format: "cjs",
  target: "es2021",
  logLevel: "info",
  sourcemap: production ? false : "inline",
  treeShaking: true,
  outfile: "main.js",
  platform: "node",
});

if (production) {
  await context.rebuild();
  process.exit(0);
} else {
  await context.watch();
}
