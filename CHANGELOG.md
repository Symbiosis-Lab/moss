# Changelog

All notable changes to moss will be documented here.

## [Unreleased]

### Fixed

- Floated images, embeds, and folder listings now collapse to a full-width block on phones instead of staying pinned at their desktop float width or an explicit size. `![[photo.jpg|align-right]]` without a size now floats at half the column on desktop, as documented; before, it rendered full width because the cap never matched.
- On vertically typeset sites, a `:::hero {.plate}` image is no longer cut off on long pages, and a plate with a caption no longer disappears entirely; the caption runs vertically beside the plate, centred on it.
- `--moss-card-cover-fit` now applies to list-style folder listings too, not just grids, so a site can show whole pictures in its card thumbnails instead of a centre crop.

## [0.7.14] - 2026-06-27

### New

- **Front-matter properties, reorganized** — the property picker now groups fields by scope (This Page / Child Pages / Child Styles / Whole Site) instead of "common/uncommon", orders them by how commonly they're used, and uses clearer names and tooltips. The two fields that both read "Also In" are now distinct ("Feed Slot" vs "Cross-list In"), and "Style" is now "Child Layout".

- **Playable video and audio in the editor** — embedded videos and audio now render as real, playable players in the live editor, with a hover handle to resize them just like images.

- **`%%` private comments** — wrap text in `%%...%%` to keep notes in your source that are stripped from the published site, matching Obsidian.

- **Zoom the editor and preview** — Cmd-plus / Cmd-minus / Cmd-0 adjust the editor font size and scale the preview pane.

- **Nested-site guard** — moss now prevents creating a new site inside a folder that's already part of a published site.

### Fixed

