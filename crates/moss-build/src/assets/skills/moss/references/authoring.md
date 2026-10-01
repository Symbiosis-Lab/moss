# Authoring a moss site

The canonical worldview is already embedded in this skill. Always confirm live
vocabulary with `moss describe --json` — it reports the token names, values and
component vocabulary of the binary you are running, which is the only
authority. Never take a `--moss-*` name from prose, including this file.
Site-specific notes live in the project's `.moss/AGENTS.md` (if present).

## The one-sentence author model

Edit `.moss/theme/style.css`: override any `--moss-*` token in `:root {}` (light)
and `:root[data-theme="dark"] {}` (dark), then write ordinary CSS selectors for
the rest. No `!important`, no `@layer`.

## The four styling rungs

Reach for the cheapest rung that works; earn each step:

1. **Token override.** Set a `--moss-*` token in `:root {}` (and
   `:root[data-theme="dark"] {}` for dark mode). Run `moss describe --json` for
   every current token name, its light value, and its dark value.
2. **Selector on semantic HTML.** Target the existing semantic structure (a
   heading, a blockquote, a list) with a CSS rule. No new markup required.
3. **Named-class fenced div.** `::: {.your-class}` wraps markdown in a `<div>`
   with that class. Write the CSS under the class name. One descriptive class,
   not three stacked utilities.
4. **Structural override.** Re-lay-out a page against moss's defaults.

### Rung 4 in detail

The first three rungs restyle a component in place. Rung 4 is for the other
kind of design call — a page whose whole shape differs from the default: a
front page that is one full-bleed image, a section index laid out as a poster
wall. Three moves, in order:

**Scope to a declared attribute.** `moss describe --json` reports
`scope_attributes`: the attributes moss guarantees it sets, on which element,
with which values. Scope to those rather than to something merely unique to
this page today.

```css
body[data-page="home"] .moss-hero { --moss-hero-object-position: center; }
```

**Set the component's custom properties.** A component's declared properties
are its supported configuration surface — they are the difference between
reconfiguring a component and fighting it. Re-declaring `grid-template-columns`
on an internal class breaks on the next release; setting `--moss-card-min` does
not.

**Read the rule you are displacing.** `moss describe --css <selector>` prints
every shipped rule mentioning that selector, across all of moss's stylesheets,
with its `@layer` and its comments. Read it before overriding it — half of a
default you did not know about is worse than none of it.

Everything you write lands in the `themes` layer, which is last, so your rules
already win. If you find yourself reaching for `!important`, the selector is
wrong, not the specificity.

## Dark mode

moss sets `data-theme="dark"` on `<html>` **before the first paint** (via an
inline script), so the browser never shows a flash of the wrong theme. One dark
block covers both the manual toggle and the system preference.

Set tokens in `:root {}` (light) and `:root[data-theme="dark"] {}` (dark). Do
not write an `@media (prefers-color-scheme: dark)` block — moss handles that for
you via `data-theme`.

A theme that sets a colour token on `:root` without the dark counterpart wins over moss's own dark values, silently: dark mode then paints the light background behind light text. The build does not warn, so every colour token declared in `:root {}` needs its pair in `:root[data-theme="dark"] {}`.

Example — a warm "lamplight" dark palette:

```css
:root {
  --moss-color-bg: #faf7f2;
  --moss-color-text: #2c2825;
  --moss-color-accent: #7c5c3e;
}

:root[data-theme="dark"] {
  --moss-color-bg: #1e1a16;
  --moss-color-text: #e8e0d5;
  --moss-color-accent: #c4956a;
}
```

A `logo:` image is not exempt from this. Dark ink on a transparent background disappears against a dark `--moss-color-bg`, and moss ships no dark-mode handling of its own for it — the nav logo is a plain `<img class="site-logo">`, so the fix is ordinary CSS scoped to `:root[data-theme="dark"] .site-logo {}`, the same selector pattern as any other dark-mode override. Two ways: point at a second file (`content: url("logo-dark.svg");`), or recolour the one file with a filter (`filter: invert(1);`, right when the mark is a single flat colour on transparency).

## Quiet chrome

moss splits "accent" in two: `--moss-color-accent` is the content accent (links
in prose), and `--moss-color-ui-accent` is the chrome accent (nav, buttons). To
keep chrome neutral while content links stay accented, point the chrome one at
the text color:

