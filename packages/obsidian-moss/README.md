# moss for Obsidian

Check and preview notes as [moss](https://mosspub.com) pages on Obsidian desktop and mobile. Diagnostics and page previews run locally using the bundled WebAssembly core, without installing the moss CLI.

On desktop, the plugin can also build and preview the whole site with the moss CLI, and open the vault in the moss app for publishing. It finds the CLI on `PATH` or in common installation folders, or you can set its path in the plugin's settings.

## Install from source

Building from source requires Rust with the `wasm32-unknown-unknown` target and `wasm-pack`. The build embeds the core in `main.js`; no separate wasm file needs to be installed.

The plugin is not in Obsidian's community plugin directory and has no release yet, so it installs by copying three files into the vault.

```sh
pnpm install
cd packages/obsidian-moss
npm run build
mkdir -p "<vault>/.obsidian/plugins/moss"
cp main.js manifest.json styles.css "<vault>/.obsidian/plugins/moss/"
```

Then, in Obsidian, open Settings → Community plugins, turn off Restricted mode if it is on, and enable "moss". Rebuild and copy again to update.

## What it does

- Marks frontmatter errors, unknown fields, shortcode problems and unresolved note links. Hover explains diagnostics and known fields; completion includes fields contributed by installed plugins.
- "moss: Preview this page" renders the current editor text in a side pane, updating after edits and file changes. It includes embedded notes, relative images and `.moss/theme/*.css`, in a sandboxed iframe with scripts disabled.
- The language comes from `[site] lang` in `.moss/config.toml`, defaulting to English. Site context refreshes when vault files or `.moss/` change.

The following commands are available on desktop:

- "moss: Build site" builds the vault into `.moss/`.
- "moss: Preview site" starts the preview server and shows the site in a side pane; "moss: Stop preview server" or closing the pane stops it.
- "moss: Publish with the moss app" opens the vault in the moss desktop app, where publishing happens. The plugin itself publishes nothing.

## What gets published

When you publish from the moss app, every note in the vault becomes a page of the site, except notes marked `draft: true`, which show in the preview only. This is the opposite of Obsidian Publish, where nothing is public until you select it: before publishing a vault that holds private notes, mark them `draft: true` or keep them in another vault.

## What it touches

Page checking and preview read the vault through Obsidian's Vault API and do not write files. The one-page preview includes page content and CSS; it does not run site scripts or build the site's navigation and listings.

Desktop site commands run the moss CLI as a child process with the vault as its working folder. The CLI writes the built site under `.moss/`, adds a `uid:` line to the frontmatter of each note it builds, and writes guidance for coding agents into `.claude/` and `.cursor/`. The site preview pane loads the CLI's local server. The plugin collects nothing.

## Tests

After building the wasm core, run `pnpm run typecheck` and `npx vitest run`. The unit tests use the real wasm and reject Node and Electron imports when loading the mobile entry.

`pnpm run test:e2e` launches Obsidian with desktop and mobile-emulation capabilities. On headless Linux use `xvfb-run -a pnpm run test:e2e`. Mobile emulation exercises the mobile UI in Electron; the import-isolation unit test covers the absence of Node and Electron APIs.
