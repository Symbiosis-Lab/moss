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

A row with `N` cards or fewer already fits on one line, so on a wide enough screen it renders as an ordinary grid — no dots, no drag, every card at full width — and only turns into a slideshow once the screen narrows past the same width where a plain grid would otherwise wrap.

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

`label="..."` names the row for assistive technology (`role="region"` plus an `aria-label`) — without it, the row's name defaults to the nearest heading above it (so add `label` only when that heading doesn't already say what the row is, or when there's no heading above it at all); with neither, the row is still keyboard-scrollable (focus it and press the arrow keys) but has no accessible name of its own.

Set the `--moss-grid-scroll-peek` custom property to control exactly how many pixels of the next card show at the row's edge (default `2.5rem`, so with the defaults that's 40px of the next card, not an approximation); lower it toward `0` to hide the cue, or raise it for a more insistent one.

Under vertical (right-to-left column) typesetting, `scroll` still keeps the row on one line, but the line itself runs down the page instead of across it, so the row scrolls vertically — perpendicular to the page's own sideways scroll between columns — with the same peek cueing more cards below.

## Collage

Add `collage` to show a collection's highlights as a collage: the first cell is the lead picture and the others are smaller tiles, pictures or quotes. No column count is needed.

```markdown
:::grid {collage}
![Lead picture|focus:40%,30%](a.jpg)
*__The harbour__ 1890 · City Museum · Public domain*
+++
[![Second](b.jpg)](/works/second/)
*__Second__ 1902 · Public domain*
+++
> Communication is health; communication is truth.

*[Source, 1924](https://example.org/essays/)*
+++
![Title page|contain](c.jpg)
*__Title page__ 1855*
:::
```

On a wide screen the lead takes one column at full height, half the collage, or less when its picture is upright like a poster. The other tiles pack the six columns beside it, and moss chooses the rows and each tile's width from what the cells hold. A row is at least `--moss-collage-row` tall and grows only to fit a quote, never for a picture. On a phone the lead runs full width, a quote takes a full row at its own height, and pictures sit in rows of three; a last row with fewer pictures is widened. Pictures crop to fill their tile, as the words after their pipe set: `focus:X%,Y%` keeps a point in view and `contain` shows the whole picture.

The italic line under a picture is its credit. A bold run at its start is the work's title, set on its own line (two at most) above a smaller line of facts; a tile too small for both keeps the title, and a linked picture's title has ↗ beside it. The credit is roman and set from the start, takes at most 40% of the tile and stops at a whole line (the viewer shows a long credit whole), and sits on a band of the page colour at the tile's foot and slides up on hover and keyboard focus. On a touch screen a tap opens the picture viewer with the full credit, and for a linked picture a link to its page.

A blockquote followed by one italic line is a quote, with the line as its source; only the source can be a link. It is set as type on the page's paper inside a hairline and is never cut off. In a collage its size follows its tile's width, so a short quote's longest word fits a narrow tile; a word too long for the smallest size breaks rather than run into the hairline. Its face is `--moss-font-quote` (the body face unless set), and a quote over about 140 characters is set smaller. This works in a plain grid too.

