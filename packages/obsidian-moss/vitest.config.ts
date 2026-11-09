import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [{
    name: "wasm-bytes",
    load(id) {
      if (id.endsWith(".wasm")) return `export default Uint8Array.from(atob(${JSON.stringify(readFileSync(id).toString("base64"))}), c => c.charCodeAt(0));`;
    },
  }],
  resolve: {
    alias: {
      // Obsidian's API only exists inside the app; unit tests stub it and the
      // wdio e2e suite covers the real thing.
      obsidian: fileURLToPath(new URL("./test/obsidian-stub.ts", import.meta.url)),
    },
  },
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
