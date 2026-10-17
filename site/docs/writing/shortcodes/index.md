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

## Nesting

A fence with a name or `{attributes}` opens a block, and a bare line of three or more colons closes the innermost open block. The number of colons does not matter, so use more of them only if you like the look: `:::grid` can contain `::::buttons`, or another `:::` block, and every closer can be plain `:::`.

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

A block that is never closed, and a closing line with nothing left to close, each produce a build warning naming the line, and `--strict` fails on them.

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
