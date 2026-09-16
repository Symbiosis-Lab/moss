import { defineConfig } from "tsdown";

export default defineConfig({
  entry: {
    index: "src/index.ts",
    "cm6/index": "src/cm6/index.ts",
  },
  format: ["esm"],
  dts: true,
  sourcemap: true,
});
