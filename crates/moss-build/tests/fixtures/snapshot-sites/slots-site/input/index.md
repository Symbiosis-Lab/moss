---
title: Slots Baseline
uid: a3f7c291
description: A page for verifying the no-provider slot baseline.
---

# Slots Baseline

This page verifies that slot markers are stripped from built HTML when no
plugin provides content for them.

moss writes HTML comment markers at six positions during generation:

- `head-end` — before `</head>`
- `after-title` — after the article title and date
- `before-article-end` — before `</article>`
- `after-article` — after `</article>`
- `footer-right` — inside the footer
- `body-end` — before `</body>`

When built with `--no-plugins`, all six markers must be absent from the
output. This page has no special authoring syntax — slots are a template
concern, not a content concern.
