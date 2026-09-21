---
title: Shortcodes
uid: a3f9c12e
weight: 6
description: Layout blocks and components that extend markdown.
---

Shortcodes are blocks in your markdown that create layouts and components beyond standard markdown: grids, galleries, hero images, and more. They use `:::` block syntax. ^def-shortcode

```markdown
:::grid 2
Column one content.
+++
Column two content.
:::
```

## Arity rule for nesting

When one shortcode lives inside another, the inner fence uses more colons than the outer. A `:::grid` (3 colons) contains `::::buttons` (4 colons); `::::buttons` could contain `:::::callout` (5 colons), and so on. Each fence's closing delimiter must match its opener exactly.

```markdown
:::grid 2 2:3 {.hero-split}
# Pitch deck

::::buttons {.inverted}
[Download PDF](https://example.com/deck.pdf)
[Request intro](mailto:hi@example.com)
::::
---
Contact info
:::
```

Pages that never nested fences continue to work unchanged.

Nesting two fences at the same colon count emits a build warning and renders as literal text.

## Cell dividers

Inside `:::grid` and `:::buttons`, cells are separated by a line containing only `+++`. A block with no `+++` is a single cell.

`---` is the standard markdown thematic break, a horizontal rule. It is never a moss cell control character. Use `+++` to split cells.

## Configuration

Pass options in the attribute block, not the body. For example: `:::subscribe {placeholder="..." button="..."}` or `:::grid {per-line=2}` (the positional form `:::grid 2` also works, and is what the docs below lead with; the older `cols=` spelling still works but is deprecated). Descriptive prose belongs above or below the block, not inside the option line.

## Shortcodes

| Shortcode | Description |
|-----------|-------------|
| [[grid]] | Multi-column layout with optional ratio and CSS classes |
| [[hero]] | Full-width hero section with background image and overlay text |
| [[gallery]] | Image grid for photo galleries |
| [[buttons]] | Styled row of buttons from markdown links |
| [[Fenced Divs]] | Named-class fenced divs for styled regions |
