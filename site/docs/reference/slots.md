---
title: Slots
uid: f81de8fe
weight: 22
description: Template injection points and the enhance phase.
translationKey: docs-extend-slots
---

## Template slots

Slots are named injection points in the HTML template where plugins can insert content. ^def-template-slots

<!-- auto:start:slots -->
| Slot | Position | Authorable |
|---|---|---|
| `head-end` | Before </head> — for stylesheets, scripts, and meta tags. | no |
| `after-title` | Inside <article>, after the title/date row — for article metadata (e.g. book block, review colophon). | no |
| `before-article-end` | Inside <article>, before </article> — for article addenda. | no |
| `after-article` | Between </article> and </main> — for comments, reactions (NOT part of the article). | no |
| `footer-shape` | The data-moss-shape attribute value on the <footer> open tag. Advanced: controls footer chrome mode. | no |
| `footer-left` | Inside footer, leading position — filled by footer.md or any file with slot: footer-left frontmatter. | yes |
| `footer-end` | Inside footer, trailing position — for the auto-injected subscribe form and plugin widgets. | no |
| `body-end` | Before </body> — for scripts that must run after DOM is ready. | no |
<!-- auto:end:slots -->

## How slots work

During generation, moss writes HTML comment markers at each slot position:

```html
<article>
  <h1>Page Title</h1>
  <!-- slot:after-title -->
  <p>Content...</p>
  <!-- slot:before-article-end -->
</article>
<!-- slot:after-article -->
```

During the **enhance phase**, each plugin with the `enhance` capability returns content for the slots it fills. moss replaces the markers with that content. Unfilled markers are stripped from the final output and never appear in the published HTML.

Multiple plugins can write to the same slot. Their content is concatenated in plugin load order.

## EnhanceResult

The enhance hook returns an object mapping slot names to HTML strings:

```typescript
async enhance(ctx) {
  return {
    slots: {
      "after-article": `<section class="comments">
        <script src="https://comments.example/embed.js"></script>
      </section>`,
      "head-end": `<link rel="stylesheet" href="https://comments.example/comments.css">`
    }
  };
}
```

Include only the slots your plugin fills. Omitted slots are left for other plugins or stripped.

## Zero-flicker preview

During preview, moss rebuilds the site on every file change. To prevent flickering during rebuilds:

1. New output is built to `.moss/site-stage/`.
2. The preview server atomically switches its pointer to `site-stage/`.
3. The staged content is copied to `.moss/site/` (the canonical directory).
4. The pointer switches back to `.moss/site/`.

The preview server never serves from a half-built directory. The switch is instant — a pointer update, not a file rename.
