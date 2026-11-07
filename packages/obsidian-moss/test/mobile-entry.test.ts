import { build } from "esbuild";
import { builtinModules, createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { afterEach, expect, it, vi } from "vitest";
import type { MossSite } from "../src/core";
import * as obsidian from "./obsidian-stub";

afterEach(() => vi.useRealTimers());

it("imports main and loads on mobile with Electron and every Node module forbidden", async () => {
  const bundled = await build({
    entryPoints: ["src/main.ts"], bundle: true, write: false, format: "cjs", platform: "browser",
    loader: { ".wasm": "binary" }, external: ["obsidian", "electron", "node:*", "@codemirror/*", "@lezer/*"],
    plugins: [{ name: "observe-desktop", setup(builder) {
      builder.onLoad({ filter: /\/desktop\.ts$/ }, ({ path }) => ({
        contents: `globalThis.desktopLoaded = true;\n${readFileSync(path, "utf8")}`, loader: "ts",
      }));
    } }],
  });
  vi.useFakeTimers();
  const nativeRequire = createRequire(import.meta.url);
  const forbidden = new Set(builtinModules.flatMap((name) => [name, `node:${name}`]));
  const sandbox = {
    module: { exports: {} as { default: new (app: unknown) => obsidian.Plugin & { onload(): Promise<void>; onunload(): void; site: MossSite } } },
    exports: {}, desktopLoaded: false,
    require(name: string) {
      if (name === "electron" || name.startsWith("node:") || forbidden.has(name)) throw new Error(`Forbidden import: ${name}`);
      return name === "obsidian" ? obsidian : nativeRequire(name);
    },
    console, TextEncoder, TextDecoder, WebAssembly, Uint8Array, atob, setTimeout, clearTimeout,
  };
  runInNewContext(bundled.outputFiles[0].text, sandbox);
  const events: Record<string, (...args: unknown[]) => void> = {};
  const files = [{ path: "page.md" }];
  const hidden: Record<string, string> = {};
  const app = {
    vault: {
      adapter: { exists: async (path: string) => path in hidden, read: async (path: string) => hidden[path] },
      getFiles: () => files,
      on: (name: string, callback: (...args: unknown[]) => void) => { events[name] = callback; return {}; },
    },
    workspace: { iterateAllLeaves() {}, onLayoutReady() {}, detachLeavesOfType() {} },
  };
  const plugin = new sandbox.module.exports.default(app);
  await plugin.onload();
  expect(plugin.commands.map((command) => command.id)).toEqual(["preview-page"]);
  expect(sandbox.desktopLoaded).toBe(false);
  expect(plugin.site.check("page.md", "[[new]]")).toContain("unresolved link");
  files.push({ path: "new.md" });
  events.create();
  events.rename();
  await vi.advanceTimersByTimeAsync(249);
  expect(plugin.site.check("page.md", "[[new]]")).toContain("unresolved link");
  await vi.advanceTimersByTimeAsync(1);
  expect(plugin.site.check("page.md", "[[new]]")).not.toContain("unresolved link");
  hidden[".moss/config.toml"] = '[site]\nlang = "fr"';
  events.raw(".moss/config.toml");
  await vi.advanceTimersByTimeAsync(250);
  expect(JSON.parse(plugin.site.preview("page.md", "# Page", "")).html).toContain('lang="fr"');
  files.pop();
  events.delete();
  await vi.advanceTimersByTimeAsync(250);
  expect(plugin.site.check("page.md", "[[new]]")).toContain("unresolved link");
  plugin.onunload();
});
