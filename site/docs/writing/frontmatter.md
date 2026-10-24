---
title: Frontmatter
uid: 658c6387
weight: 1
description: Metadata fields that control how moss treats each page.
---

## What is frontmatter

Frontmatter is a block of YAML at the very top of a markdown file, between two `---` lines. It tells moss about the page: its title, date, visibility, and more. ^def-frontmatter

```yaml
---
title: My First Post
date: 2024-06-15
description: A short summary for search engines and list previews.
---

The rest of the file is your content.
```

Frontmatter is optional. A file without it still becomes a page; moss uses the filename as the title.

### Fixed schema

Frontmatter uses a **fixed schema**: moss only recognizes the fields listed on this page. Arbitrary custom fields are silently ignored. For custom per-page metadata, store it in body content: a markdown paragraph, a `::: {.meta}` fenced div, or a data table.

Every recognized field is discoverable via `moss describe --json`, which prints the full schema as machine-readable JSON. The human-readable version is at [Reference](/docs/reference/).

`description:` feeds SEO meta tags, Open Graph previews, and list-card summaries; when it's left out, moss extracts one from the page's own opening paragraph instead. The value is inline markdown — `_emphasis_`, `**strong**`, `` `code` ``, and links all work — rendered as real markup on a list card and reduced to plain text in a meta tag or feed, where markup has no meaning; block-level markdown (headings, lists, images) isn't supported and is dropped rather than shown as literal syntax. It is **not** rendered as a visible deck. For a visible subtitle, use a `> blockquote` immediately after the `# H1` — the default theme styles `h1 + blockquote` as a deck.

## Identity

These fields describe what the page is.

<!-- auto:start:frontmatter-identity -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `title` | string | filename | Page title |
| `description` | string | (none) | SEO meta description and list previews |
| `date` | string | (none) | Publication date (`YYYY-MM-DD`; a year or year and month alone, or a full timestamp like `2024-06-15T09:30:00Z`, is also accepted) |
| `tags` | list | (none) | Content tags |
| `lang` | string | auto-detected | Language override (`"en"`, `"zh-hans"`, `"zh-hant"`) |
<!-- auto:end:frontmatter-identity -->

`title:` is the page's title. On article pages moss always renders it as `<h1 class="moss-article-title">` above the body, using the filename (title-cased) when `title:` is absent. If the body opens with a `# H1` whose text equals the title, moss adds no injected copy above it, so the reader sees one title; an opening `# H1` with different text sits below the injected title.

Body H1s anywhere else are section headers, not document titles. moss does not infer the title from body structure. Write the title in `title:` or rely on the filename.

To suppress the injected title on a specific article (e.g., a bespoke landing page), set `title: ""`. This is distinct from omitting the field, which falls back to the filename. Folder and index pages never inject a title; their bodies render as authored.

### Name files after the page

moss uses the filename as the page title, so the standard is to **name each file after its title, in its own language** — no `title:` field and no body `# H1` needed. `隐私.md` renders as "隐私", `部署.md` as "部署", `Privacy.md` as "Privacy".

When the filename isn't a clean ASCII slug (non-ASCII, or with spaces), **pin a stable URL** with `url:` so the published path stays short and language-neutral — `隐私.md` + `url: privacy` publishes at `/privacy`. The filename is for people browsing the folder; `url:` is the address readers see.

Pinning `url:` also keeps `[[wikilinks]]` working when you rename a file, because moss resolves them by the url slug. Renaming a file in the app rewrites links to it automatically; from the terminal, `moss rename <old> <new>` does the same.

Link to a section by its heading anchor (`[Contact](#contact)`), not a hand-written `<a id>`. moss generates the id from the heading text, CJK included.

## Navigation

These fields control how the page appears in the site's navigation and chrome.

<!-- auto:start:frontmatter-navigation -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `nav` | boolean | `true` for root pages | Show in header navigation |
| `weight` | integer | (none) | Sort order in nav and lists (lower = first) |
| `breadcrumb` | boolean | site default | Show breadcrumb trail on this page. Set `true` on the homepage to activate breadcrumbs site-wide; set `false` on any page to opt it out. |
| `footer` | boolean | site default | Show this page as a link in the site footer |
<!-- auto:end:frontmatter-navigation -->

## Visibility

<!-- auto:start:frontmatter-visibility -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `draft` | boolean | `false` | Shown in the preview only: never published, and left out of lists, feeds and the sitemap |
| `listed` | boolean | `true` | Set `false` to leave the page out of lists, feeds and the sitemap; it is still published at its own address |
<!-- auto:end:frontmatter-visibility -->

## Children

