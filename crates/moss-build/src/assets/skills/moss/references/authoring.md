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
3. **A declared field with a `place`, or a named-class fenced div.** First ask
   whether the content is a fact about the page (a year, a count, a role, a
   place). If it is, declare it as a field and give it a `place` (see Declared fields below),
   so moss draws it from frontmatter instead of a classed block repeated on
   every page. Otherwise `::: {.your-class}` wraps markdown in a `<div>`
   with that class. Write the CSS under the class name. One descriptive class,
   not three stacked utilities. A heading takes no attributes: `## Title {.x}`
   prints the braces as text, in the heading and in its anchor id, and the build
   warns; wrap the section in the fenced div instead. The warning looks at `#`
   headings that start a line: a heading inside a blockquote or a list item, and
   a setext heading (underlined with `===` or `---`), are not checked.
4. **Structural override.** Re-lay-out a page against moss's defaults.

A text field shows focus on its own border, never as a ring inside or outside it. A theme's site-wide focus ring should exclude fields, for example `:focus-visible:not(input, textarea, select)`, and style a field's focus on its own border. moss's search field already does: its input and result rows draw no outline whatever the theme says, so restyle the panel's border or the `.is-selected` row instead.

### Rung 4 in detail

The first three rungs restyle a component in place. Rung 4 is for the other
kind of design call — a page whose whole shape differs from the default: a
front page that is one full-bleed image, a section index laid out as a poster
wall. Three moves, in order:

**Scope to a declared attribute.** `moss describe --json` reports `scope_attributes`: the attributes moss guarantees it sets, on which element, with which values. Scope to those rather than to something merely unique to this page today. To style one page or one section, scope to `data-path`, the page's published address: `body[data-path="/contact/"]` matches that page and `body[data-path^="/work/"]` every page under `/work/`. A selector written on a page's address must be updated if the page is renamed, because the address changes with it. To give a whole folder an accent, put `color: "#a3312a"` on the folder's home page: that page and every page under the folder then carry `data-section="<folder>"` and `--moss-section-color` on `<body>`, and a listing card for such a page carries the same property. Accent colours are often poor text colours, so moss also writes `--moss-section-text`: the same hue, adjusted until it reaches 4.5:1 contrast against moss's default page background, one value for the light theme and one for the dark, switching with `data-theme` like moss's own colour tokens. Write `color: var(--moss-section-text)` for text in the section's colour and keep `var(--moss-section-color)` for fills, borders and anything that is not text. The guarantee is against moss's default background, so a theme that changes the page background checks its own contrast. Style a section with `[data-section]` and `var(--moss-section-color)` instead of `body[data-path^=…]`, which breaks when a folder is renamed.

```css
body[data-page="home"] .moss-hero { --moss-hero-object-position: center; }
```

**Set the component's custom properties.** A component's declared properties are its supported configuration surface — they are the difference between reconfiguring a component and fighting it. Re-declaring `grid-template-columns` on an internal class breaks on the next release; setting `--moss-card-min` does not. These replace selectors themes used to write by hand: `--moss-header-bg` and `--moss-header-ink` paint the header band and its nav links and site name (a `header=over` hero still wins on its own page), `--moss-header-rule` sets the hairline under the navigation (`transparent` hides it on a banded header), `--moss-footer-bg`, `--moss-footer-ink` and `--moss-footer-rule` do the same for the footer, and `--moss-cover-color: transparent; --moss-cover-ink: var(--moss-color-text)` gives plain cards on the page background with readable text. Set them on `:root`, with dark-mode pairs.

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
  relative name. The files the stylesheet refers to are also published under
  names that change whenever the file changes, and the built stylesheet points
  at those (`url("fonts/a.woff2")` becomes `url("fonts/a.<hash>.woff2")`, the
  hash being 16 lowercase hex digits before the extension), so a host can let
  browsers keep them indefinitely. The plain `/_moss/theme/fonts/a.woff2` is
  still published for pages and HTML you write yourself. Only relative
  `url()`s to fonts and pictures up to 5 MB that sit inside `.moss/theme/` are
  rewritten; larger files, absolute paths, full URLs and `data:` URLs stay as
  written.
- For JS that needs theme assets, moss injects `window.mossTheme.base` (the
  `/_moss/theme/` URL) before your `.moss/theme/script.js` runs. Resolve assets
  with `new URL("asset.woff2", mossTheme.base)`.

## Site icon

Drop `assets/favicon.svg` (or `.png`/`.ico`) at the project root for the site's own tab icon; without one, moss ships its own default mark on a round ground of its own (paper in a light browser, near-black in a dark one) rather than leaving the tab blank, so the mark stays legible on any tab strip. A favicon you supply is used as is and never given a ground. An SVG favicon — yours or moss's default — is rasterized automatically into `assets/favicon-16.png`, `assets/favicon-32.png`, and `assets/favicon-180.png` (the last doubles as the apple-touch-icon). A `.png` favicon passes through moss's ordinary image optimizer like any other picture on the site, so the served file is often smaller than the one you dropped in. If it is at least 32 px on its shorter side, moss also writes `favicon-16.png` and `favicon-32.png` from it, and `favicon-180.png` when it is at least 180 px; each is scaled down to fit a square and centred on transparency, and never enlarged. A smaller `.png`, or an `.ico`, is served alone at its own size.

The SVG must be pure vector, with no embedded bitmap. moss's SVG renderer ships without raster-image decoding, so an `<image>` element inside the SVG — a logo exported as "SVG" that actually wraps a PNG or JPEG — renders as nothing: the icon comes out blank (or missing whatever part of it was the raster), with only a warning in the build log to say why.

## The not-found page

A page named `404.md` at the site root is the page a visitor sees at an address that does not exist. moss builds it at `/404/` like any page and also writes the same page at `/404.html`, where static hosts look for it. It is left out of the sitemap, feed, search and `llms.txt` and out of every listing, so it needs no `listed: false`, and both addresses carry a `noindex` robots tag so a crawler never indexes it. A `404.html` file of your own at the root is served as written and wins over the generated copy.

## A source-owned HTML homepage

For a homepage that must own its complete HTML document, keep `index.md` for site metadata, put the authored document at `index.html`, and declare the exact file in `.moss/config.toml`:

```toml
[build]
passthrough = ["index.html"]
```

A passthrough page receives none of moss's injected features: no visit counter, comments or subscribe form. To be counted on a site published to mosspub.com, it adds `<script src="/_moss/js/beacon.js" defer></script>` itself.

The explicit entry makes the source HTML win only the root `index.html` output collision. moss still generates every Markdown page, and `--watch` rebuilds when either source changes. The HTML document owns its own head, styles, scripts, accessibility, and asset links. A root `index.html` without this explicit entry remains an ordinary source asset and does not replace moss's generated homepage. The `index.md` beside a declared document, at the root or in a language folder like `fr/`, is only that page's metadata: its title and description still serve the site, but its text is never published to `llms.txt`, a feed or search, and the build warns when it has any.

## Files moss does not render, and where they belong

A file in the site folder that is not a page — a script, a data file, a stylesheet a page links itself — reaches the built site at the same path: `closing.js` beside `index.md` is served at `/closing.js`, byte for byte. Images and videos go through moss's media conversion instead, the same as an embed's. A folder is passthrough when it holds its own `index.html`, or when `[build].passthrough` names it (`passthrough = ["index.html", "demo"]`; a leading `!` opts out a folder moss detected). Its videos are copied byte for byte and it gets no folder listing page, but its images are still re-encoded in place, at their own size and path (a lighter JPEG, a palette PNG), so an app that needs a pixel-exact image cannot rely on passthrough for it. Markdown inside it is still rendered as pages.

A raw `.html` page is listed in `sitemap.xml` unless its first 8 KiB holds `<meta name="robots" content="noindex">` (or `none`); `listed: false` is the frontmatter equivalent for a Markdown page.

Where a file goes follows what owns it. `.moss/theme/` is the site's theme, for every page moss generates: its `style.css` and `script.js` load on each of them, and whatever sits beside them ships with the theme under `/_moss/theme/`. A source-owned HTML document owns its own scripts and assets instead, so keep them beside it as ordinary files, or in a passthrough folder, and link them by path. One page's script put in the theme would travel with the theme to every page and tie that page to the theme's mount path.

## Embedding media

All media via embeds — never raw `<img>`, `<video>`, or `<audio>`. The standard Markdown form is the default; the wikilink form renders the same thing:

```
![](photo.jpg)          → image with LQIP, WebP, dimensions
![](clip.mp4)           → video player with controls
![](track.mp3)          → audio player
```

