# Harvested moss interface bank

These files preserve real moss interface states for the landing page and documentation. Reuse them before drawing a substitute interface.

| File | State | Intended reuse |
|---|---|---|
| `editor.html` | Self-contained harvested editor with mock data and local command adapters | Live editor scenes and interaction reference |
| `editor@2x.png` | Static editor capture, 880 × 1120 | Fallback when the HTML scene is too heavy or its interaction is irrelevant |
| `shell.html` | Self-contained harvested preview/publish shell | Live preview and publish scenes |
| `shell@2x.png` | Static preview capture, 1760 × 1120 | Static preview fallback |
| `mark.svg` | moss mark used by the harvested shell | Interface chrome only |
| `moss-ui-demo.js` + `moss-ui-demo.css` | Reusable `<moss-ui-demo>` controller and frame | User-started documentation demos around a harvested fixture |
| `editor-demo.html` | Standalone editor-writing demo in three locales (`?lang=zh-hant` / `zh-hans`) | No-script fallback and direct test surface |

The editor guide uses `../assets/guides/editor-ui-source.png` and `../assets/guides/choose-page-source.png` directly. `capture-manifest.json` records the desktop source commit, source paths, visible states, and derivation. The older `editor-map.svg` documentation montage remains in the bank but is not used by the guide.

## Refresh recipe

1. Rebuild or export the desktop editor and preview as self-contained HTML, preserving the app CSS, icons, locale strings, mock command boundary, and the exact state being documented.
2. Serve `site/` through the native moss preview, then run `PLAYWRIGHT_MODULE=/path/to/node_modules/playwright/index.mjs node scripts/capture-editor-guide-assets.mjs --preview=http://localhost:PORT/`. When Playwright is installed in this repository, omit `PLAYWRIGHT_MODULE`.
3. The script sets and focuses the sample through `window.__editor`, captures the compact editor state, clicks the real breadcrumb summary, waits for the expanded tree, captures that state, and closes the browser in `finally`.
4. Update `capture-manifest.json` with the source commit and visible state. Never animate a gesture the harvested state does not implement.

## Current state manifest

- Editor fixture: a William Blake site with a selected Markdown page, breadcrumb/file tree, property chips, title, body, and bottom toolbar.
- Shell fixture: the matching generated site in its preview and publish chrome.
- Locale: interface chrome is English. The localized landing scene supplies separate Chinese content; these banked files are not translated screenshots.
- Interaction boundary: `editor.html` is a deterministic mock of the captured editor, suitable for the interactions it exposes. It is not the desktop application and must not be used as proof of filesystem, Git, deploy, or operating-system behavior.
- Demo contract: `<moss-ui-demo>` never autoplays. Its quiet play/stop control sits above the harvested editor, and reader interaction pauses scripted typing. The `static` attribute prefills the real editable fixture without playback controls; reduced motion also shows completed content without animation.
- Guide loading: authored `<moss-ui-demo>` markup survives Markdown rendering. `.moss/theme/script.js` loads this banked module through `window.mossTheme.base`; the module resolves its stylesheet and harvested iframe from `import.meta.url`, so all three URLs retain a deployment subpath.
- Source of truth for behavior: current desktop code and release documentation. When either disagrees with a capture, refresh the capture or use text instead.

The source manifest distinguishes the captured fixture's commit from the desktop commit used to verify the written instructions.
