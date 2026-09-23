# Harvested moss interface bank

`app-editor.html` is a self-contained harvest of the app's own `editor.html` + `editor-main.ts`, with only the Tauri seams mocked — opening files, saving, the app's menus, and versions mode are the app's own code, backed by a real in-memory project per locale (`?lang=en` a William Blake project, `?lang=zh-hant`/`zh-hans` a 朱耷 project). `capture-manifest.json` records the source commit, generator, and derivation of the states it captures.

`demo/` is `<moss-editor-demo>` + `<moss-demo-marker>` — the one real interface a documentation page shows, pinned beside the text, and the markers that hand it a scripted scene; see `demo/README.md` for the full contract and how to add a scene.

This directory is served verbatim (`[build].passthrough` in `.moss/config.toml`) — nothing under it is built as a page.

## Refresh recipe

1. Rebuild or export the desktop editor as self-contained HTML, preserving the app CSS, icons, locale strings, mock command boundary, and the exact state being documented.
2. Serve `site/` through the native moss preview, then run `PLAYWRIGHT_MODULE=/path/to/node_modules/playwright/index.mjs node scripts/capture-editor-guide-assets.mjs --preview=http://localhost:PORT/`. When Playwright is installed in this repository, omit `PLAYWRIGHT_MODULE`.
3. Update `capture-manifest.json` with the source commit and visible state. Never animate a gesture the harvested state does not implement.

## Interaction boundary

`app-editor.html` is a deterministic harvest of the real editor, suitable for the interactions it exposes. It is not the desktop application and must not be used as proof of filesystem, Git, deploy, or operating-system behavior.