```css
:root {
  --moss-color-ui-accent: var(--moss-color-text);
}
```

That leaves prose links accented and makes buttons and nav blend with the
surrounding text. Run `moss describe --json` for both current values.

## Never use @layer

Never write `@layer` in your `style.css` — moss handles cascade order for you.

moss declares the order once, `@layer reset, tokens, base, layout, shortcodes,
plugins, themes;`, and links your stylesheet with an explicit layer assignment:
`<link rel="stylesheet" href="/_moss/theme/style.<hash>.css" layer="themes">`.
`themes` is last, so your rules beat every built-in rule wherever they appear in
the document. You never need `!important`.

A `@layer` block written inside your own `style.css` creates a *nested* layer
inside `themes` and changes precedence unpredictably. Write plain CSS.

## The `.moss/theme/` packaging API

`.moss/theme/` is mirrored **verbatim** to `/_moss/theme/` in the built site.

Consequences you can rely on:

- `.moss/theme/style.css` is served at `/_moss/theme/style.css` (content-hashed)
  and linked with `layer="themes"`, which is why your rules win — see the
  cascade section above, not source order.
- Sibling assets resolve **relatively**: `url("grain.png")` in `style.css`
  points at `.moss/theme/grain.png` → `/_moss/theme/grain.png`. Drop fonts,
  textures, and overlay videos next to `style.css` and reference them by
  relative name.
- For JS that needs theme assets, moss injects `window.mossTheme.base` (the
  `/_moss/theme/` URL) before your `.moss/theme/script.js` runs. Resolve assets
  with `new URL("asset.woff2", mossTheme.base)`.

## Site icon

Drop `assets/favicon.svg` (or `.png`/`.ico`) at the project root for the site's own tab icon; without one, moss ships its own default mark rather than leaving the tab blank. An SVG favicon — yours or moss's default — is rasterized automatically into `assets/favicon-16.png`, `assets/favicon-32.png`, and `assets/favicon-180.png` (the last doubles as the apple-touch-icon). A `.png`/`.ico` favicon is not copied byte for byte: it passes through moss's ordinary image optimizer like any other picture on the site, so the served file is often smaller than the one you dropped in — but unlike the SVG path, it is never resized or multiplied into the 16/32/180 icon set, only optimized at its own single size.

The SVG must be pure vector, with no embedded bitmap. moss's SVG renderer ships without raster-image decoding, so an `<image>` element inside the SVG — a logo exported as "SVG" that actually wraps a PNG or JPEG — renders as nothing: the icon comes out blank (or missing whatever part of it was the raster), with only a warning in the build log to say why.

## A source-owned HTML homepage

For a homepage that must own its complete HTML document, keep `index.md` for site metadata, put the authored document at `index.html`, and declare the exact file in `.moss/config.toml`:

```toml
[build]
passthrough = ["index.html"]
```

The explicit entry makes the source HTML win only the root `index.html` output collision. moss still generates every Markdown page, and `--watch` rebuilds when either source changes. The HTML document owns its own head, styles, scripts, accessibility, and asset links. A root `index.html` without this explicit entry remains an ordinary source asset and does not replace moss's generated homepage.

## Files moss does not render, and where they belong

A file in the site folder that is not a page — a script, a data file, a stylesheet a page links itself — reaches the built site at the same path: `closing.js` beside `index.md` is served at `/closing.js`, byte for byte. Images and videos go through moss's media conversion instead, the same as an embed's. A folder is passthrough when it holds its own `index.html`, or when `[build].passthrough` names it (`passthrough = ["index.html", "demo"]`; a leading `!` opts out a folder moss detected). Its videos are copied byte for byte and it gets no folder listing page, but its images are still re-encoded in place, at their own size and path (a lighter JPEG, a palette PNG), so an app that needs a pixel-exact image cannot rely on passthrough for it. Markdown inside it is still rendered as pages.

Where a file goes follows what owns it. `.moss/theme/` is the site's theme, for every page moss generates: its `style.css` and `script.js` load on each of them, and whatever sits beside them ships with the theme under `/_moss/theme/`. A source-owned HTML document owns its own scripts and assets instead, so keep them beside it as ordinary files, or in a passthrough folder, and link them by path. One page's script put in the theme would travel with the theme to every page and tie that page to the theme's mount path.

