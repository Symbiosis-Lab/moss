---
title: Links & Embeds
url: wikilinks-and-embeds
uid: 5555db30
weight: 4
description: Wikilinks, embeds, cross-references, and bilingual content.
---

## Wikilinks

Link to any page in your site using double brackets:

```markdown
Read the [[structure]] page for details.
```

moss resolves the link by matching the text against filenames, titles, and slugs. No full path needed. `[[from-matters]]` finds `docs/start/from-matters.md` automatically.

**Display text:**

```markdown
See the [[structure|content structure docs]] for more.
```

**Link to a heading:**

```markdown
Check [[structure#Stems]] for the definition.
```

**Link to a block reference:**

```markdown
As defined in [[structure#^intro]].
```

### Resolution rules

When multiple files could match, moss picks the best one:

1. Exact filename match
2. Filename without extension
3. Folder note (self-named file)
4. Same language tree as the source page (e.g. `![[footer]]` from `zh-hans/about.md` prefers `zh-hans/footer.md` over root `footer.md`)
5. Closest directory first (files nearer to the linking page win remaining ties)

Explicit paths like `[[zh-hans/footer]]` always win over the language-tree preference.

## Image embeds

Embed images with `!` before the brackets:

```markdown
![[photo.jpg]]
![[poster-farewell.webp]]
```

moss resolves the filename by searching the entire content tree — no relative path needed. Put images in an excluded asset folder (`assets/`, `images/`, `static/`, `public/`) so they are served but not treated as content pages, then reference them by bare filename from any markdown file.

```
work/
├── index.md         ← uses ![[poster-farewell.webp]]
└── assets/
    └── poster-farewell.webp   ← not a page; served as a static file
```

The language-tree preference (rule 4 above) applies to image resolution too: `![[photo.jpg]]` inside `zh-hans/about.md` prefers `zh-hans/photo.jpg` if it exists.

**Pipe syntax** controls display — a fit/position keyword (`![[photo.jpg|contain top]]`), a size (`![[photo.jpg|400]]`), or a caption (`![[photo.jpg|Morning light]]`). See [[media#Display control]] and [[media#Captions]] for the full rules.

## Embeds

Pull content from another page inline using `!` before the brackets:

```markdown
![[structure#^intro]]
```

This inserts the referenced paragraph directly into the current page. Embeds work with:

- **Full page**: `![[media]]`: embeds the entire page content
- **Section**: `![[structure#URLs]]`: embeds everything under that heading
- **Block**: `![[structure#^intro]]`: embeds a single paragraph marked with a block ID
- **Folder listing**: `![[journal/]]`: embeds another folder's children as cards (see below)

moss detects circular embeds and stops before an infinite loop.

## HTML and interactive embeds

Embed an interactive or animated `.html` file (an animation, a small demo, a widget) with the same `![[...]]` syntax:

```markdown
![[folder-to-site.html]]
```

moss serves the file as-is inside an iframe, so its own CSS and animation run normally. Keep the file in an excluded asset folder (`assets/`, `static/`) and reference it by bare filename. moss resolves the path and keeps the URL correct wherever the page is deployed. A query string passes straight through: `![[chart.html?series=a,b]]`.

This is the preferred way to embed interactive content. Do not hand-write a raw `<iframe>` or an absolute `/assets/...` URL. The wikilink computes the right relative path. The same `![[folder-to-site.html]]` resolves to a per-language copy via the language-tree rule: put the default file at `assets/animations/folder-to-site.html` and a translation at `zh-hant/assets/animations/folder-to-site.html`, and each page picks its own.

The embed fills the page width in a 16:9 frame, so design the file to read well at that shape.

## Folder listings

A wikilink whose path ends with `/` embeds a folder's children inline:

```markdown
# A Reader's Notebooks

A personal record of my reading and writing.

## Recent journal

![[journal/|limit:5,more]]

## Books
```

The trailing slash signals a folder embed, not a page. The cards inherit the target folder's `sort` (see [[frontmatter#Sort]]), so a `journal/` folder with dated entries renders date-sorted cards, and a `projects/` folder of dateless entries renders alphabetically.

**Pipe parameters** (comma-separated):

| Param | Meaning |
|---|---|
| `limit:N` | cap at N items |
| `more` | append a "More →" link to the source folder when truncated |
| `sort:date` / `sort:weight` / `sort:title` | override the source folder's sort axis for this embed only |

Examples:

```markdown
![[journal/]]                       # full listing
![[journal/|limit:5]]               # 5 most recent (or whatever the journal's sort picks)
![[journal/|limit:5,more]]          # 5 newest + "More →" link
![[news/|sort:date,limit:3]]        # override sort axis per-embed
```

Paths follow the same resolution as other wikilinks: relative paths anchor at the current page's folder, absolute paths (`/journal/`) anchor at the site root.

### When to use folder listings vs `children_source`

Two ways to render another folder's children on a page:

| Mechanism | When to use |
|---|---|
| Inline `![[folder/]]` wikilink | Multiple embeds per page, or you want the listing between other markdown sections. The author chooses where each listing appears. |
| `children_source: "[[folder]]"` in frontmatter | Page-scope: the page IS a listing of another folder. One transclusion per page. Renders at the slot determined by your theme. |

For a page with a single primary listing, `children_source` is the right choice. Use `![[folder/]]` when you want the listing inline among other content.

## Block references

Mark any paragraph with a `^block-id` at the end of the line:

```markdown
moss turns a folder of markdown files into a website. ^intro
```

Other pages can then link to or embed this paragraph using `[[page#^intro]]` or `![[page#^intro]]`. Block IDs are stable — they don't change when you reword the paragraph or rename the heading above it.

## Markdown links

Standard markdown links work as you'd expect:

```markdown
[Visit Example](https://example.com)
[About page](about.md)
[Section link](/docs/writing/structure/)
```

Both relative and absolute paths work. External links open in a new tab.

## Bilingual content

Create translations by appending a language suffix to the filename:

| File | Language |
|------|----------|
| `about.md` | site default |
| `about.zh-hans.md` | Simplified Chinese |
| `about.zh-hant.md` | Traditional Chinese |
| `about.en.md` | English (explicit) |

When translations exist, a language toggle appears on the page automatically.

### Language detection

moss resolves a page's language in this order:

1. `lang` frontmatter field
2. Filename suffix (`.zh-hans.md`)
3. Content auto-detection
4. Site default language

### Linking translations with different names

When translations have different filenames, use `translationKey` to connect them:

```yaml
# posts/hello.md
---
translationKey: hello-post
---

# posts/ni-hao.md
---
translationKey: hello-post
lang: zh-hans
---
```

Both files show a language toggle linking to each other.