- Switching the app language while previewing a folder now reopens that folder instead of dropping to the launcher.
- The navigation pill and advisory panel no longer drag the window when clicked.
- Going offline no longer triggers a blocking error from Matters — a quiet status now appears on the Matters settings page, and a stale custom-domain step no longer posts a red error on every preview.
- Dragging an image into the editor now drops it on its own line with a clear horizontal guide; dragging into the file tree targets the folder directly under the cursor, works on breadcrumb folders, and auto-scrolls near the top and bottom edges.
- Clicking a folder in the file tree now only expands or collapses it; clicking a file (including a folder's home page) opens it.
- Importing from a URL is now a single, consistent action available on any folder.
- Front-matter inputs no longer auto-correct structured values like URLs and slugs, and property chips no longer get cut off at the edge of the bar.
- Clicking an asset in the editor now shows it in the preview as it appears on the published site, and no longer spins forever.
- The grid shortcode background now lines up with your content, and breadcrumbs use the full width when the editor is wide.
- Advisory messages are now translated, and "Open in Finder" reads correctly in Chinese.

## [0.7.13] - 2026-06-25

### New

- **Slash menu works mid-line, with block commands** — type `/` anywhere on a line (not just at the start) to insert a heading, bullet or numbered list, quote, code block, divider, image, or link, alongside the existing shortcodes. It stays out of your way inside URLs and mid-word.

- **References follow renames** — renaming a file or folder now rewrites every wikilink, embed, and Markdown link that points to it across your vault. Deleting a file that others link to shows a confirmation listing what will break, with the option to clean up those references for you.

- **Reader apply form, refined** — the `:::apply` form now shows field labels and an email helper line, left-aligned to match the site design, and renders in each page's own language on bilingual sites.

- **One copy button everywhere** — a single, consistent copy control across moss; the "Send logs" confirmation can now copy the ticket id with one click.

### Fixed

- Matters no longer re-attempts permanently-dead image downloads on every build, so the repeating "Image could not be downloaded" notice is gone. Images that are genuinely unavailable are now listed quietly on the Matters settings page instead.
- The frontmatter date field now shows the date in your locale with no time component and no off-by-one, and defaults to today when you add one.
- Settings → Deployment and Analytics now appear instantly from local data and refresh in the background, with an offline hint instead of a blank wait.
- Creating a new file in the editor's file tree reliably opens the rename field, writes the file, and selects it; reopening the editor starts with the tree collapsed.
- Dragging onto a folder shows a clear dashed outline, and dropping a file from Finder onto a row now files it in that row's folder instead of the project root.
- Background activity such as comment sync and update checks no longer interrupts you with advisories or blocking toasts; comment status moved to Settings → Services with a Retry. The advisory panel's expand/collapse handle and animation were tidied up.
- The layout divider no longer bounces back when you collapse it, and the right-edge controls stay fixed while you drag.
- Send Logs now captures failures from actions that previously failed silently (deploy, identity, email, config, plugins), so a problem report carries what's needed to diagnose it.

## [0.7.12] - 2026-06-24

### New

- **Reopenable Matters login** — connect your Matters account any time from Settings or while previewing an unlogged vault, without running an import. Login opens in a clean single-bar titlebar (identity, back, close), appears once on an unlogged preview (dismissible so it doesn't nag), and confirms with a "Connected to Matters" toast. If the login page can't load (for example behind the Great Firewall), you now get a clear message and a Retry button instead of a blank panel.

- **Ambient looping video** — embed a muted, autoplaying, looping background video with `![[clip.mp4|loop]]`. A subtle pause/play button appears on hover and is keyboard-accessible.

- **The app frame follows your preview** — the window titlebar now tints to match the backdrop of the page you're previewing, so the app chrome and your site read as one surface, including when you flip between light and dark.

- **Per-language footers and folder links** — bilingual sites can give each language its own footer (e.g. `zh-hans/footer.md`), and a folder link inside a language tree now resolves to that language's folder instead of the site root.

- **Fuller `moss describe`** — `moss describe --json` now reports the complete plugin contract (hooks, manifest fields, slots, and CLI commands) for plugin authors and tooling.

### Fixed

- The editor now renders `+++` dividers, nested `::::buttons`, and bare `![[note]]` embeds the same way the published site does.
- A folder or language-root homepage no longer shows a stray auto-generated folder title.
- Footers no longer leak a stray `>` character.
- In macOS fullscreen, the editor breadcrumb no longer leaves a gap where the traffic-light buttons sat.
- The editor no longer stacks a screenful of blank space above the body.
- Closing the login or browser panel before it finished loading no longer crashes the app.
- Article titles now match the spacing of body headings.

## [0.7.11] - 2026-06-23

### New

- **Unified dark mode** — the moon toggle in the editor is now the single control for dark/light: it pushes the theme into the preview immediately (no reload), pre-paints the correct theme before the first frame to eliminate flash, tracks OS-level preference changes, and removes the now-redundant theme radio buttons from App Settings.
- **Design tokens as a single source of truth** — all moss CSS tokens (colors, font sizes, heading weight, code palette, highlight colors) are defined in one place with explicit light and dark values, and the build emits them automatically. Your theme CSS in `.moss/theme/style.css` now takes top cascade priority without needing `!important`.
- **Heading weight is now themeable** — the `--moss-font-heading-weight` token controls the weight of all headings and article/folder/card titles (default 480; set 600 for bold). It was previously defined but inert.
- **Neutral navigation option** — a new `--moss-color-ui-accent` token separates nav/button chrome from content-link color, so you can keep links colored while making the navigation neutral.
- **`:::apply` form shortcode** — embed a reader invite/apply form in any page with `:::apply`; it collects an email, shows language-appropriate placeholder text, and confirms on submit.
- **`children_style: minimal`** — a folder option that forces the compact date + title listing on any folder, without relying on year-grouping auto-detection.
- **`moss describe` now includes frontmatter fields** — `moss describe --json` returns the full built-in frontmatter schema (type, widget, allowed values, default) so tooling and assistants work from the real schema.
- **Blueprint grid on every empty editor state** — the animated background now shows on all no-source pane states, not just the empty-folder onboarding screen.
- **Cloudflare Turnstile captcha support** — the comment form transparently handles a captcha challenge. Inactive until enabled server-side; existing comments are unaffected.
- **Matters force-fresh login** — each Matters login now clears the stored token and all domain cookies first, so you always authenticate fresh, and the folder rebinds to the newly authenticated account (self-healing a folder that drifted to the wrong account).
- **Friendly invite message when not allowlisted** — if the closed beta gate rejects your email during deploy setup, moss now shows the localized invite copy and the apply link instead of an opaque error.
- **`.moss/state.toml` can be tracked in git** — it's no longer auto-gitignored, so teams using a shared repo for cross-machine deploys can commit it.

### Fixed

- GitHub Pages deploys were publishing nothing — the plugin now reads from the active build generation instead of the always-empty legacy output directory.
- The progress panel showed a stuck "Waiting for you to Connect to Matters" advisory after a Matters import and couldn't be collapsed; the advisory now clears and the panel starts collapsed.
- After a Matters import into an empty folder, the editor stayed stuck on the "Create home file" placeholder even though the home file had been written; it now checks the filesystem directly when the build map lags.
- `article-map.json` is now written atomically, preventing partial-read failures during concurrent builds.
- The expanded progress panel could accidentally drag the OS window; the whole pill cluster is now no-drag.
- Matters login cookies (`__access_token`, `cf_clearance`) weren't cleared on macOS for httpOnly `SameSite` cookies; clearing now uses a domain-scoped path that handles them.
- The new-file creation flash used a loud emergency-green color; it now uses the same calm ambient-accent tint as the nav pill.
- The tray "Settings" item opened the per-folder settings modal instead of App Settings.
- Clicking the Dock icon / "Show Window" with nothing visible now surfaces the launcher (or the live preview) instead of silently reopening the last folder.
- The update-download toast no longer crushes CJK message text to a single character wide.
- Launcher dropdowns (language and subscriber-scope selectors) now pick up theme styling.
- The `:::apply` form placeholder is now language-specific instead of Chinese in every language.

## [0.7.10] - 2026-06-22

### New

- **青苔 — the app speaks Chinese as 青苔.** In Chinese, the app name, menus, tray, and onboarding now read 青苔 (the calque of the "moss" brand). English is unchanged.
- **Redesigned App Settings.** A cleaner launcher panel — a draggable titlebar that homes the window's traffic lights, with every setting's description always visible (no hover, no layout shift).
- **New files flash when created.** Files that appear in your vault — including during a Matters import — briefly highlight so you can spot them.
- **Smoother panel transitions.** Opening a plugin's external page (e.g. a Matters draft) now cross-fades from the editor instead of snapping.

### Fixed

- The editor could become uneditable after publishing — an invisible overlay left over from the publish animation swallowed clicks, so you couldn't place the cursor. It's now cleared the moment publishing finishes.
- The empty editor (no file open) showed a doubled divider; it's now a single line.
- "Published with moss" now follows your system language instead of defaulting to English when a site declares none.
- Canceling a Matters sign-in returns you to the editor, with a calmer sign-in label.
- A Matters import now titles your homepage from the vault folder name rather than your Matters display name.
- Comment author names now link to the author's website, and the reply arrow is monochrome.
- Fixed a race that could mis-render a plugin's browser panel when it first opened.
- Onboarding hints now reset correctly when you return to the editor.

## [0.7.9] - 2026-06-22

### New

- **Localized in three languages** — the whole app now speaks English, Simplified Chinese, and Traditional Chinese: editor, launcher, tray, settings, onboarding, the publish flow, and progress messages. Your published site also gets the correct language tags (Traditional Chinese is no longer bucketed as Simplified).
- **A real email composer** — sending a post to subscribers opens a full editor in the side panel (not a cramped modal), with an editable from-name, an auto-filled subject, a live preview field, and a clear recap of exactly who will receive it before you hit send. The email is its own draft and never rewrites your post.
- **Moderate comments from the preview** — hover any comment while previewing your published site and click Hide; it disappears on the next publish, with an Undo. Hidden comments stay hidden and can't reappear when comments sync.
- **Private notes in the editor** — press ⌘⇧M to leave a comment on your own draft. Click the gutter marker to read, edit, or resolve it. Notes never reach your published page or your email.
- **Sharper live preview** — clicking into bold, italic, a link, or inline code now reveals just that piece of raw markdown instead of the whole line, and a revealed link keeps its colored text while showing its URL muted beside it.
- **Drag files straight in** — drag images, video, audio, or PDFs from Finder/Explorer (or from the file tree) into the editor and moss imports them and drops in the right link at your cursor. Drop a whole folder to import it. Paste and drop now behave the same.
- **A calmer publish** — publishing shows a single progress ring covering every step (build, deploy, email, Matters), the editor locks for the few seconds a publish is in flight so an edit can't fork what ships, and syndication to Matters auto-detects when your post goes live.
- **Faster, safer deploys** — each build is an immutable snapshot, static files ship with content-hash names (no more stale-cache surprises after deploy), large files upload in chunks (no more Cloudflare timeouts), the progress bar fills by bytes uploaded, and a deploy that would wipe an unexpected number of files is blocked.
- **`listed: false`** — keep a page reachable at its URL but out of your feeds, nav, sitemap, and RSS. (`draft: true` still keeps a page out of your published site entirely.)
- **A launcher that stays put** — the launcher window now coexists with your editor and reopens instantly; Settings opens over whatever you're doing without disrupting it; the editor opens and closes with a smooth animation and the first file opens instantly.
- **Cover colors and more formats** — folder and grid cards pick up a cover color from the page they link to (or an explicit `color=` you set), and avif images and aac audio are now supported.
- **Crash reporting, scoped to crashes** — product analytics are gone; the only diagnostic is opt-out crash reporting, with file paths and personal data scrubbed before anything leaves your machine.
- **Matters handles login expiry gracefully** — an expired Matters session is detected up front with a clear prompt to log back in instead of failing mid-sync, and audio, images, and covers now upload reliably.

### Fixed

- The preview no longer shows a blank blueprint grid on cold start — it serves your last build immediately.
- Opening an article right after you rename its URL no longer gets stuck on a 404.
- Deploys no longer re-upload unchanged videos every session, and videos are no longer missing from the upload.
- Draft pages no longer leak into your top navigation or RSS feed.
- The publish-in-progress divider no longer flashes mid-drag, and the editor/preview no longer leave a gap or overlap at rest.
- Dark mode now reaches the editor and the email composer; the window's shadow follows its rounded corners.
- Matters: only a confirmed publish marks a post as synced; stray `#` permalinks no longer appear in Matters posts; comments now download for short-link Matters URLs.
- Building from the command line with a relative folder path no longer leaves the preview pointed at a stale build.

## [0.7.8] - 2026-06-11

### New

- **Email composer** — sending a post to your subscribers now opens a full composer in the
  side panel instead of a small modal. The email is its own draft (your post is never
  rewritten), it auto-saves while you edit, and each article shows a lamp in the channel
  rail: draft, armed, sent, or failed at a glance.
- **Ship with a ring** — publishing shows a progress ring around the Ship button, and your
  editors go read-only for the few seconds a send is in flight, so a mid-send edit can't
  fork what your readers receive.
- **Subscribe form from day one** — with email enabled, the footer subscribe form now
  appears in preview before your site is even published, renders identically in every kind
  of build, and publish verifies the form is wired to your live site before pushing.
- **Paste, drop, and formatting in the editor** — paste a URL onto selected text to make a
  link, paste screenshots straight from the clipboard, drop files at the cursor; bold and
  italic toggle whole words; ⌘K leaves your cursor in the link target; shortcodes insert as
  templates you can tab through.
- **Richer text rendering in the editor** — inline markup now renders inside table cells,
  markdown syntax reveals itself whenever your selection touches it (one consistent rule
  everywhere), and unresolved links dim quietly instead of flagging while you're still
  drafting.
- **Faster editor open** — the editor panel boots in one round-trip with a static skeleton,
  replacing the blank flash on cold start.
- **Settings from anywhere** — tray → Settings now opens App Settings from any folder (the
  window shrinks home to the launcher), the launcher lists your recent folders, and the
  tray menu is down to four entries. App Settings itself is reorganized into three calm rows.
- **Steadier window** — the preview never shifts while you drag the panel divider or resize
  the window; only the panel moves, and divider drags are smoother.
- **One progress language** — builds, publishes, domain checks, and plugin tasks all report
  through the same job system: consistent phrasing and counts in the panel, and quieter,
  short-lived toasts.

### Fixed

- Windows packaged builds no longer fail at the frontend build step.
- The email subscribe footer no longer appears and disappears depending on which build wrote
  the site last, and sites without a home note now get the form on the homepage too.
- Video wikilinks with a size hint (`|50%`) are no longer promoted to image figures.
- The settings modal keeps its size when switching tabs, and App Settings fits the launcher
  window.
- Escape pressed in the editor stays in the editor, and each media embed renders exactly one
  card.
- macOS native fullscreen exits cleanly before the window shrinks home to the launcher.
- The preview scrollbar redraws correctly after the divider moves.

## [0.7.7] - 2026-06-10

### New

- **Native fullscreen** — moss now uses real macOS fullscreen (the green button, Ctrl-Cmd-F, or
  View → Enter Full Screen): the window gets its own Space, the menu bar reveals on hover, and the
  moss title bar stays visible. The window can no longer get stuck in a half-fullscreen state.
- **App Settings in the launcher** — set your language, check for updates, and install the
  command-line tool from a new App Settings screen, opened from the launcher via the moss mark or ⌘,.
- **iCloud-aware video conversion** — videos stored in iCloud Drive no longer produce a flood of
  "moov atom not found" conversion errors. moss waits for the source to download before converting,
  and shows a gentle "still downloading from iCloud" note if it isn't ready yet.
- **Editor renders images from source** — the editor now shows images straight from your source
  files and refreshes them live as the source changes, rather than the published/optimized variant.
- **Build-progress surface** — a refreshed build-progress indicator with a single progress bar and
  a calm resting state.
- **Tidier build advisories** — when many files hit the same problem, the build panel collapses them
  into a single summary row instead of one row per file.

### Fixed

- Collections with an explicit `order:` are no longer re-sorted by date — your chosen order wins.
- Right-click → Publish on a cold start now opens the right folder at the right size, with no
  launcher flash.
- Error messages no longer show raw Cloudflare challenge HTML — just a short, readable message.
- Bundled plugins rebuild correctly instead of silently shipping a stale version (matters → 1.1.1).

### Work in Progress

Active branches not yet in this release — landing in a future version:

- **App Settings, phase 2** — reach App Settings from inside a folder (shrink back to the launcher).
- **Build advisory panel polish** — finishes the in-app build-advisory surface.
- **Open-source publishing pipeline** — publish a site's source alongside the built output.
- **Email surface redesign** — reworked email/invite flow.
- **Opt-in usage telemetry** — consent-gated diagnostics to help find crashes.
- **Windows support** — Windows build fixes and end-to-end smoke tests.

## [0.7.6] - 2026-06-08

### New

- **Per-site AI policy in `robots.txt`** — Every published site now states how AI crawlers may use
  its content, and sites **decline AI training by default**. No setup needed; it ships in your
  site's `robots.txt` automatically.

- **Matters import opens to a real home page** — Importing from Matters.town now creates a
  self-named home file (with a `home: true` marker), so the imported site opens to a proper home
  instead of landing on an arbitrary first article.

- **Build advisories in the app** (early) — Build-job advisories now surface in moss's interface —
  the first slice of a larger build-status design. More polish is on the way.

- **Point a project at a non-production backend** — A project can now target **production**
  (default), **staging**, or **local** through an `environment` key in `.moss/config.toml`, readable
  and writable with the new `moss env` command. When a project isn't on production, the titlebar
  shows a **STAGING** / **LOCAL** badge so you always know which backend you're talking to. This is
  primarily for testing — normal sites stay on production with no change.

### Fixed

- Links inside grid cards now resolve against their page, so multilingual sites no longer collide
  across languages.
- The update-download notification no longer flickers on slow connections.
- "Send logs" now handles Cloudflare challenge responses gracefully instead of surfacing raw error
  HTML.
- The desktop app no longer writes redundant DNS records for domains already hosted on moss.

### Work in Progress

Active branches not yet in this release — landing in a future version:

- **Build advisory panel polish** — finishes the in-app build-advisory surface previewed here.
- **Editor image refresh** — embedded images update in the editor when the source file changes on disk.
- **iCloud-aware video conversion** — handles iCloud "optimize storage" video inputs (the "moov atom not found" race).
- **Invite UI** — invite-based onboarding and sharing.
- **Open-source publishing pipeline** — a control panel with a publication manager, plus a publishing runbook.
- **Email-flyout removal** — a simpler subscribe experience.
- **seta error consolidation** — uniform error handling across backend calls.
- **Opt-in product telemetry** — consent-gated, so it's off unless you turn it on.
- **LLM-automation testability** — a headless harness for agent-driven testing of moss.
- **Windows port** — CI triage and an end-to-end smoke test (Windows is still not a shipping target).

## [0.7.5] - 2026-06-07

### New

- **Connect an AI coding agent to your site** — `moss agents install-skill` teaches Claude Code or
  Cursor how to author, style, extend, and import a moss site the canonical way; moss detects each
  installed tool and writes the skill into it. When a supported agent is present, the launcher also
  shows a one-tap "Connect" hint that installs the skill (and, on macOS, the `moss` command-line
  tool) and then quietly fades away.

- **Pick any file as a folder's home page** — Add `home: true` to a file's frontmatter to make it
  that folder's home regardless of filename; the marker survives folder renames, and on a
  multilingual site a home carries over to its translations automatically. "Start writing" and the
  create-home buttons now make a self-named home file (e.g. `blog.md`) with the marker in place.

- **Import your writing from Matters.town** — The Matters channel now imports reliably end to end:
  a single build pulls in every article and image, Chinese sites get a `文章` folder, and after the
  import the home opens, the file tree expands to your new articles, and progress shows live in the
  editor — the breadcrumb pulses while articles land and flashes green when it finishes.

- **Embed and resize folders, PDFs, videos, and apps** — `![[folder/]]`, `![[file.pdf]]`,
  `![[clip.mp4]]`, and the like now render as full-width cards in the editor that you can drag-resize
  from the corner. Embedded folders and apps also honor a size on the published site:
  `![[app/|80%]]`, `![[app/|800x600]]`, `![[app/|80vh]]`.

- **Share cards for every page** — Section landing pages and folder index pages now generate their
  own Open Graph / Twitter share card, with an auto-made card image, so pasting a link like
  `/research/` into Slack, iMessage, or X unfurls with that page's own title and image. The homepage
  also emits WebSite structured data so search engines and AI tools recognize your site.

- **Settings no longer closes your editor** — Opening Settings now dims only the preview and leaves
  your editor open underneath; the editor panel animates open and closed with a smooth morph, and
  the Settings dialog sizes to its content (no stray scrollbar).

- **Immersive controls on every embedded app** — Fullscreen and open-in-new-tab controls now appear
  on every local embedded app or article on a page (previously only the first), while external
  embeds are left alone.

### Fixed

- Embedded folders and apps (`![[/Resources/app/]]`, or any folder whose name has capital letters or
  spaces) could 404 on the published site even though they worked in local preview; they now resolve
  on deploy. Rebuild and republish to apply.
- A multilingual site's home page listed articles from the other languages; it now lists only its
  own (default) language.
- The home page's browser-tab title read "index" instead of your site's name; the tab title and the
  share-card title now always agree.
- Empty folders showed a "No articles yet." placeholder; they now render honestly, with no
  placeholder and no empty card box.
- Opening a folder or home that has no home file could silently open an unrelated article in the
  editor; it now offers to create the home file instead.
- Broken or not-found links and embeds now show in red rather than muted gray.
- Frontmatter validation no longer flags moss's own managed fields (like `home`) as "unknown field."
- Editor images no longer overhang the body text, tall images show at full width, and a small image
  given a `|NN%` width now scales up to match on the published page.
- A folder index without its own page now shows the same heading text and casing as one that has a page.
- `llms.txt` no longer includes raw `<style>` / `<script>` blocks.

### Notes for theme authors (breaking)

- **`layout: visual` is removed.** The black, full-bleed Visual template no longer exists. A page
  that still sets `layout: visual` now renders with the standard Page chrome, and the `.moss-visual`
  class is no longer emitted; no frontmatter value reproduces the old look.
- **Folder-derived names are humanized.** A folder `tech-tips/` without its own page now renders the
  heading "tech tips" (matching the page-backed case) instead of "Tech Tips", and a site whose name
  comes from a folder like `my-site` displays as "my site". Set an explicit `name` in
  `.moss/config.toml` to pin a specific site name.

## [0.7.4] - 2026-06-05

### New

- **Resize and place images in the editor** — Drag an image's bottom-right corner to
  resize it to a percentage width (double-click to reset), or type `|50%` after the
  filename. Images render inline with their source line just above (Obsidian-style),
  and images narrower than the column center automatically. Percentage widths carry
  through to the published page.

- **Live link and asset checking** — The editor flags broken links, broken
  `![[wikilinks]]`, and case-mismatched targets as you type, with a hover tooltip
  explaining each one. Image and asset references are checked the same way, and the
  build now resolves `./assets/…` subfolder paths and case mismatches more reliably.

- **In-place preview updates** — The live preview morphs in place on rebuild instead
  of doing a full reload, eliminating the blank flash WebKit showed on every edit. A
  slim progress bar runs along the nav pill while the site rebuilds, flashes green when
  it lands, and your scroll position is preserved.

- **Clearer editor panes for folders and empty pages** — Selecting a folder or a page
  with no source no longer shows stale content; the pane reflects the real state — an
  empty content folder, for example, offers a "create index" action.

- **Cleaner top-nav pages** — Pages and folders in the top nav no longer get a duplicate
  auto-injected title (the nav already names them) and use a tidier page layout.

- **Resilient frontmatter** — One malformed frontmatter field no longer blanks the whole
  page. moss keeps every valid field, self-heals values like an all-numeric `uid:` (which
  previously dropped all frontmatter and fell back to the filename), and surfaces a real
  parse error instead of failing quietly.

- **Website-only publishing by default** — Publishing starts with syndication channels
  unselected (website only), and resets to website-only when you switch folders.

- **File tree starts out of the way** — The file tree opens collapsed and is a sticky
  toggle, so navigating around your site never pops it open.

### Fixed

- Publishing could hang indefinitely on a stalled upload; it's now bounded by timeouts and
  fails retryably, and large-file uploads got a more generous per-request timeout (no more
  spurious 120-second failures).
- Edits sometimes didn't refresh the preview; the rebuild baseline is now race-free,
  double-fires are coalesced, and switching folders drops the previous folder's stale baseline.
- Editor-to-preview scroll sync used incorrect line numbers and could mis-map; it now uses
  real file lines.
- Text and images now stay aligned under any theme font — the content column uses a
  font-stable measure.
- `figcaption` could render at the wrong size from an undefined token; it now uses the
  defined caption size.
- Dark-mode hover color on article links was too dark; it's lightened to match the palette.
- Links inside grid prose cells keep their underline again (the navigational-card underline
  strip is now scoped to link cells only).

### Notes for theme authors (breaking)

- **Reading-size tokens renamed** — `--moss-font-size` / `--moss-font-size-base` are now
  `--moss-reading-size` / `--moss-reading-size-base`. Update any theme that overrode the
  old names.
- **Caption-less images in grid-card links now wrap in `<figure class="moss-image">`**
  (previously a bare `<p><img>`). If your theme positions grid-card images via
  `.moss-grid-card[data-kind="link"] > p:first-child`, add
  `> figure.moss-image:first-child` to that selector.

## [0.7.3] - 2026-06-03

### New

- **Find in editor and preview** — `Cmd+F` opens a find pill in both the editor
  (CodeMirror-backed find + replace, regex, case toggle) and the published-site
  preview (uses WebKit's window.find via an iframe RPC bridge). Same keystroke,
  same visual pill, two surfaces.

- **App language and locale** — moss now detects your system locale on first launch
  and switches the editor, file tree, import modal, folder mode, and chip-bar UI
  between English, Simplified Chinese, and Traditional Chinese. Change it from the
  tray menu; the app restarts safely to apply the new locale across all windows.

- **Editor formatting shortcuts** — `Cmd+B` for bold, `Cmd+I` for italic, `Cmd+K`
  for link, `Cmd+S` for save, `Cmd+~` for code (and the markdown toolbar) now
  toggle the surrounding markers on a selection. Toggling off respects nesting
  (italic inside bold is removed without touching the bold markers) and handles
  URLs containing parentheses correctly.

- **External link previews and embeds** — Paste a bare URL on its own line and
  moss turns it into a link preview card (with title, favicon, domain). YouTube,
  Vimeo, and other recognized providers turn into responsive iframe embeds via
  the new `![[url]]` synthesizer. Add `?t=120` or `&t=2m` to a YouTube URL and the
  embed picks up the timestamp.

- **External links open in a new tab** — `<a>` tags pointing outside the published
  site now carry `target="_blank" rel="noopener"`. In-site and same-origin links
  are unaffected.

- **Hero mobile layout** — `:::hero` blocks now honor a `mobile=` attribute
  (`mobile=overlay` keeps the overlay style on narrow screens; default stacks the
  text below the image instead of cropping wide photos on both sides). The hero
  also picks up a dominant cover color and exposes it as a CSS variable for theme
  authors.

- **Shortcode blocks render live in the editor** — `:::grid`, `:::hero`, and other
  block shortcodes now show as styled blocks while you type instead of raw fence
  syntax, with no flicker on edit. A slash menu inserts shortcodes, and `:::` and
  attribute names autocomplete. Click a rendered block to drop the cursor back into
  its source.

- **Theme assets mount verbatim** — Anything in your `.moss/theme/` folder is now
  copied to `/_moss/theme/` with its structure intact (previously flattened), so
  theme JS, fonts, and media keep their paths. A new `window.mossTheme.base` value
  gives theme scripts a stable prefix for asset references.

- **Passthrough subtrees** — A folder containing its own `index.html` is copied to
  the output verbatim — no image transcoding, no SPA injection — so you can drop a
  pre-built site (a game, a bespoke page) inside your moss site. Override detection
  with `[build].passthrough` in config (use a leading `!` to exclude).

- **Source-mapped previews** — Every shortcode block, breadcrumb, series-nav, and
  auto-injected article H1 now carries a `data-source-*` attribute. Clicking
  preview chrome in the editor jumps the cursor back to the line that produced it.

- **Live subscribe form in preview** — The newsletter subscribe form now renders
  fully styled in preview with a local success animation and hint when you submit,
  instead of appearing grayed out. Submitting in preview never sends a real request
  or fires confirmation emails.

- **Smoother launcher-to-editor transition** — Opening a folder from the launcher
  now grows the launcher window into the editor in place instead of swapping to a
  separate window, and the launcher, preview, and folder picker open centered on
  screen instead of jumping to the upper-left after a resize.

- **Smarter file-tree behavior** — When you delete the file you're editing, the
  editor falls back to the parent folder instead of going blank.

- **Settings polish** — Sites deployed via moss-host (with a `site_id`) hide the
  Buttondown api-key section in settings (the seta subscribe form takes over).
  Sites without a site_id keep the Buttondown configuration as before.

### Fixed

- Wikilink fragments with section headings (`[[Page#Heading]]`) are now slugged to
  match the rendered heading id. Section links never worked before for non-ASCII
  or multi-word headings.
- The hero overlay text on mobile no longer crops wide images on both sides — text
  flows below the image at its natural width.
- Heading anchor `#` symbols are excluded from text selection; clicking the
  permalink no longer drags-selects the heading text.
- The folder-embed `more:` frontmatter flag was removed — a "More" link now
  appears automatically on truncation.
- Heading margins respect Ma 間 rhythm: H1/H2/H3 use level-specific top/bottom
  spacing so headings gravitate visually toward the content below them.
- Subscribe form gating switched from `deploy_method` to `site_id`. Sites with
  a site_id but absent or stale `deploy_method` (older moss versions, or sites
  with custom workflows) now get the seta form correctly.
- File watcher refreshes the in-memory build hash store on rebuilds instead of
  re-reading the stale `hashes.json` from disk.
- Files with image extensions but non-image content (e.g. text saved as `.png`)
  are now skipped during compression instead of corrupting the pipeline.
- PNG images with transparency are flattened onto white before WebP encoding, so
  transparent regions no longer turn black in the published output.
- The frontmatter asset chip thumbnail is vertically centered and the chip uses a
  two-box layout so the overflow menu button is never clipped; thumbnails preserve
  transparency.
- Breadcrumb segments no longer truncate to an ellipsis a fraction of a pixel early
  on wide panels.
- Editor asset autocomplete no longer suggests files inside `.moss/` or other
  hidden directories.
- Heading anchor spacing and touch target visibility improved on mobile.
- Children-list links no longer inherit article link underlines.
- Live preview no longer crashes on a single-character frontmatter `title:` value.
- The editor keeps its undo history when a file is reloaded from disk (e.g. after
  an external edit), so Cmd+Z still works across reloads.
- Publishing skips macOS Finder aliases that can't be resolved instead of copying
  the raw alias blob into your site.
- Diagnostic logs sent via Send Logs are quieter and more useful — repetitive
  coherence-check noise is collapsed and deploy events are marked.


## [0.7.2] - 2026-05-31

### New

- **Heading permalink anchors** — Every heading in your published site now has a `#`
  anchor link. Hover to reveal it; click to copy the URL to clipboard.

- **Wikilink autocomplete with headings** — The `[[` autocomplete now suggests page
  headings alongside pages and assets. Type `[[Page#` to filter sections within a page.

- **Deploy any static site with moss** — `moss deploy --prebuilt=<dir>` publishes an
  externally-built site (Next.js, Quire, etc.) through your moss domain. moss handles
  hosting, custom domains, and CDN — you bring the output folder.

- **Unified folder listing** — Frontmatter `children:` and embedded `![[/folder/]]`
  now go through one renderer. Sorting, styling, and card density are consistent
  whether you configure them in frontmatter or via an embed.

- **Optimistic file operations** — Deleting or duplicating a file in the tree updates
  immediately without waiting for a rebuild confirmation.

### Fixed

- Nav width now tracks `content_width: wide` and `content_width: full` correctly.
  Previously it stayed at the default width regardless of the setting.
- Live preview no longer crashes when hovering over a wikilink to an image that
  hasn't been resolved yet.
- Multi-line linked-card delimiters (`:::grid`, `:::cards`) are now hidden in live
  preview, matching how they look in the published site.
- Heading anchors are suppressed on hero overlay headings, where they would have
  overlapped the cover image.
- Wikilink autocomplete no longer triggers inside code blocks; `]]` closes
  automatically on accept.


## [0.7.1] - 2026-05-31

### New

- **`moss` CLI on PATH** — Install the `moss` command system-wide from the tray menu.
  Once installed, use `moss compile`, `moss preview`, and `moss deploy` from any terminal
  without pointing at the app bundle.

- **Grid card descriptions** — Frontmatter `description:` now appears below the title
  in `{{grid}}` cards, giving readers a summary without opening the page.

- **Page titles never pulled from body H1** — moss now follows Obsidian's convention:
  the page title always comes from the filename or `title:` frontmatter, never from the
  first heading in the document body.

- **Adaptive file tree columns** — The file tree breadcrumb and indent columns now
  resize together based on your actual folder depth, so deep hierarchies stay readable
  without wasting space on shallow ones.

- **Save status moves to titlebar** — The save indicator now lives in the window titlebar
  rather than the editor chip bar. It only appears on failure — successful saves are silent.

- **Flush save on quit** — Closing or quitting moss now waits for any in-flight editor
  save to complete before the app exits, so unsaved changes are never lost.

### Fixed

- Launcher now remembers recently opened folders across sessions.
- Description text on dominant-color grid cards is now white for legibility.
- Summary and year-group card titles were missing their hover accent color.
- Plugin tasks were crashing on every run due to a wire type mismatch in task IDs.


## [0.7.0] - 2026-05-30

### New

- **Silent auto-save** — The editor now saves automatically as you write. There's no
  save button and no success notification — the indicator only appears if a save fails.

- **File tree overhaul** — The file tree is fully rebuilt with keyboard navigation,
  multi-select (Shift+click, Cmd+click), drag-and-drop between folders, OS drag-in
  from Finder, drag-out to external apps, and batch delete/move. Chevron and filename
  are now separate hit targets so expanding a folder doesn't also select it.

- **GFM tables in live preview** — Markdown tables now render as styled HTML in the
  editor preview, matching what your published site shows.

- **Wikilink base survives rename** — If you rename a file, any `[[wikilink]]` or
  asset reference in the editor that pointed to it updates its base path automatically.

- **Folder page empty state** — When you open a folder that has no index page yet,
  the editor shows a "Create page" prompt instead of a blank preview.

- **Onboarding launcher** — A new launcher window appears when no recent folders are
  available, with animated background and a single "Open folder" CTA.

- **Bundled blog and portfolio templates** — New site creation includes starter
  templates for blog and portfolio layouts.

- **`moss edit <path>`** — Open a specific file in the moss editor directly from the
  command line, scriptable from other tools.

- **Summary layout auto-detection** — Folder indexes with no dated children
  automatically switch to summary card layout.

### Fixed

- Preview navigation (SmartDiff) now tracks the page actually shown, not the last
  entry in history — fixes stale preview after clicking between pages quickly.
- Folder index URLs now resolve correctly in the editor link panel.
- `moss preview --watch` flag was silently ignored; it now works.
- Plugins failed to appear after first install without restarting the app.
- Grid folder cards were sorted by URL path instead of date.
- Web importer now handles image URLs that contain parentheses.
- `external_url` frontmatter now warns if the value isn't a valid http(s) URL.
- Hero and Gallery shortcode image URLs now resolve against the content graph
  rather than the filesystem path.
- correct ops_for doc-comment — production executes mount_preview directly
- moss-claude.sh auto-runs pnpm install in a fresh worktree (#728)
- preview follows breadcrumb/tree rename — wait on FileChanged not BuildComplete (#731)
- preview follows breadcrumb/tree rename — wait on FileChanged not BuildComplete (#731)
- restore @tauri-apps/plugin-dialog + grant dialog:allow-open
- unify rename-path math; fix root-level file rename (leading slash)
- inline folder-note pair as flex row; update grid-based tests to --ft-depth contract
- align bc seg inset to --ft-base (kill 4px spine shear); restore 1px row gap
- heading rename must pass absolute paths to rename_entry
- address pre-merge review (flash guard, listener signal, docs)
- render nested markup + inline images in CM6 live preview
- mount fresh preview into persistent shell, never destroy main (ADR-017)
- deterministic listener cleanup for inline-URL card
- one-shot auto-open latch per opened folder
- load empty-home artifact instead of empty-state overlay
- scope ambient hairline to action-panel tasks; kill idle "Idle" tooltip
- drop left-click handler so clicking the icon just drops the menu
- sync — disable actions/checkout extraheader (matches feat/open-source-pipeline)
- make onboarding T6/T9 tests typecheck-clean
- hide animated blueprint-grid canvas in folder-mode-empty snapshot
- round-3 nit — accurate Reshow-race docstring
- round-2 review feedback — panic safety + restructure
- round-1 review feedback — gate, post_show, helpers
- drop quit-on-close so closing doesn't kill the app
- stub resolveUrlForFile with a Result in editor-external-update
- ESM-safe __dirname resolution in e2e helpers
- register moss-recent vocabulary in COMPONENTS
- work around Tauri 2.10 PathResolver init-order panic
- scrape-progress payload pages_scraped is string per bindings, parse with Number()
- use Tauri emit_to for cross-webview drill-down (CustomEvent doesn't cross webviews)
- debug_assert single-block reparse to harden block_meta invariant
- split file-tree DOM into bc-strip + nav-body siblings
- subgrid breadcrumb strip; drop sticky + tree-grid container
- TaskHandle methods no-op when Tauri unavailable
- .nav-grid 2-row grid; .nav-body subgrid w/ unconditional overflow
- clean up on tone escalation (inline -> awaiting, etc.)
- coordinate slot.display between Narrator and Awaiting
- add data-inline-task-anchor="save" for T7's Inline badges
- mount Inline + toast subscriber in editor-main.ts (action-panel webview)
- exclude frontend/app/preview/__tests__/e2e/** from vitest collection (T2 follow-up)
- large card spans full row width per spec mockup
- gate tasks_dev behind cfg(debug_assertions) to prevent release leak
- restore ActivationPolicy on auto-open failure path
- cover Shortcode::Recent in dispatch_wikilink_embeds
- align icon-armed default with publish gate (round-3 convergence)
- dispatch_wikilink_embeds descends into Grid cells + Hero overlay
- convergence round-2 — articleScope wire + icon re-derive + phantom analytics
- exclude breadcrumb segs from selection visual state
- scrollEditorToLine(1) sets scrollTop=0
- scrollToSourceLine(1) goes to scrollTop=0
- preview reports line 1 when at literal top
- wire openSendModal into publish flow + finish lang→scope rename
- convergent Tier-A review fixes for Task 5.7
- audience line includes "confirmed" per spec
- gate icon-state override on user touch (Task 5.4)
- address P2 caveats from architecture + code review
- PR3.5 — classify wikilink-image alt to match production
- v2 amendment — keep Stage 1 callouts.rs until PR7a flips production
- drop dead _primary_lang param + comment/test polish
- tighten stale comment + document parse_inline_event scope gap
- PR0.6 — preserve inline content in tight-list items
- PR1 v2 — use MarkdownInline for DefaultHooks::render_image
- review feedback for dual-session-ports
- restore figure.image wrap for matters' sanitizer
- close native gap so hairline is the only visible seam
- wire-domain conversions for cross-webview boundaries (#711)
- close Stage 2 dispatch coverage gaps (list-item Tag::Image, format_img_tag)
- remove dead || true guard around setDropHighlight(null)
- add aria-posinset and aria-setsize to tree-body rows
- first row keyboard-reachable when no selection (initial focus)
- add aria-label to tree container (WAI-ARIA)
- updateTabIndex moves DOM focus + scrollIntoView for SR users
- replace in-place override mutations with functional rebuild (ADR-014)
- move_one same-dir check uses canonical paths
- add project-root containment to import_files_to_dir (security)
- extract commitRename helper; preserve EntryId across in-app renames
- normalize source-domain wire paths to absolute before registry route
- layered Esc — clear selection first, collapse tree on second press
- fix TypeScript strict-mode errors in keyboard integration tests + editor-main
- sniff format by content, not extension
- port multi-select to Overrides model + unmount cleanup
- add wireDragOut to home file row (arch review #2)
- preserve toggle anchor on deselect; reconcile falls back to survivor
- omit Rename from project-root context menu


## [0.6.7] - 2026-05-26

### Added
- drag commits expansion via Overrides (bug #1, mode b)
- add nextExpandableFolder for drag-past-top reveal
- ImageContext::HeroBare for no-snapshot hero fallback
- add visibleStateV2 + effectiveNavHV2 (Overrides API)
- add toggleNodeV2 with unified Overrides
- add Overrides type and applyOverride helper
- register audio/PDF/static assets in AssetRegistry via copy_deferred_assets
- emit data-placeholder-src/poster/dims natively (regex-parity)
- fix getPreviewBaseUrl race with lazy getter in hover extension
- carry LQIP from AssetState::Pending through to Ready
- promote currentEntryId to sole source of truth
- track active entry by EntryId; swap isActive check
- swap editor-main identity comparisons to EntryId
- route through synthesize_iframe_html, retire moss-core escape valve
- build-time warning for raw <img>/<video> in markdown
- ImageContext::SiteLogo + route nav.rs through it
- ImageContext::EmailBody + route newsletter.rs through it
- ImageContext::TrackingPixel + route rss.rs through it
- route FileChangeEvent source-domain fields to EntryRegistry
- track currentEntryId alongside currentFilePath
- populate EntryRegistry during refreshTree
- add EntryRegistry module for session-scoped file identity
- asset widget with inline thumbnail + path validation
- asset hover preview + CSS seam
- cm-asset-resolver — body asset path validation
- asset resolution Tauri commands
- ChipBar + editor-main use FrontmatterStore
- save_editor_content returns mtime for echo suppression
- FrontmatterStore reactive module
- detect and warn when port 1420 is already held
- create_file writes empty files, no title: field
- monotonic heading routing — filename is the title
- Stage 2 dispatcher for moss-extension embeds
- add onBeforeDestroy callback for DOM rescue
- synthesize_image_html consumes AssetSnapshot
- synthesize_model_html implementation
- synthesize_pdf_html implementation
- synthesize_iframe_html implementation
- synthesize_video_html implementation
- synthesize_audio_html implementation
- populate source-domain fields on FileChangeEvent
- add compute_source_change_set helper
- add source-domain fields to FileChangeEvent
- Stage 1 sweep — native ![](file.X) routes by extension
- accept trailing {.class} attribute block
- Stage 1 emits markdown not HTML
- build AssetSnapshot + thread to moss-core resolve
- MediaAttrs gains class_names + extra_attrs passthroughs
- AssetSnapshot data contract for synthesizer
- title-attribute params grammar (moss: prefix)
- align-left/align-right pipe keyword for runaround layout
- inline ★ at end of kicker (Publisher · Year ★)
- linkblog `★` permalink mark next to external-url cards
- email checkbox in publish flow + two-phase progress UI
- email icon hover tooltip with current audience
- send-mode section (all / lang / test)
- subscribers section with table + filter + edit-lang
- Tauri commands for subscriber + send operations
- migrate legacy .moss/data/subscribers.csv path on first open
- sync orchestration with rejection + partial-import surfacing
- EmailSendMode config + audience resolver
- EmailClient using existing MossSetaClient + JSON helpers
- unified storage module — outbox + subscribers + sent
- types module — Subscriber, OutboxOp, ArticleSend, SentRecord
- pipe `publisher:` frontmatter into card kicker slot
- emit external_url + cover in imported article frontmatter
- route external_url pages through linkblog semantics
- replace check badge with circle/dot status badges
- moss import CLI + Clipper-quality article extractor
- add Resolved<T> wrapper with origin tracking
- register image variants + serve LQIP placeholders
- reject absolute symlink/alias targets

### Fixed
- clean up drag-session state on pointer interrupt + ancestor guard
- prune overrides for deleted paths after refresh
- drag bounds include manual override rows
- drag preserves manual expansions (bug #2)
- ChipBar asset widget uses lazy preview-URL getter
- keep current breadcrumb seg readable via min-width: max-content
- skip thumbnail when preview baseUrl is undefined
- render after rename success; don't depend on watcher echo
- restore ModelViewer camera-controls/auto-rotate defaults
- drop <img> → <figure class=image> wrap
- emit canonical ![alt](src) markdown not raw <img>
- emit canonical ![alt](src) markdown not raw <img>
- emit canonical ![alt](src) markdown not raw <img>
- suppress legitimate-gap drift warnings
- asset preview review fixes
- asset resolver searches asset dirs for wikilink-style basenames
- FrontmatterStore correctness fixes from review
- heading text is always filename (full monotonic)
- rescue #editor-heading before CmEditor.destroy()
- full-width breadcrumb divider; restack so tree scrolls behind it
- floor active breadcrumb columns to keep them readable
- preserve iframe HTML for direct (non-pipeline) callers
- preserve editor/preview context on source-stable URL changes
- delineate file-tree breadcrumb as sticky header
- match mobile-collapse specificity to default rule
- decouple title and star hover on preview cards
- address PR-2 review — numeric mtime + drop dead event
- deferred reload on dirty→clean to stop silent clobber
- rename MockFormRenderer → MockChipBar in editor:file-renamed test
- double-click rename, mid-edit input survival, post-rename home jump
- keep linkblog kicker inside card, ★ inline at its end
- finish channel-policy rework — delete cache, add provider input, centralize warnings
- close TOCTOU bind window + physically wire health marker
- channel policy — fresh fetch, no-provider warning, tab dot
- dual-loopback bind + /__moss_health/ marker for reuse-detection
- row-gap > line-height so text cells don't merge across rows
- wire-format alignment with moss-seta + remove legacy CSV path
- always-write WebP for png/jpg/jpeg + combined "{publisher} · {year}" kicker
- break stage↔site hardlinks to recover from iCloud 0-byte stubs
- scope card surface/radius/overflow to grid-shape only
- drop stale moss-summary-layout wrapper class
- thread media_lookup through to resolved card emission
- emit meta-before-title to honor kicker-position spec
- scope shape-agnostic .moss-card rules to bare selector
- emit data-layout=\"minimal\" for year-grouped children
- match <a>-tag link cells in process_folder_links_in_grids
- regenerate flatten-* fixtures with relative meta URLs
- rework flatten-no-year-group fixture to take the flatten path
- re-resolve sort axis on flattened scope via helper
- preserve explicit children_group across non-Date sort axis
- unify partial/full branches; preserve moss-grid wrapper
- emit relative meta URLs when site URL is not deployed
- emit manifest registration on fingerprint-skip; review fixes
- preserve registration on fingerprint-skip; gate AssetReady on canonical link
- re-detect local iframe after folder-embed marker resolution
- force full-rebuild refresh emit when output dir is missing
- lowercase dir segments of relative symlink/alias targets
- gate webp_variant on cached encoded output
- mem::take sources payload; drop EmitMessage Clone; pin seal invariant
- propagate sources cache through coordinator (closes followup #2)


## [0.6.6] - 2026-05-18

### Added
- auto-iframe folder/ embeds with source index.html
- wire data-width on embed wrappers (P9 follow-up)
- register new vocabulary from listing-sort feature
- wire data-width on wikilink-synthesized images (P9 follow-up)
- wire data-width attribute on hero/gallery/grid (Phase 1 follow-up to P9)
- warn on pre-v1 class vocabulary in user themes (Phase 1 PR-1d)
- collapse buttons, grid-card kinds, image aspects (Phase 1 PR-1c.4-6)
- collapse embeds to .moss-embed[data-type] (Phase 1 PR-1c.3)
- collapse callouts to .callout[data-type] (Phase 1 PR-1c.2)
- collapse cards vocabulary (Phase 1 PR-1c.1)
- wrap default CSS in @layer cascade and stamp html version (Phase 1 PR-1b)
- write .moss/contract.json on every build (Phase 2)
- moss agents init - scaffold .moss/AGENTS.md (Phase 2)
- stage v1 vocabulary in COMPONENTS (Phase 1 PR-1a)
- moss describe - JSON + human output of moss contract (Phase 2)
- generate reference.md from COMPONENTS + tokens (Phase 0b Task 4)
- seed COMPONENTS with current emitted vocabulary (Phase 0b Task 2)
- canonical agents.md worldview for AI agents (Phase 2)
- add COMPONENTS const table for HTML contract (Phase 0b Task 1)
- render folder-list markers via shared listing pipeline
- emit folder-list marker for ![[/folder/|params]]
- drive chrome from resolved_sort.series_default
- emit container wrapper + data-list-axis attributes
- auto-fill + container queries; per-axis card density
- folder count subtitle when no description (non-date only)
- meta slot becomes sort-driven; migrate call sites
- drive listings through resolved_sort
- populate resolved_sort on every folder index
- ParsedDocument carries sort + resolved_sort; impls SortableDoc
- normalize legacy series:[list] at deserialize
- add sort:; move order: alias from series:
- sort_by_resolved dispatch
- resolve_folder_sort inference cascade
- resolve_folder_sort inference cascade
- SortField + SortAxis + ResolvedSort
- SortField + SortAxis + ResolvedSort
- add generate-contract-docs dev-bin (Phase 0a)
- add format_root_block formatter for tokens.json (Phase 0a)
- add contract::tokens loader for W3C Design Tokens (Phase 0a)
- add tokens.json — declarative source for CSS public-API tokens (Phase 0a)
- tokenizeDividerLine accepts +++ and ---
- async shortcode scanner plugin via Tauri IPC
- expose scan_shortcodes Tauri command
- editor_scan skips fenced code blocks
- editor_scan recognizes +++ and --- grid dividers
- editor_scan finds top-level shortcode block fences
- scaffold editor_scan position-aware shortcode scanner
- Step 8 — synthesizer owns figure wrapper for standalone images
- retire first_body_image regex (Step 5 of structural-html-emission)
- post-Step-7 follow-ups (cloud-storage, timeout UX, e2e tests)
- route colophon cover through image synthesizer
- retire wrap_img_in_picture (Step 7 of structural-html-emission)
- route gallery thumbnails through synthesizer
- cascade homepage hero into OG image and description
- wire production callers through render_cover_html manifest
- scaffold media_lookup + eager params on render_cover_html
- route link-preview favicon through structural synthesizer
- route :::hero image through structural synthesizer
- route markdown Tag::Image through structural synthesizer
- visual polish — band centering, kicker styling, vertical mode
- format article count as CJK numerals in vertical CJK mode
- add optional kicker slot to FolderCardProps + ChildItemProps
- embed Inter font for deterministic rasterization
- resolve_cover_chain returns typed CoverRef end-to-end
- fallible from_source + typed for_* constructors
- emit absolute og:image and twitter:image URLs
- plumb resolved SiteUrl through HTML generation
- accept --site-url override for build-time host injection
- resolve_site_url walks CLI → env → state.toml → localhost
- introduce SiteUrl type with parse, from_host, to_absolute, host

### Fixed
- make canonical-link failure fatal; document load-bearing carry-forward
- drop intermediate hashes.json write; sync seal+persist
- mark-and-sweep prune untouched carry-forward at seal
- never register a .webp variant that did not land on disk
- apply pretty-URL adjustment to iframe src
- right-align form-only auto-injected footer via data-moss-shape
- slugify folder_id for url_path lookups
- await background handle in headless builds with exits_after_build
- hoist pointer-events:none on cover video to shared rule
- scope list-layout card overrides to data-layout="list"
- repair frontend CSS tests broken by v1 contract refactor
- make nextStage monotonic — unstuck "Generating pages… (N / N)"
- flip Open-Q1 entries to Status::Emerging (Phase 0b review fix)
- remove em-dashes from user-facing descriptions
- restore internal tokens that were inadvertently deleted in Phase 0a Task 4
- add error-path tests + drop unused import (Task 2 review fixes)
- fold service accepts hyphenated shortcode names
- scanner tolerates undefined/partial invoke results
- EditorRange uses u32 so TS binding is number not string
- editor_scan accepts hyphens in shortcode names
- seed atomic in bridge_to_atomic to eliminate cancel race
- address arch review findings (DRY wrap_in_figure, detector helper, CSS comment)
- handle resolve-phase raw-HTML media in figure helper
- strip resolver sentinels at body_cover_path capture (arch review)
- address pre-merge review findings (warning UI surface, doc, names)
- same off-by-one in video task counter
- off-by-one in image counter + surface server-start failure
- apply task-completed counters; surface server-start failure
- persist + reuse dir_overrides for colophon synthesizer lookup
- top-align grid card text within the band
- bound per-notebook I/O with a 30s timeout
- let grid card band grow with description text
- wire child_summary covers through synthesizer manifest
- suppress LQIP style when extra_attrs carries style=
- register ImageOutputs/ImageVariants into wire manifest
- address impl-review findings on structural-html-emission branch
- align <picture>-wrapped images in single-image paragraphs
- update test selectors to bare [data-typesetting] form
- rename inner head wrapper to .moss-card-list-head
- skip moss-folder-title H1 on translation-home pages
- remove footer hint bar; defer selection-toolbar layout reads
- update iCloud pre-warm critical list for /_moss/ namespace
- update hardcoded /js/ paths to /_moss/js/ namespace
- drop dead dir_overrides param + field
- clarify allow comment on body-link format!
- trim double-blank + bind url once in test
- code review followups for Phase 5
- correct doc tense, add empty-input test, localhost note
- load system fonts so title and site name actually render


## [0.6.5] - 2026-05-12

### Added
- migrate non-normalized hashes.json keys on load
- add typed OutputPath newtype enforcing slug normalization
- migrate non-normalized hashes.json keys on load
- add typed OutputPath newtype enforcing slug normalization
- debug-assert catches missed source_to_output registration
- prepend folder-title h1 when folder index has no cover
- emit semantic h1 via folder_title helper
- add folder_title helper
- promote series-nav to sibling of <article> so the comment toggle hugs its divider
- breadcrumb extension hiding, flex layout, no horizontal overflow
- five-zone tree sort with publish-date and folder recency
- add read_frontmatter_only helper for tree construction
- DateSource enum, resolve_publish_date helper, TreeNode publish_date fields
- add date_from_filename_prefix primitive
- add date_from_frontmatter primitive
- mirror divider gap above toggle, drop "0" prefix at zero count
- drain pending_renames + suppress shadowed deleted_paths
- push old/new pair to pending_renames after fs::rename succeeds
- add pending_renames queue to RebuildState
- bump summary-card type ladder to 1.125 / 1 / 0.875rem
- dual-mode heading commit routed by HeadingSource
- FormRenderer.setFieldValue (empty-preserving)
- pipeline + editor backend through compute()
- unify children/sidebar feed family with location + limit
- drop heading: frontmatter field
- consolidate title resolution + body-based hero detection
- folder cards consume scan-cached dominant color
- auto-inject Buttondown form on self-hosted with api_key
- default order — links above auto-injected subscribe form
- tab visibility tracks section registration
- default-hidden tabs + setAvailable API
- emit <link rel=alternate hreflang> in <head>
- add build_hreflang_link_tags helper
- Pandoc-style implicit figures + uniform empty-alt guard
- deprecation warnings + stale comment cleanup (#613)
- add thumb-swap.js for missing-thumbnail fallback
- ship pass + restore for_deferred dual-arg + module docs
- BackgroundHandle materialization barrier
- ManifestCoordinator + EmitMessage + BuildContext channel form
- BuildContext emit handle (render-phase forms)
- PendingManifest/SealedManifest typestate
- complete Step 2c — delete :::toc, rename shortcode.rs
- port :::grid to typed AST (Step 2b of #613)
- port :::hero to typed AST (Step 2 Hero)
- rename type alias PayloadFor → MossEventOf
- add onMossEvent helper for typed dispatch
- derive Clone on MossEvent + drop noise comment
- add typed MossEvent bus + wrapper
- migrate :::subscribe to attribute-based config (Task F)
- pure-CSS regions and unknown-name fallback (Tasks D + E)
- multi-line attribute blocks in shortcode opener
- +++ cell divider helper for unified shortcode grammar
- attribute-block parser for unified shortcode grammar
- watch channel + set_progress for menu-bar consumers
- plumb cancel flag into execute_generate_plugin (closes #548 follow-up)
- wire bridge_call for *ConversionState cancellation
- cancel asset copy on window close (closes #506)
- close window immediately, drain JoinSet in background
- construct FolderSession per open folder, drain on switch
- add FolderSession with cancel token + JoinSet
- add TaskSupervisor with typed Detached metadata

### Fixed
- lowercase-only dir normalization + moss-host CLI deploy bypass
- address progress panel review feedback
- preserve filename + jupyter passthrough; filter dotfiles; drop em-dash
- skip dot-prefixed files in asset copy phase
- asset link normalization + actionable deploy error
- asset link normalization + actionable deploy error
- apply code/arch review feedback
- pass entityId on embed uploads, mirror cover-upload reorder
- scope comments override so series-nav→footer keeps 80px gap
- surface error toast on catastrophic plugin failure
- drop em-dash from search_domains error toast
- add breathing room above footer divider (80px)
- align Buttondown detection with newsletter send routing
- tighten unified-grid invariant per code+arch review
- publish click no-ops on article when email channel has no audience
- drop redundant breadcrumb separator glyph
- restore unified breadcrumb+tree grid for column alignment
- byte-safe date parsing, no panic on non-ASCII filenames
- align series-nav divider with footer divider via ::before pseudo
- mobile edge-alignment without forced wrap (v3)
- folder-h1 sources from doc.title to match dedup source
- address review feedback on mobile edge-alignment
- index-page dedup so folder-title h1 doesn't duplicate body h1
- edge-align nav and subscribe form below 32rem
- sort children by date before take(n) on body listings
- restore selection→quote→comment float-shell flow
- clear source_to_output on PendingManifest::new + unify mutex-poison recovery
- preserve `?query` and `#fragment` in iframe src=
- bump comment section margin-top selector specificity
- persist source→output mapping in manifest, drop heuristic guessing
- resolve hints to output paths via manifest, not extension swap
- root self-named home file detection (regression)
- collapse comment toggle flush above footer divider
- cover picture-wrapped images in figure margin reset
- address reviewer findings
- address review feedback on feed consolidation
- tighten figure-caption spacing, switch to child combinator
- video covers get dominant color via .thumb.jpg
- badge corner-positioning works for all callers
- slugify asset URL directory segments; footer breathing room
- also slugify Phase 2 cascade prefix; tighten tests + docs
- slugify folder segments in URL paths
- restore divider + left alignment when flex-shape rule activates
- generation counter prevents stale-apply on rapid folder switch
- tighten ApplyOpts types to match production callers
- one runProjectOpenHooks call per folder lifecycle
- make setAvailable a true no-op when visibility unchanged
- always unlink orphan *.placeholder.svg from output dir
- center loader by overriding object-position on placeholder swap
- self-heal fingerprint skip when canonical outputs are missing
- atomic link_to via temp+rename; sweep stale .tmp siblings
- loosen figcaption margin to space-sm for adequate visual separation
- tighten figure caption pairing, restore section-h1 top margin
- purge stale opt-out narrative + add integration tests
- empty alt for `![[file]]` no-alias; drop v2→v3 opt-out
- width-based chip truncation + promote-from-more-menu
- eager migration + CSS scope + test coverage
- include `_*` files and percent-encode wikilink asset URLs
- recurse into CssRegion + Unknown bodies for nested fenced divs
- drop Option wrapper in #621 variant A test call (post-rebase)
- defer stale-file cleanup past seal (closes #621)
- tighten copy_deferred_assets tx parameter to non-Option; clean up stale blocking_insert! comment refs
- folder_session.rs:33 stale doc reference (post-B rename)
- restore spinner UX in thumb-swap; clarify Pattern A/E comment
- unit test + accurate comments for register_with_hash mode-prefix preservation
- remove SVG placeholder generation (Pattern E, closes #615)
- rename legacy MossEvent interface; update orchestrator doc comments
- delete orphan window-resized listener and dead test mock
- persist SealedManifest after each build (closes B1 from review)
- use spawn_blocking for image/video dispatch (post-Task 8)
- use xxHash3 in PendingManifest::register for byte-identical manifests
- apply Step 2a reviewer feedback (3 blockers + cleanup)
- shutdown prior session if reg.insert returns a replacement
- review fixups (DetachedScope on panic, dock icon when detached, runtime consistency, doc comments)
- align breadcrumb with traffic lights, match preview titlebar height


## [0.6.4] - 2026-05-02

### Added
- drive heading element from compute_heading_state
- compute heading state in Rust as single source of truth
- verbatim authored surface — strip slot chrome
- preserve filename case verbatim, mirror Rust pipeline
- preserve filename case verbatim, drop per-word title-casing
- strip ASCII punctuation, collapse hyphens, trim per segment
- heading element bound to filename, rename + breadcrumb trim coupled to heading: bool
- dblclick folder rows to rename
- drop .md from tree filenames, dim suffix for other types
- add pinned heading element above CM6
- thicken divider hairline 1px → 2px
- auto-H1 uses filename, gated on heading: false (not title="")
- add heading: Option<bool> frontmatter field
- rewrite title cascade as strict contract
- add dedup_leading_h1 helper
- add has_h1_in_hero helper
- wire cover resolver and favicon rasterizer into pipeline
- move date row + reading prefs below the title block
- inject site meta defaults into bundled SPA index.html
- generate auto OG card per page when no frontmatter cover
- wire canonical link and twitter card tags into templates
- auto-inject article H1 from title or filename
- canonical URL helper enforcing www-prefix for purchased domains
- og:locale, og:image dimensions, twitter card tags
- add inject_article_title_h1 helper
- rasterize favicon to PNG (16/32/180), letter-mark fallback
- auto-generated OG card via resvg (1200x630, content-hashed)
- cover-image fallback chain (frontmatter → body → card)
- emit mode-tagged manifest entries + upload symlinks
- resolve macOS Finder Aliases as URL aliases
- resolve macOS Bookmark alias files via CoreFoundation
- detect macOS Bookmark Data alias files by magic
- clean up stale symlinks between builds
- preserve symlinks during asset copy
- scaffold symlink preservation module
- default 16/9 aspect-ratio for html iframe embeds
- guard against author-written moss-newtab: literal
- asset links open in new tab
- synchronously prewarm link-meta before render in CLI mode
- redesign bare-URL link preview, scope global img rule
- inherit doc language from ancestor lang folder
- wire frontend panel + Tauri commands
- user-note dialog + build provenance + retention bumps
- client-side state machine for deployment setup tracking
- expand file-tree context menus + fix breadcrumb right-click
- surface ticket id + Error.name preservation test
- structured payment warns + console+uncaught bridge
- v1 to v2 migration promotes per-service sections to [services.<kind>]
- add Delete and Reveal to file-tree context menus
- introduce PageKind { Article, Folder, Asset }
- route [text](target) links through ContentGraph
- fake scrollbar below titlebar clearance zone

### Fixed
- move generate-bindings outside src/bin/ to dodge Tauri disk scan
- auto-inject moss-hosted subscribe form only when no footer.md
- comments require explicit opt-in on every host
- dblclick on breadcrumb segs starts inline rename
- address review — race guard, focus guard, error log, more tests
- always show current file in breadcrumb
- render at default body font-size
- align divider with body text on all viewports
- align divider with body content extent (not article outer box)
- skip pageview beacon while serving preview
- move font-size to children — fixes ch-unit max-width shrink
- tighten default chrome — smaller, closer to divider, left-aligned
- restore default chrome (divider + muted) on footer.container
- emit subscribe landing pages for all moss-deployed sites
- is_close_fence panics on multi-byte UTF-8 lines
- sweep unresolved href prefixes from shortcode-emitted HTML
- apply P1 findings from final review round
- hydration + button-state CSS lost track of moss-subscribe-form rename
- honor [services.comments] enabled = false
- close listen-during-stop race + parity polish
- derive deployment tab dot from live setup snapshot
- apply review feedback for change-card swap
- don't flash "Processing..." on submit while swapping card
- apply followup-PR review feedback
- hrefs/title-h1 in draft + titlebar strip painted
- heading polish — no hover/focus chrome; show file in expanded tree
- heading sits above CM6 content (flex-direction column)
- apply code/arch review feedback
- heading scrolls with markdown body (re-parent into cm-scroller)
- syndicate strictly post-deploy + path/heartbeat/titlebar fixes
- heading resolves index→parent-folder; remove dead cascade entry
- preserve body-H1 → title fallback for index pages
- force no-drag on nav pill descendants
- gate generate-bindings bin behind dev-tools feature
- emit <video src=> so .mov→.mp4 rewriter matches
- inject title above buried section H1s; tighten date spacing
- inject favicon link into bundled SPA <head>
- keep frontmatter chips in sync with the .md file
- skip empty meta tags; document no-head SPA passthrough
- walk scan limit to char boundary in inject_shell_frame_class
- regression tests + twitter alt for auto OG card
- gate injection on raw <h1 presence, not extract_h1_only
- make generate-bindings compile in release mode
- skip og:locale when locale is blank; doc twitter alt fallback
- guard og_card against long titles and bad color input
- make generate-bindings compile in release mode
- sort grid-style child cards by url_path when not pre-sorted
- tied weights fall through to alphabetical for cross-platform determinism
- second round of CI fixes for v0.6.4
- CI failures on v0.6.4 release PR
- final review fixes (SAFETY comments, non-macOS alias warn)
- address initial review I1+I2 (live_symlinks invariant, sync_dir partial-failure)
- preserve symlinks through stage→site sync
- detect local iframes with query strings or fragments
- address review of CLI link-meta prewarm
- make non-page-map asset hrefs root-aware
- add 1px border to .link-preview for dark-mode legibility
- address review of 021f09e8b
- promote translationKey:home to folder index (closes #587)
- preserve query string through resolve_link
- preserve query string in markdown link resolution
- address review findings on link-preview redesign
- trim archeology comment after dual-walk removal
- media files in assets/ folders reach variant pipeline
- widen detect_project_lang sample size from 5 to 20
- emit `<html lang>` per page, not site default
- thread site_lang through build pipeline (closes #545)
- eliminate 48px chrome-clearance flash on iframe load
- backfill first_checking_at on resume for legacy state.toml
- prefer extension-matching candidates in resolve_path tiebreaker
- R4 review fixes — recheck Noop debounce, defensive bounds
- no em-dash in user-facing cert copy; clear streak on recheck
- replace --moss-chrome-top var probe with class toggle
- file-tree scrollbar only on overflow + match breadcrumb spacing
- tighten pi16 assertions, expand PII denylist
- respect chrome-top safe area in mobile header padding
- call elements.submit() before stripe.confirmPayment()
- suppress Metadata(Any) events on directories to break iCloud rebuild loop
- restore top clearance under floating titlebar via opt-in CSS var
- add Debug+PartialEq to VideoConversionOutcome, update pipeline singleflight test
- restore test-build by using crate:: paths in relocated asset tests
- collection card cover owns geometry, children fill absolutely
- address code-review findings on file-tree delete
- update sync test to use skip_schema field name
- filter children listings by PageKind
- recognize nested callouts
- render inline markdown in first paragraph; close divs at column 0
- parse ![](path) markdown image syntax inside :::gallery
- grant CodeQL actions:read for telemetry API


## [0.6.3] - 2026-04-23

### Added
- post-nav scroll re-assertion for editor-driven navigations
- identity validation for editor→preview scroll-sync
- identity validation for preview→editor scroll-sync
- show blueprint grid pre-IPC and wire stage transitions
- include url in moss-scroll-position messages
- stage-aware progress label with scan/materialize/compile phases
- always emit navigate-preview on file select; fall back to / for 404 recovery
- add progress-panel stage state machine
- wire echo token for editor-driven preview navigation
- add PreviewStage field to state machine
- echo suppression token for editor-driven navigations
- emit scan-progress and scan-complete events
- add activePage canonical state to NavigationManager
- add ScanProgress and ScanComplete event payloads
- gate Modify events on source-content change
- add should_rebuild_for_paths gate over hashes.json
- add source_metadata_matches with mtime+size fast path
- add SiteHashes::source_metadata_for accessor
- separate installed/enabled/syndicated storage; migrate hooks.syndicate to [channels.*]
- versioned config migration module with v0 to v1 (hooks.syndicate to channels)

### Fixed
- preserve notebook hashes from carried-forward stale prune
- address review — move reset into function, add TODO(#524) markers
- isolate notebook cancellation from video cancel flag
- clear pendingEcho on no-op navigateToPage to prevent stale-echo misclassification
- bidirectional identity hardening — pathname comparison + sourceFile null-window drop
- prevent double scroll on editor-driven same-URL navigation
- clear stage on compile error so 'Opening folder…' doesn't stick
- reset .site-logo vertical margins
- route raw-TOML writers through migration path
- address review — TODO(#524) markers + negative test assertions
- preserve notebook outputs from stale cleanup
- per-page toggle is in-memory only; isSyndicatedTo accepts email:<date>
- label cloud downloads with actual provider, not always iCloud
- remove horizontal drift, add revert-to-idle
- absolutely center nav pill on the preview midpoint
- replace lang-equality filter on home_children with language-tree scoping
- release.yml detects stale tag pointing at wrong SHA (P1-10)


## [0.6.2] - 2026-04-23

### Added
- add sign_and_build helper with proptest invariant
- emit per-page progress from html_pages render loop
- update Rust register_domain to new server contract
- rebuild domain payment widget around resolveRegistrant
- ISO 3166-1 alpha-2 country list for domain widget
- emit markdown render progress through pipeline
- wire get_stored_payment_method to seta /api/payment/methods
- surface silent publish phases + skip unchanged rebuilds
- charge saved card server-side for repeat domain purchases
- charge_saved_for_domain Tauri command
- resolve subscription via /auth/whoami (PR D)
- emit default /subscribe/{confirmed,expired}/ landing pages
- detect revocation via always-on whoami reconcile (PR B)
- always register section, show re-verify on 403
- sort lookup (exact TLD) before creative suggestions
- inline AJAX + animated confirmation for moss-hosted forms
- set dropdown max-height to 360px
- pin exact match; cap AVAILABLE section at 8
- lower minQueryLength to 1
- dropdown uses NAME_SUGGEST path for creative alternatives
- propagate exactMatch through search state
- surface exact_match from NAME_SUGGEST through Tauri command
- deploy registers Detached; close handler unconditionally tears down UI
- run redirect pipeline at publish time
- add write_bytes_if_changed helper for derived state
- add .no-cards opt-out for grid folder-link conversion
- prefill registrant name and phone from last purchase
- persist selected host in pill input during purchase flow
- add compound-link pre-pass for grid cells
- put geo map and country list side-by-side
- add 'open in new window' button to immersive mode
- toggle_syndicator_hook owns rebuild; add BUILTIN_CHANNEL_TOGGLED event
- remove FeatureKey::Subscribe, email now driven by hooks.syndicate
- consume reconcile store in deployment tab; reset on project switch
- move internal-files toggle from nav to root context menu
- migrate legacy [features].subscribe via project-open hooks
- add reconcile session store with localStorage-backed cooldown
- add ReconcileOutcome + project-open coordinator; drop redundant PublicIdentity.verified
- add run_project_open_hooks sequencer for on-open side effects
- implement Change B — generalized link-cell wrap
- implement Change A — bare fenced divs
- warn on mixed multilingual structure (folder + sibling)
- fenced-div nesting via inner-more-colons arity rule
- scope nav items to current page's language tree
- wikilink resolution prefers same-language-tree candidates
- add debug/error logging to domain search controller
- restore email-gate footer link in domain dropdown from v0.6.1
- honor explicit site.lang in .moss/config.toml
- warn when style.css is at project root instead of .moss/theme/
- extract ComboSearchController, adopt domain-flow-state, section-descriptor labels
- accept .zh suffix as shorthand for .zh-hans
- default CSS for .callout-pending (dashed border, pale fill)
- lazy email reconciliation via /auth/whoami
- partition domain search into Available+Taken, sort by price, two-phase TLD search
- add pending callout type for placeholder content
- wire local-first aggregation into AnalyticsSection
- client-side aggregate + JSONL parser
- Rust events sync + Tauri command
- show search-failed row with retry on domain search errors
- consolidate 5-stack placeholders into one coming-soon note
- use alias names + full ISO-2 lookup for countries
- apply country filter to header + fade/suppress breakdowns
- render dismissible filter chip when country selected
- use renderGeoMap in place of COUNTRIES bar chart
- add CSS tokens and classes for geo map + filter chip
- restore keyboard navigation in domain dropdown
- renderGeoMap pure renderer for choropleth + list
- bucketForViews shading helper for geo map
- generate projected country paths for geo map
- Spotlight-style domain combo with inline purchase results
- moss domain list/link for custom-domain management
- arrow-key navigation + ARIA for Domain dropdown
- inline confirm/error UX for domain reassignment
- 30s domain status polling while Waiting for DNS
- transitional warning for pages missing body H1
- folder-cover renders label via moss-cover-label span
- chrome surfaces consume label, not title
- stop injecting article-title H1
- inline purchase-flow expansion below Domain caption
- derive label and has_body_h1 in markdown processing
- Domain pill-combo + dropdown + caption state machine
- add label field to ParsedDocument
- seta bindings for owned-domain listing, assignment, and status
- Email field with Verified/Unverified/Waiting pill states
- close editor on open, restore on close
- 24px corner radius matching window
- inject --moss-window-radius from WINDOW_CORNER_RADIUS
- glass-pill settings components
- add --moss-window-radius + dropdown surface tokens
- dispatch preview mode to iframe on toggle and load
- toggle moss-mobile class on moss-preview-mode message
- add --moss-radius-window matching WINDOW_CORNER_RADIUS
- canonical www URL + Live polling + CDN indicator
- CriticMarkup support in editor and compiler
- rebuild on .moss/theme/** and .moss/data/**
- pre-deploy hint banner
- TOML syntax highlighting via @codemirror/legacy-modes
- add hammer-icon toggle to show .moss/ in tree
- plumb showInternal through FileTree
- add show_internal flag to list_tree/list_directory
- moss tab — app-level settings section with auto-update preference
- app-level config infrastructure with auto-update preference
- StateMessage — shared empty/loading/error presentation
- Table — aligned row pattern for tabular data
- Toggle — reusable switch with TS helper and tests
- Section — add general-purpose section primitive
- Field — add __help and __error slots
- Input — unify --boxed modifier, refactor select as thin layer
- Button — add --small, --danger; adopt transition and focus tokens
- add spacing scale, transitions, focus ring to app tokens
- distinguish empty/error/ok states; surface sync failures
- mirror divider hover/drag state on action panel's right edge
- multi-format file viewer for non-text files in editor
- Visual template for image pages — black background, no chrome
- glass pill chrome redesign (Liquid Glass titlebar)
- image-as-page generation + visual page layout
- parse notebook title from .ipynb JSON metadata
- add mark-and-sweep garbage collection for orphaned cache entries (#355)
- redesign vertical typesetting with Chinese numerals, cards, and dates
- shortcode syntax highlighting + build-time code highlighting
- single-column nav + vertical summary cards
- add wheel-to-horizontal scroll for vertical-rl mode
- full-body vertical-rl layout + colophon i18n
- date range switches all dashboard sections, not just chart
- add hover tooltip to analytics time-series chart
- auto-start moss from mcp-server-wrapper.sh
- add email flyout component, bindings, and event bus integration
- add vertical typesetting support for CJK content
- implement subscriber management UI
- auto-add data-track to :::buttons links for analytics
- auto-track clicks on elements with data-track attribute
- add browser, OS, screen, UTM, and events sections to analytics dashboard
- enhance beacon script with screen_bucket and UTM fields
- add time-series sparkline and comparison indicator to analytics dashboard
- wire native email syndication into channel pipeline
- multi-language editor support, syntax highlighting, and ASCII art rendering
- add drag-in and drag-out support to editor action panel
- add copy_files_to_project command and tauri-plugin-drag
- render home file on same row as its folder in file tree
- add email settings section and update channel terminology
- PublicationManager loads built-in channel icons
- unified channel grid in settings modal
- add unified channel model backend

### Fixed
- filter home_children by language in multilingual sites (#542)
- sign the encoded path in upload_file
- harden subdomain field — IME safety, length cap, invalid-char hint, submit guard
- always slugify subdomain input, strip invalid chars as typed
- post-review P1 findings on v0.6.2 batch (19 new commits)
- right-align lone footer-right instead of flipping to left
- drop obsolete client-side subscription pre-flight
- drop saturate(8) filter hack from trial-expiring text
- caption no longer self-references the custom domain
- prune sources cache + add warm-cache regression test
- make lazy runtime spawn idempotent + tauri:// fallback for CLI dev
- plumb country_hint through Rust PaymentIntent + ChargeSavedResult
- show City field for everything except US (matches server reality)
- snapshots regen + tooltip selector + search step for new UX
- tolerate missing `active` field from whoami
- update hero CSS test and domain dropdown scenarios for shipped UX
- typecheck failures blocking PR #502
- P0/P1 findings from post-v0.6.1 code review
- off-session consent disclosure + drop handleNextAction any-cast
- accept requires_capture status for phase 2 manual capture
- use seta-returned billing address for saved-card flow
- force full billing address collection via granular Stripe fields
- stop loaderror render loop on terminal PaymentIntent
- reset button chrome on .moss-link and migrate title= tooltips
- revoked-state gating + double-click guard + rename to re-authorize
- surface Stripe configuration failures instead of empty gap
- migrate deploy-time subscription check to whoami path
- normalize channel icon size to 18px
- re-search on focus, dynamic preconnect URL, stale test comment
- address review — register in manifest, user override
- add SiteDataResponse.subscribers + update MOSSPUB_VPS_IP
- drop AVAILABLE cap, hide taken creative suggestions
- address review — consolidate lang enum, scope CSS
- sanitizer drops non-ASCII chars (defense-in-depth)
- dropdown opens on focus only when owned domains exist
- refresh pill on built-in channel toggle
- open-new-window button uses anchor + opener plugin
- preserve natural aspect on mobile when no overlay
- preserve dots in Rust sanitizer so user TLD reaches moss-seta
- extract TaskGuard helper; apply Detached to plugin-based deploy_site
- widen DeployGuard to cover entire push_site_inner
- include redirect stub hashes in upload manifest
- domain combo placeholder now on input, not separate label
- re-enable button before exit click in aria-label revert test
- regenerate snapshots + fix 2 behavioral failures
- clear pending-purchase pill on payment error; shrink try/catch
- emit target=_blank on compound external links
- correctly detect nested brackets in compound-link pre-pass
- friendly hint for invalid input, strip raw API error noise
- show TLD fallback price when registry returns null
- guard open-deployed-host click against non-https schemes
- channel pill spacing, deployment panel polish
- remove duplicate test_theme_style_edit_regenerates_custom_css
- re-enable button before second click in immersive toggle test
- re-enable button before exit click in exitFullscreen test
- add removeToastById to toast-manager mock in app-open.test
- mock runProjectOpenHooks in app-events.test.ts
- update deployment-email tests for reconciliation architecture change
- update test expectations for email-channel unification refactor
- remove orphaned ReconcileOutcome definition, use canonical from identity::commands
- remove duplicate pull_site_data_if_hosted_impl definition
- align-items flex-start for consistency with rest of site.css
- remove stale panelTrigger selector and add internal-btn regression test
- immersive mode for directory-style iframes + nav icons alignment
- replace grid-auto-rows: 1fr with align-items: start in .moss-grid
- add missing --- opening delimiter to people.md
- remove orphaned panel-trigger dead code
- address review findings
- align buttons EOF behavior with grid (best-effort render)
- friend-card auto-convert triggers only when cell is exclusively a link
- Batch G review cleanup — encapsulation, test helper wiring, retry guard, label kind, hint reorder
- show Searching-more hint on empty-popular; show AVAILABLE label when TAKEN also renders
- rebuild events cursor from events.jsonl when sidecar missing
- serialize concurrent event syncs with per-site mutex
- syncAnalyticsEvents returns number not string (matches Rust usize)
- restore Stripe checkout via DomainPaymentWidget; retire stacked combo
- filter chip uses DOM construction; document mouseover/mouseleave split
- revert combo on click-outside; preserve in-flight marker for newer query
- add land-neutral token to prefers-color-scheme dark block
- harden renderGeoMap against dup/unknown/empty ISO-2 inputs
- caption includes subdomain when no custom domain configured
- domain dropdown overlays below instead of pushing layout
- restyle Re-verify as subtle link with tooltip
- add sidebar gutter so tab pill doesn't touch edges
- don't restore editor on close
- add aria-label to Domain combobox + focus Reassign on confirm
- swap three missed chrome sites; clarify test framing
- preserve body H1 in all title-derivation branches
- caption uses subdomain, not custom host; rename OwnedDomain to AssignableDomain
- pass folderPath explicitly and reset cache on folder switch
- error feedback, post-unmount guards, focus on reverify
- wire tab id + contentEl aria-labelledby; drop dead addStaticItem call
- extend backdrop over titlebar stripe
- dark-mode dropdown coverage + themed underline color
- gate injected titlebar padding on :not(.moss-mobile)
- breathable horizontal gap on wrapped mobile nav
- round mobile phone frame to match window corner
- anchor consequence curve to window right edge, not preview iframe edge
- raise bump-SVG z-index above titlebar so curves reach top
- move .moss gitignore ownership into .moss/.gitignore
- window.location.href; untrack remaining built JS from git
- divider color, collapse-curve polish, titlebar consolidation, surgical internal-toggle, shared tooltips, code highlight
- address review follow-ups on setContent + setValues
- preserve cursor + focus on preview refresh
- map webp_variants through dir_overrides and derive srcset from <img src>
- guard wrap_img_in_picture against count mismatch
- register partial .webp outputs on cancellation (arch re-review gap)
- protect .webp outputs and isolate cancel flag (arch review)
- address Task 3 code-review nits
- preserve legacy theme files when .moss/build/ exists
- address architecture-review criticals + important items
- glass pill system polish — Safari-faithful dark theme, 48px titlebar, iframe scroll-under
- close see-through gap between editor and preview webviews
- hide moss-internal fields from chip bar
- polish hammer toggle — kebab-case key, distinct active state, a11y
- clear FileTree indexes on reload; strengthen re-home check
- address final code-review nits — dead branch + Windows atomicity doc
- address arch-review gates — auto-update UI + --moss-error token
- drop stale dark-mode fallback on border-light
- universal breadcrumb middle-truncation pattern
- show muted hint for invalid input instead of raw API error
- nav wraps based on actual content fit, not viewport heuristic
- page color via postMessage (cross-origin iframe requires it)
- divider line extends to titlebar, remove close button, guard title listener
- reduce control panel margin and center channel pill
- add --moss-color-bg CSS variable fallback for page color
- address code review findings for file viewer
- use .any() in watcher pre-filter to avoid dropping mixed-path events
- read --moss-color-bg CSS variable for page color extraction
- use setTimeout(100) for theme change color re-extraction
- scope matters auth token to project storage, preventing cross-project leaks
- delay page color extraction after theme toggle
- remove color-mix transparency — use page color at full opacity
- nav buttons are circles (44px, border-radius 50%)
- nav button hover clipped to pill shape
- iframe background matches page color (removes white stripe)
- page color semi-transparency + full-height nav button hover area
- nav pill focus state, iframe boundary, page color tinting
- pill polish — circles, hover states, channel divider, page color tinting
- use TypeScript sources for fullscreen + series nav
- URL autocomplete matches CJK paths and bare "/"
- use URL-space paths consistently in image-page detection
- Visual template auto-detect precedence + JS asset sync
- muted shortcode colors, folder-note link resolution, Cmd+hover cursor
- reload editor frontmatter when file changes on disk
- close settings modal on editor open + fix breadcrumb text collapse
- quote UIDs in YAML frontmatter to prevent scientific notation corruption
- run cache GC on blocking thread to avoid UI freeze
- only count bytes_freed after successful file removal
- HTML-escape notebook title to prevent XSS from metadata
- truncate breadcrumb segments on mobile to prevent overflow (#481)
- breadcrumb dot spacing above and below
- enforce content height constraint with min-height: 0
- spacing, card sizing, colophon, and alignment
- correct card layout, title-before-count order, Ma spacing
- cancel stale video conversion when opening folder without existing window
- theme-aware scrollbar colors
- dark mode toggle specificity + left scrollbar in vertical layout
- heal editor-preview sync after rebuild
- eliminate white strips + force visible scrollbar
- simplify body CSS for WebKit scroll compatibility
- correct child summary card layout in full-body vertical-rl
- detect duplicate UIDs and allow iCloud atomic save events
- reinitialize settings sections on folder switch
- redesign chart hover with column hit targets, split labels
- prevent PluginSection from overwriting EmailSection in settings
- address code review — doc ownership, derive Default, indent
- address critical review issues — unsubscribe URLs, syndicator filter, email-only flow, UTF-8 safety
- address code review issues in newsletter renderer
- compute comparison indicator from daily data for correct date ranges
- address all review findings for syntax highlighting feature
- use Option+drag for native drag-out, fix image URL encoding
- address code review — reuse copy_dir_recursive, handle partial failures
- address code review findings for unified channels


## [0.6.1] - 2026-04-10

### Added
- support PNG and ICO favicons in addition to SVG
- website button opens settings when it's the sole channel
- wire redirect system into build pipeline and deploy snapshot
- switch generate_uid to random hex and add redirect path helpers
- upgrade reqwest 0.11 -> 0.12 for HTTP/2 support
- use bulk upload for faster deploys
- add redirects module with pure redirect logic and tests (Task A)
- highlight active footer link, matching nav behavior
- unify divider snap/curve triggers + fix full-width titlebar layout
- add updateActionPanelBump Tauri command binding
- migrate [deployment] from config.toml to state.toml for existing sites

### Fixed
- correct article-map.json path in cli_tests
- remove header padding gap above hero section
- update stale comments referencing old threshold values
- improve mobile footer layout — stack vertically, widen subscribe form
- replace broken @tauri-apps/plugin-fs calls with Tauri commands
- prevent rebuild loop from save_site_lang writing config.toml
- show accurate upload progress during bulk deploy
- add re-entry guard to async handleMouseDown
- service toggles now trigger preview rebuild
- emit per-file progress during bulk upload, extend deploy stale timeout
- skip JupyterLite HTML in slot injection
- auto-configure custom domains for moss-hosted sites
- use append_data for tar paths longer than 100 bytes
- address code review findings
- use absolute URLs in redirect stubs
- use real SHA-256 hashes for notebook outputs instead of sentinel
- address architecture review findings
- add upload_url origin validation and path traversal protection
- improve chain resolution with cycle detection and use log::warn
- simplify first_error to plain Option after mutex removal
- remove mutex serialization in file uploads for true concurrency
- divider position architecture — localStorage as single source of truth
- layout-btn shows only in full-width + clamp restore position
- remove index.lock before write-tree to handle iCloud re-locking
- deploy heartbeat re-emits last known step instead of hardcoded 5/10
- await Rust command before hiding layout-btn, keep button always visible
- editor at y=0 in full-width mode so breadcrumb sits on titlebar
- correct divider curve direction + restore full-width pencil button
- GitHub Pages deploy pushes from .moss/build/site/ (not stale .moss/site/)
- review feedback — push preview offscreen below window, fix stale comment
- restore JupyterLite assets path to ~/.moss/assets/
- reduce check_subscription logging verbosity and simplify error handling
- decode URL-encoded pathname before syndication article lookup
- await toggle save before re-rendering to prevent UI/config desync
- close sub-pixel gap between card cover and content
- left-drag bump not showing + pencil icon broken after snap


## [0.6.0] - 2026-04-08

### Added
- surface tension snap indicator for divider
- unified preview→editor click-to-source for shortcodes, frontmatter, and template chrome
- split deployment state from config.toml into state.toml
- hover link preview for all moss sites
- add loading state for notebook viewer pages
- immersive mode for notebooks + data file co-location
- support .moss/ as preferred location for custom CSS, JS, and theme assets
- adaptive flat/organized site mode with smart defaults
- add JupyterLite support for interactive notebooks
- add Pandoc-style {.class} attribute syntax to all ::: shortcodes
- snap preview closed at mobile width (390px) instead of 40px from edge
- add children_style: grid, remove inline image-heading card restructuring
- add LinkedIn, X, and 小红书 plugins
- add 豆瓣 plugin — structured review pull, push scaffold
- add Substack plugin — pull, push, sync
- children accepts wikilink to target specific folder
- add progress bar, redesign toast, rename deploy_method to "moss"
- default subdomain from site name + real availability check with hint reset
- write cumulative CHANGELOG.md to moss-releases on each release
- add bound account guard to prevent unwanted article sync
- unify lock timings, annotate title, add selection sync
- add is_explicit flag to extract_date_from_doc
- detect script.js and emit custom.js with cache busting
- add logo frontmatter field for nav site logo
- add user_js_tag placeholder for custom.js injection
- add user_js_path() for custom.js cache busting
- dispatch moss-theme-change event on theme toggle
- language-aware text alignment defaults
- per-language site name, convention-based logo, home path
- add RSS toggle to Settings UI
- add rss feature toggle to suppress RSS footer link and feed generation
- add "Published with moss" colophon badge to generated sites
- use directory-based URLs for translated pages
- add language toggle in nav bar and fix translation URLs
- wire click-to-navigate through app shell to editor
- proportional scroll interpolation + click-to-navigate
- add data-source-line to li, hr, table, tr elements
- strip data-source-line from production HTML, keep in preview
- improve chip bar overflow menu and narrow divider snap zones
- add --moss-nav-width CSS variable for decoupled nav/footer width
- add per-page content_width frontmatter field
- add :::buttons directive for styled CTA button rows
- environment-aware subdomain suffix and UI badge in first-publish modal
- beacon host derived from resolved environment
- environment resolution for oracle URL, site suffix, and beacon host
- show subscription status in publishing settings
- add services settings page with toggles for analytics, comments, email
- inject Artalk widget and subscribe form for moss-hosted sites
- add analytics dashboard in settings reading from local data
- inject analytics beacon for moss-hosted sites + pull data on open
- add success toast, soft lock UX, opaque progress bar
- redesign first publish modal with trial flow
- add subscription check before deploy, expose check_subscription_status command
- add llms.txt generator and fix RSS tag passthrough
- parallel uploads, client-side validation, server timestamp
- add moss-service-inactive class to forms when endpoint is missing
- add [features] toggle, [services] endpoint config, and service status CSS
- add FirstPublishModal wizard for first-time publishing
- add FIRST_PUBLISH_COMPLETE event to event bus
- add push_site, setup_moss_host, and get_deploy_config commands
- add site hosting API types and methods
- add deploy progress types and event emission
- add site_id field to DomainDeploymentConfig
- add hero heading and CTA link styles to core CSS
- migrate from OS keychain to file-based key storage (v3)
- complete data fetching, client JS, syndication, analytics migration
- add review data fetching from NeoDB API (TDD)
- wire native features into run_pipeline()
- add generate_native_slots() for pipeline integration
- add native email subscribe form rendering (TDD)
- add native comment section rendering (TDD)
- add native review colophon rendering (TDD)
- add FeaturesConfig and review/rating frontmatter fields

### Fixed
- align tauri-plugin-fs Rust crate (2.5) with JS package (2.5.0)
- resolve_page_source prefers ArticleMap for multilingual file lookup
- left-drag bump direction, accent line style, snap at MOBILE_WIDTH
- target hover preview to wikilinks only, improve seta logging, update doc paths
- match site name and breadcrumb sizes on mobile, fix vertical alignment
- watcher uses theme/ not assets/, update stale doc comments
- theme pipeline, migration timing, and test fixtures
- divider can't be dragged far enough right (feedback loop)
- expand only direct children, show home files first
- preserve ^block-id prefix in embed markers
- use configured site language for primary page selection, support block ref embeds
- address code review findings for notebook enhancements
- address code review findings for notebook integration
- update code syntax accent colors to match new dark mode palette
- restore toggle alignment on narrow screens without nav links
- notebook viewer HTML missing from canonical output + stale cleanup
- replace card subgrid with flexbox to eliminate white line artifact
- services settings empty + RSS missing from footer
- count all leaf descendants recursively for collection grid cards
- resolve 240+ Dependabot security alerts
- always render article-title H1, remove matching body H1
- cosmetic improvements — nav alignment, article grammar, RSS footer default
- stale cleanup was deleting JupyterLite assets from output
- address code review findings (path traversal, URL encoding)
- correct merged doc comment and stale path in output layout
- index notebook files in build_content_graph
- JupyterLite path resolution and contents manifest
- footer layout, subscribe form CSS bug, and mobile nav overflow
- restore prose in doc comments changed by mechanical rename
- update stale class names in snapshot_tests.rs normalization regexes
- remove moss-moss- double prefix in card-video.test.ts
- make AppKit functions thread-safe, remove dead autoresize masks
- dispatch AppKit calls to main thread in action panel creation
- nav/footer follow content_width, auto-detect summary for descriptions
- inline grid equal image/row heights + regression tests
- close subgrid gap between card cover and content, use 4:3 cover
- card grid visual alignment — subgrid, 3:2 aspect ratio, smart text wrap
- adaptive card layout — same-line count vs stacked subtitle
- collection card layout — stack title and subtitle vertically
- grid card subtitle uses frontmatter description only, empty for bare leaf pages
- grid card subtitle shows description for folders too, document grid design
- children YAML pre-processing stops after frontmatter closes
- nav site-name wraps naturally on mobile, toggle aligns to last row
- nav site-name right spacing and toggle vertical alignment
- allow nav links to wrap on mobile to prevent horizontal overflow
- nav layout, hero crop, and site-name clamp for mobile
- address review feedback on CSS changes
- improve mobile responsiveness for tables, hero, and grid cards
- clear stale site config + use native opener for visit button
- align nav-surface height with titlebar (38px)
- position traffic lights via AppKit after window/webview creation
- use absolute path for tauri-specta bindings output
- consolidate slot injection to prevent marker leaks + rebuild before deploy
- address architecture review — handleGitHub result check, typed bindings
- address code review — trailing slash, unit tests, push failure toast
- deploy method routing, error recovery, and GitHub Pages guard
- user-friendly error messages + log full errors to Rust backend
- address code review — reqwest timeout, 404 vs 5xx, blur rerender
- route auth through Rust backend, debounced availability check
- address code review — i18n error strings, dead CSS, stale JSDoc
- preserve line count in shortcode processors
- hide Comment button when comments disabled, i18n copy attribution
- hide trailing space after heading markers in live preview
- align site logo with breadcrumb text baseline
- override base .moss-btn:hover green fill on secondary buttons
- use structural translation root check for auto-index filter
- make translation root directories transparent in breadcrumbs
- componentize BaseModal — shared CSS for all modals
- simplify secondary button hover to border-only
- secondary button hover text unreadable when green
- copy small videos (<100MB) as regular assets
- prevent duplicate title heading on homepage translations
- add data-moss-preview attribute for preview-only service forms
- re-apply hidden title after adding action-panel webview
- skip root script.js in asset copy loop
- address logo review findings
- reset window title after action-panel webview opens
- hide empty footer via CSS, keep slot marker for plugin injection
- decouple footer RSS from domain, hide empty footer, render inactive service forms
- also suppress feed.xml generation when rss feature is disabled
- handle root homepage in to_pretty_url
- sync permalink after slug dedup, add 3-language test
- address review findings — stale docs and shortcode annotations
- switch server back to site-stage/ after no-changes rebuild
- remove remaining MAX_BROWSER_WIDTH references causing runtime error
- replace fixed MAX_BROWSER_WIDTH with dynamic max, narrow snap-full to 40px
- address code review for :::buttons directive
- remove preview iframe title tooltip on hover
- exclude .moss-btn from article link hover color override
- add missing serde import for HostingEnvironmentInfo in domain commands
- deduplicate homepage translation instead of creating /index/ directory
- add title suffix and description for Page-template pages
- recognize language-suffixed home files and prioritize site language in slug dedup
- invoke pull_site_data_if_hosted on folder open for analytics
- RegisterSiteResponse deserializes server's 'id' field
- address code and architecture review findings
- address code review issues
- align sync protocol with oracle API
- prevent user style.css from overwriting default CSS in output
- add site_id field to integration test struct literals
- preserve existing dns_target when saving deployment info
- load comments from all social sources, not just comment.json
- ensure review cover image path is absolute
- cache comment provider, spawn_blocking for native slots, dedup article-map parsing
- use moss-core home detection for analytics migration
- address code review findings
- skip re-downloading comments deleted by user
- show progress during enhance phase and auto-refresh preview after slot injection