`![[photo.jpg]]`, `![[clip.mp4]]` and `![[track.mp3]]` are the wiki spelling of the same three embeds. Write the path relative to the page; if the file is not there moss tries the site root, then the nearest file with that name, and `%20` stands for a space. A wikilink needs only the file name. Text in the brackets plays the role the text after the pipe plays in a wikilink: `![640x360 loop](clip.mp4)` equals `![[clip.mp4|640x360 loop]]`, and on an image it is the caption unless an italic line follows the image or the text ends in a bare `|` (below). A page, table, notebook or folder embeds the same way in either spelling: `![](notes/essay.md)` equals `![[essay]]`, and `![limit:2](journal/)` equals `![[journal/|limit:2]]`. On a page, table or notebook the bracket text is ignored. One word is not interchangeable: `cover` (and the other display keywords) crops an image after the pipe but captions it in the standard brackets.

A width token after the pipe sets the band an image is laid out in: `![[harbor.jpg|page]]` lays the picture out at page width and tells the browser to fetch a larger copy for it, where a plain embed gets the body-column size. The tokens are `body`, `wide`, `page` and `screen`; `full` means `screen`.

An image followed on the next line by one italic line becomes a figure: the bracket text is its alt text and the italic line is the visible caption, as in `![Harbour from the east pier](harbour.jpg)` then `*Dusk, 14 May*`. A caption-only image, `![Dusk, 14 May](harbour.jpg)`, keeps `alt=""` so the text is read once. A bare `|` ending the bracket text makes it alt only, with no caption: `![Two dancers face each other on a pink stage|](dancers.jpg)` renders the image with that alt text and no figcaption. A width token can sit between the alt text and that bare `|`, and the image keeps the width with no caption: `![A lake at dusk|page|](lake.jpg)`. A picture a reader may want to see closely opens in moss's full-screen viewer when they click it or press Enter on it: a figure, a picture alone in its paragraph, a picture in a `:::grid` cell and a `:::gallery` photo, with the caption (links and credit included) under it, and the arrow keys step through all of them on the page in reading order. It needs a name, so give it alt text or a caption; `![](x.jpg)` with neither is decorative and stays a plain image. A picture inside a link (`[![](a.jpg)](page/)`), a hero, a listing card or an element with the class `no-viewer` stays plain too: wrap pictures that should not open in `:::{.no-viewer}`, or add `{.no-viewer}` to the `:::grid` or `:::gallery`. Without scripting every picture is an ordinary image.

A folder embed takes comma-separated options after the `|` that follows the folder name, for example `![[Opera/|style:grid,limit:6,sort:date]]`. The options are `limit:N`; `sort:` one of `date`, `date-asc`, `weight`, `title`; `style:` one of `list`, `summary`, `grid`; `group:` `year` or `none`; `depth:all` (every descendant, not just the direct children), `covers:only` (only pages that have a listing picture, below) and `more:<folder>` (where the "More" link points). `style:list` is the compact dated index; `style:summary` is rows with covers and descriptions. A folder note can list its own folder as `![[<FolderName>/]]`: when the relative target is missing beside the note and starts with the note's own folder name, it is read from the note's parent folder, so `![[Opera/]]` in `work/Opera/Opera.md` lists `work/Opera/`. A path that starts with `/` names the folder from the site root, so it reads the same from any page: `![[/work/Opera/]]`.

moss warns and you lose enhancement (LQIP, WebP encode, thumbnails, dimensions) if you use raw HTML tags for media that moss owns.

A picture that is only decoration, with nothing a reader needs to be told (a texture, a band of ornament, a faded page behind a heading), belongs in the theme, not in a page: put the file in `.moss/theme/` and paint it with CSS (`background` or `mask` on a moss selector or pseudo-element). An image embedded in a page is content, so it needs alt text, and `--strict` reports one without it. Giving decoration made-up alt text to quiet that report makes screen readers announce it.

**Ambient loop video.** Add `|loop` to a video embed for a silent background
loop — autoplays, no controls, respects `prefers-reduced-motion`:

```
![loop](clip.mp4)           → ambient loop
![640x360 loop](clip.mp4)   → ambient loop + explicit size
```

The wiki spelling is `![[clip.mp4|loop]]`.

The `|loop` preset atomically forces `autoplay muted loop playsinline` and
removes the control bar. It is a fixed preset, not a set of per-attribute flags
— moss enforces this to keep the browser's autoplay policy from blocking
the video.

For the exact per-type attribute vocabulary (data-loop, data-type, sizing
tokens, etc.) run `moss describe --json`, or see `mosspub.com/docs/reference`
— those reflect the running binary.

A picture can sit beside the running text: `![[photo.jpg|align-left 40%]]` floats it to the left at 40% of the column and the text runs around it; `align-right` does the same on the other side, and with no percentage the float takes at most half the column. It works for an image, a captioned figure, an embed and a folder listing. Below 48rem the float is dropped and the picture takes the full width, so nothing needs a theme rule for phones.

### Text from a file

When an installed plugin can read a kind of file (a PDF, a deck), `![[report.pdf#text]]` puts the text it derived into the page as ordinary markdown, with a placeholder while it is still being read and a plain note if the file could not be read. Text that is ready sits in `<div class="moss-derived" data-fragment="text">`, so a theme can set it apart from your own writing or lay it beside its source file; the `moss-derived--pending` and `moss-derived--skipped` forms are the placeholder and the note. The fragment name (`text` here) is the plugin's; `moss import --status` and the plugin's manifest list what it offers. A page whose only content is such embeds is a **stub**: `moss import <folder> --allow-plugins` writes one beside each file in a folder, containing `inferred: [title, description]` in its frontmatter (the page takes its title and description from the first embedded file that has them, and a `title:` or `description:` you write wins) and the embeds the plugin's layout asks for. Write your own text above or below them; a stub is never overwritten. `moss expand <page> --allow-plugins` replaces each line holding only an embed with the text it stands for, when the page should keep the text without the source file. `--allow-plugins` is needed whenever the plugin came with the folder rather than being allowed in the app; without it import and expand stop and say so.

### External media embeds

Embed a video or pen by URL. The standard form makes the same player as the wiki form for a YouTube, Vimeo or CodePen address; any other `![](https://…)` stays a plain image:

```
![](https://www.youtube.com/watch?v=dQw4w9WgXcQ)
![](https://vimeo.com/123456789)
![wide](https://www.youtube.com/watch?v=abc)
![640x360](https://www.youtube.com/watch?v=abc)
```

The wiki spelling is `![[https://www.youtube.com/watch?v=dQw4w9WgXcQ|640x360]]`. YouTube, Vimeo, and CodePen get provider-aware players; only the wiki form turns another https page URL into a generic iframe. Give a frame a title and a size together as `![[https://example.test/form|Sign-up form|420x620]]`; a frame without a title gets a default name: the player's for YouTube, Vimeo and CodePen, or the site's for any other page.

## Content structure

The home page's title is the site's name; moss adds it as a hidden heading for screen readers unless the page opens with its own `# ` heading or a hero.

A page's title is its visible heading, including a page in the navigation, whose nav label may differ from its title. A home page, and any page whose opening `:::hero` stands in for its masthead, gets its title as a visually hidden `<h1>` instead, so screen readers have a heading, unless the body already supplies one: a `# Heading` in the body, or in a `:::hero` block. A hero of only an image and a link does not supply one. To show a visible title on an article, write the title as a `# Heading` that opens the body: when its text equals the page's title, moss keeps it as written and adds no second copy above it. A different opening `# Heading` sits below the injected title.

A folder with no file anywhere beneath it publishes nothing, and the build names it; it gets its page once it holds a page or a file. A home page or folder page lists its child pages by default. `children: false` in its frontmatter turns that listing off, and on a place page it also drops the place's term listing, while links to the place still resolve.

Quote a frontmatter value that contains a colon followed by a space, such as `title: "Notes: spring"`. Unquoted, it is invalid YAML, and the whole frontmatter block is ignored rather than the one field, so every field on the page falls back to its default. The build prints `frontmatter: invalid YAML`; `moss build --strict` turns that warning into a failure.

A term line followed by a line starting with `: ` is a definition list, which renders as `<dl>` with the term as `<dt>` and the definition as `<dd>`, as in a credit written `Set Design` then `: Ines Varga`. Put a blank line between one term-and-definition pair and the next: without it, the next term is read as another line of the previous definition and renders inside its `<dd>` after a line break.

A page's listing picture is what its card shows in a folder listing, a grid card or a `covers:only` embed. Write `cover: photo.jpg` to choose it; with no `cover:`, moss uses the page's own first picture, the opening hero's image or else the first figure, gallery picture or grid-cell picture in the body, so a page that opens with its picture needs no `cover:` line. `cover: false` lists the page with no picture. A picture set in a sentence, a heading or a table, inside a link to another page, a vector or video, and a remote or `data:` address are not taken. The listing picture is for listings and the share image only and is not drawn in the page body: a leaf page shows only the pictures its body places, and a folder's home page, or a page that claims a name, shows a written `cover:` beside its title, which a first picture inferred from the body never does.

