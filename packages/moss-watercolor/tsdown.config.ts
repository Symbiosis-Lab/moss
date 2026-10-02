import { defineConfig } from "tsdown";

export default defineConfig({
  entry: {
    index: "src/index.ts",
    "engine/index": "src/engine/index.ts",
    "paper/index": "src/paper/index.ts",
    "model/index": "src/model/index.ts",
  },
  format: ["esm"],
  dts: true,
  sourcemap: false,
});
