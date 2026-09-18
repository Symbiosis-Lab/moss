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

`description:` feeds SEO meta tags, Open Graph previews, and sitemap summaries. It is **not** rendered as a visible deck. For a visible subtitle, use a `> blockquote` immediately after the `# H1` — the default theme styles `h1 + blockquote` as a deck.

## Identity

These fields describe what the page is.

<!-- auto:start:frontmatter-identity -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `title` | string | filename | Page title |
| `description` | string | (none) | SEO meta description and list previews |
| `date` | string | (none) | Publication date (`YYYY-MM-DD`) |
| `tags` | list | (none) | Content tags |
| `lang` | string | auto-detected | Language override (`"en"`, `"zh-hans"`, `"zh-hant"`) |
<!-- auto:end:frontmatter-identity -->

`title:` is the page's title. On article pages moss always renders it as `<h1 class="moss-article-title">` above the body, using the filename (title-cased) when `title:` is absent. If the body itself opens with `# Same Title`, that body H1 is deduplicated so the reader sees one title, not two.

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
| `footer_align` | string | `"left"` | Align this page's footer link (`"left"` or `"right"`) |
<!-- auto:end:frontmatter-navigation -->

## Visibility

<!-- auto:start:frontmatter-visibility -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `draft` | boolean | `false` | Skip generation entirely |
| `unlisted` | boolean | `false` | Generate but hide from lists and sitemap |
<!-- auto:end:frontmatter-visibility -->

## Children

These fields control how a [[structure#^folder-page|folder page]] displays its child pages.

<!-- auto:start:frontmatter-children -->
| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `children` | boolean | `true` | Show child page list on section pages |
| `children_style` | string | `"list"` | `"list"`, `"summary"`, `"card"`, or `"minimal"` |
| `children_group` | string | `"none"` | Group by `"year"` or `"none"` |
| `children_depth` | string | `"direct"` | `"direct"` (immediate children) or `"all"` (all descendants) |
| `children_source` | string | (none) | Wikilink to folder whose children to show instead |
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
2. At least 80% of children have a `date` field → `sort: date`
3. Otherwise → `sort: title`

Most folders need no `sort` declaration: a blog folder is automatically `date`, a docs folder with weights is automatically `weight`, and a folder of dateless projects is automatically `title`.

**Why sort drives appearance:** date listings put the date in each card's meta slot; weight and title listings omit the meta slot entirely (no empty space). Folder cards in non-date listings show a small "N articles" subtitle only when they have no description.

The legacy `order: [...]` field is a back-compat alias for `sort: [...]`.

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