## Embedding media

All media via wikilink embeds — never raw `<img>`, `<video>`, or `<audio>`:

```
![[photo.jpg]]          → image with LQIP, WebP, dimensions
![[clip.mp4]]           → video player with controls
![[track.mp3]]          → audio player
```

moss warns and you lose enhancement (LQIP, WebP encode, thumbnails, dimensions)
if you use raw HTML tags for media that moss owns.

**Ambient loop video.** Add `|loop` to a video embed for a silent background
loop — autoplays, no controls, respects `prefers-reduced-motion`:

```
![[clip.mp4|loop]]          → ambient loop
![[clip.mp4|640x360 loop]]  → ambient loop + explicit size
```

The `|loop` preset atomically forces `autoplay muted loop playsinline` and
removes the control bar. It is a fixed preset, not a set of per-attribute flags
— moss enforces this to keep the browser's autoplay policy from blocking
the video.

For the exact per-type attribute vocabulary (data-loop, data-type, sizing
tokens, etc.) run `moss describe --json`, or see `mosspub.com/docs/reference`
— those reflect the running binary.

### External media embeds

Embed a video or pen by URL with the same wikilink-embed syntax:

```
![[https://www.youtube.com/watch?v=dQw4w9WgXcQ]]
![[https://vimeo.com/123456789]]
![[https://www.youtube.com/watch?v=abc|wide]]
![[https://www.youtube.com/watch?v=abc|640x360]]
```

YouTube, Vimeo, and CodePen get provider-aware players; other https URLs
become a generic iframe.

## Content structure

### Shortcodes

Shortcodes are fenced blocks (`:::hero`, `:::grid`, …); callouts use
`> [!note]`. The set is version-specific, so read it rather than assuming:

```
moss describe --json | jq -r '.components[] | select(.authorable) | .class'
```

Each entry carries `example_markdown`, which is the invocation moss itself tests — copy that shape rather than reconstructing one. Add a site-specific class to a shortcode invocation and style the class; keep the shortcode generic.

`:::hero`'s overlay text (the markdown written inside the fence) has exactly one position on every overlaid layout: bottom-left. No attribute moves it. If the crop needs some region of the image kept clear for legibility, either keep the overlay content away from that corner or drop overlay entirely — an image-only hero (`:::hero {image=…}` with nothing between the fences, followed by ordinary markdown as the page's own next block) or `caption="…"` both put the words below the image instead of on top of it.

`:::hero {.plate}` (2026-09-11) is a moss default, not a per-site class to style: it renders the hero image whole — never cropped, never enlarged past the resolution it was delivered at, shrunk to fit the column instead. Reach for it over a plain hero whenever the image's own shape, not the page layout, should decide how large it appears — an artwork, manuscript page, or photograph reproduction where cropping would cut off part of the object, and especially a wide or tall outlier (a handscroll, a long strip) that a viewport-relative `100vw` sizing would otherwise fetch too small and stretch blurry. A plain `:::hero` (or `:::hero {caption="…"}`, which already avoids cropping but still bounds the frame at the default height cap) stays right for a banner meant to fill its slot.

The same rule extends past physical-object reproductions: any image that carries its own lettering, or whose full frame is the point rather than a subject inside it — a poster, a flyer, a book cover, a chart — is a plate too, because a crop can cut off a headline or an axis exactly as it would a manuscript's edge. Overlay text (written as markdown inside the `:::hero {…} … :::` fence) belongs on a photograph with calm, uncluttered ground for it to sit on; a plate is never cropped, so overlay text lands wherever that particular image's own content happens to fall, and often collides with lettering already there. Give a plate's own text a `caption="…"` instead — moss places it below the image, in the reading column, never over it, the same slot a captioned (non-plate) hero uses. There is no documented way to set a plate beside running text on a wide screen; the caption below it is the only placement moss supports.

`:::grid N {scroll}` (2026-09-21) keeps a grid's row on one line and lets the reader drag it sideways instead of it wrapping — `N` becomes how many cards fit in view at once, with a slice of the next one showing as the cue to keep going. Reach for it on a "related articles" or "more like this" strip where reading order matters more than seeing every card at once; add `label="…"` to give the row an accessible name when the surrounding heading doesn't already say what it is. Skip it when every card must be visible without scrolling and use the plain wrapping grid instead.

