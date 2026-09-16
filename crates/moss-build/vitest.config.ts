import { defineConfig } from "vitest/config";

// Ported wholesale from the desktop repo's frontend/site and frontend/bridge
// __tests__ (deleted there in ffb20d7a1 once these sources moved here). Every
// file below drives real DOM (elements, matchMedia, canvas) so the whole set
// runs jsdom, unlike moss-syntax's node/dom split — none of these ported
// clean as DOM-free.
export default defineConfig({
  test: {
    root: import.meta.dirname,
    globals: false,
    environment: "jsdom",
    include: ["src/js-src/**/__tests__/**/*.test.ts"],
  },
});
