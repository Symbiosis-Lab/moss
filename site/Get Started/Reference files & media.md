---
uid: 252bd472
description: Link pages, embed content, and reference media with double brackets — no paths required.
url: links
weight: 40
translationKey: docs-author-links
---
moss references anything with wikilinks: write a filename, title, or media name inside `[[double brackets]]`. moss searches your whole folder for the filename, so you never have to write out a path.

## Links

Links match by filename, title, and URL. Common variants:

| Syntax | What it does |
|------|------|
| `[[filename\|link text]]` | Custom display text (what follows the pipe is what readers see) |
| `[[filename#Section heading]]` | Link to a heading |
| `[[filename#^intro]]` | Link to a paragraph (block reference) |

Add `^intro` at the end of a paragraph to give it a stable block ID, then reference it elsewhere with `[[page#^intro]]`.

## Embeds

Add `!` before the brackets to **inline** the target's content into the current page, instead of just linking to it:

| Syntax | What it embeds |
|------|---------|
| `![[filename]]` | The whole file |
| `![[filename#Section name]]` | One section of a file |
| `![[filename#^intro]]` | One paragraph of a file |
| `![[folder/]]` | A folder's subpages, rendered as cards |

A path ending in `/` means a folder. Folder embeds take parameters — for example, `![[albums/|limit:5,more]]` shows only the latest 5 items and appends a "More →" link.

## Media

Drop images, videos, and other files into your folder, then reference them with a wikilink or standard markdown. moss resolves the filenames and optimizes the media files automatically.

```markdown
![[sunset.jpg]]
![Sunset over the coast](photos/sunset.jpg)
![[sunset.jpg|Sunset over the coast]]
```

In the last two forms, "Sunset over the coast" becomes the **caption** below the image, and also serves screen readers and search engines.

An embed needs an empty line above it to become a figure. An embed that hugs the previous paragraph is treated as an inline image, and its caption and sizing parameters are ignored.

Supported media types:

- **Images** — `.jpg`, `.png`, `.gif`, `.svg`, `.webp`, `.avif`
- **Video** — `.mov` is transcoded to `.mp4` automatically; reference it by its original filename
- **Audio** — `.mp3`, `.wav`, `.ogg`, `.flac`, `.aac`, `.m4a`, `.opus` become browser audio players
- **Documents** — files such as `.pdf` remain available to open or download
- **Notebooks** — `.ipynb` runs right in the browser via JupyterLite, no server needed
- **HTML** — `.html` files are served as-is, for interactive content

Put media in an excluded asset folder (`assets/`, `images/`, `static/`, `public/`) and it's served publicly but never becomes a page.

### Control the display

Append parameters after a pipe to control an image's size, fit, and position:

```markdown
![[cover.jpg|400]]           # 400px wide
![[cover.jpg|100%]]          # 100% wide
![[cover.jpg|200x150]]       # width × height
![[headshot.jpg|cover top]]  # fit + position
```

**Size**: write a width or width × height. Units: `px` (default), `%`, `vh` — e.g. `400`, `100%`, `200x150`.

**Fit** (maps to CSS `object-fit`): `cover` (default — fill and crop), `contain` (show the whole image), `fill`, `none`, `scale-down`. **Position**: `center`, `top`, `left`, `bottom-right`, and so on.

Each pipe carries one meaning — size, fit and position, or a caption — never mixed in the same pipe.

## Go deeper

For the finer points — resolution priority, circular-embed handling, cover images, `children_source` — see [Links & Embeds](/docs/writing/wikilinks-and-embeds/) and [Media](/docs/writing/media/).