A `:::grid` cell holding nothing but a page reference becomes one of two different things depending on the `!`. `![[page.md]]` is a transclusion — it inlines that page's markdown as the cell's own content, same as a partial anywhere else in a body. `[[page]]` or `[[page|Custom Title]]`, alone in the cell, becomes a **card** instead: the page's own cover, title and description, styled the same way a folder listing cards its children. The cover comes from the page's `cover:` frontmatter (an empty box if it has none), the title is the page's own (`|Custom Title` overrides it), and the text is the page's `description:` field only — never an excerpt pulled from the body, so a page with no `description:` set shows a card with no text under its title. A card's cover is cropped to a fixed box by default (`4 / 3`), same as any other photograph; a cover with lettering or a full-frame subject wants to be shown whole instead, which is a scope override (rung 4), not a per-card frontmatter switch — set `--moss-card-cover-ratio` to the image's own ratio and `--moss-card-cover-fit: contain` on `.moss-card-cover` (or a narrower scope) rather than cropping it.

### Partials

Extract repeated blocks to a partial file and transclude with
`![[partial-name]]`. Partials are content reuse, not styling — they live outside
the styling rungs.

### Term kinds

Name-list fields are listable dimensions, not just metadata. Every value generates a page listing what carries it: `author: 趙雲` gives `/authors/趙雲/`, `tags: [城市]` gives `/tags/城市/`. Nothing to set up, and a link to a term page never 404s.

Five fields work this way: `author:`, `tags:`, `editor:`, `jury:`, `location:`. Out of the box `author:` feeds `/authors/` and `tags:` feeds `/tags/`; `editor:`, `jury:` and `location:` feed nothing until a site says where they go.

A **term kind** is one namespace fed by one or more of those fields. Declare one in `.moss/config.toml`:

```toml
[terms.people]
fields = ["author", "editor", "jury"]
title = "People"
```

Now all three fields feed one namespace: someone who wrote one piece and edited another has a single page at `/people/<name>/`, not two. That page lists their works in sections — "Author", then "Editor", then "Jury", in the order `fields` gives — so the listing says what each credit was. A person reached through only one of the fields gets a plain listing with no section headings.

Naming a field in a declared kind takes it out of its built-in namespace: with the block above, `author:` no longer feeds `/authors/`. The old `/authors/<name>/` URLs keep working, and so does the bare `/authors/` root itself — moss emits a redirect to the new page for as long as the field belongs elsewhere. That redirect never lands on a real page: if you have hand-authored content living at the old address, moss leaves it alone rather than burying it under a meta-refresh.

Any real page can take a generated one's place. `author_page: 趙雲` — or `true`, meaning "the name is my title" — makes that page the author's page: its body is the bio, its `url:` is wherever you want it, and the works listing attaches below. Every field has its claim: `tag_page:`, `editor_page:`, `jury_page:`, `place_page:`. Any of a kind's claims takes over that kind's page, so in the `people` example above a bio page claims with whichever credit fits. Term mentions site-wide then link there instead of at the generated URL. Claiming adds that listing below, and also gives the page's own `cover:` a visible place in the body — beside the title, the same layout a folder-index page claiming the same term already uses. An unclaimed leaf's `cover:` only ever reaches share/OG metadata; it never becomes a visible image in the body.

Two things that surprise people:

- **Inline `#hashtags` derive no pages** — only frontmatter `tags:` do. An inline tag still reaches `article:tag` metadata and JSON-LD keywords; it is prose, not cataloguing.
- **Namespace roots are not in nav and not listed by their parent.** A folder holding every author name is wrong as a nav item on most sites, so `/authors/`, `/tags/` and any declared kind's root stay reachable through term links, sitemap and search instead.

To stop a dimension generating pages, either switch the built-in off — `[terms].author = false` or `[terms].tags = false` — or drop the field from the `fields` list of the kind that claims it.

### Places

`location:` is a name-list field like `author:`/`editor:`/`jury:`, feeding whichever kind's `fields` names it — but a kind that also sets `type = "place"` gets three things the others don't: a hand-edited gazetteer, a parent hierarchy, and an automatic place line.

```toml
[terms.places]
type = "place"
fields = ["location"]
title = "Places"
```

