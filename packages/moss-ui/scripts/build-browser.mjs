// Builds the browser UI `moss edit --serve` serves into `dist-browser/`: the
// entry page and the shell and editor frames it holds, each a page of the
// same origin. `crates/moss-cli/build.rs` embeds that directory.
import { build } from 'vite';
import { fileURLToPath } from 'node:url';

const pages = new URL('../src/browser/', import.meta.url);
const page = (name) => fileURLToPath(new URL(name, pages));

await build({
  configFile: false,
  root: fileURLToPath(pages),
  base: '/',
  logLevel: 'warn',
  build: {
    outDir: fileURLToPath(new URL('../dist-browser/', import.meta.url)),
    emptyOutDir: true,
    target: 'es2022',
    // The CLI gzips every file it embeds; a map would only add to it.
    sourcemap: false,
    rollupOptions: {
      input: { index: page('index.html'), shell: page('shell.html'), editor: page('editor.html') },
      // Hex, so the server can tell a hashed name and let a browser keep it.
      output: { hashCharacters: 'hex' },
    },
  },
});