These fields control how a [[structure#^folder-page|folder page]] displays its child pages.

<!-- auto:start:frontmatter-children -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `children` | boolean | `true` | Show child page list on section pages |
| `children_style` | string | (none) | How child pages are rendered. When absent, moss picks from the children: `summary` if most of them have a cover or a description (or all but three do), otherwise `list` if any child has a date, otherwise `grid`. Values: `"list"`, `"summary"`, `"grid"` |
| `children_group` | string | `"none"` | Group by `"year"`, `"none"`, or `"upcoming"` |
| `children_depth` | string | `"direct"` | `"direct"` (immediate children) or `"all"` (all descendants) |
| `children_source` | string | (none) | Wikilink to folder whose children to show instead |
| `children_count` | `false` or string | (none) | What the folder cards in this listing say they count. `false` hides it; text such as `"{n} productions"` replaces "N articles" (`{n}` is the number; the first `|` always splits a singular form, used for 1, from the plural, so `"{n} production|{n} productions"`; an empty form hides the count) |
<!-- auto:end:frontmatter-children -->

`children_source` takes a wikilink target, e.g. `children_source: "[[news]]"`.

Card layout uses the `cover` image from each child's frontmatter.

`children_style: minimal` renders a compact, year-grouped text index — one line per article, sorted by date with year headings. Good for dense archives where card covers or summaries would add noise.

Set `children: false` to opt out of the auto-emitted child listing on a folder page or the homepage. This suppresses both listing styles — the inline list/summary layout (`.moss-article-listing`) and the card grid (`.moss-collection-grid`) — so custom homepages and folder landing pages can lay out their own content without a trailing auto-generated index.

### Sort

`sort` controls how a folder's children are ordered, which in turn shapes how the cards look.

| Value | Order | Card meta |
|---|---|---|
| `date` | newest first | year · month |
| `date-asc` | oldest first | year · month |
| `weight` | by `weight` integer, lowest first; unweighted fall to the end | (none) |
| `title` | alphabetical | (none) |
| `[a, b, c]` | explicit list of child stems first, in that order; rest by inferred axis | (none) |

```yaml
---
title: Projects
sort: title
---
```

When `sort` is absent, moss infers it from the children:

1. Any child has a `weight` field → `sort: weight`
2. At least half of the children have a `date` field → `sort: date`, with undated children listed last
3. Otherwise → `sort: title`

Most folders need no `sort` declaration: a blog folder is automatically `date`, a docs folder with weights is automatically `weight`, and a folder of dateless projects is automatically `title`.

**Why sort drives appearance:** date listings put the date in each card's meta slot; weight and title listings omit the meta slot entirely (no empty space). Folder cards in non-date listings show a small "N articles" subtitle only when they have no description.

A child folder with its own `date:` (on its home page) sorts by that date on a `date`/`date-asc` listing, the same as any page, instead of always leading the list — its card shows that date (and its `location:`, if set) ahead of the article count, so a chronology that includes a dated section still reads in order.

The legacy `order: [...]` field is a back-compat alias for `sort: [...]`.

## Places

`location:` names one or more places from `.moss/places.toml`, in the order you write them; a site with a place-typed term shows each named place on a map, sized to the coarsest precision among them (`exact`/`city` a small dot, `region`/`country` a soft area). Set `route: true` alongside an already-ordered `location:` list to draw that order as a route instead: a dashed line through the stops with numbered badges, on the page's own map and its locator; where stops are too close together for every badge, each stop keeps its dot and the numbers shown are the ones that fit, always including the first and last. A country-precision stop is too coarse a point to draw a line through, so it blocks the whole route and prints a build diagnostic naming the page and that stop; a region-precision stop still joins the route, at the centre of its own area marker, with its badge drawn as an outline rather than filled. The automatic "Location: …" line can be dropped for the whole site with `line = false` in the place-typed `[terms.<key>]` table of `.moss/config.toml`; the default is `true`. With it off, no page renders the line, while `location:` keeps feeding listing cards, the places root, each place's page and every map exactly as before. Like `explorer`, the key is read only for a place-typed kind and ignored elsewhere. `map:` is the page's own map switch: `map: false` hides the locator on a located article, and `map: true` shows it even when the site's `locator` is `none`; unset follows the site. On a place page the key keeps its own meaning — that page's term map, on or off — and a place page never also gets a locator, even with `location:` and `map: true`, so `map` has exactly one meaning per page. Where the locator sits depends on the page. On a page with a facts column (declared fields with `place = "facts"`) it heads that column, above the first fact, at the column's width; in the right margin or, on a narrow screen, in the flow at no more than its usual width on the end edge. On a page without one it floats beside the first text block of the body, and when the body has no text it follows the body as a block of its own at its usual width on the end edge. In vertical typesetting the locator is not floated: it follows the opening text as a block of its own in the column flow, starting at the same head edge as the other columns, and at phone width it keeps that position at full width. A place-typed term can read its names from a field called `place` instead (`fields = ["place"]`); `place_page:` claims the place page for it as for `location:`, as long as no other term also lists `location`. A place in `.moss/places.toml` with no `lat`/`lng` is allowed: the build prints a note (not a warning, so `--strict` still passes), pages naming it still build, and the place simply does not appear on any map.

## Events

A page becomes an event by carrying `start:`. `date:` stays the posted date; `start:` is when the event happens. If you are porting from a generator where `date` is the event time, move that value to `start`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `start` | string | (none) | When the event starts: `YYYY-MM-DD` for an all-day event, or `YYYY-MM-DD HH:MM` |
| `end` | string | (none) | When it ends, same forms as `start`; an all-day `end` includes that day |
| `timezone` | string | (none) | IANA zone the times are in, e.g. `Asia/Taipei` |
| `status` | string | (none) | `"cancelled"`, `"postponed"`, `"moved-online"`, or `"rescheduled"`; absent means scheduled |
| `tickets` | string | (none) | URL for tickets or registration |
| `online` | string | (none) | URL to attend online |

Times are wall-clock time where the event happens: write `14:00`, not an offset or `Z`, and name the zone in `timezone:`. An all-day multi-day event ends on its last day, so `start: 2026-11-01` with `end: 2026-11-03` runs through the 3rd. moss warns when `end` is before `start`.

## Media

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cover` | string | (none) | Cover image path for card layouts and page headers |
| `cover_type` | string | auto-detected | `"image"`, `"video"`, or `"iframe"` |
| `logo` | string | (none) | Logo image displayed in the site header |

See [[media#Cover images]] for details.

## Layout

These fields control how the page's content is typeset and sized.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `typesetting` | string | `"horizontal"` | `"horizontal"` or `"vertical"` (right-to-left columns for CJK) |
| `content_width` | string | default (`67ch`) | `"wide"` (80ch, good for grids/tables) or `"full"` (site max, good for dashboards) |
| `title_block` | string | title first | `"after-picture"` draws the title block (eyebrow, title, subtitle, date and byline, key figures, facts) under the body's leading picture, however it is written; `"beside-picture"` sets it beside that picture in two columns on a wide screen, stacked on a phone |
| `reader_controls` | boolean | `true` | `false` removes the text-size control ("Aa" button) beside a dated page's date; event pages never draw it |
| `contents` | string | ruler of dashes | `"pictures"` puts a small picture of each section in the contents list in the left margin of a wide screen. At rest the list is the usual ruler of dashes; on a window at least 64rem wide with a mouse or trackpad the text column narrows to keep a lane free for it, and the floating bar stays away; hovering it opens the rows around the pointer like a wheel, each section with an image showing a picture of its first image beside its title, and clicking follows the row under the pointer. The field brings the nav island with it, so `floating_nav` need not be set; the sections panel on a phone shows the same pictures. `contents=end` on a fenced block or a heading (see Navigation & footer) marks where the main sections stop, whatever this field says |

Set these in a folder's `cascade` to apply them to a whole section:

```yaml
---
title: Dashboards
cascade:
  content_width: full
---
```

## Cross-listing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `also_in` | list | (none) | Folder paths where this article also appears in child lists |
| `series` | boolean | inferred | Render prev/next chrome on each child of this folder |

`also_in` makes an article appear in multiple sections without duplicating the file.

```yaml
---
title: Building a Trellis
also_in:
  - projects
  - featured
---
```

`series` toggles the prev/next navigation chrome at the bottom of each child article. It defaults to on for folders with `sort: weight` (or an explicit list). moss assumes authored order implies sequential reading. For `sort: date` and `sort: title` it defaults to off. Set it explicitly to override:

```yaml
# A weighted docs folder, but you don't want prev/next chrome
---
title: Reference
series: false
---
```

You can also opt a single child out by setting `series: false` on that article's own frontmatter.

## Advanced

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cascade` | map | (none) | Push values to all descendant pages |
| `url` | string | from file path | Custom URL override |
| `translationKey` | string | (none) | Link files as translations of each other |
| `uid` | string | auto-generated | Content-addressable ID |
| `layout` | string | auto | Template: `"page"` or `"article"` |
| `sidebar` | string | (none) | Wikilink to a folder to use as sidebar navigation |
| `review_of` | string | (none) | URL of the work being reviewed |
| `rating` | integer | (none) | Rating (1–5) for reviews |
| `comments` | boolean | `true` | Show comments section |
| `inferred` | list | (none) | Fields this page takes from the files it embeds when it does not set them itself. `title` takes the first embedded file's derived title; a title written in the page always wins |
| `map` | boolean | `true` | Show the place term map on a claimed term page or a real page at a place namespace root — the geographic map, not `cascade`'s key-value map |

### uid

moss stamps `uid` for you; you rarely set it by hand. If you do, quote it. An unquoted `uid: 12345` is parsed as a YAML integer and can be silently dropped. Use `uid: "12345"` instead.

### Cascade

`cascade` pushes frontmatter values to all descendants, so you don't repeat them on every file:

```yaml
---
title: Documentation
cascade:
  breadcrumb: true
  comments: false
---
```

Every page nested under this one inherits these values unless it overrides them in its own frontmatter.
