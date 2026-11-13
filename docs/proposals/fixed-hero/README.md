# A hero that stays while the page rises over it

**Status:** Proposed. Everything here lives in this repository: one hero argument and stylesheet rules, no script.

## What it is

An opening hero as tall as the window that stays where it is while the rest of the page scrolls up over it, with the picture dimming as it is covered. Many landing pages open this way. A theme can build it today only by restating the hero's box rules and offsetting `main` by hand, which breaks whenever the hero's own rules change.

```markdown
:::hero {sticky}
![The harbour at dusk](harbour.jpg)
# Harbour
:::
```

`sticky` is a bare word, the one `header_scroll = "sticky"` already uses for a part of the page that stays. It is read on a page's opening hero only. A hero further down the page, or the closing picture in `footer.md`, ignores it and the build prints a note.

## How it is drawn

The hero is `position: sticky; inset-block-start: 0` and `block-size: 100svh`, and `main` and the footer get the page background and a stacking level above it. Sticky is chosen over `position: fixed` because the hero stays in the flow: nothing has to know its height to offset `main`, and the cases below fall out of ordinary layout. The design depends on one fact of the markup: an opening hero is a child of `<body>` and a sibling of `<main>`, so the box it sticks within is the whole page and it holds until the page ends.

| Case | What happens |
|---|---|
| Phone URL bar | The height is `100svh`, the window with the bar showing, so the first screen is exactly the hero and nothing jumps when the bar slides away. By then the page has scrolled, and `main` already covers the strip the taller window opens. |
| Words taller than the window | The hero is exactly one window tall, so words that do not fit scroll inside their own box (`overflow-y: auto` on the words box), and every line can be reached without script. A sticky offset cannot do this: it would need the hero's own height, and a percentage there resolves against the window. The render gate must prove it at 390 with `mobile=overlay` (the last line scrolls into view by touch and by keyboard) before this ships; the guide tells an author to keep a sticky hero's words to what a phone shows. |
| Phone without `mobile=overlay` | The words are stacked under the picture, not over it, so there is nothing to rise over: the hero scrolls with the page as it does today. With `mobile=overlay` it stays. |
| A page shorter than the window | The page scrolls only as far as `main` is tall, and `main` covers that much of the picture. No gap and no extra scroll length. |
| Print | `position: static` and the hero's ordinary print height. |
| Vertical typesetting | Ignored: the page scrolls sideways and the hero is a column, not a screen. |
| `header=over` | The header is laid over the picture as today and scrolls away with the page. A site with `header_scroll = "sticky"` or `"hide"` keeps its header by that setting's own rules; this argument adds nothing to them. |
| Reduced motion | Under `prefers-reduced-motion: reduce` the hero scrolls with the page. A background that holds still while text moves over it is the same effect as parallax for a reader who asked for less motion. |

## The dim

The picture darkens as the page covers it: an overlay on the hero whose opacity runs from 0 to `--moss-hero-scroll-dim` (default `0.5`; `0` turns it off) over the first `100svh` of scroll. It is a scroll-driven animation (`animation-timeline: scroll(root)`) inside `@supports`, so an engine without scroll-driven animations shows the picture undimmed and loads nothing in its place. The words in the hero are above the overlay and keep their ink.

## What a theme sets

- `--moss-hero-scroll-dim`: how dark the covered picture gets.
- The page background that rises over the picture is `--moss-color-bg`, which `main` and the footer now paint on such a page. A theme that gives a band its own background keeps it.

## Tests

- The parser reads `sticky` and the hero carries `data-sticky`; a second hero and the closing picture ignore it with a note.
- A render gate in Chromium and WebKit at 1280 and 390: after scrolling, the hero's top is still at the window's top and the point under the fold is `main`, not the picture; words taller than the window can be scrolled to their last line; a page shorter than the window has no gap; a phone without `mobile=overlay`, print media and reduced motion all show the hero scrolling with the page; where scroll-driven animations exist, the overlay's opacity grows with the scroll.
