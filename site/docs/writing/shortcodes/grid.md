---
title: Grid
uid: b7e2d4a1
weight: 1
description: A multi-column layout with optional ratio and CSS classes.
translationKey: docs-author-shortcodes-grid
---

`:::grid N` creates a multi-column layout where `N` is the number of columns. Cells are separated by `+++`.

## Basic grid

:::grid 2 {.sc-demo}
```markdown
:::grid 2
Urban Heat & Environmental Justice

How do we mitigate heat disparities in cities?
+++
Ecosystem Resilience

How can ecosystems withstand droughts, fires, and floods?
:::
```
+++
::::grid 2
Urban Heat & Environmental Justice

How do we mitigate heat disparities in cities?
+++
Ecosystem Resilience

How can ecosystems withstand droughts, fires, and floods?
::::
:::

## Column count and ratio

Add an `a:b` ratio after the column count to set fractional widths. The ratio must have the same number of segments as the column count.

:::grid 2 {.sc-demo}
```markdown
:::grid 2 1:2
Yi Yin
| Principal Investigator

Yi combines satellite data and inverse modeling to quantify greenhouse gas emissions.
+++
Xinlei Liu
| Postdoctoral Researcher

Xinlei focuses on emissions from wildfires and combines field observations with statistical approaches.
:::
```
+++
::::grid 2 1:2
Yi Yin
| Principal Investigator

Yi combines satellite data and inverse modeling to quantify greenhouse gas emissions.
+++
Xinlei Liu
| Postdoctoral Researcher

Xinlei focuses on emissions from wildfires and combines field observations with statistical approaches.
::::
:::

The column count is also settable by name — `:::grid {per-line=2}` — for when it needs to sit alongside other attributes on the same line, such as `scroll` below. "Per line" is how many cards fit along one line, and a line runs the way your text runs, so on a vertical page the cards stack downward instead of across. The older `cols=` spelling still works but is deprecated.

## Scrolling row

Add `scroll` to keep the row on one line and let the reader drag it sideways, with part of the next card showing as a cue to keep going. `N` still sets the column count, but under `scroll` it means how many cards fit in view at once rather than how many sit per row.

Use it for a "related articles" or "more like this" strip where order matters more than seeing every card at once. Skip it when every card must be visible without scrolling — a small comparison set the reader should scan as a whole — and use the plain wrapping grid instead.

:::grid 2 {.sc-demo}
```markdown
:::grid 3 {scroll label="Related articles"}
Urban Heat & Environmental Justice
+++
Ecosystem Resilience
+++
Emissions Modeling
+++
Wildfire Recovery
:::
```
+++
::::grid 3 {scroll label="Related articles"}
Urban Heat & Environmental Justice
+++
Ecosystem Resilience
+++
Emissions Modeling
+++
Wildfire Recovery
::::
:::

`label="..."` names the row for assistive technology (`role="region"` plus an `aria-label`) — add it whenever the surrounding heading doesn't already say what the row is. Without a label the row is still keyboard-scrollable (focus it and press the arrow keys) but has no accessible name of its own.

Set the `--moss-grid-scroll-peek` custom property to control exactly how many pixels of the next card show at the row's edge (default `2.5rem`, so with the defaults that's 40px of the next card, not an approximation); lower it toward `0` to hide the cue, or raise it for a more insistent one.

Under vertical (right-to-left column) typesetting, `scroll` has no effect: extra cards already advance along the page's own horizontal scroll, so the row renders as the ordinary wrapping grid instead.

## Cell content

Cells are full markdown: headings, paragraphs, lists, images, and links all work. Cells also recognize:

- **Wikilinks to folders or articles**: `[[folder_name]]` or `[[Article Title]]`: rendered as cards with the target's cover, title, and date or child count.
- **Images**: `![alt](path.jpg)` or `![[photo.jpg]]`: inlined with responsive sizing. Pipe syntax (`|contain top`) works; see [[media]].
- **Markdown links**: `[text](url)`: rendered inline.
- **Bare URLs**: `https://example.com` on its own line: auto-linked.

## Single-link cells

A cell whose only content is exactly one markdown link is rendered as a single `<a>` wrapping the whole cell.

