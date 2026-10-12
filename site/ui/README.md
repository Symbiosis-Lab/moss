# Harvested moss interface bank

These files are real moss interface, not drawings of it. They are generated from the desktop app's own code, so the landing page and the documentation show what the app shows. Reuse them before drawing a substitute interface, and never edit them by hand: a hand edit is lost at the next refresh.

| File | What it is | Who uses it |
|---|---|---|
| `app-editor.html` | The app's own editor, booted end to end with only the native seams mocked. Opening files, saving, menus, templates and versions are the app's code, running against an in-memory project per locale (`?lang=en` a William Blake project, `?lang=zh-hant` / `?lang=zh-hans` a 朱耷 project). | The documentation demo (`demo/`) and the two fallback stills below |
| `editor.html` | A hand-assembled editor panel built from the app's components, with a typing driver | The landing page |
| `shell.html` | The preview and publish shell | The landing page |
| `mark.svg` | The moss mark the shell draws | `shell.html` only |
| `demo/` | `<moss-editor-demo>` and `<moss-demo-marker>`: the one editor a documentation page shows, and the markers that hand it a scene | Documentation pages with a `#scene=` link; see `demo/README.md` |
| `../assets/guides/editor-ui-source.png`, `choose-page-source.png` | Stills of `app-editor.html`, shown where JavaScript is off | Meet the editor, in all three languages |

`capture-manifest.json` records which commit of the desktop app each harvested document came from: `app_editor_source` for `app-editor.html`, and `harvest_commit` with `captured_on` for the landing pair.

## Refresh

One command, run in the desktop app repository on a clean checkout of a pushed commit:

```bash
pnpm harvest:docs -- --site /path/to/this/repo/site
```

It builds the three documents from the app's current code, drives the result through the harvest's own end-to-end check in Chromium and WebKit, and only then copies `app-editor.html` here and writes its commit into `capture-manifest.json`. It then runs this repository's `docs:after-harvest` script (see `package.json`), which retakes the two stills from the new document with `scripts/capture-editor-guide-assets.mjs` and runs the scene checks that need no browser. Add `--landing` to copy `editor.html` and `shell.html` as well; that is a landing-page change, so run `node scripts/check-landing-all.mjs` before keeping it.

Then prove the pages a reader gets, against a real build:

```bash
target/debug/moss-cli build site
node scripts/check-docs-demo.mjs http://127.0.0.1:PORT/   # every scene plays, in both engines
node scripts/check-docs-media.mjs                          # every image, video and demo on the Get Started pages
```

`check-docs-demo.mjs` needs the build served (`moss-cli build site --serve` prints the address); `check-docs-media.mjs` serves the build itself when given no address. Both load Playwright from `PLAYWRIGHT_MODULE` when it is not installed here.

When a refresh breaks a scene, the app renamed or moved something a scene clicks. The fix is one line in `demo/surfaces/editor.js`, which is the only file that knows the harvested document's selectors; the scenes, the stills and the checks all resolve targets through it.

## What the harvest is not

- `editor.html` and `app-editor.html` run against mock data. Neither is proof of filesystem, version-control, publishing or operating-system behavior.
- Interface text in the harvested documents is the app's own; the landing page supplies its Chinese content separately.
- `shell.html` stays the harvest. What the landing page adds to it lives in `site/landing-shell.js`, which leans on the shell's `window.__shell` and on the app's publish receipt styles; `check-landing-desktop-pace` fails if a refresh changes either under it.
- When the app and a capture disagree, the app is right: refresh the capture, or use text.
