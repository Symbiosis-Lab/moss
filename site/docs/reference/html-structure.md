---
title: HTML structure
uid: e5a2951d
weight: 43
translationKey: docs-reference-html
description: The DOM skeleton moss emits on every page. The stable selectors to hook into from script.js or style.css.
---

Every page moss builds shares this DOM layout, mirroring the `shell.html`, `article-content.html`, and `page-content.html` templates inside moss.

<!-- auto:start:html-dom -->
```html
<html data-theme="light">
  <body>
    <nav class="main-nav">
      <div class="nav-content">...</div>
    </nav>
    <main class="container">
      <article>
        <h1 class="article-title">...</h1>
        <div class="article-meta">
          <time class="article-date">...</time>
        </div>
        <!-- page content -->
      </article>
    </main>
    <footer class="container">
      ...
      <div class="moss-colophon">...</div>
    </footer>
  </body>
</html>
```

### Key selectors

| Selector | What it is |
|----------|------------|
| `.main-nav` | Top navigation bar |
| `.container` | Main content wrapper |
| `article` | Page content area |
| `.article-title` | Page heading |
| `.article-meta` | Date and metadata |
| `.moss-colophon` | The moss credit, the last child of the footer (after `</main>` on a page with no footer) |
<!-- auto:end -->

Content inside `article` and `.moss-colophon` is safe to modify from `script.js`. The navigation structure is managed by moss — add styles, but do not remove or reorder its children.

### Data attributes

These attributes are set by moss on HTML elements. Read them in `script.js` or target them in `style.css`.

| Attribute | Element | Values | Purpose |
|-----------|---------|--------|---------|
| `data-theme` | `<html>` | `"light"`, `"dark"` | Current color theme |
| `data-moss-preview` | `<body>` | present/absent | Set when in preview mode |
| `data-comments` | `<article>` | `"true"`, `"false"` | Whether comments are enabled |
| `data-source-line` | headings, paragraphs | line number | Source markdown line (dev aid) |
| `data-content-width` | `<article>` | `"wide"` | Content width override |

Read `data-theme` in JavaScript:

```javascript
const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
```
