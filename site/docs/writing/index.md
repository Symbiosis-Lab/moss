---
title: Writing
uid: eae1ec89
weight: 20
description: Shape your pages — frontmatter, structure, links, media, and shortcodes.
translationKey: docs-author
---

Once your folder is a site, writing is the work: shaping each page and how pages connect. This section covers everything you do in your Markdown.

Control each page with [[frontmatter]] — title, date, visibility, and more. See how [[structure|files, folders, and URLs]] fit together. Connect pages with [[Links & Embeds|wikilinks and embeds]], shape the site's [[Navigation & footer|navigation and footer]], call out content with [[callouts]], and add layouts with [Shortcodes](/docs/writing/shortcodes/) — grids, galleries, hero images. Your [[media]] is tidied for the web for you, and a site can be [[Multilingual sites]]. Write in the [[Editor Shortcuts|built-in editor]] or any Markdown editor you like.

## Choosing the right primitive

When a page needs a specific visual treatment, try in this order:

1. **Plain markdown + CSS selector.** Use `## Mission`, `> quote`, `*byline*`, `- item`. Let the theme select the resulting HTML. Zero new vocabulary.
2. **A moss-native shortcode.** `:::grid N`, `::::buttons`, `:::hero`, `:::gallery`, `> [!pending]`. These emit well-known class names that the default theme already styles. See [Shortcodes](/docs/writing/shortcodes/).
3. **Named-class fenced div.** `::: {.class}` attaches a CSS class to a region without introducing a new shortcode. Pair with a CSS rule in `.moss/theme/style.css`. See [Shortcodes](/docs/writing/shortcodes/#named-class-fenced-divs).
4. **Custom shortcode.** Rare; only when you need new behavior the parser doesn't already support.

### Special cases

| Goal | Primitive |
|------|-----------|
| Deck / subtitle / lead paragraph after a title | `> blockquote` immediately after `# H1`, styled via `h1 + blockquote` |
| Identity line / tagline under the title | `::: {.tagline}` fenced div after the H1 |
| Kicker / eyebrow before the title | `::: {.eyebrow}` fenced div before the H1 |
| Pull quote inside a section | `> quote` styled differently from callouts via `h2 ~ blockquote` |
| Clickable compound card in a grid | Wrap cell content in `[…](/url)`; moss emits one `<a>` with block children |
| Repeated block across pages | Extract as a partial (`unlisted: true, nav: false, children: false`), transclude via `![[partial-name]]` |
| Placeholder / in-progress section | `> [!pending] Title` callout |

### Markdown attributes that do NOT work

These Pandoc-style attribute syntaxes are **not** supported by moss:

- `# Title {.class}`: heading attributes
- `paragraph\n{.class}`: paragraph attributes
- `---\n{.section-divider}`: hr attributes

Use `::: {.class}` fenced divs or CSS element selectors instead.
