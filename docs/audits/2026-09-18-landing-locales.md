# Landing locale audit — 2026-09-18

## Implementation

The landing supports English, Simplified Chinese, and Traditional Chinese through `?lang=en`, `?lang=zh-hans`, and `?lang=zh-hant`. The former `?lang=zh` URL remains compatible and resolves to Simplified Chinese. When the URL has no supported locale, the last explicit choice in local storage is used, then English. The active locale sets the document `lang`, title, description, all five scene headings and body copy, calls to action, install and download text, form labels and messages, footer text, iframe titles, video controls, and accessible names. Product and operating-system names remain unchanged.

`site/landing-i18n.js` owns the locale catalogs and the URL/storage policy. The compact native language selector offers English, 简体中文, and 繁體中文 without flags. A language change navigates to the equivalent query URL so the language-specific editor, preview, and image plates initialize through the same path as a direct visit. Scroll position, the beta email value, and semantic form status are stored per URL in session storage and restored after navigation and browser Back. Completed statuses persist by message key and are translated into the destination locale instead of carrying old rendered text forward. The selector is disabled while a beta request is pending so changing language cannot abandon an in-flight POST.

Both Chinese locales use the authentic 八大山人 homepage fixture and artistic source text; the marketing interface alone is translated. The English editor uses the William Blake fixture. Homepage files suppress the extra editor title in all locales. Frontmatter remains represented as metadata while the editor body contains Markdown content, and the complete canonical Blake source remains available to the scene harness.

The language selector follows the page transition without changing geometry. On the light page it is dark text over a translucent paper surface; on the dark closing it is white text over a translucent dark surface. Native option rows stay dark on light for operating-system menu readability. The selector's visible options and accessible label are localized.

## Documentation destinations

Landing links use built output routes established by the source frontmatter rather than source filenames. English uses `/get-started/`, `/get-started/editor/`, `/get-started/design/`, `/docs/writing/media/`, and `/get-started/extend/`. Simplified Chinese uses the corresponding `/zh-hans/开始使用/`, `editor/`, `design/`, `links/`, and `extend/` routes. Traditional Chinese uses the parallel `/zh-hant/開始使用/` routes. Privacy continues to use the canonical `https://mosspub.com/privacy` address. All fifteen internal destinations were requested from the native preview and verified by HTTP status and page title so an HTML fallback could not count as a passing route.

## Native preview

The portable development command is:

```bash
cargo run -p moss-cli -- build site --serve --watch
```

The package and binary are both named `moss-cli` in `crates/moss-cli/Cargo.toml`. The preview port is selected dynamically; use the URL printed by the command. After the server is ready, the repository checker can validate routes and same-origin asset response types:

```bash
node scripts/check-site-preview.mjs http://localhost:<printed-port>/
```

The final scoped build passed 14 pages and 10 same-origin assets with no failures.

## Browser verification

The integrated native preview at port 8080 was checked in Chromium and WebKit at 390×844 for all three locales. Each engine verified the initial mobile view, Scene 1 after the intro, the closing and footer strings and controls, locale-specific editor URLs, localized titles and accessible names, and the language selector value. A language change from English to Simplified Chinese and browser Back both restored `scrollY = 1200` with the page's manual scroll-restoration policy.

Focused Chromium and WebKit checks also measured the selector in both tonal states. Both engines reported `rgb(44, 40, 37)` over `rgba(250, 248, 245, 0.9)` on the light page and white over `rgba(32, 32, 32, 0.9)` on the closing. The selector accessible names were 语言 and 語言. The final Chinese intro and closing screenshots under `/tmp/final-zh-*.png` were produced from the scoped locale worktree to inspect copy and selector contrast; they are not evidence for the integrated branch's final scene geometry.

Form verification mocked the existing beta endpoint without changing its request contract. It covered pending-state selector disabling, localized sending and success messages, semantic success translation across locale navigation, and selector re-enabling after completion. Clipboard checks covered localized announcements and removal of the transient copied message. Loop controls were checked after arrival and after toggling in both Chinese locales so later video state changes could not restore English labels.

## Fixture limit

The harvested editor fixture uses the real CodeMirror syntax, live shortcode, wikilink, embed, metadata-chip, and inline-widget components. The standalone landing fixture does not run the desktop application's Rust-backed wikilink completion command, so it does not provide the full production completion candidate dropdown. No fake completion backend was added. Completion behavior beyond the harvested local rendering remains an explicit fixture limit.