A quote written in Chinese or Japanese is set in `--moss-font-cjk` (by default the heading face, which leads with Han serifs and follows the page's Traditional or Simplified setting) at 17px to 21px, 16px on a phone, with wide letter-spacing; a long quote keeps its smaller size. moss reads the quote's own letters, so no markup is needed, and a quote with enough kana is also marked `lang="ja"`. A theme's `--moss-font-quote` takes precedence.

A collage takes 3 to 9 cells. With any other count the build warns with the line and shows a plain grid. Set `--moss-collage-gap` for the space between tiles (default `6px`) and `--moss-collage-lead` for the lead's column. Use a collage for a handful of highlights; use [[gallery]] for a set of photos to go through and a plain grid when each cell is a card.

## Equal rows

Add `rows=equal` and the parts of the cards in a row line up across it: if the first card's picture is taller than its neighbour's, both titles still start at the same height, and so do the texts under them.

```markdown
:::grid 3 {rows=equal}
![Lighthouse](a.jpg)

### The lighthouse

A short line.
+++
![Ferry](b.jpg)

### The ferry that crossed at night

A longer text that takes three lines where the others take one.
+++
### A card with no picture

Some text.
:::
```

It works for cards of any shape: moss counts the children each card renders (a picture, a heading, a paragraph, a list, a fenced `::: {.label}` block each count one) and gives each card that many shared tracks, so a card with fewer parts than its neighbours leaves its last tracks empty. A collection card made from a `[[note]]` link has two parts, its cover and its text. A wrapper around a whole card, raw HTML or a fenced `::: {.work}`, is one child, so the parts inside it are not lined up; leave the wrapper off. The parts of a card sit `--moss-grid-part-gap` apart (10px by default), and `--moss-grid-card-gap` is the space between cards (the grid's usual row gap by default). The grid's own `gap` does not set it: a row gap there is the gap between parts, because a card's tracks are the grid's, so moss ignores a row gap set on such a grid. A value below the part gap is raised to it. Raw HTML with an unclosed tag cannot be counted: every build prints a warning for it, and `moss build --strict` makes it fatal. The browser must support subgrid, as every current one does; without it the cards keep their own heights. `rows=equal` has no effect on a collage or a scrolling row.

A grid can change its column count between 48rem and 64rem with `mid=N` (`:::grid 3 {mid=2 rows=equal}` is three columns on a wide screen, two on a tablet). It replaces an author ratio in that range, and a collage or a scrolling row ignores it. Use it instead of a theme rule that changes the grid's columns: `rows=equal` spaces the rows of cards by counting the columns moss emitted, so a theme that changes the count itself loses the card gap. The card gap is placed by that count, one card per column in the order written: a hidden card (the `hidden` attribute) is skipped, but a card that spans two columns, or a theme rule that changes the columns, puts it on the wrong cards.

## Cell content

Cells are full markdown: headings, paragraphs, lists, images, and links all work. Cells also recognize:

- **Wikilinks to folders or articles**: `[[folder_name]]` or `[[Article Title]]`: rendered as cards with the target's cover, title, and date or child count.
- **Images**: `![alt](path.jpg)` or `![[photo.jpg]]`: inlined with responsive sizing. Pipe syntax (`|contain top`) works; see [[media]].
- **Markdown links**: `[text](url)`: rendered inline.
- **Bare URLs**: `https://example.com` on its own line: auto-linked.

## Single-link cells

A cell whose only content is exactly one markdown link is rendered as a single `<a>` wrapping the whole cell — one card kind, whether the link stays on the site or leaves it.

**Bare internal link, no authored content** (`[Get Started](/docs/)`) → an automatic page card, `.moss-card`: moss looks up the target in the build and fills in its cover, title, date or child count. See [folder auto-conversion](#folder-auto-conversion) below.

**External link** (`http://` or `https://`) → also `.moss-card`, with `data-external` and `target="_blank" rel="noopener"`. The kicker shows the link's domain, with a favicon inline when moss has one cached for it. The title is your own link text when you wrote real words; otherwise moss uses a fetched page title if one is cached, falling back to the URL's domain and path:

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

**Internal link that names nothing in the build** (a broken or not-yet-written path) → `.moss-grid-card` with `data-kind="link"`, no metadata, no cover — the link's own brackets rendered as one clickable wrapper.

Any of these link shapes can be compound: put an image, a heading, and a paragraph inside the brackets, and moss renders one anchor wrapping all of it:

```markdown
[![[cover.jpg]] ## Title

Short description](/target)
```

Do not hand-write the `<a>` wrapper. Author the single markdown link and let moss emit the anchor.

An authored image inside the link always wins, on both internal and external cells: `[![Cover](cover.jpg)](/target)` keeps your image as the card's cover even when `/target` names a page in the build, or when `https://…` has metadata cached for it — a fetch never displaces what you wrote. Only a bare-text link (no image) is eligible to be filled in from elsewhere.

A cell with anything else (two links, text plus a link) renders as regular cell content, unwrapped — so you can mix clickable cards and rich cells in the same grid.

Theme CSS targets each flavor independently:

```css
.moss-card[data-external]          { … }  /* external link cell */
.moss-grid-card[data-kind="link"]  { … }  /* internal link, unresolved */
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
@media (width < 48rem) {
  .two-col-split { grid-template-columns: 1fr; }
}
```

The grid container renders as `<div class="moss-grid two-col-split">`. Your class sits alongside the built-in `moss-grid` and can set `grid-template-columns`.

**A written ratio is a default, not a lock.** Passing a ratio (`:::grid 2 2:1 {.two-col-split}`) writes the custom property `--moss-grid-ratio` on the container (`style="--moss-grid-ratio:minmax(0, 2fr) minmax(0, 1fr)"`), not an inline `grid-template-columns`. A class rule that sets `grid-template-columns` overrides it, including inside `@media`, with no `!important`. A class rule that sets `--moss-grid-ratio` does not: the container's own inline value wins for that property, so change `grid-template-columns` instead.

**Rule of thumb.** Use the ratio form (`:::grid 2 2:1`) for one-off layouts. Use a named class with no ratio when the same shape repeats on several pages, so the columns live in one place.

See [[components|component classes]] for the full list of component class names you can target.