**External link** (`http://` or `https://`) → `.moss-grid-card.friend-card`. moss fetches link metadata if configured. Use for link directories and blogrolls:

:::grid 2 {.sc-demo}
```markdown
:::grid 3
[MDN](https://developer.mozilla.org)
+++
[Rust](https://rust-lang.org)

A memory-safe systems language.
+++
[GitHub](https://github.com)
:::
```
+++
::::grid 3
[MDN](https://developer.mozilla.org)
+++
[Rust](https://rust-lang.org)

A memory-safe systems language.
+++
[GitHub](https://github.com)
::::
:::

**Internal link** (site-relative path: `/foo`, `./foo`, or a wikilink target) → `.moss-grid-card.link-card`. No metadata fetch. The link's brackets can contain an image, headings, and paragraphs; moss emits one `<a>` wrapping all of them.

The link text may be compound. Put the image, heading, and paragraph inside the brackets and moss renders one card-link wrapping all of it:

```markdown
[![[cover.jpg]] ## Title

Short description](/target)
```

Do not hand-write the `<a>` wrapper. Author the single markdown link and let moss emit the anchor.

An image inside the link takes precedence over [folder auto-conversion](#folder-auto-conversion) below: `[![Cover](cover.jpg)](/target)` keeps your image inside the `<a>` even when `/target` names a page in the build — only a bare-text link (no image) is eligible to become an automatic page card.

A cell with anything else (two links, text plus a link) renders as regular cell content, unwrapped — so you can mix clickable cards and rich cells in the same grid.

Theme CSS targets each flavor independently:

```css
.moss-grid-card.friend-card { … }  /* external link cell */
.moss-grid-card.link-card   { … }  /* internal link cell */
```

## Folder auto-conversion

A cell whose only content is an internal link to a known folder is automatically converted into a `moss-collection-card` (the same card used by `children_style: card`). moss fetches the folder's cover image, title, and child count.

:::grid 2 {.sc-demo}
```markdown
:::grid 3
[[Get Started]]
+++
[Writing](/docs/writing/)
+++
[Reference](/docs/reference/)
:::
```
+++
::::grid 3
[[Get Started]]
+++
[Writing](/docs/writing/)
+++
[Reference](/docs/reference/)
::::
:::

**Opt out with `.no-cards`** to bypass auto-conversion. Use `.no-cards` for navigation grids, hero-split layouts, or compound-link grids where you want `.link-card` or `.friend-card` rendering instead:

:::grid 2 {.sc-demo}
```markdown
:::grid 3 {.no-cards}
[[Get Started]]
+++
[Writing](/docs/writing/)
+++
[Reference](/docs/reference/)
:::
```
+++
::::grid 3 {.no-cards}
[[Get Started]]
+++
[Writing](/docs/writing/)
+++
[Reference](/docs/reference/)
::::
:::

CSS targets:

```css
.moss-collection-grid { … }   /* auto-converted folder grid */
.moss-collection-card { … }   /* individual collection card */
```

## Custom CSS classes

Attach a named class with `{.classname}` when you need responsive layout control or the same shape on multiple pages.

**Do this.** Use `:::grid N {.your-class}` with no ratio, then define the ratio in CSS:

```markdown
:::grid 2 {.two-col-split}
Main content area: headings, paragraphs, images, anything.
+++
Sidebar with call-outs or metadata.
:::
```

```css
/* .moss/theme/style.css */
.two-col-split {
  grid-template-columns: 2fr 1fr;
  gap: 2rem;
}
@media (max-width: 768px) {
  .two-col-split { grid-template-columns: 1fr; }
}
```

The grid container renders as `<div class="moss-grid two-col-split">`. Your class sits alongside the built-in `moss-grid` and can override `grid-template-columns`.

**Avoid this.** Passing a ratio (`:::grid 2 2:1 {.two-col-split}`) emits an inline `style="grid-template-columns:2fr 1fr"` on the container. Inline styles beat stylesheet rules, so your `@media` query has no effect without `!important` on every property — a maintenance trap once the pattern spreads.

**Rule of thumb.** Use the ratio form (`:::grid 2 2:1`) for one-off layouts where responsive overrides are not needed. Use a named class when you need `@media` behaviour or the same shape on multiple pages.

See [[components|component classes]] for the full list of component class names you can target.
