---
title: Buttons
uid: e4c7a915
weight: 4
description: A styled row of buttons from markdown links.
translationKey: docs-author-shortcodes-buttons
---

`:::buttons` turns a list of markdown links into a styled row of buttons. The first link is the primary button; the rest are secondary.

## Basic buttons

:::grid 2 {.sc-demo}
```markdown
:::buttons
[Download](https://example.com/download)
[View source](https://github.com/example)
:::
```
+++
::::buttons
[Download](https://example.com/download)
[View source](https://github.com/example)
::::
:::

## Modifier class

Use `{.inverted}` for a light-on-dark variant, or any other class for custom styles from `style.css`.

:::grid 2 {.sc-demo}
```markdown
:::buttons {.inverted}
[Primary action](https://example.com/action)
[Secondary](https://example.com/secondary)
:::
```
+++
::::buttons {.inverted}
[Primary action](https://example.com/action)
[Secondary](https://example.com/secondary)
::::
:::

The first link is always the primary button; all subsequent links are secondary. To change which action is primary, reorder the links.

Each line is one `[text](url)` link. A trailing attribute block such as `{role=secondary}` is ignored (the position decides primary or secondary), and a line that is not a link is left out; the build warns about either and names the line.
