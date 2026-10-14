---
title: Fenced Divs
url: fenced-divs
uid: f8d3b526
weight: 5
description: Named-class fenced divs for styling regions of markdown without a named shortcode.
translationKey: docs-author-shortcodes-fenced-divs
---

To style a region of markdown without introducing a named shortcode, use a bare `:::` fence with a `{.class}` attribute block. Full markdown renders inside: headings, lists, images, links, and nested shortcodes all work normally.

## Basic fenced div

:::grid 2 {.sc-demo}
```markdown
::: {.tagline}
Civic engagement through community-centered performance.
:::
```
+++
::: {.tagline}
Civic engagement through community-centered performance.
:::
:::

This renders as:

```html
<div class="tagline">
  <p>Civic engagement through community-centered performance.</p>
</div>
```

## Multiple classes

:::grid 2 {.sc-demo}
```markdown
::: {.hero .narrow}
## Our story

Founded in 2018, we make theatre that listens.
:::
```
+++
::: {.hero .narrow}
## Our story

Founded in 2018, we make theatre that listens.
:::
:::

Use named-class fenced divs instead of `<div class="…">` HTML wrappers. The class lives in `.moss/theme/style.css`; the markdown stays readable. A bare `:::` with no attribute block is not a shortcode: it closes the innermost open block, and when nothing is open it stays literal text in the output and the build warns about it.
