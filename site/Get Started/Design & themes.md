---
uid: bb01030e
title: Design & themes
translationKey: docs-design
url: design
weight: 60
description: Customize your site's look and behavior with CSS and JavaScript — no build step required.
---

Start with moss’s default theme when you want to publish quickly. Choose this guide when the site needs its own colors, typography, spacing, component treatment, or small browser interactions.

Put a `style.css` and, when needed, a `script.js` in `.moss/theme/`. moss loads them automatically for the site. CSS changes are the usual path; JavaScript is appropriate for behavior the page itself needs, and it runs in the published site.

The theme files are site-local and remain ordinary files in the project. There is no separate theme build step. Preview the site after each change so the light and dark states, responsive layout, and generated components stay usable.

For the selector and variable contracts, component names, dark-mode rules, and script context, continue to [Write a theme](/docs/design/). The [CSS tokens](/docs/reference/css-tokens/) and [Components](/docs/reference/components/) references fill in the exact surface.
