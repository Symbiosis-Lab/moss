---
title: Write a theme
url: design
uid: 0df1b907
weight: 30
description: How to style a moss site with CSS and JavaScript. The canonical guide for humans and AI agents.
---

How to take a site from stock to styled.

## Where your files go

Put your custom CSS and JavaScript here:

```
my-site/
├── .moss/
│   └── theme/
│       ├── style.css
│       └── script.js
├── index.md
└── ...
```

moss serves `.moss/theme/` verbatim to `/_moss/theme/` in the built site. Both files load automatically on every page. No build step, no config entry.

## How your CSS wins

moss loads **`.moss/theme/style.css`** into the last CSS layer (`@layer themes`). It wins over all of moss's built-in styles by layer order. No `!important` needed. No `@layer` needed.

## The three styling rungs

Work at the level that matches what you're changing.

### Token override

Change a CSS custom property and every component that uses it follows:

```css
:root {
  --moss-color-accent: #2d5a2d;
  --moss-font-body: "Inter", -apple-system, sans-serif;
  --moss-content-width: 72ch;
}
```

Use this rung for colors, fonts, spacing, and width — anything that should be consistent across the whole site.

### CSS selector on semantic HTML

Target moss's stable class names for component-level changes. moss emits `.moss-collection-card`, `.moss-article-listing`, `.moss-colophon`, and many others. Target them directly:

```css
.moss-collection-card {
  border-radius: 0;
}
```

The full list is in [[components|Component classes]].

### Named-class fenced div

Attach a custom class to a shortcode block with `{.class}` syntax, then target the combination:

```markdown
:::grid 3 {.profiles}
...
:::
```

```css
.profiles .moss-grid-card {
  border-radius: 50%;
}
```

Use this rung for one-off layout variations on a single page.

## Dark mode

moss sets `data-theme` on `<html>` before first paint. It reads `localStorage["moss-theme"]` and falls back to the OS `prefers-color-scheme`. One block in your stylesheet covers both the toggle and the system preference.

Do not write `@media (prefers-color-scheme: dark)`. Write this instead, which applies whenever dark mode is active regardless of how the visitor arrived there:

```css
:root[data-theme="dark"] {
  --moss-color-bg: #0f0f0f;
  --moss-color-accent: #6abf6a;
}
```

## Quiet chrome

`--moss-color-ui-accent` controls nav links, buttons, and site controls. It defaults to `var(--moss-color-accent)`, tying navigation color to your content accent. To make the chrome recede while content links keep the accent, set it to a neutral:

```css
:root {
  --moss-color-ui-accent: var(--moss-color-text);
}
```

## Scrollbars

moss leaves scrollbar visibility and appearance to the browser and the reader’s operating system, including on vertical pages and nested scroll rows. Scrollbars work independently of link previews and other optional scripts.

To keep a vertical reading surface free of a page bar, put this override in your theme. It preserves sideways column navigation and leaves nested scrollbars native:

```css
body[data-typesetting="vertical"] {
  scrollbar-width: none;
}
body[data-typesetting="vertical"]::-webkit-scrollbar {
  display: none;
}
```

## Self-hosted fonts

Drop `.woff2` files in `.moss/theme/fonts/` and reference them from `style.css`:

```css
@font-face {
  font-family: "MyFont";
  src: url("fonts/myfont.woff2") format("woff2");
}
```

For JavaScript, moss sets `window.mossTheme.base` to the `/_moss/theme/` URL before `script.js` runs. Resolve assets against it:

```javascript
const url = new URL("asset.woff2", mossTheme.base);
```

## Where to find every token and class

For the full set:

- [[css-tokens|CSS tokens]] — every `--moss-*` custom property, grouped by category
- [[components|Component classes]] — every `.moss-*` class name moss emits
- [[html-structure|HTML structure]] — the DOM skeleton and data attributes
- Run `moss describe --json` for the live values of your installed moss
