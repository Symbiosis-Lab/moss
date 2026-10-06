---
title: Gallery
uid: d1b6e924
weight: 3
description: An image grid for photo galleries.
translationKey: docs-author-shortcodes-gallery
---

`:::gallery` lays out a set of images in a responsive masonry-style grid. Each line inside the block is one image.

## Basic gallery

:::grid 2 {.sc-demo}
```markdown
:::gallery
![](/assets/animations/new folder.gif)
![](/assets/animations/editing.gif)
![](/assets/animations/first time publish.gif)
:::
```
+++
::::gallery
![](/assets/animations/new folder.gif)
![](/assets/animations/editing.gif)
![](/assets/animations/first time publish.gif)
::::
:::

## Column count

Pass a number after `gallery` to set the column count.

:::grid 2 {.sc-demo}
```markdown
:::gallery 3
![](/assets/animations/new folder.gif)
![](/assets/animations/editing.gif)
![](/assets/animations/first time publish.gif)
:::
```
+++
::::gallery 3
![](/assets/animations/new folder.gif)
![](/assets/animations/editing.gif)
![](/assets/animations/first time publish.gif)
::::
:::

The same count is also settable by name — `:::gallery {per-line=3}` — for when it needs to sit alongside other attributes on the same line. "Per line" is how many images fit along one line, and a line runs the way your text runs, so on a vertical page the images stack downward instead of across. The older `cols=` spelling still works but is deprecated.