### Folder cascade

A folder's home page can declare `cascade:` to push values to every page beneath it, so a value that every page in the folder repeats is written once. The keys cascade carries are `nav`, `breadcrumb`, `footer`, `children`, `children_style`, `children_group`, `children_depth`, `tags`, `description`, `content_width`, `title_block`, `sort` and `timezone`. The page that declares it is not included, and a page that sets one of these keys itself keeps its own value. A key outside that list has no effect; the build warns once per source page and names the keys cascade does carry.

### Shortcodes

Shortcodes are fenced blocks (`:::hero`, `:::grid`, …); callouts use
`> [!note]`. The set is version-specific, so read it rather than assuming:

```
moss describe --json | jq -r '.components[] | select(.authorable) | .class'
```

Each entry carries `example_markdown`, which is the invocation moss itself tests — copy that shape rather than reconstructing one. Add a site-specific class to a shortcode invocation and style the class; keep the shortcode generic.

A hero's words belong on its image: the markdown written inside the `:::hero {…} … :::` fence overlays the picture, and that is what makes it a hero rather than an illustration with a heading. The overlay sits bottom-start on every overlaid layout by default; `align=end` moves it to the inline-end edge instead — the right in a left-to-right page, the left in a right-to-left one; `align=center` sets the words in the middle of the picture with centred text, for a short title over a calm image. The overlay itself sits on a panel tinted from the image's own colour (`--moss-hero-panel-bg`, computed per image) and sized to its own content, never the hero's full height, so a busy photo with no calm corner no longer costs legibility and a short line of text never washes out most of the picture. Cropping still matters for which part of the picture shows: set `--moss-hero-object-position` so a deliberate subject (a face, a line of text in the photo itself) lands where you want it, and look at the result at desktop and phone width in light and dark mode. The hero frame grows to fit however much the overlay holds, so a long paragraph is never clipped by a fixed-height box. Phones show the image whole with the words below it unless the hero carries `mobile=overlay`, and under that mode the panel is already full width, so `align=end` makes no visible difference there; that mode also always keeps a band of the image showing above the panel — `--moss-hero-mobile-band`, default `min(45vb, 320px)` — so a long paragraph's panel never covers the whole picture. Move the crop first; when the crop cannot move the subject out from under the words — a portrait image already spanning the full width of a wide frame cannot move sideways — put the words at the other end with `align=end` instead. An image-only hero (`:::hero {image=…}` with nothing between the fences) or `caption="…"` is for an image that must be seen whole, which is a plate (next paragraph). `header=over` on the page's opening hero (`:::hero {image=… header=over}`) lays the site header over the photo instead of above it, in light type, for a page whose first block is the picture.

`:::hero {.plate}` (2026-09-11) is a moss default, not a per-site class to style: it renders the hero image whole — never cropped, never enlarged past the resolution it was delivered at, shrunk to fit the column instead. Reach for it over a plain hero whenever the image's own shape, not the page layout, should decide how large it appears — an artwork, manuscript page, or photograph reproduction where cropping would cut off part of the object, and especially a wide or tall outlier (a handscroll, a long strip) that a viewport-relative `100vw` sizing would otherwise fetch too small and stretch blurry. A plain `:::hero` (or `:::hero {caption="…"}`, which already avoids cropping but still bounds the frame at the default height cap) stays right for a banner meant to fill its slot.

The same rule extends past physical-object reproductions: any image that carries its own lettering, or whose full frame is the point rather than a subject inside it — a poster, a flyer, a book cover, a chart — is a plate too, because a crop can cut off a headline or an axis exactly as it would a manuscript's edge. That holds where the whole frame is shown, such as the work's own page; when the same poster heads another page as its hero, crop it to its picture, keep its lettering out of frame, and let the page's own heading and text overlay it, since they say what the lettering said. Give a plate's own text a `caption="…"` instead — moss places it below the image, in the reading column, never over it, the same slot a captioned (non-plate) hero uses. There is no documented way to set a plate beside running text on a wide screen; the caption below it is the only placement moss supports.

A hero can rotate through several pictures (2026-07-27, undocumented until 2026-10-02): write one image embed per line at the top of the fence, before the overlay, and leave `image=` off — `:::hero` / `![](hall.jpg)` / `![](lake.jpg)` / `# Title` / `:::` (`![[hall.jpg]]` works the same). The first embed is the primary slide: its pipe attributes set the crop for every slide, its colour tints the panel and picks the scrim, and it is the only picture a reader with reduced motion sees. The alt text of the first embed in a hero fence becomes the picture's alt, and an embed with no alt text leaves the picture decorative. The rest crossfade behind the words in order — seven seconds a slide, a 1.5 s dissolve, at most six slides; a seventh is dropped with a build warning. The rotation pauses while the hero is hovered or holds keyboard focus, and a small pause button in the corner opposite the panel stops it for good (a checkbox, no script; it is hidden for readers with reduced motion, who see only the first slide). It pauses the crossfade only: a video slide keeps playing. Image rotations stay interchangeable moods of one subject, never a set a reader needs to see whole: that is `:::gallery`. A slide may carry a caption, written as its embed's pipe text (`![[a.mp4|*Title* · Company · Year]]`); a pipe made only of frame keywords such as `cover top` sets the crop and carries no caption. A caption may link, for example to the production on screen: `![[a.mp4|[*Title*](work/title.md) · Company · Year]]`, and the link resolves exactly as it does in the body, and it is underlined in the caption's colour so it reads as a link. Captions never overlap the hero's words: with `align=end` or `align=center` on a wide screen the caption sits at the bottom start, clear of the words; otherwise (and always on a phone) the caption sits directly above the words over the picture, and in a stacked phone hero (no `mobile=overlay`) it is a quiet credit line between the picture and the text block. When captions are present the pause button stays at the bottom end corner (top on a phone). `--moss-hero-max-height: 100svh` also makes a `mobile=overlay` hero fill the phone's first screen. Every slide fills the hero at any height. On a phone with `header=over` the pause button sits just below the header instead of at the top corner. A hero whose every slide is a video plays as a sequence instead of rotating on a timer, and each caption names what is on screen. The script adds a clip indicator above the caption, one thin bar per clip: each bar is a button that jumps to that clip, and the playing clip's bar fills as it plays. The pause checkbox holds the current video and stops the advance until it is unchecked. Under reduced motion no video plays and neither control is shown; the first slide and its caption stay on screen. A slide may be a video: in an image rotation it gets the same ambient loop as a video hero, but in a sequence each video plays once and the next one follows when it ends. An `image=` hero has exactly one picture: its whole body is overlay, so an embed there is a picture inside the words, not a slide.

Separate a grid's cells with a line that holds only `+++`. A grid takes one to six columns: `:::grid N` with N above six builds at six columns and prints a warning. A two-column grid of two cards looks like this:

```
:::grid 2
[[harbor-walk]]
+++
[[lantern-trail]]
:::
```

`:::grid N {scroll}` (2026-09-21) keeps a grid's row on one line and lets the reader drag it sideways instead of it wrapping — `N` becomes how many cards fit in view at once, with a slice of the next one showing as the cue to keep going. Reach for it on a "related articles" or "more like this" strip where reading order matters more than seeing every card at once; add `label="…"` to give the row an accessible name when the surrounding heading doesn't already say what it is. Skip it when every card must be visible without scrolling and use the plain wrapping grid instead.

Under a scroll row, moss shows where the reader is. A row of up to seven cards gets one dot per card. A longer row gets the current card's number and the total ("12 / 40") between ‹ and › buttons that move one view, because dots stop telling a reader where they are past about seven. The number is the first card in view, or the last card once the row is at its end, and its box is sized for the widest number so nothing beside it moves.

A theme can replace that indicator for one row. The row fires a `moss-scroll-position` event whenever the current card or the cards in view change, with `detail` holding `current`, `first` and `last` (1-based card numbers) and `count`; the indicator carries the same numbers as `data-current`, `data-first`, `data-last` and `data-count`, so a script that starts later can read where the row already is. Hide the default for that row and draw your own from the event in `.moss/theme/script.js`; move the row by scrolling a card into view (`card.scrollIntoView({ inline: "start", block: "nearest" })`):

```css
.moss-grid.timeline + .moss-scroll-dots { display: none; }
```

```js
document.querySelector(".moss-grid.timeline")?.addEventListener("moss-scroll-position", (e) => {
  const { current, count } = e.detail;
  // draw your own indicator for card `current` of `count`
});
```

