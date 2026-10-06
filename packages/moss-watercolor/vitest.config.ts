import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    root: import.meta.dirname,
    globals: false,
    environment: "node",
    include: ["src/**/__tests__/**/*.test.ts"],
    exclude: ["**/node_modules/**"],
  },
});
