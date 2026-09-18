---
title: Hero
uid: c5a8f037
weight: 2
description: A full-width hero section with a background image and optional overlay text.
translationKey: docs-author-shortcodes-hero
---

`:::hero` creates a full-width section hoisted out of the article flow and rendered edge-to-edge. The first line inside the block is the media reference; everything after it becomes overlay content.

## Basic hero

:::grid 2 {.sc-demo}
```markdown
:::hero
# Welcome to my site
A personal corner of the web.
:::
```
+++
::::hero
# Welcome to my site
A personal corner of the web.
::::
:::

## Hero with image

Pass a wikilink, markdown image, or bare filename as the first line to set a background image:

:::grid 2 {.sc-demo}
```markdown
:::hero {image=/assets/animations/editing.gif}
# Welcome to my site
A personal corner of the web.
:::
```
+++
::::hero {image=/assets/animations/editing.gif}
# Welcome to my site
A personal corner of the web.
::::
:::

## Display control with pipe syntax

Use pipe syntax on the image reference to control how the image fills the hero area.

:::grid 2 {.sc-demo}
```markdown
:::hero
![[/assets/animations/new folder.gif|contain top]]
:::
```
+++
::::hero
![[/assets/animations/new folder.gif|contain top]]
::::
:::

## Overlay content

Any content after the first line becomes overlay text rendered on top of the background.

:::grid 2 {.sc-demo}
```markdown
:::hero
![[/assets/animations/first time publish.gif]]
# Our work
Community theatre rooted in lived experience.
:::
```
+++
::::hero
![[/assets/animations/first time publish.gif]]
# Our work
Community theatre rooted in lived experience.
::::
:::

The hero renders full-width and is hoisted out of the article content flow, so it ignores the article's `content_width` setting.