A grid is also how two things sit side by side, because a cell takes any markdown, not only a card: headings, paragraphs, lists, images. `:::grid 2 1:2` gives a narrow column and a wide one (an address beside a text, a note beside a picture); the ratio has one part per column. Cells start level at the top and have no box or border, and at 768px and narrower the grid becomes one column in the order you wrote the cells. Three defaults were chosen for cards and show in a prose cell: a picture in a cell is cropped to a square with rounded corners (set `--moss-grid-image-ratio: auto` and `--moss-grid-image-radius: 0` on the grid's class to show it whole), a cell's paragraphs sit 8px apart, not a paragraph's usual distance, and a cell holding nothing but a link becomes a card. When the page opens with a grid whose cell holds the first heading and that heading's text equals the page title, it is the page's title (an article gets no second one above the grid) and the `byline:` is printed right below it inside the cell; a different heading leaves the title above the grid with the byline under it, and a grid that comes after other content leaves the byline at the top of the page. Reach for it before any theme rule that floats, absolutely positions or places by `:nth-of-type` to get columns. Use the ratio for a one-off; when the theme needs to change the columns at a breakpoint, write `:::grid 2 {.your-class}` with no ratio and set `grid-template-columns` on the class, because a ratio is written as `--moss-grid-ratio` on the element: a class rule setting `grid-template-columns` overrides it (also inside `@media`), but a class rule setting `--moss-grid-ratio` does not, since the element's own value wins. (2026-10-13: a builder looking for a side column searched this guide for "sidebar" and "column", found only the child rail, and wrote eight float-and-offset rules that a grid replaces.)

A set of photos meant to be seen together is a `:::gallery` block with one image per line, written as `![](a.jpg)`, `![[a.jpg]]`, or a bare file name; `:::gallery N` sets how many columns it has. `:::gallery rows` lays the photos out in justified rows that fill the width and keep each photo's proportions, and `--moss-gallery-row-height` sets the row height. Each photo opens full screen in moss's own viewer, where the arrow keys step through the page's pictures in reading order (the gallery's photos and the page's figures) and Escape closes it and returns focus to the last picture shown. A photo with no alt text gets the link name "Open image 2 of 5", so write the description in the alt text whenever the picture needs one. A photo followed on the next line by one italic line takes that line as its caption, as a figure does: `![Harbour](a.jpg)` then `*Dusk, [the quay project](projects/quay.md), 1931*`. The caption shows under the photo, in `rows` too, may hold a link, and is what the viewer shows under the large photo; a photo without the line has no caption. The bare `|` form works in a gallery too: `![Two dancers on a pink stage|](a.jpg)` gives the photo that alt text with no title in the viewer. A gallery is a set a reader goes through, unlike the hero rotation, whose interchangeable moods of one subject give no sign of which one is showing, and unlike `:::grid`, whose cells are cards that link to pages.

A `:::grid` cell holding nothing but a page reference becomes one of two different things depending on the `!`. `![[page.md]]` is a transclusion — it inlines that page's markdown as the cell's own content, same as a partial anywhere else in a body. `[[page]]` or `[[page|Custom Title]]`, alone in the cell, becomes a **card** instead: the page's own cover, title and description, styled the same way a folder listing cards its children. The cover is the page's listing picture (below); a page with none shows a card with no picture. The title is the page's own (`|Custom Title` overrides it), and the text is the page's `description:` field or, when that is empty, its first `byline:` row — never an excerpt pulled from the body, so a page with neither shows a card with no text under its title. (Folder listings in list and summary styles are different: they use `description:`, then the first `byline:` row, then an excerpt from the body.) A folder shown as a card shows its folder note's `description:` in a paragraph under its title, with its page count beside the title, so a folder of folders reads like a contents page. A card's cover is cropped to a fixed box by default (`4 / 3`), same as any other photograph; a cover with lettering or a full-frame subject wants to be shown whole instead, which is a scope override (rung 4), not a per-card frontmatter switch — set `--moss-card-cover-ratio` to the image's own ratio and `--moss-card-cover-fit: contain` on `.moss-card-cover` (or a narrower scope) rather than cropping it.

A fence with a name or `{attributes}` opens a block, and a bare line of three or more colons closes the innermost open block; the number of colons does not matter, so use more only if you like the look. `::: {.intro}` around `::: {.figure}` nests, every closer can be `:::`, and `::::grid 2` around `::: {.note}` reads the same way. An opener nothing closes, or a closer with nothing left to close, is a build warning that names the line.

### Partials

Extract repeated blocks to a partial file and transclude with `![[partial-name]]`. Partials are content reuse, not styling — they live outside the styling rungs. Two limits: a partial file is also a page, published at its own address like any other page, and `draft: true` or `listed: false` hides it from listings without removing that address; a dot-prefixed file name keeps it out of the build, but then the embed does not resolve either. An embed expands only when it is the only thing on its line, so `![[partial-name]]` written inside a sentence is left unexpanded and its text does not appear — put the embed on a line of its own.

A fenced div takes a width word like any other block: `:::{.band screen}` makes the div as wide as the window (`wide` and `page` are narrower), and its children stay at the reading width, centred, so the theme paints a full-width band with `.band { background: … }` and no `100vw` or `box-shadow` trick. The div must sit directly in the page text, not inside another block.

```markdown
:::{.band screen}
A statement on a full-width background.
:::
```

### Footer

A `footer.md` at the top of the site is the footer. Its markdown renders as written inside the page's `<footer>`, on every page, and replaces the row of links moss otherwise builds from pages marked `footer: true`. Shortcodes work there as they do in a page, so footer columns are a grid, and the theme only sets their widths:

```
:::grid 3
## Visit

12 Harbor Lane

+++
## Write

hello@example.com

+++
## Follow

[Newsletter](https://example.com)
:::
```

A heading in the footer gets no permalink mark (a `slot:` page has none on its own page either, since one render serves both), and a `---` there is a plain line, not the three-dot ornament. A dark footer band is three properties in the theme: `--moss-footer-bg`, `--moss-footer-ink` (text, links and headings) and `--moss-footer-rule` (the line above the footer and any `---` in it), with no selectors and no colours repeated for each element. The band runs the full page width. The grid's columns are equal and become one column at 768px; for other widths, or a different arrangement on a phone, give the grid a class (`:::grid 4 {.footer-cols}`) and set `grid-template-columns` on it in the theme. Short link lists read better as two columns side by side on a phone, with a long block (identity, address) spanning both; one column stacks them into a long footer. The footer sits in the page column; a width word (`wide`, `page`, `screen`) does not widen it. moss's own one-line colophon is the footer's last line, inside it, so a footer band's background covers it. To hide the mark, the theme sets `.moss-colophon { display: none; }` in `.moss/theme/style.css`; there is no config switch for it. The footer keeps its own end space whether or not the mark shows, so hiding the mark leaves that space. A theme that places the mark in its own row sets the colophon's whole `margin` (the end margin cancels the footer's end space) and padding and keeps the link's padding, which holds the line the hover wording appears on. A picture below everything is the footer's own `::after`, and the footer sets `padding-block-end: 0` so nothing follows it (hide moss's colophon with `.moss-colophon { display: none }` if the picture should be the very last thing). A picture behind the footer's text needs more: draw it as the footer's `::before`, which replaces moss's divider, so reset that rule's `margin-block-end` and border; make the footer a flex column, put the columns, legal row and colophon before it with `order`, and lift it behind them with a negative top margin. Small text needs near-empty ground, and a footer picture has no hero panel behind it: if the picture's quiet area is not tall enough for the text, extend the picture with an even fade above its subject rather than setting text over detail (2026-10-14: a legal row was unreadable over the dense top of a footer picture). A site in several languages can keep one `footer.md` per language folder. (2026-10-13: a rebuilt site's theme carried 24 rules placing footer columns by `:nth-of-type` because nothing here said a footer could hold a grid; written as a grid, every one of them went.)

### Welcome email

A `welcome-email.md` at the top of the site is the email each new subscriber gets once, right after they confirm. It is not a page: it has no address and no nav item. Write it in `zh-hans/welcome-email.md` (or any other language folder) for subscribers of that language; a subscriber of a language without its own file gets the top-level letter.

```
---
subject: Welcome to Harbor Notes
preview: What to expect, and how to write back
from: hello@example.com
reply_to: me@example.com
---

Hi {{email}},

Thanks for joining {{site_name}}. New posts arrive at {{site_url}}.
```

`subject:` is required; a file without one sends nothing and the build warns. `preview:` is the line mail apps show beside the subject. `from:` and `reply_to:` are optional. The `from:` address is used only when the site's mosspub.com hosting has verified that sending domain; otherwise mail goes out from its default, and replies still reach the `reply_to:` address if you set one. The body is markdown; `%%comments%%` are removed, so notes to yourself do not reach subscribers. Three variables are filled in per subscriber: `{{site_name}}`, `{{site_url}}` and `{{email}}`. An unsubscribe line is always added at the end for you, so do not write one. When the site is published to mosspub.com hosting, the build writes the file to `.moss-email/welcome.<language>.json` (`welcome.default.json` for the top-level one) and publishes it with the site; on any other host the letter is not used and nothing is written.

### Term kinds

Name-list fields are listable dimensions, not just metadata. Every value generates a page listing what carries it: `author: 趙雲` gives `/authors/趙雲/`, `tags: [城市]` gives `/tags/城市/`. Nothing to set up, and a link to a term page never 404s.

Any field a `[terms.<key>]` block names in its `fields` list is a name-list field for that site, so you can invent the dimension you need. Five built-in fields need no declaration: `author:`, `tags:`, `editor:`, `jury:`, `location:`. Out of the box `author:` feeds `/authors/` and `tags:` feeds `/tags/`; `editor:`, `jury:` and `location:` feed nothing until a site says where they go.

A **term kind** is one namespace fed by one or more of those fields. Declare one in `.moss/config.toml`:

```toml
[terms.people]
fields = ["author", "editor", "jury"]
title = "People"
```

Now all three fields feed one namespace: someone who wrote one piece and edited another has a single page at `/people/<name>/`, not two. That page lists their works in sections — "Author", then "Editor", then "Jury", in the order `fields` gives — so the listing says what each credit was. A person reached through only one of the fields gets a plain listing with no section headings.

A declared field takes its value the way `author:` does: one name as a string, or a list of names. For example, a concert site that credits people by role:

```toml
[terms.performers]
fields = ["conductor", "soloist"]
title = "Performers"
```

```yaml
---
title: Spring Concert
conductor: Ada Example
soloist: [Ben Sample, Ada Example]
---
```

Here `/performers/ada-example/` lists the Spring Concert under "Conductor" and "Soloist", the label being the field's own name with its first letter capitalized. A declared field has no translated label, so it reads as written.

Add `cards = true` to the kind and its names show on listing cards, with their role: a card for the Spring Concert reads `Ada Example, conductor · Ben Sample, soloist`, the fields in `fields` order. The card and each credit are classed `moss-card-credits`, `moss-credit` and `moss-credit-role`, so a theme can style them. The names are plain text, never links, because a card is already one link. Without `cards = true`, a card shows only its date and place, as before.

Add `nav = true` to the kind and its root page (`/performers/`) joins the nav bar after the content entries, labelled with the kind's `title` or, with none, a built-in kind's translated title or the key capitalized; several such kinds keep the order of their blocks in the config file. The root page is headed by the same name, nav entry or not, in its `<title>` and in the breadcrumb of every term page under it: `[terms.performers]` with no `title` heads `/performers/` "Performers". Add `weight = 2` (an integer) to place the entry among the pages by weight, the way a page's own `weight` does: lower first, ties alphabetical by label, and entries without a weight after those with one. The entry appears once the kind has at least one term, in each edition that has one: the site language's nav shows `/performers/`, a language tree's (`de/`) shows `/de/performers/`, and a page shows its own edition's entry only. A `title` can name the kind per language, the way a declared field's `label` does: with `title = { en = "Performers", de = "Mitwirkende" }` the German entry and the German root page read `Mitwirkende`, and an edition the table has no entry for reads the `en` one. A plain string is the title in every edition.

Term pages are per edition, as a language tree is. A page in `de/` that names `Ada Example` appears on `/de/performers/ada-example/`, which lists only German pages; the default edition's page for the same name, `/performers/ada-example/`, lists only its own. The two are translations of each other, so the language switcher and `hreflang` pair them, and a page's byline links, credits and a listing plugin's `ctx.terms` point at its own edition's page. A `<field>_page: true` claim is per edition too: the English page and its German translation can both claim `Ada Example` without a duplicate-claim warning, while two claims inside one edition still warn. The kind's root (`/performers/`, `/de/performers/`) is per edition as well. A tree for the site's own language (`en/` on an English site) is part of the default edition: its pages share the default edition's term pages, and the root home lists them. On a site whose root holds nothing but its home page, that tree is an edition like the others. Place-typed kinds (`location:`) are the exception and stay site-wide for now.

A name only a language tree uses has its page in that tree alone, and its address without the tree, `/performers/<name>/`, forwards to `/de/performers/<name>/`; so does the kind's root `/performers/` when only a tree uses the kind. A name the default edition also uses keeps its page at `/performers/<name>/`.

Naming a field in a declared kind takes it out of its built-in namespace: with the block above, `author:` no longer feeds `/authors/`. The old `/authors/<name>/` URLs keep working, and so does the bare `/authors/` root itself — moss emits a redirect to the new page for as long as the field belongs elsewhere. That redirect never lands on a real page: if you have hand-authored content living at the old address, moss leaves it alone rather than burying it under a meta-refresh.

Any real page can take a generated one's place. `author_page: 趙雲` — or `true`, meaning "the name is my title" — makes that page the author's page: its body is the bio, its `url:` is wherever you want it, and the works listing attaches below. Every field has its claim: `tag_page:`, `editor_page:`, `jury_page:`, `place_page:`. A declared field claims with `<field>_page:` the same way, so `conductor_page: true` on a page called Ada Example makes that page the page for `conductor:` names. Any of a kind's claims takes over that kind's page, so in the `people` example above a bio page claims with whichever credit fits. Mentions of the term on the pages of the claiming page's own edition then link there instead of at the generated URL, and the generated URL forwards to the claiming page, so a link someone saved before the claim still arrives. Claiming adds that listing below the body, or just above it when the body closes on a sources section (a last `## Sources` or `## References`, `## Quellen` on a German page, `## 来源` on a Chinese one), and also gives the page's own `cover:` a visible place in the body — beside the title, the same layout a folder-index page claiming the same term already uses. An unclaimed leaf's `cover:` is its listing picture and share image, and is never drawn in its own body.

Two things that surprise people:

- **Inline `#hashtags` derive no pages** — only frontmatter `tags:` do. An inline tag still reaches `article:tag` metadata and JSON-LD keywords; it is prose, not cataloguing.
- **Namespace roots are not in nav and not listed by their parent.** A folder holding every author name is wrong as a nav item on most sites, so `/authors/`, `/tags/` and any declared kind's root stay reachable through term links, sitemap and search instead.

To stop a dimension generating pages, either switch the built-in off — `[terms].author = false` or `[terms].tags = false` — or drop the field from the `fields` list of the kind that claims it.

### Declared fields

A site can declare its own frontmatter fields in `.moss/config.toml`, each with a type. moss checks every page's value against that type and warns, naming the file and the field, when the value does not fit; listing plugins receive the parsed value. A declaration cannot redefine one of moss's own fields, except that `description` and `publisher` can take a `place` (see below).

```toml
[fields.kind]
type = "choice"
of   = { Building = { en = "Built", de = "Errichtet" }, Service = "Founded" }

[fields.years]
type = "period"

[fields.architects]
type = "pages"
in   = ["Housing"]
```

A page then writes plain values, such as `kind: Building`, `years: 1927/1933` or `architects: ["[[Anna Weber]]"]`. Quote a page link, because unquoted `[[…]]` is read as a nested list. A link is checked as the author wrote it, so `[[Riverside Court]]` and `[[Housing/Riverside Court]]` both pass. A field a page does not carry is fine.

| Type | A page writes | Plugins receive |
|---|---|---|
| `text` | any text | the text |
| `number` | `48.1` | the number |
| `date` | `1930-10-12`, `1930-10` or `1930`; `1923?` uncertain, `~1930` approximate, `1930%` both | `{ value, precision, qualifier }` |
| `url` | `https://…` or `/path/` | the address |
| `choice` | one of the values in `of` | `{ value, label }`, the label in the page's language |
| `period` | `1927/1933` or `1927–1933`, `1927/..` for an open end, or one date | `{ start, end, open, text }`, each date as `{ value, precision, qualifier }`; `open` is true only for an interval with no end |
| `count` | `1,382 flats` | `{ number, noun, text }`, or `{ text }` when the value is not a number followed by a noun |
| `page`, `pages` | `"[[Page name]]"` or `"[[Page name\|text]]"` | `{ target, alias? }` per link |

A choice's `of` gives one label for every language when it is a string. A table gives a label per language tag (`en`, `de`, `zh-hans`, `zh-hant`); a page in a language without its own label uses the `en` one, then the value itself. Choices keep the order the file writes them, in either form; plugins receive them in that order.

A count's digits follow the page's language. On a German page `.` groups digits and `,` is the decimal, so `1.382 Wohnungen` is 1382 and `48,1 km` is 48.1. Other pages read `,` as the grouping mark and `.` as the decimal.

`in` limits a field to some folders, and a language tree counts as inside its folder: `in = ["Housing"]` also checks `de/Housing/…`. `moss build --strict` turns a value that does not fit into a failure.

`place` makes moss draw the field on every page that carries a valid value, so the fact is written once in frontmatter and never repeated as a styled block. The value is read in the page's language, so a German page shows an open period as "ab 1923" and groups a count with ".". A folder index and a generated term page draw nothing.

| `place` | Drawn as | Class |
|---|---|---|
| `eyebrow` | a small line above the title; every eyebrow field, in the order the file declares them, one `<span data-field>` each | `moss-eyebrow` |
| `subtitle` | a line under the title; only the first subtitle field is drawn | `moss-subtitle` |
| `key-figure` | a large number under the title, then its noun, for a number or a count; any other value, such as `Start` or a text field, draws as one `<span>`, so a theme can style `.moss-key-figure > span:only-child`; one line per field | `moss-key-figure` |
| `facts` | a label and value row per field, in a list | `moss-facts` |

Dates and periods are `<time datetime>` elements carrying the EDTF value (`1927/1933`, or `1923/..` when open). A count is a `<data value>` element with the number as displayed, then its noun. A choice shows its label. A `page` or `pages` value links to the page where moss can resolve it, and shows the link as written (the text after `|` if there is one, else the last part of the name); where it cannot resolve, it shows the same text without a link; several pages are joined with ", ". A field whose value is missing or does not fit is not drawn on that page.

`description` and `publisher` can take a place too, and their type is their own. `[fields.description]` with `place = "eyebrow"` draws the page's own `description:` as written, never the description moss derives from the body when the page has none. A built-in takes only `place` and `label`; no other moss field can take a place.

Every placed element carries `data-field` with the field's name, and `moss describe --json` lists it among the scope attributes, so it is a guaranteed hook. Style one field with `[data-field="years"]` instead of a selector on the page's address.

A facts row is labelled with the field's `label`, a string or a table per language in the same form as a choice's labels (`label = { en = "Architect", de = "Architektin" }`); without one the label is the field name with its first letter capitalised. On a screen at least 76rem wide the facts sit in the right margin beside the title, in the space sidenotes use; on a narrower screen, on a vertical or full-width page, beside a sidebar, and on a page with sidenotes (which use that margin) they stay in the column under the title. The eyebrow sits right before the title. After the title come the subtitle, the page's date and byline, the key figures and the facts, all before the page body.

A page whose design puts its picture first, with the title under it, says so in its frontmatter with `title_block: after-picture`. For every page in a folder, the folder's home page sets it once through cascade (see Folder cascade):

```yaml
cascade:
  title_block: after-picture
```

Such a page draws its whole title block (eyebrow, title, subtitle, date and byline, key figures, facts) right after the body's leading picture: the first picture on a line of its own, when nothing but classed divs comes before it. A page whose body opens with text, a heading or a list, or has no picture, keeps the title block first, and so do folder index pages and pages that draw their own cover. Without the key the title block comes first. A theme that needs some other order still lays out `article` as a grid.

The eyebrow's fields are separated in the markup by one space only. The stylesheet draws the visible separator from `--moss-eyebrow-separator`, a CSS string defaulting to `" · "`, so a theme sets `--moss-eyebrow-separator: " "` or `" / "`. Give the string spaces on both sides: the space between fields merges with a leading one, and `"/"` alone would read "A /B".

To move a fact that pages already write as a classed div into a field, use `moss migrate --lift <class> --as <field> --place <place>`. Without `--apply` it prints the diff; with it, it saves a version of the site first, then writes. Each `::: {.class}` div holding one paragraph of plain text becomes a frontmatter line on its page, the div is removed, moss draws the value at `--place` instead (`none` keeps it in the frontmatter only, so it leaves the page; `--place` sets the field's place for the whole site, and the diff shows that change to `.moss/config.toml`), and the field is declared in `.moss/config.toml` as `text` if it is not declared yet. A paragraph wholly in italics or bold is lifted without them, so set that style on `[data-field="…"]`. A page whose div holds a link or other formatting, holds it twice, or whose frontmatter already gives the field another value is left as written and listed. `moss doctor --surface --all` then shows whether any of the class is left, and `moss doctor --surface` names theme rules still keyed on it.

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

Three fields, all optional: `lat`/`lng` place the pin (missing either one still keeps the place — its page, parent and precision all still work, just with nothing to put on a map); `parent` names another place by its display name, and that place rolls up into the parent's listing too — a page in Kyoto also appears on Japan's page, and Japan gets its own generated page even with no gazetteer row of its own, purely from being named as somebody's parent; `precision` is the privacy control, not a display preference — `exact`, `city`, `region` or `country`, and a missing or unrecognized value coarsens to `country`, the widest ring, rather than defaulting to the most precise one; an unrecognized value also prints a build diagnostic naming the file, the place, the value you wrote and the allowed set, so a typo (`"town"`) is visible rather than a silent privacy downgrade.

A place page — claimed with `place_page:`, or generated like any other unclaimed term — gets a breadcrumb up its parent chain and a list of its own children, each with how many pages are under it (counting every page anywhere in that child's own subtree, not just its direct members). A page with `location:` set gets one more thing for free: an automatic line under its byline naming every place it declared, each linked to that place's page. There is no frontmatter key to write that line yourself, and no per-page opt-out — leave `location:` unset and the page gets none; to drop the line site-wide, set `line = false` (below). The same resolved place also shows next to that page's compact date wherever it's listed as a card (grid cards, summary cards, and the plain date-and-title rows alike) — so a folder of dated, located pages reads as a chronology of places, not just a column of dates.

Place pages draw an offline map automatically when at least one relevant gazetteer entry has coordinates. A real page of your own at a place term's root (`places/index.md`, say, with no `place_page:` claim) keeps its own title and content — the map still draws below them, not instead of them. Set `map: false` in that page's frontmatter to turn the map off; this `map:` is the geographic map on the page, unrelated to `cascade:`'s own key-value map. `map:` is the one per-page switch for a page's own map, on a place page and on a located article alike (see the locator below).

Set `route: true` on a page whose `location:` list is already in the order you travelled it, to draw that order as a route: a dashed line through the stops with numbered badges (1..N), on that page's own map and its locator alike. There is no second ordered-list field — `location:` already keeps its declared order, and `route:` just says "connect these". Precision is still the privacy control: every stop must resolve to at least region precision, since a country-precision stop is too coarse a point to draw a line through — such a stop blocks the whole route and prints a build diagnostic naming the page and the stop, rather than drawing a line that jumps past it. A region-precision stop still joins the route, at the centre of the same soft area marker it's already shown with, and its badge is drawn as an outline instead of filled, so it reads as an area rather than a point. A listing map — the `/places/` root, a folder's `style: map` card — never draws a route, regardless of `route:`, since there is no single page-ordered list behind it to draw one from. Known limitation: a route with two stops on opposite sides of the 180° meridian draws a straight line across the whole map rather than the short way around it.

To add the smaller locator map after each authored page's place line, opt in site-wide:

```toml
[site]
locator = "align-right"
```

The default is no locator. `"none"` also disables it. On narrow screens the right-aligned locator collapses into the reading flow at full width.

A page can override the site setting with `map:` in its frontmatter: `map: false` hides that page's locator, and `map: true` shows it on a site whose `locator` is `none` (drawn right-aligned, the only placement there is). Unset follows the site. On a place page the key keeps its own meaning — that page's term map, on or off — and a place page never also gets a locator, even with `location:` and `map: true`, so `map` has exactly one meaning per page. A page with `map: true` but no `location:` that resolves to a place with coordinates draws nothing, with no error. `route: true` draws on whichever map the page shows.

A place namespace, parent, or leaf can also be embedded explicitly with `style:map`, for example `![[/places/kyoto/|style:map]]`. The map uses the same members and privacy precision as the place page. If the target is not a place term, or none of its entries has coordinates, moss warns and renders the ordinary listing instead.

A place's page — its own leaf (`/places/kyoto/`), or an ancestor reached only through roll-up (`/places/japan/`, with no page of its own naming it directly) — embeds as a listing anywhere in the body the same way a real folder does: `![[/places/kyoto/|style:grid]]`. Nothing on disk backs that path; it resolves through the same term membership the page itself is built from, member order included, whether the page underneath is generated or claimed with `place_page:`.

A place namespace's root page can also carry an interactive explorer layered over the same offline map — on by default, turned off with `explorer = false` in that kind's `[terms.<key>]` table (`places`, or whatever key you declared `type = "place"` on). The key is read only for a place-typed kind; setting it elsewhere is silently ignored rather than an error.

The automatic place line can be dropped for the whole site with `line = false` in the same `[terms.<key>]` table; the default is `true`. With it off, no page renders the "Location: …" line, while `location:` keeps feeding listing cards, the places root, each place's page and every map exactly as before. Like `explorer`, the key is read only for a place-typed kind and ignored elsewhere.

### Events

A page becomes an event by carrying `start:`. `date:` stays the posted date; `start:` is the event time, so when porting from a generator where `date` is the event time, move that value to `start`.

- `start` — when it starts: `YYYY-MM-DD` (all-day) or `YYYY-MM-DD HH:MM`.
- `end` — when it ends, same forms; an all-day `end` is inclusive (`end: 2026-11-03` runs through the 3rd) and must not be before `start`.
- `timezone` — IANA zone the times are in, e.g. `Asia/Taipei`.
- `status` — `cancelled`, `postponed`, `moved-online` or `rescheduled`; absent means scheduled.
- `tickets`, `online` — URLs.

Times are venue-local wall-clock time: write `14:00`, never an offset or `Z`, and name the zone in `timezone:`.

A folder of events can set its zone once with `cascade: {timezone: …}` on the folder's home page; an event that names its own zone keeps it.

What an event page shows: the meta line under the title carries the event instead of the posted date. It reads the time in the page's language (`Sunday, November 1, 2026, 2:00–4:00 PM`; an all-day or multi-day event shows dates only), a label for `status` beside it (a cancelled event's time is struck through), the `location` as text when it is not a known place (a place with its own page keeps the linked "Location:" line below), and `Tickets` / `Online` links for `tickets` / `online` (http and https only). The `<time datetime>` is the start as written, with no zone offset; `timezone:` is not shown on the page. The time carries the same `.moss-when-weekday` / `-month` / `-day` / `-year` / `-time` spans as a listing, so a theme can set the date and the time on separate lines. The separator between the date and its time (a comma in English) is a `.moss-when-sep` span, so a theme that sets `.moss-when-time` on its own line can hide it. Theme these through `.moss-event-meta` and its children (`moss describe --css .moss-event-meta`).

In a listing (a folder's children or a `![[folder/|…]]` embed) an event is ordered and shown by its time, not its `date:`: `sort: date` and `sort: date-asc` put a page by `start` if it has one, else by `date`, so an event needs no `date:` to sit among posts. Its row, card or grid card shows the written time inside `<time class="moss-when" datetime="…">`, with the weekday, month, day, year and time of day each in a `.moss-when-weekday` / `-month` / `-day` / `-year` / `-time` span (a theme can lay these out as a large day numeral), followed by the place when the page has `location:`. An event card with `tickets:` carries its own Tickets link, and an event card with `online:` carries an Online link after it; a card with either one links its title to the event page. `children_group: upcoming` (or `group:upcoming` on an embed) puts events that have not ended first, soonest first, in `<section class="moss-cards-group" data-group="upcoming">`, then everything else, newest first, in `data-group="earlier"`; ordinary articles are always in the second. This order does not depend on `sort:`, and `limit:` counts after it, so `![[events/|group:upcoming|limit:1]]` is the next event. It is decided when the site is built, with the build machine's clock, so a long-lived site should be rebuilt as events pass. A page with neither `start` nor `date:` is undated and lists after every dated page. A theme can target that place as `.moss-event-place` after a `.moss-event-sep` separator, and the event page's links as `.moss-event-tickets` and `.moss-event-online`, each also `.moss-event-link`.

Calendar files come with the page, with nothing to configure:

- Every event page gets an `.ics` file for that event, served beside the page: `Events/talk.md` (page `/events/talk/`) → `/events/talk/event.ics`. Its meta line, after the Tickets and Online links, carries an "Add to calendar" link to it; an event page shown without a meta line (one with `nav: true`, say) carries the link at the end of its body instead.
- Every folder with a home page of its own gets `calendar.ics` listing exactly the events that folder's listing shows: its direct children, or nested ones when its `children_depth` reaches them (the site home page lists everything, so `/calendar.ics` holds every event). A folder moss indexes by itself, with no home page, gets none. The links (download, plus a `webcal://` subscribe link on a deployed site) follow the listing they describe: after an embedded `![[folder/]]` listing of that folder while it has an event that has not ended, once per page, after the last embed of that folder; and at the end of a folder page that shows its own listing. A page with `children: false` prints no folder links at its end, so an embed is the way to place them. Embedding a past season shows none; that folder's own page still carries them. Event and calendar pages also declare the file in `<head>` as `rel="alternate" type="text/calendar"`. These addresses are published: people subscribe to them, so never rename them. A file you keep at the same address in the site folder replaces the generated one.
- With `timezone:` the file carries the zone (`TZID` plus a `VTIMEZONE`), so the time lands correctly in every calendar app. Without it a timed event is written as floating local time, which each app reads in the viewer's own zone, and the build warns once per site: put `timezone:` on every timed event page. A `timezone:` that is not a zone name is reported and not applied, whether the page sets it or a folder's `cascade:` does (a page's own is listed once per site, a cascaded one once per folder); an unreadable `start`/`end` and a bad `online` are each reported once per site rather than dropped silently. All-day events need no zone.
- An all-day `end` is inclusive on the page; the file stores the day after, as iCalendar requires. Nothing to adjust.
- `status: cancelled` writes `STATUS:CANCELLED`; `status: postponed` writes `STATUS:TENTATIVE`, because iCalendar has no "postponed" value and `TENTATIVE` (date not settled) is the closest. `moved-online` and `rescheduled` write no `STATUS` (the page's own `start`/`end` are the new time). `online` is written as `CONFERENCE`. The event `UID` is the page's `uid` at the site's domain (the address `--site-url` gives, or the deployed site's; a local build without either writes `localhost`). Calendar exports omit the optional `SEQUENCE` revision counter because a static build has no revision history; importing an edited file does not guarantee that a calendar client updates an earlier import. `online` is written only when it is an http(s) address.
- Drafts get no file; a `listed: false` event keeps its own file but stays out of every folder calendar; an unparseable `start` means no file.

Any page with a parseable `start:`, whatever its layout, gets schema.org `Event` JSON-LD in its `<head>` in place of the `Article` block, built from these fields plus the cover and `tags`; `location:` is emitted as a name-only `Place` (full addresses wait for place data), and timed values carry the zone's UTC offset when the page sets `timezone:`, and are written without an offset otherwise.

### Long archives

moss has **no pagination**: no `paginate:`, no `offset`, no `/page/2/`. Do not
invent one — unknown frontmatter keys are silently ignored, and hand-built
`/page/N/` folders drift the moment an article is added or removed.

Split a long archive by folder instead — by year, by series, by section. Give each folder's home `sort: date` for a newest-first chronological stream, `sort: date-asc` for the same stream oldest-first — a sequence of lectures, say, read in the order they were given — or `sort` also accepts `weight` and `title`. A subfolder in that stream sorts by its own `date:` the same way: a section of the series with a dated home page takes its place in the chronology instead of always leading the list, and its card shows that date (and its `location:`) ahead of its article count.

`date:` takes `YYYY-MM-DD`, `YYYY-MM`, or a bare year, so a work whose day or month is unknown can still sort and show what is known. Quote a bare year — `date: "1695"` — because YAML reads an unquoted `1695` as an integer and the build rejects it. On a vertical CJK page every date, on the article and on its cards, renders in Chinese numerals (一六九五年·三月).

Under `typesetting = "vertical"` the column is a page, not a scroll: its height is capped at 38em or the viewport minus margins, body type scales with viewport *height* (the `--moss-vertical-font-size` token, override it in `.moss/theme/style.css` if a site wants larger or smaller type), and the free space above it is 38.2% of the total, the golden-section head margin of a printed page. Do not centre or re-margin the column in a theme: a `margin` or `align-items` rule on `body` under vertical mode fights the default and reads as a bug on the next moss release. Added 2026-09-11 after a tall-screen round on a vertical site where the placement was nearly patched site-side.

`sort:` also accepts a list of child stems instead of an axis name, declaring an explicit order rather than a rule: children not named in the list follow after, sorted by the axis the folder would have inferred on its own (weight if any child carries one; else date when at least four in five children are dated; else title).

```yaml
sort: [intro, setup, advanced]
```

A page's own `weight:` is an integer that `sort: weight` orders by — lower first, and pages with no weight follow after the weighted ones, tied among themselves by stem.

A section can show a shorter name in the nav bar than its title: `nav_label: Reading` on a page titled "Course of Reading". The nav bar and the footer links (`footer: true`) use it; the page's own heading, the browser tab title, listing cards, breadcrumbs and feeds keep the title. A blank `nav_label` is ignored. The home page needs no nav entry for this: with `header = "nav"` under `[site]` in `.moss/config.toml`, the nav bar's link list always opens with a Home link, whatever the home page's `weight:` is (the build inserts it at the front of the list).

Breadcrumbs are switched on for the whole site by `breadcrumb: true` on the home page (left unset, they appear only when the site has no nav items), and on any other page `breadcrumb: false` hides them for that page while `true` does nothing. `breadcrumb: label` on the home page shows only the parent section, as a small link above each page's title, instead of the trail in the header.
`url:` sets where a page is published, relative to its folder: `url: share/ID` publishes at `share/id/`. A `/` is kept as a path separator, each part is cleaned up on its own, and `..` parts are dropped.

`series:` on a folder index turns on prev/next chrome for its children: `true` follows the folder's own order, a list of wikilinks declares an explicit sequence, `false` turns it off. Set `series: false` on a page inside the folder instead, and that one page drops out of the reading order — no prev/next of its own, and it stops being any sibling's prev or next. Declaring `sort:` as an explicit list, or as `sort: weight`, turns `series` on by default.

Show a capped feed on the homepage by pointing at that folder:

```yaml
children: "[[2026]]"
children_limit: 10
```

The automatic "More" link only appears on a **cross-folder** feed that actually truncated something. A `children_limit` on a page listing its own children truncates silently with no "More" link, because linking back to the page you are already on is meaningless. So always name the target folder when you want the link to appear.

A listing's folder cards say how many pages they hold ("46 articles"). `children_count` on the listing page changes that text or hides it; `{n}` is the number, and the first `|` always splits the form used for 1 from the plural (there is no way to write a literal `|`); an empty form hides the count.

```yaml
children_count: "{n} production|{n} productions"   # or false to hide it
```

### Moved and removed pages

Every page carries a `uid:`, and each build compares that uid's current address against the one recorded at the last deploy — an address that changed gets an automatic redirect, a small page at the old address forwarding to the new one. Never hand-write one of these: it isn't tracked as a redirect, so moss cannot retire it later, and a stray one at the site root gets crawled and can leak into the sitemap. The comparison is against what was actually *deployed*, so a first port with no deploy on record yet gets no redirect for anything renamed before that first publish — only a move made after at least one deploy is caught automatically. To keep the old address of a page you move before the first deploy, as a port's restructure does, add the old address to `.moss/redirects.json` as the next paragraph describes; that is the hand-declared redirect this paragraph's warning does not cover.

`url:` is one path segment that renames the page inside its folder; a `/` becomes `-`. To change a page's folder, move the file, then keep the old address with a `[redirects]` entry (below).

A page you removed, or merged into a section of another, leaves nothing for moss to compare uids against, so add the forwarding link yourself: edit `.moss/redirects.json`, a flat JSON object mapping the old address to the new one, neither with a leading slash — `{"old-page/": "new-page/#section"}` (a fragment on the target works). This takes effect on an ordinary local `moss build`; no deploy is required for a hand-written entry to start forwarding. If a real page now exists at the old address, moss leaves it alone — the redirect is dropped rather than overwriting it.

For an address that is not a page moss tracks — a hand-made `.html` file you moved, or a link out to another site — declare it in `.moss/config.toml` instead: `[redirects]` maps the old address to the new one, both site-absolute (`"/scale-compare.html" = "/assets/scale-compare.html"`, `"/old-post/" = "/writings/new-post/"`, `"/shop/" = "https://example.com/shop"`). The new address is one the site serves or an `https://` URL. A page address or a `.html` file gets a forwarding page; any other file gets a byte copy of its target, so its target must be a file on the site (an external or page target for a non-HTML file has no static form and is reported, working only on a host that reads `_moss/redirects.json`, which lists every redirect). An old address with capitals or spaces in a folder name cannot be a file path, so it gets no forwarding page and works only on a host that reads the table. An old address the site really serves is left alone and reported. The table is optional and needs no `schema_version` change.

`.moss/redirects.json` is the site's whole redirect history, automatic renames and hand-added entries together, and git tracks it, so a fresh clone or another machine keeps every old address working. Commit it with the site. A site that still has the history at `.moss/data/redirects.json` is moved over by its next build; no edit is needed.

## Search

Turn on the site search with `search = true` under `[site]` in `.moss/config.toml`. The build then writes a client-side Pagefind index and shows the search button; it is off when the key is unset.

```toml
[site]
search = true
```

## Templates

A template is a starting page that a person makes new pages from. It lives in `.moss/templates/` as a file named from the template's name: `production.md` for a template named Production. The file is an ordinary page whose four `_template_` keys moss reads and does not copy into the pages it makes: `_template_name` (the name the create menu shows), `_template_kind` (`page` for one new file, or `folder` for a new folder with a home page inside it), `_template_folder` and `_template_created_at`.

`_template_folder` is optional and names a folder from the site root in the default language, such as `work/`. Every page made from the template goes into that folder, whatever folder the author was in. Without it, a page goes where it was asked for. If the folder does not exist, `moss new` refuses and creates nothing, and `moss build` warns.

A version of a template in another language sits beside it with the language code before `.md`: `production.zh-hans.md` is the Simplified Chinese version of `production.md`. A page made inside the `zh-hans/` folder starts from it, and from the default file when there is none. A language version names no folder of its own, so its pages go to the same folder inside the language folder (`zh-hans/work/`). A language version for a language with no folder in the site is never used, and `moss build` warns about it.

`moss new` makes pages from the command line:

```
moss new work/ --template production --title "Example Show"
moss new --translation work/example-show.md --lang zh-hans
moss new --list --json
```

The first form prints the path it wrote; add `--lang zh-hans` to make the page in that language's tree from the template's version for that language. The second makes the page's version in another language at the mirrored path, with a `translationKey` shared by both pages. `--list --json` prints the site's templates, their folders and languages, and the site's language folders, so a script or an agent can check them before it makes a page.

A listing reads its folder when the build runs, so a page made from a template appears in a listing that reads that folder without being named anywhere. A home page that should show new work lists the folder instead of naming pages: `![[work/|style:grid,group:upcoming,limit:3]]`, which also puts events that have not ended first.

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
supports exactly four editions" below. `ja` is on the first list and not the
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

**moss has interface strings for exactly four languages: English, German, Simplified Chinese, and Traditional Chinese.** This is a hard ceiling, not a special case for a particular tree shape: moss's UI-language type has exactly four variants and resolves only `en`, `de` (also `de-de`, `de-at`, `de-ch`), `zh`/`zh-hans`/`zh-cn`, and `zh-hant`/`zh-tw`. A `de/` tree gets German interface strings (navigation, search, dates such as "15. Oktober 2026", comment and subscribe forms) and `<html lang="de">`, with `DE` as its switcher label (as English is `EN`). `en-us/` and `en-gb/` are accepted as directory names but have no interface strings of their own either.

**A language tree outside those four is still an edition, dressed in the site language's interface strings.** The ceiling is real, but it is a ceiling on *interface*, not on the page: a `ja/` tree gets its own term pages, nav entries and home listing like any edition. Measured on a `ja/` tree:

- `<html lang>` is **the document's own language** — a page in `ja/` is served as `lang="ja"`. moss recognizes far more codes as trees and filename suffixes (`.languages`) than it has interface strings for, and `<html lang>` describes the content, not the chrome. Screen readers and crawlers get the truth.
- The interface strings are the site language's. There are no Japanese ones, so this is the honest outcome rather than a bug — but a reader does land on Japanese body text inside another language's chrome, whichever `[site] lang` names.
- The `<title>` gets the site name appended as a suffix, which a home in an interface language (`/en/`) does not get.

So: for a fifth language, translate the body content and expect to supply the chrome yourself — an authored `ja/footer.md`, theme CSS, and your own nav if the default one is unacceptable. What you do *not* have to do any more is correct the `lang` attribute by hand.

Give each language tree its own authored home page. Without one, moss generates a home that lists the tree, but an authored home is the reliable path to a switcher entry. (A switcher entry can also come from a translation pair or from an in-language subtree, but an authored home page is the one that always works.) The ordinary folder-home rule applies, so `en/index.md` and `en/en.md` are equally valid; match whatever the site already does rather than converting it.

An edition's home and top level behave like the root's. `de/index.md` lists the whole `de/` tree, subfolders included, and never a page of another edition or of the root. A page directly inside `de/` (`de/Über.md`) joins the German nav bar without `nav: true` and is left out of the German home's listing, the way `About.md` is at the root. A folder (`de/Partner/`, or `Partners/` at the root) does not join the nav by itself; give its home page `nav: true` to put it there. Set `children_depth: direct` on the edition's home for the narrow listing of its direct children, or `children_source` to list another folder. A tree for the site's own language is not an edition, and its pages belong to the default edition, unless the root holds nothing but its home page and every language has its own tree. Then the root is a language chooser: every tree is an edition, its site name leads to its own home, and the root lists none of their pages.

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
For your own stylesheet, `moss doctor --surface` answers "does this rule style
anything?": it lists every rule in `.moss/theme/` whose selectors match no
element on any page of the last build. A rule that matches nothing but names a
class or attribute a script on the site mentions is listed apart, since the
script may add it when the page runs.

Never hardcode token names or shortcode classes from memory. They change between
releases. The output of `moss describe` always reflects the running binary.