Every name in `location:` is looked up by its display name in `.moss/places.toml`, a file you edit by hand — moss never writes it. One quoted-key table per place:

```toml
["Kyoto"]
lat = 35.0116
lng = 135.7681
precision = "city"
parent = "Japan"
```

Three fields, all optional: `lat`/`lng` place the pin (missing either one still keeps the place — its page, parent and precision all still work, just with nothing to put on a map); `parent` names another place by its display name, and that place rolls up into the parent's listing too — a page in Kyoto also appears on Japan's page, and Japan gets its own generated page even with no gazetteer row of its own, purely from being named as somebody's parent; `precision` is the privacy control, not a display preference — `exact`, `city`, `region` or `country`, and a missing or unrecognized value coarsens to `country`, the widest ring, rather than defaulting to the most precise one.

A place page — claimed with `place_page:`, or generated like any other unclaimed term — gets a breadcrumb up its parent chain and a list of its own children, each with how many pages are under it (counting every page anywhere in that child's own subtree, not just its direct members). A page with `location:` set gets one more thing for free: an automatic line under its byline naming every place it declared, each linked to that place's page. There is no frontmatter key to write that line yourself, and no opt-out — leave `location:` unset and the page gets none.

Place pages draw an offline map automatically when at least one relevant gazetteer entry has coordinates. A real page of your own at a place term's root (`places/index.md`, say, with no `place_page:` claim) keeps its own title and content — the map still draws below them, not instead of them. Set `map: false` in that page's frontmatter to turn the map off; this `map:` is the geographic map on the page, unrelated to `cascade:`'s own key-value map.

To add the smaller locator map after each authored page's place line, opt in site-wide:

```toml
[site]
locator = "align-right"
```

The default is no locator. `"none"` also disables it. On narrow screens the right-aligned locator collapses into the reading flow at full width.

A place namespace, parent, or leaf can also be embedded explicitly with `style:map`, for example `![[/places/kyoto/|style:map]]`. The map uses the same members and privacy precision as the place page. If the target is not a place term, or none of its entries has coordinates, moss warns and renders the ordinary listing instead.

A place's page — its own leaf (`/places/kyoto/`), or an ancestor reached only through roll-up (`/places/japan/`, with no page of its own naming it directly) — embeds as a listing anywhere in the body the same way a real folder does: `![[/places/kyoto/|style:grid]]`. Nothing on disk backs that path; it resolves through the same term membership the page itself is built from, member order included, whether the page underneath is generated or claimed with `place_page:`.

### Long archives

moss has **no pagination**: no `paginate:`, no `offset`, no `/page/2/`. Do not
invent one — unknown frontmatter keys are silently ignored, and hand-built
`/page/N/` folders drift the moment an article is added or removed.

Split a long archive by folder instead — by year, by series, by section. Give each folder's home `sort: date` for a chronological stream (`sort` also accepts `weight` and `title`).

`date:` takes `YYYY-MM-DD`, `YYYY-MM`, or a bare year, so a work whose day or month is unknown can still sort and show what is known. Quote a bare year — `date: "1695"` — because YAML reads an unquoted `1695` as an integer and the build rejects it. On a vertical CJK page every date, on the article and on its cards, renders in Chinese numerals (一六九五年·三月).

Under `typesetting = "vertical"` the column is a page, not a scroll: its height is capped at 38em or the viewport minus margins, body type scales with viewport *height* (the `--moss-vertical-font-size` token, override it in `.moss/theme/style.css` if a site wants larger or smaller type), and the free space above it is 38.2% of the total, the golden-section head margin of a printed page. Do not centre or re-margin the column in a theme: a `margin` or `align-items` rule on `body` under vertical mode fights the default and reads as a bug on the next moss release. Added 2026-09-11 after a tall-screen round on a vertical site where the placement was nearly patched site-side.

`sort:` also accepts a list of child stems instead of an axis name, declaring an explicit order rather than a rule: children not named in the list follow after, sorted by the axis the folder would have inferred on its own (weight if any child carries one; else date when at least four in five children are dated; else title).

```yaml
sort: [intro, setup, advanced]
```

A page's own `weight:` is an integer that `sort: weight` orders by — lower first, and pages with no weight follow after the weighted ones, tied among themselves by stem.

`series:` on a folder index turns on prev/next chrome for its children: `true` follows the folder's own order, a list of wikilinks declares an explicit sequence, `false` turns it off. Set `series: false` on a page inside the folder instead, and that one page drops out of the reading order — no prev/next of its own, and it stops being any sibling's prev or next. Declaring `sort:` as an explicit list, or as `sort: weight`, turns `series` on by default.

Show a capped feed on the homepage by pointing at that folder:

```yaml
children: "[[2026]]"
children_limit: 10
```

The automatic "More" link only appears on a **cross-folder** feed that actually truncated something. A `children_limit` on a page listing its own children truncates silently with no "More" link, because linking back to the page you are already on is meaningless. So always name the target folder when you want the link to appear.

### Moved and removed pages

Every page carries a `uid:`, and each build compares that uid's current address against the one recorded at the last deploy — an address that changed gets an automatic redirect, a small page at the old address forwarding to the new one. Never hand-write one of these: it isn't tracked as a redirect, so moss cannot retire it later, and a stray one at the site root gets crawled and can leak into the sitemap. The comparison is against what was actually *deployed*, so a first port with no deploy on record yet gets no redirect for anything renamed before that first publish — only a move made after at least one deploy is caught automatically.

A page you removed, or merged into a section of another, leaves nothing for moss to compare uids against, so add the forwarding link yourself: edit `.moss/data/redirects.json`, a flat JSON object mapping the old address to the new one, neither with a leading slash — `{"old-page/": "new-page/#section"}` (a fragment on the target works). This takes effect on an ordinary local `moss build`; no deploy is required for a hand-written entry to start forwarding. If a real page now exists at the old address, moss leaves it alone — the redirect is dropped rather than overwriting it.

A site kept in git must un-ignore this one file. moss's own `.moss/.gitignore` excludes `.moss/data/` as `data/*`, deliberately the one line in that file spelled so a single file underneath can still be re-included — but nothing adds that exception for you, so add `!data/redirects.json` on its own line beneath `data/*`, or `git add -f` the file once. Left ignored, an ordinary `git add .` silently drops it: a fresh clone then has no redirect history at all, so every earlier hand-declared forward for a removed or merged page is gone — there is no `uid:` for moss to reconstruct it from — while a rename since the last deploy still gets caught fresh, since the deploy record itself is tracked in git.

## Multilingual sites

moss recognizes exactly two conventions for a translated file, and no others.
Invented names (`english/`, `EN/`, `article.english.md`) are recognized by
nothing and are just treated as ordinary content.

1. **A top-level directory named for a language code** — `zh-hant/about.md`.
2. **A filename suffix** — `about.zh-hans.md`.

In both cases the code must be on moss's allowlist. Check the name you intend
to use *before* creating the directory — an unrecognized one fails silently
(see below), so building first tells you nothing:

```
moss describe --json | jq -r '.languages[]'
```

There is no second allowlist to extend by hand.

**Being on that list is not the same as being an edition**, and the two
questions have different answers. `.languages` decides whether a directory name
is read as a language tree at all — ~50 codes. Whether that tree gets interface
strings of its own is a separate, much smaller ceiling, described under "moss
supports exactly three editions" below. `ja` is on the first list and not the
second: a `ja/` tree publishes correctly and is dressed in another language's
chrome. Neither answer is a bug, but reading the first as the second is the way
to be surprised by the result.

`translationKey:` links two files as translations when their filenames differ. Files with matching stems (`about.md` and `about.zh-hans.md`) pair automatically without it.

`[site] lang` in `.moss/config.toml` names the **default edition**: the one that publishes at `/`, while every other edition publishes under `/<code>/`.

Read that as naming the edition, not as choosing which files land at the root.
Directory structure decides that — anything outside a language-code directory
publishes at `/`, whatever language it is written in. And a page's own language
comes from its frontmatter `lang:`, its filename suffix, its ancestor
directory, or detection from its text, in that order; `[site] lang` is only the
last resort when all four are silent. So a config saying `lang = "en"` over a
tree of Chinese pages does not make them English — it decides which interface
strings the pages that *did* fall through to it are dressed in. If the two ever
disagree, believe the page: `moss list` reports each page's own language, and
that is the one that ships in `<html lang>`.

**moss supports exactly three editions: English, Simplified Chinese, and
Traditional Chinese.** This is a hard ceiling, not a special case for a
particular tree shape: moss's UI-language type has exactly three variants and
resolves only `en`, `zh`/`zh-hans`/`zh-cn`, and `zh-hant`/`zh-tw`. `en-us/` and
`en-gb/` are accepted as directory names but resolve to no edition either.

**A language tree outside those three gets correct content in the default
edition's chrome.** The ceiling is real, but it is a ceiling on *interface*, not
on the page. Measured on a `ja/` tree:

- `<html lang>` is **the document's own language** — a page in `ja/` is served
  as `lang="ja"`. moss recognizes far more codes as trees and filename suffixes
  (`.languages`) than it has interface strings for, and `<html lang>` describes
  the content, not the chrome. Screen readers and crawlers get the truth.
- The chrome is the default edition's: site name, nav links, footer, and a
  switcher listing only the editions that exist — and the switcher marks that
  edition as the current one, because `ja` resolves to no edition of its own.
  There are no Japanese interface strings, so this is the honest outcome rather
  than a bug — but a reader does land on Japanese body text inside another
  language's chrome, whichever `[site] lang` names.
- The `<title>` gets the site name appended as a suffix, which a real edition
  home (`/en/`) does not get: `ja` is a tree, not an edition.

So: for a fourth language, translate the body content and expect to supply the
chrome yourself — an authored `ja/footer.md`, theme CSS, and your own nav if
the default one is unacceptable. What you do *not* have to do any more is
correct the `lang` attribute by hand.

Give each language tree its own authored home page — the build never
synthesizes a folder home for a language tree, so this is the reliable path to
getting a working edition with a switcher entry. (A switcher entry can also
come from a translation pair or from an in-language subtree, but an authored
home page is the one that always works.) The ordinary folder-home rule applies,
so `en/index.md` and `en/en.md` are equally valid; match whatever the site
already does rather than converting it.

A `[[reference]]` written from a page inside a language tree prefers that reader's own language — bare stem or full path alike. `[[work/spring-show]]` written from a page under `zh-hans/` resolves to `zh-hans/work/spring-show.md` when that file exists, the same way the bare `[[spring-show]]` already prefers `zh-hans/spring-show.md` over a root one; either form only falls back to the un-prefixed path when no same-language copy exists. To link across languages on purpose — a Chinese page pointing at the English original, say — write the full path including the language folder: `[[en/work/spring-show]]`.

## Live vocabulary: moss describe

`moss describe --json` is the single source of truth for:

- **Tokens** — every `--moss-*` name set in `:root {}`, its light value, its
  dark value (`dark_value`), and an optional description. Site-wide: set one
  and every component that reads it follows.
- **Custom properties** — every `--moss-*` name a *component* reads, with the
  `owner` class that reads it and its default. Scope-level: set one on a
  component or a page and you reconfigure that thing, not the site. These are
  the difference between configuring a component and fighting it — see rung 4
  above. Whole themes get built without touching the site-wide tokens at all.
- **Scope attributes** — the structural `data-*` attributes moss guarantees it
  sets, with the element they land on and their possible values. These are what
  a structural override scopes to, and one of them may already do what you were
  about to hand-write: check the list before building a mode from scratch.
- **Components** — every `moss-*` class moss emits, with `authorable: true`
  flagging the author-facing shortcodes.
- **Frontmatter** — every built-in frontmatter field moss recognizes, its type,
  and allowed enum values where applicable. Check this array before adding any
  new top-level frontmatter key of your own: a name that collides with one
  already here is read by moss and silently changes behaviour instead of
  staying the inert custom field you meant it to be.

Tokens and custom properties are both `--moss-*` names and are easy to confuse.
The test is where you set it: a token belongs in `:root {}` and cascades
site-wide; a custom property belongs on the component or the scope you are
reconfiguring. `moss describe --json` keeps them under separate keys for
exactly this reason — `.tokens` (an object, grouped: `.tokens[]` yields groups,
`.tokens[][]` yields tokens) and `.custom_properties` (a flat array).

`moss describe --css <selector>` answers the other question — not "what may I
set" but "what is already set." It prints every shipped rule mentioning that
selector across all of moss's stylesheets, with layers and comments intact.

Never hardcode token names or shortcode classes from memory. They change between
releases. The output of `moss describe` always reflects the running binary.
