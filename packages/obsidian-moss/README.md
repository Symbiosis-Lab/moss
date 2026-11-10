# moss for Obsidian

Check and preview notes as [moss](https://mosspub.com) pages on Obsidian desktop and mobile. Diagnostics and page previews run locally using the bundled WebAssembly core, without installing the moss CLI.

On desktop, the plugin can also build and preview a site with the moss CLI, and hand it to the moss app to publish. The site is the folder holding the open note's nearest `.moss/` folder, so one vault can hold several sites; a vault with none becomes a site at its first build. It finds the CLI on `PATH` or in common installation folders, or you can set its path in the plugin's settings.

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
- The language comes from `[site] lang` in `.moss/config.toml` at the vault root, defaulting to English. Site context refreshes when vault files or `.moss/` change.

The following commands are available on desktop:

- "moss: Build site" builds the site into its `.moss/`.
- "moss: Preview site" starts the preview server and shows the site in a side pane; "moss: Stop preview server" or closing the pane stops it.
- "moss: Publish this folder" hands the site to the moss app, which publishes it to its address, or to mosspub.com the first time.

## What gets published

Every note in the site's folder becomes a page, except notes marked `draft: true`, which show in the preview only. This is the opposite of Obsidian Publish, where nothing is public until you select it: before publishing a folder that holds private notes, mark them `draft: true` or move them out of it.

## What it touches

Page checking and preview read the vault through Obsidian's Vault API and do not write files. The one-page preview includes page content and CSS; it does not run site scripts or build the site's navigation and listings.

Desktop site commands run the moss CLI as a child process with the site's folder as its working folder. The CLI writes only under that folder's `.moss/`; it changes no note. The site preview pane loads the CLI's local server. The plugin collects nothing.

## Tests

After building the wasm core, run `pnpm run typecheck` and `npx vitest run`. The unit tests use the real wasm and reject Node and Electron imports when loading the mobile entry.

`pnpm run test:e2e` launches Obsidian with desktop and mobile-emulation capabilities. On headless Linux use `xvfb-run -a pnpm run test:e2e`. Mobile emulation exercises the mobile UI in Electron; the import-isolation unit test covers the absence of Node and Electron APIs.
