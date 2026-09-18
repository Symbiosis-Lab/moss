---
title: Media
uid: bac82b8f
weight: 7
description: Images, video, notebooks, and other files in your site.
translationKey: docs-media
---

Put images, videos, and other files anywhere in your project folder. Reference them from markdown; moss handles path resolution, optimization, and progressive loading. ^intro

## Images

Reference an image with standard markdown syntax:

```markdown
![A sunset](photos/sunset.jpg)
```

Or with a wikilink:

```markdown
![[sunset.jpg]]
```

moss resolves the path automatically. It also extracts image dimensions and generates a tiny blurred placeholder so the page doesn't jump as images load.

Supported formats: `.jpg`, `.jpeg`, `.png`, `.gif`, `.svg`, `.webp`, `.avif`.

## Video

Drop video files in your folder and reference them the same way:

```markdown
![[demo.mov]]
```

moss transcodes `.mov` files to `.mp4` (H.264 + AAC) in the background. The preview opens immediately and video loads progressively as transcoding completes. Reference the original `.mov` filename; moss serves the converted `.mp4` automatically.

## Display control

Control how media displays with pipe syntax — append a fit/position keyword, a size, or a [caption](#captions) after a `|`:

```markdown
![[photo.jpg|contain top-left]]
```

**Fit values** (maps to CSS `object-fit`):

| Value | Effect |
|-------|--------|
| `cover` | Fill the area, cropping if needed (default) |
| `contain` | Show the whole image, letterboxing if needed |
| `fill` | Stretch to fill exactly |
| `none` | Natural size, no scaling |
| `scale-down` | Like `contain`, but never enlarges |

**Position values** (maps to CSS `object-position`):

`center`, `left`, `right`, `top`, `bottom`, `top-left`, `top-right`, `bottom-left`, `bottom-right`

Combine them: `![[panorama.jpg|contain center]]`, `![[headshot.jpg|cover top]]`.

**Size** — a width, or width × height, in place of fit/position:

| Pipe | Result |
|------|--------|
| `![[photo.jpg\|400]]` | 400px wide |
| `![[photo.jpg\|100%]]` | full container width |
| `![[photo.jpg\|200x150]]` | 200 × 150px |

A bare number is pixels; `px`, `%`, and `vh` are also accepted. One value sets width; `WxH` sets both.

A pipe segment carries **one** meaning — moss reads it as size, then fit/position, then caption text — so size and fit can't share a single pipe.

This syntax works in wikilinks, markdown images, and bare filenames inside shortcodes.

## Captions

An image in its own paragraph — an empty line above it, nothing else on the line — with non-empty alt text renders as a semantic `<figure>` with a visible `<figcaption>`:

```markdown
![Morning light, Yangshuo](photos/light.jpg)
```

```html
<figure>
  &lt;img src="…/light.jpg" alt="Morning light, Yangshuo">
  <figcaption>Morning light, Yangshuo</figcaption>
</figure>
```

This is Pandoc's implicit-figure convention, on by default; the default theme sets captions italic and centered.

**Three ways to write one:**

| Syntax | Caption | Alt text |
|--------|---------|----------|
| `![Caption](photo.jpg)` | the alt text | same as the caption |
| `![[photo.jpg\|Caption]]` | the alias | same as the caption |
| `![Alt description](photo.jpg)` then `*Caption*` on the next line | the italic line | a separate description |

The third form is best for accessibility: the alt describes the image for screen readers while the italic line is the visible caption. In the first two, the alt does double duty as caption and description.

For the wikilink form, the alias becomes a caption only when it isn't a [size or fit/position keyword](#display-control) — those are read first.

**No figure is emitted when** the alt is empty (`![[photo.jpg]]` or `![](photo.jpg)`) — moss keeps a plain `&lt;img>` rather than caption an undescribed image — or when the image shares its line with other text (a trailing `*italic*` caption is the one exception, above).

For richer caption markup, hand-write a `<figure>`; raw HTML passes through untouched. Turn the behavior off site-wide with `implicit_figure = false` under `[site]` in `.moss/config.toml`.

## Notebooks

Jupyter notebooks (`.ipynb` files) run directly in the browser via JupyterLite — no server, no Python installation required. The entire runtime (~20 MB) runs as WebAssembly in the visitor's browser. Place a `.ipynb` file in your project and it appears alongside your other content.

## HTML files

Plain `.html` files in your project are served as-is. Use them for interactive content, embeds, or pages that need full control over their markup.

## Cover images

Any page can declare a cover image in its frontmatter:

```yaml
---
title: My Project
cover: screenshots/hero.png
---
```

Covers appear in card layouts when a section uses `children_style: card`. moss detects the media type from the file extension. Override with `cover_type` for non-image covers:

```yaml
---
cover: demo.mp4
cover_type: video
---
```

Valid cover types: `image` (default), `video`, `iframe`.
