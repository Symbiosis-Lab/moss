# moss for Obsidian

Check and preview notes as [moss](https://mosspub.com) pages on Obsidian desktop and mobile. Diagnostics and page previews run locally using the bundled WebAssembly core, without installing the moss CLI.

On desktop, the plugin also previews a site with the moss program, and hands it to the moss app to publish. The site is the folder holding the open note's nearest `.moss/` folder, so one vault can hold several sites; a vault with none becomes a site at its first preview. It finds the program on `PATH`, inside the moss app, or in common installation folders; Settings → moss shows where, and takes a path when it is somewhere else.

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
- On mobile, "moss: Preview this page" (and the globe in the ribbon) renders the current editor text in a side pane, updating after edits and file changes. It includes embedded notes, relative images and `.moss/theme/*.css`, in a sandboxed iframe with scripts disabled. On desktop the same render is what "Preview this folder" shows when the moss program is not installed.
- The language comes from `[site] lang` in `.moss/config.toml` at the vault root, defaulting to English. Site context refreshes when vault files or `.moss/` change.

On desktop:

- "moss: Preview this folder" or the globe in the ribbon previews the open note's site; "Preview with moss" in a folder's menu previews that folder's site, or the folder itself when no site holds it. The site shows in a side pane. While it runs, the status bar says so; click it to show the pane. "moss: Stop preview" or closing the pane stops it.
- "moss: Publish this folder" hands the open note's site to the moss app, which publishes it to its address, or to mosspub.com the first time. The plugin itself publishes nothing.

## What gets published

Every note in the site's folder becomes a page, except notes marked `draft: true`, which show in the preview only. This is the opposite of Obsidian Publish, where nothing is public until you select it: before publishing a folder that holds private notes, mark them `draft: true` or move them out of it.

## What it touches

Page checking and preview read the vault through Obsidian's Vault API and do not write files. The one-page preview includes page content and CSS; it does not run site scripts or build the site's navigation and listings.

The desktop preview runs the moss program as a child process with the site's folder as its working folder. It writes only under that folder's `.moss/` (asking once, the first time) and reads the notes. The preview pane loads its local server. A note's `uid:` line is written when the moss app first publishes it. The plugin collects nothing.

## Tests

After building the wasm core, run `pnpm run typecheck` and `npx vitest run`. The unit tests use the real wasm and reject Node and Electron imports when loading the mobile entry.

`pnpm run test:e2e` launches Obsidian with desktop and mobile-emulation capabilities. On headless Linux use `xvfb-run -a pnpm run test:e2e`. Mobile emulation exercises the mobile UI in Electron; the import-isolation unit test covers the absence of Node and Electron APIs.
