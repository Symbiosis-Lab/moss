import { defineConfig } from "vitest/config";

// Two lanes, mirroring the root vitest.config.js split: the core-layer tests
// and the pure cm6 extractors are DOM-free and run in plain node; the two
// suites that build widget DOM / an EditorView (cm-shortcode-block,
// cm-criticmarkup) need jsdom. New tests default to the node lane — add a
// file to DOM_TESTS only when it actually touches the DOM.
const DOM_TESTS = [
  "src/cm6/__tests__/cm-footnote.test.ts",
  "src/cm6/__tests__/cm-shortcode-block.test.ts",
  "src/cm6/__tests__/cm-criticmarkup.test.ts",
];

export default defineConfig({
  test: {
    root: import.meta.dirname,
    globals: false,
    projects: [
      {
        test: {
          name: "node",
          root: import.meta.dirname,
          globals: false,
          environment: "node",
          include: ["src/**/__tests__/**/*.test.ts"],
          exclude: ["**/node_modules/**", ...DOM_TESTS],
        },
      },
      {
        test: {
          name: "dom",
          root: import.meta.dirname,
          globals: false,
          environment: "jsdom",
          include: DOM_TESTS,
        },
      },
    ],
  },
});
