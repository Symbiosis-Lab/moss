# moss for Obsidian

Build and preview the current vault as a website with [moss](https://mosspub.com), without leaving Obsidian. Desktop only. It needs the moss CLI, a separate program the plugin runs; it looks for it on `PATH` and in `/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin` and `~/.npm-global/bin`, or you can set its path in the plugin's settings.

## Install from source

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

- Underlines frontmatter fields moss does not know and values of the wrong type, explains each field on hover, and completes field names as you type.
- "moss: Build site" builds the vault into `.moss/`.
- "moss: Preview site" starts the preview server and shows the site in a side pane; "moss: Stop preview server" stops it.

## What it touches

The plugin runs the moss CLI as a child process with the vault as its working folder, and the CLI writes the built site under the vault's `.moss/` folder. The preview pane loads the site from the local preview server the CLI starts on `localhost`. The plugin makes no other network requests and collects nothing.
